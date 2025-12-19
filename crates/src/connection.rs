use crate::{ports::NodeKind, runtime::NodeKey};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct ConnectionEntry {
    pub node_key: NodeKey,
    pub port_index: usize,
    pub port_rate: NodeKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Connection {
    pub source: ConnectionEntry,
    pub sink: ConnectionEntry,
}
