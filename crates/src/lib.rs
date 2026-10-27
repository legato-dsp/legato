#![cfg_attr(feature = "simd", feature(portable_simd))]
#![doc = include_str!("../README.md")]

use std::{any::Any, collections::HashMap, fmt::Debug, path::Path};

use crate::{
    builder::ValidationError,
    config::Config,
    executor::OutputView,
    graph::Connection,
    handles::{ExportsHandle, HandleError, HandleStore},
    midi::{MidiError, MidiMessage},
    msg::{LegatoMsg, NodeMessage},
    resources::{
        AudioInputKey,
        buffer::{AudioSampleError, ExternalBuffer},
        params::{ParamError, ParamKey},
    },
    runtime::{NodeKey, Runtime, RuntimeFrontend},
};

pub mod block_adapter;
pub mod builder;
pub mod config;
pub mod connection;
pub mod context;
pub mod dsl;
pub mod executor;
pub mod graph;
pub mod handles;
pub mod harness;
pub mod input;
pub mod interface;
pub mod kernel;
pub mod kernel_codegen;
pub mod kernel_emit;
pub mod kernel_plan;
pub mod math;
pub mod midi;
pub mod msg;
pub mod node;
pub mod out;
pub mod persample;
pub mod ports;
pub mod prelude;
pub mod registry;
pub mod resources;
pub mod ring;
pub mod rng;
pub mod runtime;
pub mod simd;
pub mod spec;
pub mod window;

#[cfg(feature = "docs")]
pub mod docs;
pub mod nodes;

#[derive(Debug, PartialEq, Clone, thiserror::Error)]
pub enum LegatoError {
    #[error(transparent)]
    ValidationError(ValidationError),
    #[error(transparent)]
    ParamError(ParamError),
}

pub struct LegatoApp {
    runtime: Runtime,
    msg_consumer: rtrb::Consumer<LegatoMsg>,
    audio_input_keys: HashMap<String, AudioInputKey>,
}

impl LegatoApp {
    pub fn new(
        runtime: Runtime,
        receiver: rtrb::Consumer<LegatoMsg>,
        audio_input_keys: HashMap<String, AudioInputKey>,
    ) -> Self {
        Self {
            runtime,
            msg_consumer: receiver,
            audio_input_keys,
        }
    }

    pub fn audio_input_key(&self, name: &str) -> Option<AudioInputKey> {
        self.audio_input_keys.get(name).copied()
    }

    /// Write host audio into a host-fed input, starting at frame `offset` of the next block.
    #[inline]
    pub fn write_audio_input(
        &mut self,
        key: AudioInputKey,
        chan: usize,
        offset: usize,
        src: &[f32],
    ) {
        self.runtime
            .get_context_mut()
            .get_resources_mut()
            .get_audio_input_mut(key)
            .write(chan, offset, src);
    }
    /// Queue a MIDI message at frame `frame` of the next block, e.g. from a plugin's note events.
    /// See [`crate::block_adapter::BlockAdapter`].
    #[inline]
    pub fn write_midi(&mut self, frame: usize, msg: MidiMessage) -> Result<(), MidiError> {
        self.runtime.get_context_mut().write_midi(frame, msg)
    }
    /// Pull the next block from the runtime, if you choose to manage the
    /// runtime yourself.
    ///
    /// This is useful for tests, or compatability with different audio backends.
    ///
    /// This gives the data in a LLL,RRR layout
    pub fn next_block(&mut self) -> OutputView<'_> {
        let ctx = self.runtime.get_context_mut();
        ctx.begin_midi_block();

        // Drain messages for sample update
        self.runtime.drain_external_sample_msg();

        // Handle messages from the LegatoFrontend
        while let Ok(msg) = self.msg_consumer.pop() {
            self.runtime.handle_msg(msg);
        }

        self.runtime.next_block()
    }

    pub fn get_config(&self) -> Config {
        self.runtime.get_config()
    }

    /// The kind of every node in the built graph, for asserting graph shape.
    pub fn node_kinds(&self) -> Vec<&str> {
        self.runtime.node_kinds()
    }

    /// The connections feeding a node, for inspecting or rendering the built graph.
    pub fn incoming_connections(&self, key: NodeKey) -> impl Iterator<Item = Connection> + '_ {
        self.runtime.incoming_connections(key)
    }
}

impl Debug for LegatoApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LegatoApp")
            .field("runtime", &self.runtime)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FrontendError {
    #[error("no node named '{0}' in the graph")]
    NodeNotFound(String),
    /// The message queue to the audio thread is full; the message was not sent.
    #[error("the message queue to the audio thread is full")]
    QueueFull,
}

pub struct LegatoFrontend {
    runtime_frontend: RuntimeFrontend,
    producer: rtrb::Producer<LegatoMsg>,
    node_registry: HashMap<String, NodeKey>,
    handles: HandleStore,
}

impl LegatoFrontend {
    pub fn new(
        runtime_frontend: RuntimeFrontend,
        producer: rtrb::Producer<LegatoMsg>,
        node_registry: HashMap<String, NodeKey>,
        handles: HandleStore,
    ) -> Self {
        Self {
            runtime_frontend,
            producer,
            node_registry,
            handles,
        }
    }

    /// Take the handle a node exported at `path`, e.g. `keys` or `voice.0.keys`.
    pub fn take_handle<H: Any + Send>(&mut self, path: &str) -> Result<H, HandleError> {
        self.handles.take(path)
    }

    /// Take the handle a node of type `N` exported at `path`. The handle type
    /// comes from `N`, so only a wrong path or node kind can fail.
    pub fn take_handle_of<N: ExportsHandle>(
        &mut self,
        path: &str,
    ) -> Result<N::Handle, HandleError> {
        self.handles.take_of::<N>(path)
    }

    /// Take every handle at or under `prefix`, e.g. all of `voice * 5`, in
    /// instance order. Either all of them are taken or none are.
    pub fn take_handles<H: Any + Send>(
        &mut self,
        prefix: &str,
    ) -> Result<Vec<(String, H)>, HandleError> {
        self.handles.take_all(prefix)
    }

    pub fn take_handles_of<N: ExportsHandle>(
        &mut self,
        prefix: &str,
    ) -> Result<Vec<(String, N::Handle)>, HandleError> {
        self.handles.take_all_of::<N>(prefix)
    }

    /// Paths with a handle that has not been taken yet.
    pub fn handle_paths(&self) -> Vec<String> {
        self.handles.paths()
    }

    pub fn load_sample(
        &mut self,
        buffer_name: &str,
        path: &Path,
        chans: usize,
        sr: u32,
    ) -> Result<(), AudioSampleError> {
        self.runtime_frontend
            .load_file(buffer_name, path, chans, sr)
    }

    /// Swap an already decoded buffer into `buffer_name` on the next block.
    pub fn send_buffer(
        &mut self,
        buffer_name: &str,
        buffer: ExternalBuffer,
    ) -> Result<(), AudioSampleError> {
        self.runtime_frontend.send_buffer(buffer_name, buffer)
    }

    /// Free buffers the audio thread has swapped out. Sending a buffer also
    /// does this, so call it only to release memory sooner.
    pub fn drain_garbage(&mut self) {
        self.runtime_frontend.drain_garbage();
    }

    pub fn clone_registry(&self) -> HashMap<String, NodeKey> {
        self.node_registry.clone()
    }

    pub fn set_param(&mut self, name: &str, val: f32) -> Result<(), ParamError> {
        self.runtime_frontend.set_param(name, val)
    }

    /// Look a param up once with [`Self::get_param_key`], then set it without a name lookup.
    pub fn set_param_by_key(&mut self, key: ParamKey, val: f32) -> Result<(), ParamError> {
        self.runtime_frontend.set_param_by_key(key, val)
    }

    pub fn get_param(&self, name: &str) -> Result<f32, ParamError> {
        self.runtime_frontend.get_param(self.get_param_key(name)?)
    }

    pub fn get_param_by_key(&self, key: ParamKey) -> Result<f32, ParamError> {
        self.runtime_frontend.get_param(key)
    }

    pub fn get_param_key(&self, param_name: &str) -> Result<ParamKey, ParamError> {
        self.runtime_frontend.get_param_key(param_name)
    }

    pub fn send_node_msg(
        &mut self,
        node_name: &str,
        msg: NodeMessage,
    ) -> Result<(), FrontendError> {
        let key = *self
            .node_registry
            .get(node_name)
            .ok_or_else(|| FrontendError::NodeNotFound(node_name.to_string()))?;
        self.send_msg(LegatoMsg::NodeMessage(key, msg))
    }

    /// Send a message to the LegatoRuntime
    pub fn send_msg(&mut self, msg: LegatoMsg) -> Result<(), FrontendError> {
        self.producer
            .push(msg)
            .map_err(|_| FrontendError::QueueFull)
    }
}
