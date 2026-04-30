#![feature(portable_simd)]

use std::{collections::HashMap, fmt::Debug, path::Path};

use crate::{
    builder::ValidationError,
    config::Config,
    executor::OutputView,
    midi::MidiRuntimeFrontend,
    msg::{LegatoMsg, NodeMessage},
    node::Inputs,
    resources::{
        buffer::AudioSampleError,
        params::{ParamError, ParamKey},
    },
    runtime::{NodeKey, Runtime, RuntimeFrontend},
};

pub mod builder;
pub mod config;
pub mod connection;
pub mod context;
pub mod dsl;
pub mod executor;
pub mod graph;
pub mod harness;
pub mod input;
pub mod interface;
pub mod math;
pub mod midi;
pub mod msg;
pub mod node;
pub mod out;
pub mod pipes;
pub mod ports;
pub mod registry;
pub mod resources;
pub mod ring;
pub mod runtime;
pub mod simd;
pub mod spec;
pub mod window;

#[cfg(feature = "docs")]
pub mod docs;
pub mod nodes;

#[derive(Debug, PartialEq, Clone)]
pub enum LegatoError {
    ValidationError(ValidationError),
    ParamError(ParamError),
}

pub struct LegatoApp {
    runtime: Runtime,
    midi_runtime_frontend: Option<MidiRuntimeFrontend>,
    msg_consumer: rtrb::Consumer<LegatoMsg>,
}

impl LegatoApp {
    pub fn new(runtime: Runtime, receiver: rtrb::Consumer<LegatoMsg>) -> Self {
        Self {
            runtime,
            midi_runtime_frontend: None,
            msg_consumer: receiver,
        }
    }
    /// Pull the next block from the runtime, if you choose to manage the
    /// runtime yourself.
    ///
    /// This is useful for tests, or compatability with different audio backends.
    ///
    /// This gives the data in a [[L,L,L], [R,R,R], etc] layout
    pub fn next_block(&mut self, external_inputs: Option<&Inputs>) -> OutputView<'_> {
        // If we have a midi runtime, drain it.
        if let Some(midi_runtime) = &self.midi_runtime_frontend {
            let ctx = self.runtime.get_context_mut();
            // Clear our old messages
            ctx.clear_midi();
            while let Some(msg) = midi_runtime.recv() {
                // TOOD: Realtime logging with channel maybe?
                if let Err(e) = ctx.insert_midi_msg(msg) {
                    eprintln!("{:?}", e);
                }
            }
        }
        // Drain messages for sample update
        self.runtime.drain_external_sample_msg();

        // Handle messages from the LegatoFrontend
        while let Ok(msg) = self.msg_consumer.pop() {
            self.runtime.handle_msg(msg);
        }

        self.runtime.next_block(external_inputs)
    }

    pub fn set_midi_runtime(&mut self, rt: MidiRuntimeFrontend) {
        self.midi_runtime_frontend = Some(rt);
    }

    pub fn get_config(&self) -> Config {
        self.runtime.get_config()
    }
}

impl Debug for LegatoApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LegatoApp")
            .field("runtime", &self.runtime)
            .finish()
    }
}

pub enum FrontendError {
    NodeNotFound(),
}

pub struct LegatoFrontend {
    runtime_frontend: RuntimeFrontend,
    producer: rtrb::Producer<LegatoMsg>,
    node_registry: HashMap<String, NodeKey>,
}

impl LegatoFrontend {
    pub fn new(
        runtime_frontend: RuntimeFrontend,
        producer: rtrb::Producer<LegatoMsg>,
        node_registry: HashMap<String, NodeKey>,
    ) -> Self {
        Self {
            runtime_frontend,
            producer,
            node_registry,
        }
    }

    pub fn load_sample(
        &mut self,
        buffer_name: &str,
        path: &Path,
        chans: usize,
        sr: u32,
    ) -> Result<(), AudioSampleError> {
        self.runtime_frontend.load_file(
            buffer_name,
            path.to_str().expect("Path not found!"),
            chans,
            sr,
        )
    }

    pub fn clone_registry(&self) -> HashMap<String, NodeKey> {
        self.node_registry.clone()
    }

    pub fn set_param(&mut self, name: &'static str, val: f32) -> Result<(), ParamError> {
        self.runtime_frontend.set_param(name, val)
    }

    pub fn get_param_key(&self, param_name: &'static str) -> Result<ParamKey, ParamError> {
        self.runtime_frontend.get_param_key(param_name)
    }

    // TODO: Error handling for both of these?

    pub fn send_node_msg(
        &mut self,
        node_name: &str,
        msg: NodeMessage,
    ) -> Result<(), FrontendError> {
        if let Some(key) = self.node_registry.get(node_name) {
            let _ = self.producer.push(LegatoMsg::NodeMessage(*key, msg));
            return Ok(());
        }
        Err(FrontendError::NodeNotFound())
    }

    /// Send a message to the LegatoRuntime
    pub fn send_msg(&mut self, msg: LegatoMsg) {
        let _ = self.producer.push(msg);
    }
}
