use std::collections::BTreeSet;

use crate::{node::Node, ValidationError, params::Params};

pub type NodeFactory = fn(&Params) -> Result<Box<dyn Node + Send>, ValidationError>;

/// This struct defines the node display/debug name, required and optional params,
/// as well as a node factory for a node definition. 
/// 
/// In order to let the legato DSL interact and spawn your node, 
pub struct NodeSpec {
    pub required_params: BTreeSet<String>,
    pub optional_params: BTreeSet<String>,
    pub build: NodeFactory,
}

