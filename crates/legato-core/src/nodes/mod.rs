use std::fmt::Debug;

use crate::{nodes::ports::Ported, runtime::context::AudioContext};

pub mod audio;
pub mod ports;

pub type NodeInputs = [Box<[f32]>];

pub trait Node: Ported {
    fn process<'a>(
        &mut self,
        ctx: &mut AudioContext,
        ai: &NodeInputs,
        ao: &mut NodeInputs,
        ci: &NodeInputs,
        co: &mut NodeInputs,
    );
}

/// A wrapper around nodes that we can use to more easily debug
pub struct NodeWithMeta {
    name: String,
    node_kind: String,
    node: Box<dyn Node + Send>
}

impl NodeWithMeta {
    pub fn new(name: String, node_kind: String, node: Box<dyn Node + Send>) -> Self {
        Self {
            name,
            node_kind,
            node
        }
    }
    pub fn get_node(&self) -> &Box<dyn Node + Send> {
        &self.node
    }
    pub fn get_node_mut(&mut self) -> &mut Box<dyn Node + Send> {
        &mut self.node
    }
}

impl Debug for NodeWithMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(&self.name)
            .field("node_kind", &self.node_kind)
            .field("ports", self.node.get_ports())
            .finish()
    }
}