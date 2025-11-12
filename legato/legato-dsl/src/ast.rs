use std::collections::BTreeMap;
use std::vec::Vec;

use pest::iterators::{Pair, Pairs};

use crate::parse::Rule;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ast {
    pub declarations: Vec<DeclarationScope>,
    pub connections: Vec<Connection>,
    pub exports: Vec<Export>,
}

// Declarations

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeclarationScope {
    pub scope_name: String,
    pub declarations: Vec<NodeDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodeDeclaration {
    pub node_type: String,
    pub alias: Option<String>,
    pub params: Option<Object>,
    pub pipes: Vec<Pipe>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pipe {
    pub name: String,
    pub params: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    F32(f32),
    I32(i32),
    U32(u32),
    Bool(bool),
    Str(String),
    Obj(Object),
    Array(Vec<Value>),
    Ident(String),
}

pub type Object = BTreeMap<String, Value>;

// Connections

#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub source_name: String,
    pub sink_name: String,
    pub ports: Option<PortConnectionType>, // If not present, we assume auto mapped
}

#[derive(Debug, Clone, PartialEq)]
pub enum PortConnectionType {
    Indexed { port: usize },
    Named { port: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Export {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuildAstError {
    ConstructionError(String),
}

fn build_ast(pairs: Pairs<Rule>) -> Result<Ast, BuildAstError> {
    let mut ast = Ast::default();

    for declaration in pairs.into_iter() {
        match declaration.as_rule() {
            Rule::scope_block => ast.declarations.push(parse_scope_block(declaration)?),
            Rule::connection => ast.connections.push(parse_connection(declaration)?),
            Rule::exports => ast.exports = parse_exports(declaration)?,
            Rule::WHITESPACE => (),
            _ => (),
        }
    }

    todo!()
}

fn parse_scope_block<'i>(pair: Pair<'i, Rule>) -> Result<DeclarationScope, BuildAstError> {
    let mut inner = pair.into_inner();
    let scope_name = inner.next().unwrap().as_str().to_string();
    let mut declarations = vec![];

    for pair in inner {
        match pair.as_rule() {
            Rule::add_nodes => {
                for node in pair.into_inner() {
                    declarations.push(parse_node(node)?);
                }
            }
            _ => (),
        }
    }

    Ok(DeclarationScope {
        scope_name,
        declarations,
    })
}

fn parse_node<'i>(pair: Pair<'i, Rule>) -> Result<NodeDeclaration, BuildAstError> {
    let mut node = NodeDeclaration::default();
    node.alias = None;

    for p in pair.into_inner() {
        match p.as_rule() {
            Rule::node_type => node.node_type = p.as_str().to_string(),
            Rule::alias_name => node.alias = Some(p.as_str().to_string()),
            Rule::object => node.params = Some(parse_object(p).unwrap()),
            Rule::node_pipe => node.pipes.push(parse_pipe(p).unwrap()),
            _ => (),
        }
    }

    Ok(node)
}

fn parse_pipe<'i>(pair: Pair<'i, Rule>) -> Result<Pipe, BuildAstError> {
    let mut inner = pair.into_inner();
    let name = inner.next().unwrap().as_str().to_string();
    let params = inner.next().map(|x| parse_value(x).unwrap());
    Ok(Pipe { name, params })
}

fn parse_connection<'i>(pair: Pair<'i, Rule>) -> Result<Connection, BuildAstError> {
    pair.into_inner().map(|x| {
        let mut node_and_ports_inner = x.into_inner();
    });

    todo!()
}

fn parse_exports<'i>(pair: Pair<'i, Rule>) -> Result<Vec<Export>, BuildAstError> {
    let mut exports = Vec::new();

    for p in pair.into_inner() {
        match p.as_rule() {
            Rule::ident => exports.push(Export {
                name: p.as_str().to_string(),
            }),
            _ => panic!("Unexpected value in exports!"),
        }
    }

    Ok(exports)
}

// Utilities for common values

fn parse_value<'i>(pair: Pair<'i, Rule>) -> Result<Value, BuildAstError> {
    let value = match pair.as_rule() {
        Rule::float => Value::F32(pair.as_str().parse().unwrap()),
        Rule::int => Value::I32(pair.as_str().parse().unwrap()),
        Rule::uint => Value::U32(pair.as_str().parse().unwrap()),
        Rule::true_keyword => Value::Bool(true),
        Rule::false_keyword => Value::Bool(false),
        Rule::string => Value::Str(pair.as_str().trim_matches('"').to_string()),
        Rule::object => Value::Obj(parse_object(pair).unwrap()),
        Rule::array => Value::Array(parse_array(pair).unwrap()),
        Rule::ident => Value::Ident(pair.as_str().to_string()),
        _ => panic!("Unexpected value: {:?}", pair.as_rule()),
    };
    Ok(value)
}

fn parse_object<'i>(pair: Pair<'i, Rule>) -> Result<Object, BuildAstError> {
    let mut obj = BTreeMap::new();
    for kv in pair.into_inner() {
        let mut inner = kv.into_inner();
        let key = inner.next().unwrap().as_str().to_string();
        let value = parse_value(inner.next().unwrap()).unwrap();
        obj.insert(key, value);
    }
    Ok(obj)
}

fn parse_array(pair: Pair<Rule>) -> Result<Vec<Value>, BuildAstError> {
    Ok(pair.into_inner().map(|x| parse_value(x).unwrap()).collect())
}
