//! Connections in Legato consist of a [`NodeKey`],
//! and an index for a port.
//!
//! Ports are effectively specific channels of processing.
//! Having port and node keys allows us to say mix nodes
//! a and b into a stereo c node.
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
