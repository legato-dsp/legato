#![feature(portable_simd)]

use std::{fmt::Debug, path::Path};

use heapless::spsc::{Consumer, Producer};

use crate::{
    builder::ValidationError,
    config::Config,
    midi::{MidiMessage, MidiMessageKind, MidiRuntimeFrontend},
    msg::LegatoMsg,
    node::{Channels, Inputs},
    params::{ParamError, ParamKey, ParamStoreFrontend},
    runtime::{Runtime, RuntimeFrontend},
};

pub mod ast;
pub mod builder;
pub mod config;
pub mod connection;
pub mod context;
pub mod graph;
pub mod harness;
pub mod math;
pub mod midi;
pub mod msg;
pub mod node;
pub mod out;
pub mod params;
pub mod parse;
pub mod pipes;
pub mod ports;
pub mod registry;
pub mod resources;
pub mod ring;
pub mod runtime;
pub mod sample;
pub mod simd;
pub mod spec;

pub mod nodes;

#[derive(Debug, PartialEq, Clone)]
pub enum LegatoError {
    ValidationError(ValidationError),
    ParamError(ParamError),
}

pub struct LegatoApp {
    runtime: Runtime,
    midi_runtime_frontend: Option<MidiRuntimeFrontend>,
    consumer: Consumer<'static, LegatoMsg>,
}

impl LegatoApp {
    pub fn new(runtime: Runtime, receiver: Consumer<'static, LegatoMsg>) -> Self {
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
    pub fn next_block(&mut self, external_inputs: Option<&Inputs>) -> &Channels {
        // If we have a midi runtime, drain it.
        if let Some(midi_runtime) = &self.midi_runtime_frontend {
            let ctx = self.runtime.get_context_mut();
            // Update timestamp for midi messages
            ctx.set_instant();
            // Clear our old messages
            ctx.clear_midi();
            while let Some(msg) = midi_runtime.recv() {
                dbg!(&msg);
                // TOOD: Realtime logging with channel maybe?
                if let Err(e) = ctx.insert_midi_msg(msg) {
                    dbg!(e);
                }
            }
        }
        // Handle messages from the LegatoFrontend
        while let Some(msg) = self.consumer.dequeue() {
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
    param_store_frontend: ParamStoreFrontend,
    producer: Producer<'static, LegatoMsg>,
}

impl LegatoFrontend {
    pub fn new(
        runtime_frontend: RuntimeFrontend,
        param_store_frontend: ParamStoreFrontend,
        producer: Producer<'static, LegatoMsg>,
    ) -> Self {
        Self {
            runtime_frontend,
            param_store_frontend,
            producer,
        }
    }

    pub fn load_sample(
        &mut self,
        sampler: &String,
        path: &Path,
        chans: usize,
        sr: u32,
    ) -> Result<(), sample::AudioSampleError> {
        self.runtime_frontend.load_sample(
            sampler,
            path.to_str().expect("Path not found!"),
            chans,
            sr,
        )
    }

    /// # Safety
    ///
    /// ParamKey must map to a valid index on this parameter store.
    ///
    /// To ensure this, this must be the same ParamKey made by the builder,
    /// and the array must have not been resized.
    ///
    /// This is more of an escape hatch if a downstream user has the performance requirement.
    pub unsafe fn set_param_unchecked(&mut self, key: ParamKey, val: f32) {
        unsafe {
            self.param_store_frontend
                .set_param_unchecked_no_clamp(key, val)
        }
    }

    pub fn set_param(&mut self, name: &'static str, val: f32) -> Result<(), ParamError> {
        if let Ok(key) = self.param_store_frontend.get_key(name) {
            return self.param_store_frontend.set_param(key, val);
        }
        Err(ParamError::ParamNotFound)
    }

    pub fn get_param_key(&self, param_name: &'static str) -> Result<ParamKey, ParamError> {
        self.param_store_frontend.get_key(param_name)
    }

    pub fn send_msg(&mut self, msg: LegatoMsg) {
        let _ = self.producer.enqueue(msg);
    }
}
