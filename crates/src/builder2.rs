use std::{collections::HashMap, marker::PhantomData};

use crate::{
    config::Config, pipes::Pipe, ports::Ports, runtime::{NodeKey, Runtime, build_runtime}
};

struct Unconfigured; // No config
struct Configured; // Contains config
struct ContainsNodes; // Ready to add connections or set sink, use pipes if the last node flag is set
struct ReadyToBuild; // Once the sink is set

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum NodeKeyStorage {
    Single(NodeKey),
    Multiple(Vec<NodeKey>),
}

pub struct LegatoBuilder<State> {
    runtime: Runtime,
    // Lookup from string to NodeKey
    working_name_lookup: HashMap<&'static str, NodeKey>,
    // Lookup from string to Pipe Fn
    pipe_lookup: HashMap<&'static str, Box<dyn Pipe>>,
    // When adding a node, this tracks and sets the node key for pipes
    last_node_ref_added: Option<NodeKeyStorage>,
    state: PhantomData<State>,
}

impl LegatoBuilder<Unconfigured> {
    pub fn new(self, config: Config, ports: Ports) -> LegatoBuilder<Configured> {
        let runtime = build_runtime(config, ports);
        LegatoBuilder::<Configured> {
            runtime: runtime,
            working_name_lookup: HashMap::new(),
            pipe_lookup: HashMap::new(),
            last_node_ref_added: None,
            state: std::marker::PhantomData,
        }
    }
}

impl LegatoBuilder<Configured> {
    pub fn add_node() {}
}

impl LegatoBuilder<ContainsNodes> {
    pub fn apply_pipe(&mut self, pipe_name: &'static str) {
        if let Some(node_ref) = &self.last_node_ref_added {
            let pipe = self.pipe_lookup.get(pipe_name).expect("Cannot find pipe");

        }
        else {
            panic!("Cannot apply pipe to non-existing node!")
        }
    }
    pub fn add_connection() {}
    pub fn set_sink(name: &'static str) {
        let key = 
    }
}

impl LegatoBuilder<ReadyToBuild> {
    pub fn build() {}
}
