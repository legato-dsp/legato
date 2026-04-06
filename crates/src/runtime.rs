use crate::config::Config;
use crate::context::AudioContext;
use crate::executor::{Executor, OutputView};
use crate::graph::{Connection, GraphError};
use crate::msg::LegatoMsg;
use crate::node::{Inputs, LegatoNode};
use crate::ports::Ports;
use crate::resources::buffer::{AudioSampleError, decode_with_ffmpeg};
use crate::resources::params::{ParamError, ParamKey};
use crate::resources::{ResourceFrontend, Resources};
use slotmap::new_key_type;
use std::fmt::Debug;

// Arbitrary max init. inputs
pub const MAX_INPUTS: usize = 32;

new_key_type! {
    /// A slotmap key corresponding to a particular node.
    pub struct NodeKey;
}

pub struct Runtime {
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
        assert!(block_size != 0 && block_size.is_multiple_of(2));

        self.executor.prepare(block_size);
    }
    /// Handle the message from the LegatoFrontend
    pub fn handle_msg(&mut self, msg: LegatoMsg) {
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
    pub fn next_block(&mut self, external_inputs: Option<&Inputs>) -> OutputView<'_> {
        self.executor.process(&mut self.context, external_inputs)
    }
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

/// The main frontend that you can use to send commands to the realtime
/// audio thread.
///
/// This includes features like:
///
/// - Setting shared Params (Signal nodes with smoothing)
/// - Loading shared buffers
/// - Passing messages to nodes
/// - Garbage collecting external buffers off of the realtime thread
pub struct RuntimeFrontend {
    resource_frontend: ResourceFrontend,
}

impl RuntimeFrontend {
    pub fn new(resource_frontend: ResourceFrontend) -> Self {
        Self { resource_frontend }
    }

    pub fn load_file(
        &mut self,
        name: &str,
        path: &str,
        chans: usize,
        sr: u32,
    ) -> Result<(), AudioSampleError> {
        match decode_with_ffmpeg(path, chans, sr) {
            Ok(decoded) => self
                .resource_frontend
                .send_external_buffer(name, decoded)
                .map_err(|_| AudioSampleError::FailedToSendToRuntime),
            Err(_) => Err(AudioSampleError::FailedDecoding),
        }
    }

    pub fn set_param(&mut self, name: &'static str, val: f32) -> Result<(), ParamError> {
        if let Ok(key) = self.resource_frontend.get_param_key(name) {
            return self.resource_frontend.set_param(key, val);
        }
        Err(ParamError::ParamNotFound)
    }

    pub fn get_param_key(&self, param_name: &'static str) -> Result<ParamKey, ParamError> {
        self.resource_frontend.get_param_key(param_name)
    }
}
