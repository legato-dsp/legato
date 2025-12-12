#![feature(portable_simd)]

use std::{fmt::Debug, path::Path};

use heapless::spsc::{Consumer, Producer};

use crate::{ast::Value, node::Channels, runtime::{NodeKey, Runtime, RuntimeBackend}};

pub mod ports;
pub mod simd;
pub mod config;
pub mod context;
pub mod params;
pub mod ast;
pub mod parse;
pub mod spec;
pub mod connection;
pub mod registry;
pub mod node;
pub mod graph;
pub mod runtime;
pub mod resources;
pub mod ring;
pub mod math;
pub mod sample;
pub mod builder;
pub mod out;
pub mod harness;

pub mod nodes;


/// ValidationError covers logical issues
/// when lowering from the AST to the IR.
///
/// These might be bad parameters,
/// bad values, nodes that don't exist, etc.
#[derive(Clone, PartialEq, Debug)]
pub enum ValidationError {
    NodeNotFound(String),
    NamespaceNotFound(String),
    InvalidParameter(String),
    MissingRequiredParameters(String),
    MissingRequiredParameter(String),
    ResourceNotFound(String)
}

#[derive(Debug, Clone, PartialEq)]
pub enum LegatoMsg {
    SetParam { node_key: NodeKey, param_name: &'static str, value: Value }
}


pub struct LegatoApp {
    runtime: Runtime,
    receiver: Consumer<'static, LegatoMsg>
}

impl LegatoApp {
    pub fn new(runtime: Runtime, receiver: Consumer<'static, LegatoMsg>) -> Self {
        Self {
            runtime,
            receiver
        }
    }
    /// Pull the next block from the runtime, if you choose to manage the
    /// runtime yourself. 
    /// 
    /// This is useful for tests, or compatability with different audio backends.
    /// 
    /// This gives the data in a [[L,L,L], [R,R,R],etc] layout
    pub fn next_block(&mut self, external_inputs: Option<&(&Channels, &Channels)>) -> &Channels{
        while let Some(msg) = self.receiver.dequeue() {
            dbg!(&msg);
        }
        self.runtime.next_block(external_inputs)
    }
}

impl Debug for LegatoApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.runtime.fmt(f)
    }
}

pub struct LegatoBackend {
    runtime_backend: RuntimeBackend,
    producer: Producer<'static, LegatoMsg>
}

impl LegatoBackend {
    pub fn new(runtime_backend: RuntimeBackend, producer: Producer<'static, LegatoMsg>) -> Self {
        Self {
            runtime_backend,
            producer
        }
    }

    pub fn load_sample(&mut self, sampler: &String, path: &Path, chans: usize, sr: u32) -> Result<(), sample::AudioSampleError>{
        self.runtime_backend.load_sample(sampler, path.to_str().expect("Path not found!").into(), chans, sr)
    }

    pub fn send_msg(&mut self, msg: LegatoMsg){
        let _ = self.producer.enqueue(msg);
    }
}