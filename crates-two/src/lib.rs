#![feature(portable_simd)]

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


