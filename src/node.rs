#![allow(dead_code)]

// `for` loops take ownership on iterator, but I need to pass XML events iterator
// around and do some fancy things with it, so I use `while let` instead
// to release iterator ownership when it is not being advanced.
#![allow(while_let_on_iterator)]

use std::io::Read;
use std::collections::BTreeMap;
use xml::EventReader;
use xml::name::OwnedName;
use xml::attribute::OwnedAttribute;
use xml::reader::Events;

// enum BasicTypeSig {
// Byte, // y
// Bool, // b
// Int16, // n
// UInt16, // q
// Int32, // i
// Uint32, // u
// Int64, // x
// UInt64, // t
// Double, // d
// UnixFd, // h
// String, // s
// ObjPath, // o
// Sig, // g
// Variant, // v
// }


#[derive(Debug, PartialEq, Eq, Clone)]
pub struct NodeInfo {
  pub nodes: Vec<Node>,
  pub interfaces: Vec<Interface>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Node {
  pub name: String,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Interface {
  pub name: String,
  pub methods: Vec<Method>,
  pub properties: Vec<Property>,
  pub signals: Vec<Signal>,
  pub annotations: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Property {
  pub name: String,
  pub typesig: String,
  pub access: Access,
  // TODO: not supported
  pub annotations: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Method {
  pub name: String,
  pub args: Vec<(Argument, Direction)>,
  pub annotations: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Signal {
  pub name: String,
  pub args: Vec<Argument>,
  pub annotations: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Argument {
  pub name: String,
  pub typesig: String,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Direction {
  In,
  Out,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Access {
  Read,
  Write,
  ReadWrite,
}

impl ::std::str::FromStr for NodeInfo {
  type Err = ();
  fn from_str(s: &str) -> Result<NodeInfo, ()> {
    let reader = EventReader::from_str(s);
    Ok(NodeInfo::from_xml(&mut reader.into_iter()))
  }
}

impl NodeInfo {
  pub fn from_xml<R: Read>(events: &mut Events<R>) -> NodeInfo {
    use xml::reader::XmlEvent::*;

    let mut nodeinfo = NodeInfo {
      nodes: Vec::new(),
      interfaces: Vec::new(),
    };

    while let Some(ev) = events.next() {
      if let Ok(StartElement { name: OwnedName { local_name, .. }, attributes: attrs, .. }) = ev {
        match &*local_name {
          "node" => {
            match get_name(attrs) {
              Some(name) => nodeinfo.nodes.push(Node { name: name }),
              None => (),
            }
          }
          "interface" => {
            match get_name(attrs) {
              Some(name) => nodeinfo.interfaces.push(Interface::from_xml(name, events)),
              None => (),
            }
          }
          _ => (),
        }
      }
    }

    nodeinfo
  }
}

impl Interface {
  fn from_xml<R: Read>(name: String, events: &mut Events<R>) -> Interface {
    use xml::reader::XmlEvent::*;

    let mut iface = Interface {
      name: name,
      methods: Vec::new(),
      properties: Vec::new(),
      signals: Vec::new(),
      annotations: BTreeMap::new(),
    };

    while let Some(ev) = events.next() {
      match ev {
        Ok(StartElement { name: OwnedName { local_name, .. }, attributes: attrs, .. }) => {
          match &*local_name {
            "method" => {
              if let Some(name) = get_name(attrs) {
                iface.methods.push(Method::from_xml(name, events));
              }
            }
            "signals" => {
              if let Some(name) = get_name(attrs) {
                iface.signals.push(Signal::from_xml(name, events));
              }
            }
            "property" => {
              if let Some(prop) = Property::from_xml(attrs) {
                iface.properties.push(prop);
              }
            }
            "annotation" => {
              if let Some((name, value)) = get_name_value(attrs) {
                iface.annotations.insert(name, value);
              }
            }
            _ => (),
          }
        }
        Ok(EndElement { name: OwnedName { ref local_name, .. }, .. }) if local_name == "interface" => break,
        _ => (),
      }
    }

    iface
  }
}

impl Signal {
  fn from_xml<R: Read>(name: String, events: &mut Events<R>) -> Signal {
    use xml::reader::XmlEvent::*;

    let mut signal = Signal {
      name: name,
      args: Vec::new(),
      annotations: BTreeMap::new(),
    };

    while let Some(ev) = events.next() {
      match ev {
        Ok(StartElement { name: OwnedName { local_name, .. }, attributes: attrs, .. }) => {
          match &*local_name {
            "arg" => {
              if let Some(arg) = Argument::from_xml(attrs) {
                signal.args.push(arg.0);
              }
            }
            "annotation" => {
              if let Some((name, value)) = get_name_value(attrs) {
                signal.annotations.insert(name, value);
              }
            }
            _ => (),
          }
        }
        Ok(EndElement { name: OwnedName { ref local_name, .. }, .. }) if local_name == "signal" => break,
        _ => (),
      }
    }

    signal
  }
}
impl Method {
  fn from_xml<R: Read>(name: String, events: &mut Events<R>) -> Method {
    use xml::reader::XmlEvent::*;

    let mut method = Method {
      name: name,
      args: Vec::new(),
      annotations: BTreeMap::new(),
    };

    while let Some(ev) = events.next() {
      match ev {
        Ok(StartElement { name: OwnedName { local_name, .. }, attributes: attrs, .. }) => {
          match &*local_name {
            "arg" => {
              if let Some(arg) = Argument::from_xml(attrs) {
                method.args.push(arg);
              }
            }
            "annotation" => {
              if let Some((name, value)) = get_name_value(attrs) {
                method.annotations.insert(name, value);
              }
            }
            _ => (),
          }
        }
        Ok(EndElement { name: OwnedName { ref local_name, .. }, .. }) if local_name == "method" => break,
        _ => (),
      }
    }

    method
  }
}

impl Argument {
  fn from_xml(attrs: Vec<OwnedAttribute>) -> Option<(Argument, Direction)> {
    let attrs = attrs.into_iter();
    let mut name = None;
    let mut typesig = None;
    let mut dir = Direction::In;

    for attr in attrs {
      match &*attr.name.local_name {
        "name" => name = Some(attr.value),
        "type" => typesig = Some(attr.value),
        "direction" => {
          match &*attr.value {
            "in" => dir = Direction::In,
            "out" => dir = Direction::Out,
            _ => (),
          }
        }
        _ => (),
      }
    }

    if let (Some(name), Some(typesig)) = (name, typesig) {
      Some((Argument {
        name: name,
        typesig: typesig,
      },
            dir))
    } else {
      None
    }
  }
}

impl Property {
  fn from_xml(attrs: Vec<OwnedAttribute>) -> Option<Property> {
    let attrs = attrs.into_iter();
    let mut name = None;
    let mut typesig = None;
    let mut access = Access::ReadWrite;

    for attr in attrs {
      match &*attr.name.local_name {
        "name" => name = Some(attr.value),
        "type" => typesig = Some(attr.value),
        "access" | "direction" => {
          access = match &*attr.value {
            "read" => Access::Read,
            "write" => Access::Write,
            _ => Access::ReadWrite,
          }
        }
        _ => (),
      }
    }

    if let (Some(name), Some(typesig)) = (name, typesig) {
      Some(Property {
        name: name,
        typesig: typesig,
        access: access,
        annotations: BTreeMap::new(),
      })
    } else {
      None
    }
  }
}

fn get_name<I: IntoIterator<Item = OwnedAttribute>>(attrs: I) -> Option<String> {
  attrs.into_iter().find(|a| a.name.local_name == "name").map(|a| a.value)
}

fn get_name_value<I: IntoIterator<Item = OwnedAttribute>>(attrs: I) -> Option<(String, String)> {
  let mut name = None;
  let mut value = None;

  for attr in attrs {
    match &*attr.name.local_name {
      "name" => name = Some(attr.value),
      "value" => value = Some(attr.value),
      _ => (),
    }
  }

  if let (Some(name), Some(value)) = (name, value) { Some((name, value)) } else { None }
}
