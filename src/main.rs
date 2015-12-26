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
use std::collections::HashMap;

use time::Timespec;
use dbus::{BusType, Connection, Message, MessageItem};
use fuse::{FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request};
use libc::{EACCES, ENOENT};
use users::get_user_by_uid;
use node::NodeInfo;

mod node;

struct DbusFs {
  dbus: Connection,
  inodes: HashMap<u64, PathBuf>,
  inode_attrs: HashMap<u64, FileAttr>,
  last_inode: AtomicUsize,
}

static DBUS_INSPECT_DEST: &'static str = "org.freedesktop.DBus";
static DBUS_INSPECT_IFACE: &'static str = "org.freedesktop.DBus";
static DBUS_INSPECT_PATH: &'static str = "/org/freedesktop/DBus";
static DBUS_INTROSPECT_IFACE: &'static str = "org.freedesktop.DBus.Introspectable";
static DBUS_ACCESS_ERROR: &'static str = "org.freedesktop.DBus.Error.AccessDenied";

static ROOT_DIR: FileAttr = FileAttr {
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
};

impl Default for DbusFs {
  fn default() -> DbusFs {
    DbusFs::new(BusType::System).unwrap()
  }
}

impl DbusFs {
  fn new(bus: BusType) -> Result<DbusFs, dbus::Error> {
    Connection::get_private(bus).map(DbusFs::from_connection)
  }

  fn from_connection(conn: Connection) -> DbusFs {
    DbusFs {
      dbus: conn,
      inodes: HashMap::new(),
      inode_attrs: HashMap::new(),
      last_inode: AtomicUsize::new(2),
    }
  }

  fn make_inode<P: AsRef<Path>>(&mut self, path: P) -> Option<&FileAttr> {
    let path = path.as_ref();
    if let Some((ino, _)) = self.inodes.iter().find(|&(_, p)| p == path) {
      return Some(&self.inode_attrs[ino]);
    }

    let ino = self.last_inode.fetch_add(1, Ordering::SeqCst) as u64;

    let (dest, object) = match split_path(path) {
      Some((d, o)) => (d, o),
      None => return None,
    };

    let uid = self.get_connection_unix_user(&dest).unwrap_or(0);
    let gid = get_user_by_uid(uid).map_or(0, |u| u.primary_group);

    let (nlink, perm) = match self.introspect(dest, object) {
      Ok(Some(node_info)) => (node_info.nodes.len() as u32, 0o755),
      Err(ref err) if err.name() == Some(DBUS_ACCESS_ERROR) => (0, 0o750),
      _ => return None,
    };

    let attr = FileAttr {
      ino: ino,
      size: 0,
      blocks: 1,
      atime: CREATE_TIME,
      mtime: CREATE_TIME,
      ctime: CREATE_TIME,
      crtime: CREATE_TIME,
      kind: FileType::Directory,
      perm: perm,
      nlink: nlink,
      uid: uid,
      gid: gid,
      rdev: 0,
      flags: 0,
    };

    self.inodes.insert(ino, path.to_owned());
    self.inode_attrs.insert(ino, attr);
    self.inode_attrs.get(&ino)
  }

  fn path_by_inode(&self, ino: u64) -> Option<&Path> {
    self.inodes.get(&ino).map(PathBuf::as_path)
  }

  fn attr_by_inode(&self, ino: u64) -> Option<&FileAttr> {
    self.inode_attrs.get(&ino)
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

  fn introspect(&self, dest: dbus::BusName, object: dbus::Path) -> Result<Option<NodeInfo>, dbus::Error> {
    let msg = Message::new_method_call(dest, object, DBUS_INTROSPECT_IFACE, "Introspect").unwrap();

    self.dbus.send_with_reply_and_block(msg, 1000).map(|msg| {
      match msg.get_items().into_iter().next() {
        Some(MessageItem::Str(s)) => s.parse().ok(),
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
  let obj = iter.as_path().to_str().and_then(|s| dbus::Path::new("/".to_owned() + s).ok());

  match (dest, obj) {
    (Some(dest), Some(obj)) => Some((dest, obj)),
    _ => None,
  }
}

#[inline]
fn list_dot_dirs(ino: u64, offset: u64, reply: &mut ReplyDirectory) -> bool {
  if offset == 0 {
    reply.add(ino, 0, FileType::Directory, ".") ||
      reply.add(ino, 1, FileType::Directory, "..")
  } else {
    false
  }
}


impl Filesystem for DbusFs {
  fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
    match ino {
      1 => reply.attr(&TTL, &ROOT_DIR),
      ino => {
        match self.attr_by_inode(ino) {
          Some(attr) => reply.attr(&TTL, attr),
          None => reply.error(ENOENT),
        }
      }
    }
  }

  fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: u64, mut reply: ReplyDirectory) {
    match ino {
      1 => {
        if list_dot_dirs(ino, offset, &mut reply) {
          return reply.ok();
        }

        match self.list_names() {
          Ok(items) => {
            for (no, name) in items.into_iter().skip(offset as usize).enumerate() {
              if let Some(attr) = self.make_inode(&*name) {
                if reply.add(attr.ino, offset + (no + 2) as u64, attr.kind, &*name) {
                  break;
                }
              }
            }
            reply.ok();
          }
          Err(_) => reply.error(ENOENT),
        }
      }

      ino => {
        let parent = match self.path_by_inode(ino) {
          Some(p) => p.to_owned(),
          None => return reply.error(ENOENT),
        };

        let (dest, object) = match split_path(&parent) {
          Some((d, o)) => (d, o),
          None => return reply.error(ENOENT),
        };

        match self.introspect(dest, object) {
          Ok(Some(ni)) => {
            if !list_dot_dirs(ino, offset, &mut reply) {
              if ni.nodes.is_empty() {
              } else {
                for (no, node) in ni.nodes.iter().skip(offset as usize).enumerate() {
                  let path = parent.join(&*node.name);
                  if let Some(attr) = self.make_inode(path) {
                    if reply.add(attr.ino, offset + (no + 2) as u64, attr.kind, &*node.name) {
                      break;
                    }
                  }
                }
              }
            }
            reply.ok();
          },

          Err(ref e) if e.name() == Some(DBUS_ACCESS_ERROR) => {
            reply.error(EACCES);
          },
          _ => {
            reply.error(ENOENT);
          },
        }
      }
    }
  }

  fn lookup(&mut self, _req: &Request, parent: u64, name: &Path, reply: ReplyEntry) {
    let path = match parent {
      1 => name.to_owned(),
      n => match self.path_by_inode(n) {
        Some(p) => p.join(name),
        None => return reply.error(ENOENT),
      }
    };

    match self.make_inode(&path) {
      Some(attr) => reply.entry(&TTL, attr, 0),
      None => reply.error(ENOENT),
    }
  }

  fn read(&mut self, _req: &Request, ino: u64, _fh: u64, offset: u64, _size: u32, reply: ReplyData) {
    match ino {
      1 => reply.error(ENOENT),
      // ino => {
      // match self.name_by_inode(ino).and_then(split_path).and_then(|(d, o)| self.introspect(d, o).ok()) {
      // Some(Some(data)) => reply.data(&data.as_bytes()[offset as usize..]),
      _ => reply.error(ENOENT),
      // }
      // }
    }
  }

}

fn main() {
  let mountpoint = env::args().nth(1).unwrap();
  let conn = DbusFs::new(BusType::System).unwrap();
  fuse::mount(conn, &mountpoint, &[]);
}
