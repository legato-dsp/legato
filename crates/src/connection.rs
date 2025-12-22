use crate::runtime::NodeKey;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct ConnectionEntry {
    pub node_key: NodeKey,
    pub port_index: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Connection {
    pub source: ConnectionEntry,
    pub sink: ConnectionEntry,
}
