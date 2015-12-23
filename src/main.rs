#![cfg_attr(feature = "dev", feature(plugin))]
#![cfg_attr(feature = "dev", plugin(clippy))]

extern crate users;
extern crate time;
extern crate dbus;
extern crate fuse;
extern crate libc;
extern crate xml;

use std::env;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::path::{Path, PathBuf};

use time::Timespec;
use dbus::{Connection, BusType, Message, MessageItem, FromMessageItem};
use fuse::{Filesystem, FileType, FileAttr, ReplyDirectory, ReplyEntry, ReplyData, ReplyAttr, Request};
use libc::{ENOENT, EACCES};
use users::get_user_by_uid;

mod node;

struct DbusFs {
  dbus: Connection,
  inodes: Vec<PathBuf>,
  inode_attrs: Vec<FileAttr>,
  last_inode: AtomicUsize,
}

static DBUS_INSPECT_DEST: &'static str = "org.freedesktop.DBus";
static DBUS_INSPECT_IFACE: &'static str = "org.freedesktop.DBus";
static DBUS_INSPECT_PATH: &'static str = "/org/freedesktop/DBus";
static DBUS_INTROSPECT_IFACE: &'static str = "org.freedesktop.DBus.Introspectable";
static DBUS_ACCESS_ERROR: &'static str = "org.freedesktop.DBus.Error.AccessDenied";

impl DbusFs {
  fn new(bus: BusType) -> Result<DbusFs, dbus::Error> {
    Connection::get_private(bus).map(DbusFs::from_connection)
  }

  fn inode<P: AsRef<Path>>(&mut self, path: P, size: usize, uid: u32, gid: u32, is_dir: bool) -> &FileAttr {
    let path = path.as_ref();
    match self.inodes.iter().position(|p| p == path) {
      Some(pos) => &self.inode_attrs[pos],
      None => {
        let ino = self.last_inode.fetch_add(1, Ordering::SeqCst);
        let index = ino - 2;
        self.inodes.insert(index, path.to_owned());
        self.inode_attrs.insert(index,
                                FileAttr {
                                  ino: ino as u64,
                                  size: size as u64,
                                  blocks: 1,
                                  atime: CREATE_TIME,
                                  mtime: CREATE_TIME,
                                  ctime: CREATE_TIME,
                                  crtime: CREATE_TIME,
                                  kind: if is_dir { FileType::Directory } else { FileType::RegularFile },
                                  perm: if is_dir { 0o755 } else { 0o644 },
                                  nlink: 1,
                                  uid: uid,
                                  gid: gid,
                                  rdev: 0,
                                  flags: 0,
                                });
        &self.inode_attrs[index]
      }
    }
  }

  fn name_by_inode(&self, ino: u64) -> Option<&Path> {
    self.inodes.get((ino - 2) as usize).map(PathBuf::as_path)
  }

  fn attr_by_inode(&self, ino: u64) -> Option<&FileAttr> {
    self.inode_attrs.get((ino - 2) as usize)
  }

  fn from_connection(conn: Connection) -> DbusFs {
    DbusFs {
      dbus: conn,
      inodes: Vec::new(),
      inode_attrs: Vec::new(),
      last_inode: AtomicUsize::new(2),
    }
  }

  fn list_names(&self) -> Result<Vec<String>, dbus::Error> {
    let msg = Message::new_method_call(DBUS_INSPECT_DEST, DBUS_INSPECT_PATH, DBUS_INSPECT_IFACE, "ListNames").unwrap();
    self.dbus.send_with_reply_and_block(msg, 1000).map(|msg| {
      match msg.get_items().into_iter().next() {
        Some(MessageItem::Array(items, _)) => {
          items.into_iter()
               .filter_map(|s| {
                 match s {
                   MessageItem::Str(s) => Some(s),
                   _ => None,
                 }
               })
               .collect()
        }
        _ => Vec::new(),
      }
    })
  }

  fn get_connection_unix_user(&self, name: &dbus::BusName) -> Result<u32, dbus::Error> {
    let msg = Message::new_method_call(DBUS_INSPECT_DEST, DBUS_INSPECT_PATH, DBUS_INSPECT_IFACE, "GetConnectionUnixUser")
                .unwrap()
                .append(&**name);
    self.dbus.send_with_reply_and_block(msg, 1000).map(|msg| {
      match msg.get_items().into_iter().next() {
        Some(MessageItem::UInt32(uid)) => uid,
        _ => 0,
      }
    })
  }

  fn introspect(&self, dest: dbus::BusName, object: dbus::Path) -> Result<Option<String>, dbus::Error> {
    let msg = Message::new_method_call(dest, object, DBUS_INTROSPECT_IFACE, "Introspect").unwrap();

    self.dbus.send_with_reply_and_block(msg, 1000).map(|msg| {
      match msg.get_items().into_iter().next() {
        Some(MessageItem::Str(s)) => Some(s),
        _ => None,
      }
    })
  }
}

const TTL: Timespec = Timespec { sec: 10, nsec: 0 };
const CREATE_TIME: Timespec = Timespec {
  sec: 1381237736,
  nsec: 0,
};

fn split_path<P: AsRef<Path>>(path: P) -> Option<(dbus::BusName, dbus::Path)> {
  let path: &Path = path.as_ref();
  let mut iter = path.iter();
  let dest = iter.next().and_then(|c| c.to_str()).and_then(|d| dbus::BusName::new(d).ok());
  let obj = iter.as_path().to_str().and_then(|s| dbus::Path::new(if s.is_empty() { "/" } else { s }).ok());
  println!("{:?} => {:?} {:?}", path.display(), dest, obj);

  match (dest, obj) {
    (Some(dest), Some(obj)) => Some((dest, obj)),
    _ => None,
  }

}

impl Filesystem for DbusFs {
  fn lookup(&mut self, _req: &Request, parent: u64, name: &Path, reply: ReplyEntry) {
    let (dest, obj) = match split_path(name) {
      Some(p) => p,
      None => return reply.error(ENOENT),
    };

    match parent {
      1 => {
        let uid = self.get_connection_unix_user(&dest).unwrap_or(0);
        let gid = get_user_by_uid(uid).map_or(0, |u| u.primary_group);
        match self.introspect(dest, obj) {
          Ok(Some(s)) => {
            reply.entry(&TTL, self.inode(name, s.len(), uid, gid, true), 0);
          }
          Ok(None) => {
            reply.error(ENOENT);
          }
          Err(ref err) if err.name() == Some(DBUS_ACCESS_ERROR) => {
            reply.entry(&TTL, self.inode(name, 0, uid, gid, true), 0);
          }
          Err(_) => {
            reply.error(ENOENT);
          }
        }
      }
      _ => reply.error(ENOENT),
    }
  }

  fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
    match ino {
      1 => {
        reply.attr(&TTL,
                   &FileAttr {
                     ino: 1,
                     size: 0,
                     blocks: 0,
                     atime: CREATE_TIME,
                     mtime: CREATE_TIME,
                     ctime: CREATE_TIME,
                     crtime: CREATE_TIME,
                     kind: FileType::Directory,
                     perm: 0o755,
                     nlink: 2,
                     uid: 0,
                     gid: 0,
                     rdev: 0,
                     flags: 0,
                   })
      }
      ino => {
        match self.attr_by_inode(ino) {
          Some(attr) => reply.attr(&TTL, attr),
          None => reply.error(ENOENT),
        }
      }
    }
  }

  fn read(&mut self, _req: &Request, ino: u64, _fh: u64, offset: u64, _size: u32, reply: ReplyData) {
    match ino {
      1 => reply.error(ENOENT),
      ino => {
        match self.name_by_inode(ino).and_then(split_path).and_then(|(d, o)| self.introspect(d, o).ok()) {
          Some(Some(data)) => reply.data(&data.as_bytes()[offset as usize..]),
          _ => reply.error(ENOENT),
        }
      }
    }
  }

  fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: u64, mut reply: ReplyDirectory) {
    match ino {
      1 => {
        match self.list_names() {
          Ok(items) => {
            if offset < 2 {
              reply.add(1, 0, FileType::Directory, ".");
              reply.add(1, 1, FileType::Directory, "..");
            }
            for (no, item) in items.into_iter().enumerate().skip(offset as usize) {
              if reply.add(offset + (no + 2) as u64, offset + (no + 2) as u64, FileType::Directory, &*item) {
                break;
              }
            }
            reply.ok();
          }
          Err(_) => reply.error(ENOENT),
        }
      }
      _ => reply.error(ENOENT),
    }
  }
}

fn main() {
  let mountpoint = env::args().nth(1).unwrap();
  let conn = DbusFs::new(BusType::System).unwrap();
  fuse::mount(conn, &mountpoint, &[]);
}
