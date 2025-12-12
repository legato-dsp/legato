use std::fmt::Debug;

use crate::{context::AudioContext, ports::Ports};

pub type Channels = [Box<[f32]>];

/// The node trait that any audio processing nodes must implement.
///
/// The channel inputs are slices of Box<[f32]>, that correspond to the interior graph audio and control rate.
///
/// When defining your own nodes, look at internal examples for how you can use the port builder as well.
///
/// The amount of channels passed to your node, depends on the ports given to that node when it is added to the runtime.
/// For the time being, this should not be mutated or invalidated at runtime.
pub trait Node {
    fn process(
        &mut self,
        ctx: &mut AudioContext,
        ai: &Channels,
        ao: &mut Channels,
        ci: &Channels,
        co: &mut Channels,
    );
    fn ports(&self) -> &Ports;
}

/// A small wrapper type for debugging nodes at runtime.
pub struct NodeWithMeta {
    name: String,
    node_kind: String,
    node: Box<dyn Node + Send>,
}

impl NodeWithMeta {
    pub fn new(name: String, node_kind: String, node: Box<dyn Node + Send>) -> Self {
        Self {
            name,
            node_kind,
            node,
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
            .field("ports", self.node.ports())
            .finish()
    }
}
