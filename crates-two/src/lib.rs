#![feature(portable_simd)]

use std::collections::HashMap;

use slotmap::new_key_type;

use crate::{params::Params, registry::AudioRegistry, runtime::Runtime };

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
}


new_key_type! { 
    /// A slotmap key corresponding to a particular node.
    pub struct NodeKey; 
}


pub struct AddConnectionProps {
    source: NodeKey,
    source_kind: AddConnectionKind,
    sink: NodeKey,
    sink_kind: AddConnectionKind
}


pub enum AddConnectionKind {
    Index(usize),
    Named(&'static str),
    Auto
}

/// The legato application builder.
pub struct LegatoBuilder {
    // Namespaces are collections of registries, e.g a namespace "reverb" might contain a custom reverb alg.
    namespaces: HashMap<String, AudioRegistry>,
    // Nodes can have a default/working name or alias. This map keeps track of that and maps to the actual node key.
    working_name_to_key: HashMap<String, NodeKey>,
    runtime: Runtime
}

impl LegatoBuilder {
    pub fn add_node(&mut self, namespace: &String, node_name: &String, alias: Option<&String>, params: Option<&Params>) -> NodeKey {
        todo!()
    }
    pub fn add_connection(&mut self, connection: AddConnectionProps){
        todo!()
    }
    pub fn build(self){
        todo!()
    }
}