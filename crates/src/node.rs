//! The traits that any Legato Nodes must implement to be used for audio processing.

use std::fmt::Debug;

use crate::{context::AudioContext, msg::NodeMessage, ports::Ports};

pub type Inputs<'a> = [Option<&'a [f32]>];

pub type Channels<'a> = &'a [&'a [f32]];
pub type Outputs<'a> = Channels<'a>;

/// The node trait that any audio processing nodes must implement.
///
/// The channel inputs are slices of Box<[f32]>, that correspond to the interior graph audio graph rate.
///
/// When defining your own nodes, look at internal examples for how you can use the port builder as well.
///
/// The amount of channels passed to your node, depends on the ports given to that node when it is added to the runtime.
/// For the time being, this should not be mutated or invalidated at runtime.
pub trait Node {
    /// The process function for your node. They operate on slices of Box<[f32]>.
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]);
    // Pass messages to your nodes. Values should be realtime safe and require no allocations or syscalls
    fn handle_msg(&mut self, _msg: NodeMessage) {}
    /// The param names [`Node::handle_msg`] accepts in a
    /// [`NodeMessage::SetParam`]. Read once on the control thread, so the
    /// frontend rejects a misspelled name instead of the node ignoring it.
    /// The default accepts none; `None` skips the check.
    fn runtime_params(&self) -> Option<Vec<&str>> {
        Some(Vec::new())
    }
    // Get the port information for your node. This should not change after contruction.
    fn ports(&self) -> &Ports;
}

pub trait DynNode: Node + Send {}
impl<T> DynNode for T where T: Node + Send {}

/// A small wrapper type for debugging nodes at runtime.
pub struct LegatoNode {
    pub name: String,
    pub node_kind: String,
    node: Box<dyn DynNode>,
}

impl LegatoNode {
    pub fn new(name: String, node_kind: String, node: Box<dyn DynNode>) -> Self {
        Self {
            name,
            node_kind,
            node,
        }
    }

    #[inline(always)]
    pub fn get_node(&self) -> &dyn DynNode {
        &*self.node
    }

    #[inline(always)]
    pub fn get_node_mut(&mut self) -> &mut Box<dyn DynNode> {
        &mut self.node
    }
    /// A meta wrapper that handles messages. This is used because we may need more messages in the future than just params
    #[inline(always)]
    pub fn handle_msg(&mut self, msg: NodeMessage) {
        self.get_node_mut().as_mut().handle_msg(msg);
    }
}

impl Debug for LegatoNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(&self.name)
            .field("node_kind", &self.node_kind)
            .field("ports", self.node.ports())
            .finish()
    }
}
