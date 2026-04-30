#![allow(unused_mut)]

use std::collections::BTreeSet;

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
};

/// A NodeFactory type. The resource builder allows your plugin to register shared delay or sample lines, maybe in the future generic buffers as well.
pub type NodeFactory =
    fn(&mut ResourceBuilderView, &DSLParams) -> Result<Box<dyn DynNode>, ValidationError>;

/// This struct defines the node display/debug name, required and optional params,
/// as well as a node factory for a node definition.
#[derive(Debug)]
pub struct NodeSpec {
    pub name: String,
    pub description: &'static str,
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

/// Static metadata and factory function for a node type.
///
/// Implement this alongside `Node` to make a node self-describing and
/// self-registering. The const items are available at compile time;
/// `spec()` and `doc()` derive from them at zero cost.
pub trait NodeDefinition {
    const NAME: &'static str;
    const DESCRIPTION: &'static str;
    const REQUIRED_PARAMS: &'static [&'static str];
    const OPTIONAL_PARAMS: &'static [&'static str];

    fn create(
        rb: &mut ResourceBuilderView,
        params: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError>;

    fn spec() -> NodeSpec {
        NodeSpec {
            name: Self::NAME.to_string(),
            description: Self::DESCRIPTION,
            required_params: Self::REQUIRED_PARAMS.iter().map(|s| s.to_string()).collect(),
            optional_params: Self::OPTIONAL_PARAMS.iter().map(|s| s.to_string()).collect(),
            build: Self::create,
        }
    }

    fn doc() -> NodeDoc {
        NodeDoc {
            name: Self::NAME,
            description: Self::DESCRIPTION,
            required_params: Self::REQUIRED_PARAMS,
            optional_params: Self::OPTIONAL_PARAMS,
        }
    }
}

/// Static documentation for a node, suitable for serialisation to JSON.
#[derive(Debug)]
pub struct NodeDoc {
    pub name: &'static str,
    pub description: &'static str,
    pub required_params: &'static [&'static str],
    pub optional_params: &'static [&'static str],
}

#[macro_export]
macro_rules! node_spec {
    // With explicit description
    (
        $name:expr,
        description = $desc:expr,
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
                description: $desc,
                required_params: req_params,
                optional_params: opt_params,
                build: $build,
            })
        }
    };
    // Without description — defaults to ""
    (
        $name:expr,
        required = [$($req:expr),*],
        optional = [$($opt:expr),*],
        build = $build:expr
    ) => {
        node_spec!(
            $name,
            description = "",
            required = [$($req),*],
            optional = [$($opt),*],
            build = $build
        )
    };
}
