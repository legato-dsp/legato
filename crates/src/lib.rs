#![feature(portable_simd)]

use std::{fmt::Debug, path::Path};

use ringbuf::{
    HeapCons, HeapProd,
    traits::{Consumer, Producer},
};

use crate::{
    builder::ValidationError,
    config::Config,
    executor::OutputView,
    midi::MidiRuntimeFrontend,
    msg::LegatoMsg,
    node::Inputs,
    resources::{
        buffer::AudioSampleError,
        params::{ParamError, ParamKey},
    },
    runtime::{Runtime, RuntimeFrontend},
};

pub mod builder;
pub mod config;
pub mod connection;
pub mod context;
pub mod dsl;
pub mod executor;
pub mod graph;
pub mod harness;
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

pub mod nodes;

#[derive(Debug, PartialEq, Clone)]
pub enum LegatoError {
    ValidationError(ValidationError),
    ParamError(ParamError),
}

pub struct LegatoApp {
    runtime: Runtime,
    midi_runtime_frontend: Option<MidiRuntimeFrontend>,
    consumer: HeapCons<LegatoMsg>,
}

impl LegatoApp {
    pub fn new(runtime: Runtime, receiver: HeapCons<LegatoMsg>) -> Self {
        Self {
            runtime,
            midi_runtime_frontend: None,
            consumer: receiver,
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
        while let Some(msg) = self.consumer.try_pop() {
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

pub struct LegatoFrontend {
    runtime_frontend: RuntimeFrontend,
    producer: HeapProd<LegatoMsg>,
}

impl LegatoFrontend {
    pub fn new(runtime_frontend: RuntimeFrontend, producer: HeapProd<LegatoMsg>) -> Self {
        Self {
            runtime_frontend,
            producer,
        }
    }

    pub fn load_sample(
        &mut self,
        buffer_name: &String,
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

    pub fn set_param(&mut self, name: &'static str, val: f32) -> Result<(), ParamError> {
        self.runtime_frontend.set_param(name, val)
    }

    pub fn get_param_key(&self, param_name: &'static str) -> Result<ParamKey, ParamError> {
        self.runtime_frontend.get_param_key(param_name)
    }

    /// Send a message to the LegatoRuntime
    ///
    /// TODO: Error handling!
    pub fn send_msg(&mut self, msg: LegatoMsg) {
        let _ = self.producer.try_push(msg);
    }
}
