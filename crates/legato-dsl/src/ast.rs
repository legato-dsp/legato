use std::collections::BTreeMap;
use std::vec::Vec;

use pest::iterators::{Pair, Pairs};

use crate::parse::{Rule, print_pair};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ast {
    pub declarations: Vec<DeclarationScope>,
    pub connections: Vec<AstNodeConnection>,
    pub sink: Sink,
}

// Declarations

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeclarationScope {
    pub namespace: String,
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

/// An "object" type, just a BTreeMap<String, Value>,
/// where value is an enum of potential primitive values:
///
/// i.e f32, i32, bool, another object, an array(resizable), etc.
pub type Object = BTreeMap<String, Value>;

// Connections

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AstNodeConnection {
    pub source_name: String,
    pub sink_name: String,
    pub source_port: PortConnectionType,
    pub sink_port: PortConnectionType,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum PortConnectionType {
    Indexed {
        port: usize,
    },
    Named {
        port: String,
    },
    #[default]
    Auto,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sink {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuildAstError {
    ConstructionError(String),
}

pub fn build_ast(pairs: Pairs<Rule>) -> Result<Ast, BuildAstError> {
    let mut ast = Ast::default();

    for declaration in pairs.into_iter() {
        print_pair(&declaration, 4);
        match declaration.as_rule() {
            Rule::scope_block => ast.declarations.push(parse_scope_block(declaration)?),
            Rule::connection => ast.connections.append(&mut parse_connection(declaration)?),
            Rule::sink => {
                let mut inner = declaration.into_inner();
                let s = inner.next().unwrap(); // ident or node-path
                ast.sink = Sink {
                    name: s.as_str().to_string(),
                };
            }
            Rule::WHITESPACE => (),
            _ => (),
        }
    }

    Ok(ast)
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
        namespace: scope_name,
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
            Rule::node_params => {
                let mut inner = p.into_inner();
                let obj = inner.next().unwrap();

                node.params = Some(parse_object(obj).unwrap());
            }
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

fn parse_connection<'i>(pair: Pair<'i, Rule>) -> Result<Vec<AstNodeConnection>, BuildAstError> {
    // Collect all nodes in the chain: A, B, C, ...
    let mut nodes: Vec<(String, PortConnectionType)> = Vec::new();

    for inner in pair.into_inner() {
        let (name, port) = parse_node_or_node_with_port(inner)?;
        nodes.push((name, port));
    }

    if nodes.len() < 2 {
        return Err(BuildAstError::ConstructionError(
            "connection must involve at least 2 nodes".into(),
        ));
    }

    // Turn [A, B, C, D] into edges: A→B, B→C, C→D
    let mut connections = Vec::new();

    for i in 0..nodes.len() - 1 {
        let (source_name, source_port) = nodes[i].clone();
        let (sink_name, sink_port) = nodes[i + 1].clone();

        connections.push(AstNodeConnection {
            source_name,
            source_port,
            sink_name,
            sink_port,
        });
    }

    Ok(connections)
}

fn parse_node_or_node_with_port(
    pair: Pair<Rule>,
) -> Result<(String, PortConnectionType), BuildAstError> {
    match pair.as_rule() {
        Rule::node => Ok((pair.as_str().to_string(), PortConnectionType::Auto)),

        Rule::node_with_port => {
            let mut it = pair.into_inner();

            let node = it.next().unwrap();
            let node_name = node.as_str().to_string();

            let port = if let Some(port_spec) = it.next() {
                match port_spec.as_rule() {
                    Rule::port_name => PortConnectionType::Named {
                        port: port_spec.as_str().to_string(),
                    },
                    Rule::port_index => {
                        let num = port_spec
                            .into_inner()
                            .next()
                            .unwrap()
                            .as_str()
                            .parse::<usize>()
                            .map_err(|e| BuildAstError::ConstructionError(format!("{}", e)))?;
                        PortConnectionType::Indexed { port: num }
                    }
                    _ => PortConnectionType::Auto,
                }
            } else {
                PortConnectionType::Auto
            };

            Ok((node_name, port))
        }

        _ => Err(BuildAstError::ConstructionError(format!(
            "Unexpected node rule: {:?}",
            pair.as_rule()
        ))),
    }
}

// Utilities for common values

fn parse_value(pair: Pair<Rule>) -> Result<Value, BuildAstError> {
    let v = match pair.as_rule() {
        Rule::float => Value::F32(pair.as_str().parse().unwrap()),
        Rule::int => Value::I32(pair.as_str().parse().unwrap()),
        Rule::uint => Value::U32(pair.as_str().parse().unwrap()),
        Rule::string => Value::Str(pair.as_str().trim_matches('"').to_string()),
        Rule::true_keyword => Value::Bool(true),
        Rule::false_keyword => Value::Bool(false),
        Rule::object => Value::Obj(parse_object(pair)?),
        Rule::array => Value::Array(parse_array(pair)?),
        Rule::ident => Value::Ident(pair.as_str().to_string()),
        Rule::value => {
            let inner = pair.into_inner().next().unwrap();
            return parse_value(inner);
        }
        _ => {
            return Err(BuildAstError::ConstructionError(format!(
                "Unexpected value rule: {:?}",
                pair.as_rule()
            )));
        }
    };

    Ok(v)
}

fn parse_object<'i>(pair: Pair<'i, Rule>) -> Result<Object, BuildAstError> {
    let mut obj = BTreeMap::new();
    for kv in pair.into_inner() {
        let mut inner = kv.into_inner();
        let key = inner.next().unwrap().as_str().to_string();
        let value = inner.next().unwrap();

        let value = parse_value(value).unwrap();
        obj.insert(key, value);
    }
    Ok(obj)
}

fn parse_array(pair: Pair<Rule>) -> Result<Vec<Value>, BuildAstError> {
    Ok(pair.into_inner().map(|x| parse_value(x).unwrap()).collect())
}
