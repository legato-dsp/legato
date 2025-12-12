use crate::config::Config;
use crate::context::AudioContext;
use crate::graph::{AudioGraph, Connection, GraphError};
use crate::node::{Channels, Node};
use crate::ports::{PortRate, Ports};
use crate::resources::Resources;
use crate::sample::{AudioSampleBackend, AudioSampleError};
use std::fmt::Debug;
use std::vec;

use slotmap::{SecondaryMap, new_key_type};

// Arbitrary max init. inputs
pub const MAX_INITIAL_INPUTS: usize = 32;

new_key_type! {
    /// A slotmap key corresponding to a particular node.
    pub struct NodeKey;
}

pub struct Runtime {
    // Audio context containing sample rate, control rate, etc.
    context: AudioContext,
    graph: AudioGraph,
    // Where the nodes write their output to, so node sinks / port sources
    port_sources_audio: SecondaryMap<NodeKey, Vec<Box<[f32]>>>,
    port_sources_control: SecondaryMap<NodeKey, Vec<Box<[f32]>>>,
    // Preallocated buffers for delivering samples
    audio_inputs_scratch_buffers: Vec<Box<[f32]>>,
    control_inputs_scratch_buffers: Vec<Box<[f32]>>,
    // A sink key for pulling the final processed buffer. Optional for graph construction, but required at runtime
    sink_key: Option<NodeKey>,
    ports: Ports,
}
impl Runtime {
    pub fn new(context: AudioContext, graph: AudioGraph, ports: Ports) -> Self {
        let audio_sources = SecondaryMap::with_capacity(graph.len());
        let control_sources = SecondaryMap::with_capacity(graph.len());

        let config = context.get_config();
        let audio_block_size = config.audio_block_size;
        let control_block_size = config.control_block_size;

        Self {
            context,
            graph,
            port_sources_audio: audio_sources,
            port_sources_control: control_sources,
            audio_inputs_scratch_buffers: vec![
                vec![0.0; audio_block_size].into();
                MAX_INITIAL_INPUTS
            ],
            control_inputs_scratch_buffers: vec![
                vec![0.0; control_block_size].into();
                MAX_INITIAL_INPUTS
            ],
            sink_key: None,
            ports,
        }
    }
    pub fn add_node(
        &mut self,
        node: Box<dyn Node + Send>,
        name: String,
        node_kind: String,
    ) -> NodeKey {
        let ports = node.ports();

        let audio_chan_size = ports.audio_out.iter().len();
        let control_chan_size = ports.control_out.iter().len();

        let node_key = self.graph.add_node(node, name, node_kind);

        let config = self.context.get_config();

        self.port_sources_audio.insert(
            node_key,
            vec![vec![0.0; config.audio_block_size].into(); audio_chan_size],
        );
        self.port_sources_control.insert(
            node_key,
            vec![vec![0.0; config.control_block_size].into(); control_chan_size],
        );

        node_key
    }
    pub fn remove_node(&mut self, key: NodeKey) {
        self.graph.remove_node(key);
        self.port_sources_audio.remove(key);
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
    // F32 is a bit weird here, but we cast so frequently why not
    pub fn get_sample_rate(&self) -> usize {
        self.context.get_config().sample_rate
    }
    pub fn get_node_ports(&self, key: &NodeKey) -> &Ports {
        // Unwrapping becuase for now this is only used during application creation
        self.graph.get_node(*key).unwrap().ports()
    }
    pub fn get_node(&self, key: &NodeKey) -> Option<&Box<dyn Node + Send>> {
        self.graph.get_node(*key)
    }
    // TODO: Graphs as nodes again
    pub fn next_block(&mut self, external_inputs: Option<&(&Channels, &Channels)>) -> &Channels {
        let (sorted_order, nodes, incoming) = self.graph.get_sort_order_nodes_and_runtime_info(); // TODO: I don't like this, feels like incorrect ownership

        for (i, node_key) in sorted_order.iter().enumerate() {
            // Reset all of the inputs about to be passed into this node

            let ports = nodes[*node_key].get_node().ports();

            let audio_inputs_size = ports.audio_in.len();
            let control_inputs_size = ports.control_in.len();

            // Zero incoming buffers for all inputs
            self.audio_inputs_scratch_buffers[..audio_inputs_size]
                .iter_mut()
                .for_each(|buf| buf.fill(0.0));

            self.control_inputs_scratch_buffers[..control_inputs_size]
                .iter_mut()
                .for_each(|buf| buf.fill(0.0));

            // Pass in inputs if they exist to source node. In the future, maybe make this explicity rather than from topo sort

            if i == 0 && external_inputs.as_ref().is_some() {
                let (ai, ci) = external_inputs.unwrap();

                for (c, ai_chan) in ai.iter().enumerate() {
                    self.audio_inputs_scratch_buffers[c].copy_from_slice(&ai_chan);
                }
                for (c, ci_chan) in ci.iter().enumerate() {
                    self.control_inputs_scratch_buffers[c].copy_from_slice(&ci_chan);
                }
            } else {
                let incoming = incoming.get(*node_key).expect("Invalid connection!");

                for connection in incoming {
                    // Write all incoming data from the connection and port, to the current node, and the sink port
                    debug_assert!(connection.sink.node_key == *node_key);
                    match (connection.source.port_rate, connection.sink.port_rate) {
                        (PortRate::Audio, PortRate::Audio) => {
                            for (n, sample) in self.port_sources_audio[connection.source.node_key]
                                [connection.source.port_index]
                                .iter()
                                .enumerate()
                            {
                                self.audio_inputs_scratch_buffers[connection.sink.port_index][n] +=
                                    sample;
                            }
                        }
                        (PortRate::Control, PortRate::Control) => {
                            for (n, sample) in self.port_sources_control[connection.source.node_key]
                                [connection.source.port_index]
                                .iter()
                                .enumerate()
                            {
                                self.control_inputs_scratch_buffers[connection.sink.port_index]
                                    [n] += sample;
                            }
                        }
                        (PortRate::Audio, PortRate::Control) => {
                            panic!("Audio to control not currently supported")
                        }
                        (PortRate::Control, PortRate::Audio) => {
                            todo!("Control to audio not currently supported")
                        }
                    };
                }
            }

            let audio_output_buffer = &mut self.port_sources_audio[*node_key];
            let control_output_buffer = &mut self.port_sources_control[*node_key];

            let node = nodes
                .get_mut(*node_key)
                .expect("Could not find node at index {node_index:?}")
                .get_node_mut();

            node.process(
                &mut self.context,
                &self.audio_inputs_scratch_buffers[0..audio_inputs_size],
                audio_output_buffer,
                &self.control_inputs_scratch_buffers[0..control_inputs_size],
                control_output_buffer,
            );
        }

        let sink_key = self.sink_key.expect("Sink node must be provided");
        self.port_sources_audio
            .get(sink_key)
            .expect("Invalid output port!")
    }
}

impl Node for Runtime {
    fn process<'a>(
        &mut self,
        ctx: &mut AudioContext,
        ai: &Channels,
        ao: &mut Channels,
        ci: &Channels,
        _: &mut Channels,
    ) {
        let outputs = self.next_block(Some(&(ai, ci)));

        debug_assert_eq!(ai.len(), ao.len());
        debug_assert_eq!(outputs.len(), ao.len());

        for (c, out_channel) in outputs.iter().enumerate() {
            ao[c].copy_from_slice(&out_channel);
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

/// The backend that sends commands to the runtime.
///
/// For the time being, this is primarily used to load new samples,
/// but in the future, it will likely use channels for invoking certain
/// functions on certain nodes.
///
/// TOOD: Tidy this up a bit, needs better error handling
pub struct RuntimeBackend {
    audio_sample_backend: std::collections::HashMap<String, AudioSampleBackend>,
}
impl RuntimeBackend {
    pub fn new(sample_backend: std::collections::HashMap<String, AudioSampleBackend>) -> Self {
        Self {
            audio_sample_backend: sample_backend,
        }
    }
    pub fn load_sample(
        &mut self,
        sampler: &String,
        path: &str,
        chans: usize,
        sr: u32,
    ) -> Result<(), AudioSampleError> {
        if let Some(backend) = self.audio_sample_backend.get(sampler) {
            return backend.load_file(path, chans, sr);
        }
        Err(AudioSampleError::BackendNotFound)
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
            .entry(&"ports", &self.ports)
            .entry(&"sink_key", &self.sink_key)
            .finish()
    }
}
