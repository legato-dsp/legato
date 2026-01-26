use crate::config::Config;
use crate::context::AudioContext;
use crate::executor::Executor;
use crate::graph::{Connection, GraphError};
use crate::msg::{self, LegatoMsg};
use crate::node::{Inputs, LegatoNode, Node};
use crate::ports::Ports;
use crate::resources::Resources;
use crate::sample::{AudioSampleError, AudioSampleFrontend};
use std::fmt::Debug;

use slotmap::new_key_type;

// Arbitrary max init. inputs
pub const MAX_INPUTS: usize = 32;

new_key_type! {
    /// A slotmap key corresponding to a particular node.
    pub struct NodeKey;
}

#[derive(Clone)]
pub struct Runtime {
    // Audio context containing sample rate, control rate, etc.
    context: AudioContext,
    executor: Executor,
    ports: Ports,
}
impl Runtime {
    pub fn new(context: AudioContext, ports: Ports) -> Self {
        let executor = Executor::default();

        Self {
            context,
            executor,
            ports,
        }
    }
    pub fn add_node(&mut self, node: LegatoNode) -> NodeKey {
        self.executor.graph.add_node(node)
    }
    pub fn remove_node(&mut self, key: NodeKey) -> Option<LegatoNode> {
        self.executor.graph.remove_node(key)
    }

    pub fn replace_node(&mut self, key: NodeKey, node: LegatoNode) {
        self.executor.graph.replace(key, node);
    }

    pub fn add_edge(&mut self, connection: Connection) -> Result<Connection, GraphError> {
        self.executor.graph.add_edge(connection)
    }
    pub fn remove_edge(&mut self, connection: Connection) -> Result<(), GraphError> {
        self.executor.graph.remove_edge(connection)
    }
    pub fn set_sink_key(&mut self, key: NodeKey) -> Result<(), GraphError> {
        self.executor.set_sink(key)
    }
    pub fn set_source_key(&mut self, key: NodeKey) -> Result<(), GraphError> {
        self.executor.set_source(key)
    }
    pub fn set_resources(&mut self, resources: Resources) {
        self.context.set_resources(resources);
    }
    pub fn get_context_mut(&mut self) -> &mut AudioContext {
        &mut self.context
    }
    pub fn get_context(&mut self) -> &AudioContext {
        &self.context
    }
    pub fn get_config(&self) -> Config {
        self.context.get_config()
    }
    /// Prepare and allocate all of the information needed for the audio execution plan
    pub fn prepare(&mut self) {
        let block_size = self.context.get_config().block_size;
        assert!(block_size != 0 && block_size % 2 == 0);

        self.executor.prepare(block_size);
    }
    /// Handle the message from the LegatoFrontend
    ///
    /// TODO: How do we handle nested runtimes?
    pub fn handle_msg(&mut self, msg: LegatoMsg) {
        #[cfg(debug_assertions)]
        match msg {
            LegatoMsg::NodeMessage(key, param_msg) => {
                if let Some(node) = self.get_node_mut(&key) {
                    node.handle_msg(param_msg);
                }
            }
        }
    }

    // F32 is a bit weird here, but we cast so frequently why not
    pub fn get_sample_rate(&self) -> usize {
        self.context.get_config().sample_rate
    }
    pub fn get_node_ports(&self, key: &NodeKey) -> &Ports {
        // Unwrapping becuase for now this is only used during application creation
        self.executor
            .graph
            .get_node(*key)
            .unwrap()
            .get_node()
            .ports()
    }
    pub fn get_node(&self, key: &NodeKey) -> Option<&LegatoNode> {
        self.executor.graph.get_node(*key)
    }
    pub fn get_node_mut(&mut self, key: &NodeKey) -> Option<&mut LegatoNode> {
        self.executor.graph.get_node_mut(*key)
    }

    // Execute the audio plan and return the next block
    pub fn next_block(&mut self, external_inputs: Option<&Inputs>) -> &[&[f32]] {
        &self.executor.process(&mut self.context, external_inputs)
    }
}

impl Node for Runtime {
    fn process<'a>(&mut self, _: &mut AudioContext, ai: &Inputs, ao: &mut [&mut [f32]]) {
        let outputs = self.next_block(Some(ai));

        debug_assert_eq!(ai.len(), ao.len());
        debug_assert_eq!(outputs.len(), ao.len());

        for (c, out_channel) in outputs.iter().enumerate() {
            ao[c].copy_from_slice(out_channel);
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
    fn handle_msg(&mut self, msg: msg::NodeMessage) {
        match msg {
            msg::NodeMessage::SetParam(_) => {
                unimplemented!("Runtime subgraph messaging not yet setup")
            }
        }
    }
}

/// The frontend that exposes a number of ways to communicate with the realtime audio thread.
///
/// At the moment, you can pass messages via a channel that will be forwarded to a node.
///
/// There is also a dedicated signal node that can be controlled as well.
///
/// TODO: Tidy this up a bit, needs better error handling
/// TODO: Do we need this and the legato frontend
/// TODO: How does this work with subgraphs? Do we just merge all of the params with the parent graph?
pub struct RuntimeFrontend {
    audio_sample_frontend: std::collections::HashMap<String, AudioSampleFrontend>,
}
impl RuntimeFrontend {
    pub fn new(sample_frontends: std::collections::HashMap<String, AudioSampleFrontend>) -> Self {
        Self {
            audio_sample_frontend: sample_frontends,
        }
    }

    pub fn load_sample(
        &mut self,
        sampler: &String,
        path: &str,
        chans: usize,
        sr: u32,
    ) -> Result<(), AudioSampleError> {
        if let Some(frontend) = self.audio_sample_frontend.get(sampler) {
            return frontend.load_file(path, chans, sr);
        }
        Err(AudioSampleError::FrontendNotFound)
    }
}

pub fn build_runtime(config: Config, ports: Ports) -> Runtime {
    let context = AudioContext::new(config);

    Runtime::new(context, ports)
}

impl Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entry(&"config", &self.context.get_config())
            .key(&"graph")
            .value(&self.executor.graph)
            .entry(&"graph_ports", &self.ports)
            .entry(&"sink_key", self.executor.sink())
            .finish()
    }
}
