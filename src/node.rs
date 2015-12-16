
use std::io::Read;
use xml::EventReader;
use xml::name::OwnedName;
use xml::attribute::OwnedAttribute;
use xml::reader::Events;

//enum BasicTypeSig {
    //Byte, // y
    //Bool, // b
    //Int16, // n
    //UInt16, // q
    //Int32, // i
    //Uint32, // u
    //Int64, // x
    //UInt64, // t
    //Double, // d
    //UnixFd, // h
    //String, // s
    //ObjPath, // o
    //Sig, // g
    //Variant, // v
//}


struct Node {
    name: String,
}

struct NodeInfo {
    nodes: Vec<Node>,
    interfaces: Vec<Interface>,
}

struct Interface {
    name: String,
    methods: Vec<Method>,
    properties: Vec<Property>,
    signals: Vec<Signal>,
}

struct Property {
    name: String,
    typesig: String,
    access: Access,
}

struct Method {
    name: String,
    args: Vec<(Argument, Direction)>,
}

struct Signal {
    name: String,
    args: Vec<Argument>,
}

struct Argument {
    name: String,
    typesig: String,
}

enum Direction {
    In,
    Out,
}

enum Access {
    Read,
    Write,
    ReadWrite,
}

impl NodeInfo {
    fn from_str(s: &str) -> NodeInfo {
        use xml::reader::XmlEvent::*;
        let reader = EventReader::from_str(s);

        let mut nodeinfo = NodeInfo {
            nodes: Vec::new(),
            interfaces: Vec::new(),
        };

        let mut events = reader.into_iter();
        while let Some(ev) = events.next() {
            match ev {
                Ok(StartElement { name: OwnedName { local_name, .. }, attributes: attrs, .. }) => match &*local_name {
                    "node" => nodeinfo.nodes.push(Node { name: attrs.into_iter().find(|a| a.name.local_name == "name").map(|a| a.value).unwrap() }),
                    "interface" => nodeinfo.interfaces.push(Interface::from_xml(attrs.into_iter().find(|a| a.name.local_name == "name").map(|a| a.value).unwrap(), &mut events)),
                    _ => (),
                },
                _ => (),
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
        };

        while let Some(ev) = events.next() {
            match ev {
                Ok(StartElement { name: OwnedName { local_name, .. }, attributes: attrs, .. }) => match &*local_name {
                    "method" => iface.methods.push(Method::from_xml(attrs.into_iter().find(|a| a.name.local_name == "name").map(|a| a.value).unwrap(), events)),
                    "signals" => iface.signals.push(Signal::from_xml(attrs.into_iter().find(|a| a.name.local_name == "name").map(|a| a.value).unwrap(), events)),
                    "property" => {
                        let mut attrs = attrs.into_iter();
                        let mut name = None;
                        let mut typesig = None;
                        let mut access = Access::ReadWrite;

                        for attr in attrs {
                            match &*attr.name.local_name {
                                "name" => name = Some(attr.value),
                                "type" => typesig = Some(attr.value),
                                "access" => access = match &*attr.value {
                                    "read" => Access::Read,
                                    "write" => Access::Write,
                                    _ => Access::ReadWrite,
                                },
                                _ => (),
                            }
                        }

                        iface.properties.push(Property {
                            name: name.unwrap(),
                            typesig: typesig.unwrap(),
                            access: access,
                        });
                    },
                    _ => (),
                },
                Ok(EndElement { name: OwnedName { ref local_name, .. }, .. }) if local_name == "interface" => break,
                _ => (),
            }
        }

        iface
    }
}

impl Signal {
    fn from_xml<R: Read>(name: String, events: &mut Events<R>) -> Signal {
        let mut signal = Signal {
            name: name,
            args: Vec::new(),
        };

        signal
    }
}
impl Method {
    fn from_xml<R: Read>(name: String, events: &mut Events<R>) -> Method {
        let mut method = Method {
            name: name,
            args: Vec::new(),
        };

        method
    }
}

impl Argument {
    fn from_xml(attrs: Vec<OwnedAttribute>) -> Argument {
        let mut attrs = attrs.into_iter();
        let mut name = None;
        let mut typesig = None;

        for attr in attrs {
            match &*attr.name.local_name {
                "name" => name = Some(attr.value),
                "type" => typesig = Some(attr.value),
                _ => (),
            }
        }

        Argument {
            name: name.unwrap(),
            typesig: typesig.unwrap(),
        }
    }
}
