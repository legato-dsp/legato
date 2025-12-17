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

// This ceremony with NodeClone and DynNode is needed so that we can "clone" nodes by cloning the interior and boxing the result,
// otherwise, we cannot create a v-table

pub trait NodeClone {
    fn clone_box(&self) -> Box<dyn DynNode>;
}

pub trait DynNode: Node + NodeClone + Send {}
impl<T> DynNode for T where T: Node + NodeClone + Send {}

impl<T> NodeClone for T
where
    T: Node + Clone + Send + 'static,
{
    fn clone_box(&self) -> Box<dyn DynNode> {
        Box::new(self.clone())
    }
}

/// A small wrapper type for debugging nodes at runtime.
pub struct NodeWithMeta {
    name: String,
    node_kind: String,
    node: Box<dyn DynNode>,
}

impl NodeWithMeta {
    pub fn new(name: String, node_kind: String, node: Box<dyn DynNode>) -> Self {
        Self {
            name,
            node_kind,
            node,
        }
    }
    pub fn get_node(&self) -> &Box<dyn DynNode> {
        &self.node
    }
    pub fn get_node_mut(&mut self) -> &mut Box<dyn DynNode> {
        &mut self.node
    }
}

impl Clone for NodeWithMeta {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            node_kind: self.node_kind.clone(),
            node: self.node.clone_box(),
        }
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
