use std::collections::{BTreeMap, HashMap};

use crate::{ValidationError, node::Node, params::Params, spec::NodeSpec};

/// Audio registries are simply hashmaps of String node names, and their
/// corresponding NodeSpec.
/// 
/// This lets Legato users add additional nodes to a "namespace" of nodes.
pub struct AudioRegistry {
    data: HashMap<String, NodeSpec>,
}

impl AudioRegistry {
    pub fn new() -> Self {
        let data = HashMap::new();
        Self { data }
    }
    pub fn get_node(
        &self,
        name: &String,
        params: Option<&Params>,
    ) -> Result<Box<dyn Node + Send>, ValidationError> {
        if let Some(p) = params {
            return match self.data.get(name) {
                Some(spec) => (spec.build)(p),
                None => Err(ValidationError::NodeNotFound(format!(
                    "Could not find node {}",
                    name
                ))),
            };
        }
        let temp = BTreeMap::new();
        let p = Params(&temp);
        match self.data.get(name) {
            Some(spec) => (spec.build)(&p),
            None => Err(ValidationError::NodeNotFound(format!(
                "Could not find node {}",
                name
            ))),
        }
    }
    pub fn declare_node(&mut self, name: String, spec: NodeSpec) {
        self.data
            .insert(name, spec)
            .expect("Could not declare node!");
    }
}