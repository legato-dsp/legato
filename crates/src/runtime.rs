use crate::config::Config;
use crate::context::AudioContext;
use crate::graph::{AudioGraph, Connection, GraphError};
use crate::msg::{self, LegatoMsg};
use crate::node::{Channels, Inputs, LegatoNode, Node};
use crate::ports::Ports;
use crate::resources::Resources;
use crate::sample::{AudioSampleError, AudioSampleFrontend};
use std::fmt::Debug;
use std::vec;

use slotmap::{SecondaryMap, new_key_type};

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
    graph: AudioGraph,
    // Where the nodes write their output to, so node sinks / port sources
    port_sources: SecondaryMap<NodeKey, Vec<Box<[f32]>>>,
    // Preallocated buffers for delivering samples
    scratch_buffers: Vec<Box<[f32]>>,
    // A sink key for pulling the final processed buffer. Optional for graph construction, but required at runtime
    sink_key: Option<NodeKey>,
    source_key: Option<NodeKey>,
    ports: Ports,
}
impl Runtime {
    pub fn new(context: AudioContext, graph: AudioGraph, ports: Ports) -> Self {
        let audio_sources = SecondaryMap::with_capacity(graph.len());

        let config = context.get_config();
        let audio_block_size = config.block_size;

        Self {
            context,
            graph,
            port_sources: audio_sources,
            scratch_buffers: vec![vec![0.0; audio_block_size].into(); MAX_INPUTS],
            sink_key: None,
            source_key: None,
            ports,
        }
    }
    pub fn add_node(&mut self, node: LegatoNode) -> NodeKey {
        let ports = node.get_node().ports();

        let audio_chan_size = ports.audio_out.iter().len();

        let node_key = self.graph.add_node(node);

        let config = self.context.get_config();

        self.port_sources.insert(
            node_key,
            vec![vec![0.0; config.block_size].into(); audio_chan_size],
        );

        node_key
    }
    pub fn remove_node(&mut self, key: NodeKey) {
        self.graph.remove_node(key);
        self.port_sources.remove(key);
    }

    pub fn replace_node(&mut self, key: NodeKey, node: LegatoNode) {
        self.graph.replace(key, node);
    }

    pub fn add_edge(&mut self, connection: Connection) -> Result<Connection, GraphError> {
        self.graph.add_edge(connection)
    }
    pub fn remove_edge(&mut self, connection: Connection) -> Result<(), GraphError> {
        self.graph.remove_edge(connection)
    }
    pub fn set_sink_key(&mut self, key: NodeKey) -> Result<(), GraphError> {
        match self.graph.exists(key) {
            true => {
                self.sink_key = Some(key);
                Ok(())
            }
            false => Err(GraphError::NodeDoesNotExist),
        }
    }
    pub fn set_source_key(&mut self, key: NodeKey) -> Result<(), GraphError> {
        match self.graph.exists(key) {
            true => {
                self.sink_key = Some(key);
                Ok(())
            }
            false => Err(GraphError::NodeDoesNotExist),
        }
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
        self.graph.get_node(*key).unwrap().get_node().ports()
    }
    pub fn get_node(&self, key: &NodeKey) -> Option<&LegatoNode> {
        self.graph.get_node(*key)
    }
    pub fn get_node_mut(&mut self, key: &NodeKey) -> Option<&mut LegatoNode> {
        self.graph.get_node_mut(*key)
    }
    // TODO: Try a zero-copy, flat [L L L, R R R] approach for better performance
    pub fn next_block(&mut self, external_inputs: Option<&Inputs>) -> &Channels {
        let (sorted_order, nodes, incoming) = self.graph.get_sort_order_nodes_and_runtime_info(); // TODO: I don't like this, feels like incorrect ownership

        for (_i, node_key) in sorted_order.iter().enumerate() {
            let ports = nodes[*node_key].get_node().ports();
            let audio_inputs_size = ports.audio_in.len();

            self.scratch_buffers[..audio_inputs_size]
                .iter_mut()
                .for_each(|buf| buf.fill(0.0));

            let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];

            let mut has_inputs: [bool; MAX_INPUTS] = [false; MAX_INPUTS];

            // Pass in inputs if they exist to source node. In the future, maybe make this explicity rather than from topo sort
            if self.source_key.is_some()
                && self.source_key.unwrap() == *node_key
                && external_inputs.as_ref().is_some()
            {
                let ai = external_inputs.unwrap();
                for (c, ai_chan) in ai.iter().enumerate() {
                    inputs[c] = Some(ai_chan.unwrap());
                }
            } else {
                let incoming = incoming.get(*node_key).expect("Invalid connection!");
                for conn in incoming {
                    let buffer = &self.port_sources[conn.source.node_key][conn.source.port_index];

                    has_inputs[conn.sink.port_index] = true;

                    for (n, sample) in buffer.iter().enumerate() {
                        self.scratch_buffers[conn.sink.port_index][n] += sample;
                    }
                }

                for i in 0..audio_inputs_size {
                    if has_inputs[i] {
                        inputs[i] = Some(&self.scratch_buffers[i]);
                    }
                }
            }

            let node = nodes
                .get_mut(*node_key)
                .expect("Could not find node at index {node_index:?}")
                .get_node_mut();

            let output = &mut self.port_sources[*node_key];

            node.process(&mut self.context, &inputs[0..audio_inputs_size], output);
        }

        let sink_key = self.sink_key.expect("Sink node must be provided");
        self.port_sources
            .get(sink_key)
            .expect("Invalid output port!")
    }
}

impl Node for Runtime {
    fn process<'a>(&mut self, _: &mut AudioContext, ai: &Inputs, ao: &mut Channels) {
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
    let graph = AudioGraph::with_capacity(config.initial_graph_capacity);
    let context = AudioContext::new(config);

    Runtime::new(context, graph, ports)
}

impl Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entry(&"config", &self.context.get_config())
            .key(&"graph")
            .value(&self.graph)
            .entry(&"graph_ports", &self.ports)
            .entry(&"sink_key", &self.sink_key)
            .finish()
    }
}
