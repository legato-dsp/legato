//! The [`NodeSpec`] creates a factory specification allowing
//! Legato nodes or custom nodes to be instantiated by the DSL

#![allow(unused_mut)]

use std::{collections::BTreeSet, sync::Arc};

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
};

/// Builds one node instance from its DSL params.
///
/// The factory runs once per spawned instance (`voice * 5` calls it five
/// times), so it may capture only what it can hand out repeatedly: `Copy`
/// config or an `Arc` it clones into each node. Anything unique, like one end
/// of a channel, should be created inside the call, with the application's
/// end exported through [`ResourceBuilderView::export_handle`].
pub type NodeFactory =
    Arc<dyn Fn(&mut ResourceBuilderView, &DSLParams) -> NodeResult + Send + Sync>;

/// What a factory returns: a boxed node, or why it could not be built.
pub type NodeResult = Result<Box<dyn DynNode>, ValidationError>;

/// This struct defines the node display/debug name, required and optional params,
/// as well as a node factory for a node definition.
#[derive(Clone)]
pub struct NodeSpec {
    pub name: String,
    pub description: &'static str,
    pub required_params: BTreeSet<String>,
    pub optional_params: BTreeSet<String>,
    pub build: NodeFactory,
}

impl std::fmt::Debug for NodeSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeSpec")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("required_params", &self.required_params)
            .field("optional_params", &self.optional_params)
            .finish_non_exhaustive()
    }
}

impl NodeSpec {
    /// A spec built from a closure, for nodes that need state from the
    /// application at construction time.
    ///
    /// ```
    /// # use legato::prelude::*;
    /// # use legato::spec::NodeSpec;
    /// # #[derive(Clone)] struct Level { gain: f32, ports: Ports }
    /// # impl Node for Level {
    /// #     fn process(&mut self, _: &mut AudioContext, _: &Inputs, _: &mut [&mut [f32]]) {}
    /// #     fn ports(&self) -> &Ports { &self.ports }
    /// # }
    /// let default_gain = 0.5;
    /// let spec = NodeSpec::from_fn("level", &[], &["gain"], move |_rb, p| {
    ///     let gain = p.get_f32("gain")?.unwrap_or(default_gain);
    ///     Ok(Box::new(Level { gain, ports: PortBuilder::default().audio_out(1).build() }))
    /// });
    /// ```
    pub fn from_fn<F>(
        name: &str,
        required_params: &[&str],
        optional_params: &[&str],
        build: F,
    ) -> Self
    where
        F: Fn(&mut ResourceBuilderView, &DSLParams) -> NodeResult + Send + Sync + 'static,
    {
        Self {
            name: name.to_string(),
            description: "",
            required_params: required_params.iter().map(|s| s.to_string()).collect(),
            optional_params: optional_params.iter().map(|s| s.to_string()).collect(),
            build: Arc::new(build),
        }
    }

    pub fn with_description(mut self, description: &'static str) -> Self {
        self.description = description;
        self
    }

    /// Reject unknown params and missing required ones before the factory runs.
    pub fn validate_params(&self, params: &DSLParams) -> Result<(), ValidationError> {
        for k in params.0.keys() {
            if !self.required_params.contains(k) && !self.optional_params.contains(k) {
                let mut known: Vec<&str> = self
                    .required_params
                    .iter()
                    .chain(&self.optional_params)
                    .map(String::as_str)
                    .collect();
                known.sort_unstable();
                return Err(ValidationError::InvalidParameter(format!(
                    "{} has no parameter '{k}' (expected one of: {})",
                    self.name,
                    known.join(", ")
                )));
            }
        }
        for k in &self.required_params {
            if !params.0.contains_key(k) {
                return Err(ValidationError::MissingRequiredParameter(format!(
                    "{} requires parameter '{k}'",
                    self.name
                )));
            }
        }
        Ok(())
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
        // A plain fn pointer is 'static whatever Self is, so Self needs no bound.
        let create: fn(&mut ResourceBuilderView, &DSLParams) -> NodeResult = Self::create;
        NodeSpec {
            name: Self::NAME.to_string(),
            description: Self::DESCRIPTION,
            required_params: Self::REQUIRED_PARAMS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            optional_params: Self::OPTIONAL_PARAMS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            build: Arc::new(create),
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
#[cfg_attr(feature = "docs", derive(serde::Serialize))]
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
                build: std::sync::Arc::new($build),
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
