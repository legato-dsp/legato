#![allow(unused_mut)]

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    ir::{DSLParams, Object},
    node::DynNode,
};

/// A NodeFactory type. The resource builder allows your plugin to register shared delay or sample lines, maybe in the future generic buffers as well.
pub type NodeFactory =
    fn(&mut ResourceBuilderView, &DSLParams) -> Result<Box<dyn DynNode>, ValidationError>;

/// This struct defines the node display/debug name, required and optional params,
/// as well as a node factory for a node definition.
///
/// In order to let the legato DSL interact and spawn your node,
pub struct NodeSpec {
    pub name: String,
    pub required_params: BTreeSet<String>,
    pub optional_params: BTreeSet<String>,
    pub build: NodeFactory,
}

impl NodeSpec {
    /// A quick pass to simply see if there are any keys
    /// we do not need in this context.
    pub fn check_for_bad_params(&self, params: &DSLParams) {
        for k in params.0.keys() {
            if !self.required_params.contains(k) && !self.optional_params.contains(k) {
                panic!("Invalid params {} found on node {}", k, self.name);
            }
        }
    }
}

#[macro_export]
macro_rules! node_spec {
    (
        $name:expr,
        required = [$($req:expr),*],
        optional = [$($opt:expr),*],
        build = $build:expr
    ) => {
        {
            let mut req_params = std::collections::BTreeSet::new();
            $(req_params.insert(String::from($req));)*

            let mut opt_params = std::collections::BTreeSet::new();
            $(opt_params.insert(String::from($opt));)*


            ($name, NodeSpec {
                name: $name,
                required_params: req_params,
                optional_params: opt_params,
                build: $build, // build must be factory function with type Box<dyn Fn(&Params) -> Result<AddNode, ValidationError> + Send + Sync>
            })

        }
    };
}
