use std::collections::{BTreeMap};
use std::vec::Vec;

pub struct Ast {
    declaration: Vec<DeclarationScope>,
    connection: Option<Vec<Connection>>,
    exports: Option<Vec<Export>>
}

// Declarations

pub struct DeclarationScope {
    scope_name: String,
    declaration: Vec<NodeDeclaration>
}

pub struct NodeDeclaration {
    ident: String,
    alias: Option<String>,
    node_type: String,
    params: Option<Object>,
    pipes: Option<Vec<Pipe>>
}

pub struct Pipe {
    name: String,
    params: Value
}

pub enum Value {
    F32(f32),
    I32(i32),
    U32(u32),
    Bool(bool),
    Str(String),
    Obj(Object),
    Array(Vec<Value>),
}

pub type Object = BTreeMap<String, Value>;

// Connections

pub struct Connection {
    source_name: String,
    sink_name: String,
    connection: ConnectionType
}

pub enum ConnectionType {
    Auto,
    Explicit(( PortConnectionType, PortConnectionType ))
}

pub enum PortConnectionType {
    Indexed { port: usize },
    Named { port: String }
}

// Exports

pub struct Export {
    name: String
}
