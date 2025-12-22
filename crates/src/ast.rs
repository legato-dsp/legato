use std::collections::BTreeSet;
use std::vec::Vec;
use std::{collections::BTreeMap, time::Duration};

use pest::iterators::{Pair, Pairs};

use crate::builder::ValidationError;
use crate::parse::Rule;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ast {
    pub declarations: Vec<DeclarationScope>,
    pub connections: Vec<AstNodeConnection>,
    pub source: Option<NodeSinkSource>,
    pub sink: NodeSinkSource,
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
    pub pipes: Vec<ASTPipe>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ASTPipe {
    pub name: String,
    pub params: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
/// Values used in our AST to set parameters.
///
/// Note: These are parsed with a certain priority.
/// If it can be a U32, it will be a U32 before I32, etc.
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
    Slice {
        start: usize,
        end: usize,
    },
    #[default]
    Auto,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodeSinkSource {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuildAstError {
    ConstructionError(String),
}

pub fn build_ast(pairs: Pairs<Rule>) -> Result<Ast, BuildAstError> {
    let mut ast = Ast::default();

    for declaration in pairs.into_iter() {
        match declaration.as_rule() {
            Rule::scope_block => ast.declarations.push(parse_scope_block(declaration)?),
            Rule::connection => ast.connections.append(&mut parse_connection(declaration)?),
            Rule::sink => {
                let mut inner = declaration.into_inner();
                let s = inner.next().unwrap(); // ident or node-path
                ast.sink = NodeSinkSource {
                    name: s.as_str().to_string(),
                };
            }
            Rule::source => {
                let mut inner = declaration.into_inner();
                let s = inner.next().unwrap(); // ident or node-path
                ast.source = Some(NodeSinkSource {
                    name: s.as_str().to_string(),
                });
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
        if pair.as_rule() == Rule::add_nodes {
            for node in pair.into_inner() {
                declarations.push(parse_node(node)?);
            }
        }
    }

    Ok(DeclarationScope {
        namespace: scope_name,
        declarations,
    })
}

fn parse_node<'i>(pair: Pair<'i, Rule>) -> Result<NodeDeclaration, BuildAstError> {
    let mut node = NodeDeclaration {
        alias: None,
        ..Default::default()
    };

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

fn parse_pipe<'i>(pair: Pair<'i, Rule>) -> Result<ASTPipe, BuildAstError> {
    let mut inner = pair.into_inner();
    let name = inner.next().unwrap().as_str().to_string();
    let params = inner.next().map(|x| parse_value(x).unwrap());
    Ok(ASTPipe { name, params })
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
                    Rule::port_named => {
                        let mut inner = port_spec.into_inner();
                        let name = inner.next().unwrap().as_str();
                        PortConnectionType::Named {
                            port: name.to_string(),
                        }
                    }
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
                    Rule::port_slice => {
                        let mut inner = port_spec.into_inner();

                        let start = inner
                            .next()
                            .unwrap()
                            .as_str()
                            .parse::<usize>()
                            .map_err(|e| BuildAstError::ConstructionError(format!("{}", e)))?;

                        let end = inner
                            .next()
                            .unwrap()
                            .as_str()
                            .parse::<usize>()
                            .map_err(|e| BuildAstError::ConstructionError(format!("{}", e)))?;

                        PortConnectionType::Slice { start, end }
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

pub struct DSLParams<'a>(pub &'a Object);

impl<'a> DSLParams<'a> {
    pub fn new(obj: &'a Object) -> Self {
        Self(obj)
    }

    pub fn get_f32(&self, key: &str) -> Option<f32> {
        match self.0.get(key) {
            Some(Value::F32(x)) => Some(*x),
            Some(Value::I32(x)) => Some(*x as f32),
            Some(Value::U32(x)) => Some(*x as f32),
            Some(x) => panic!("Expected F32 param, found {:?}", x),
            _ => None,
        }
    }

    // Just ms for the time being
    pub fn get_duration(&self, key: &str) -> Option<Duration> {
        match self.0.get(key) {
            Some(Value::F32(ms)) => Some(Duration::from_secs_f32(ms / 1000.0)),
            Some(Value::I32(ms)) => Some(Duration::from_millis(*ms as u64)),
            Some(Value::U32(ms)) => Some(Duration::from_millis(*ms as u64)),
            Some(x) => panic!("Expected F32 or I32 param for ms, found {:?}", x),
            _ => None,
        }
    }

    pub fn get_u32(&self, key: &str) -> Option<u32> {
        match self.0.get(key) {
            Some(Value::U32(s)) => Some(*s),
            Some(x) => panic!("Expected U32 param, found {:?}", x),
            _ => None,
        }
    }

    pub fn get_usize(&self, key: &str) -> Option<usize> {
        self.get_u32(key).map(|i| i as usize)
    }

    pub fn get_str(&self, key: &str) -> Option<String> {
        match self.0.get(key) {
            Some(Value::Str(s)) => Some(s.clone()),
            Some(Value::Ident(i)) => Some(i.clone()),
            Some(x) => panic!("Expected str param, found {:?}", x),
            _ => None,
        }
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.0.get(key) {
            Some(Value::Bool(b)) => Some(*b),
            Some(x) => panic!("Expected bool param, found {:?}", x),
            _ => None,
        }
    }

    pub fn get_object(&self, key: &str) -> Option<Object> {
        match self.0.get(key) {
            Some(Value::Obj(o)) => Some(o.clone()),
            Some(x) => panic!("Expected object param, found {:?}", x),
            _ => None,
        }
    }

    pub fn get_array(&self, key: &str) -> Option<Vec<Value>> {
        match self.0.get(key) {
            Some(Value::Array(v)) => Some(v.clone()),
            Some(x) => panic!("Expected array param, found {:?}", x),
            _ => None,
        }
    }

    pub fn get_array_f32(&self, key: &str) -> Option<Vec<f32>> {
        let arr = match self.0.get(key) {
            Some(Value::Array(v)) => Some(v.clone()),
            Some(x) => panic!("Expected array param, found {:?}", x),
            _ => None,
        }?;

        Some(
            arr.into_iter()
                .map(|x| match x {
                    Value::F32(x) => x,
                    Value::I32(x) => x as f32,
                    Value::U32(x) => x as f32,
                    _ => panic!("Unexpected value in f32 array {:?}", x),
                })
                .collect(),
        )
    }

    pub fn get_array_duration_ms(&self, key: &str) -> Option<Vec<Duration>> {
        let arr = match self.0.get(key) {
            Some(Value::Array(v)) => Some(v.clone()),
            Some(x) => panic!("Expected array param, found {:?}", x),
            _ => None,
        };

        Some(
            arr.unwrap()
                .into_iter()
                .map(|x| match x {
                    Value::F32(x) => Duration::from_secs_f32(x / 1000.0),
                    Value::I32(x) => Duration::from_millis(x as u64),
                    Value::U32(x) => Duration::from_millis(x as u64),
                    _ => panic!("Unexpected value in f32 array {:?}", x),
                })
                .collect(),
        )
    }

    pub fn validate(&self, allowed: &BTreeSet<String>) -> Result<(), ValidationError> {
        // Iterate through keys. If we have one that's not allowed, return an error
        for k in self.0.keys() {
            if !allowed.contains(k) {
                return Err(ValidationError::InvalidParameter(format!(
                    "Could not find parameter with name {}",
                    k
                )));
            }
        }
        Ok(())
    }

    pub fn required(&self, required: &BTreeSet<String>) -> Result<(), ValidationError> {
        for k in required {
            if !self.0.contains_key(k) {
                return Err(ValidationError::MissingRequiredParameter(format!(
                    "Missing required perameter {}",
                    k,
                )));
            }
        }
        Ok(())
    }
}

impl<'a> From<&'a Object> for DSLParams<'a> {
    fn from(value: &'a Object) -> Self {
        DSLParams(value)
    }
}

#[cfg(test)]
mod test {
    use pest::Parser;

    use crate::parse::{LegatoParser, print_pair};

    use super::*;

    fn parse_ast(input: &str) -> Ast {
        let pairs = LegatoParser::parse(Rule::graph, input).expect("PEST failed");

        print_pair(&(pairs.clone().next().unwrap()), 4);

        build_ast(pairs).expect("AST lowering failed")
    }

    #[test]
    fn ast_node_with_alias_and_params() {
        let ast = parse_ast(
            r#"
        audio {
            osc: square_wave_one { freq: 440, gain: 0.2 }
        }
        { osc }
    "#,
        );

        assert_eq!(ast.declarations.len(), 1);
        let scope = &ast.declarations[0];
        assert_eq!(scope.namespace, "audio");

        assert_eq!(scope.declarations.len(), 1);
        let node_one = &scope.declarations[0];

        assert_eq!(node_one.node_type, "osc");
        assert_eq!(node_one.alias.as_ref().unwrap(), "square_wave_one");

        assert_eq!(node_one.params.as_ref().unwrap()["freq"], Value::U32(440));
        assert_eq!(node_one.params.as_ref().unwrap()["gain"], Value::F32(0.2));

        let sink = ast.sink;
        assert_eq!(
            sink,
            NodeSinkSource {
                name: String::from("osc")
            }
        )
    }

    #[test]
    fn ast_graph_with_connections() {
        let graph = r#"
        audio {
            sine_mono: mod { freq: 891.0 },
            sine_stereo: carrier { freq: 440.0 },
            mult_mono: fm_gain { val: 1000.0 }
        }

        mod[0] >> fm_gain[0] >> carrier[0]

        { carrier }
    "#;

        let ast = parse_ast(graph);

        assert_eq!(ast.declarations.len(), 1);
        assert_eq!(ast.connections.len(), 2);

        let c0 = &ast.connections[0];
        let c1 = &ast.connections[1];

        assert!(matches!(
            (&c0.source_port, &c0.sink_port),
            (
                PortConnectionType::Indexed { port: 0 },
                PortConnectionType::Indexed { port: 0 }
            )
        ));

        assert!(matches!(
            (&c1.source_port, &c1.sink_port),
            (
                PortConnectionType::Indexed { port: 0 },
                PortConnectionType::Indexed { port: 0 }
            )
        ));
    }

    #[test]
    fn ast_graph_with_named_ports() {
        let graph = r#"
        audio {
            osc: stereo_osc { freq: 440.0, chans: 2 },
            gain: stereo_gain { val: 0.5, chans: 4 }
        }

        stereo_osc[1] >> stereo_gain.l

        { gain_stage }
    "#;

        let ast = parse_ast(graph);

        assert_eq!(ast.connections.len(), 1);

        assert!(ast.connections[0].source_port == PortConnectionType::Indexed { port: 1 });
        assert!(ast.connections[0].sink_port == PortConnectionType::Named { port: "l".into() });
    }

    #[test]
    fn ast_graph_with_port_slices() {
        let graph = r#"
        audio {
            osc: stereo_osc { freq: 440.0, chans: 2 },
            gain: stereo_gain { val: 0.5, chans: 4 }
        }

        stereo_osc[0..1] >> stereo_gain[2..4]

        { gain_stage }
    "#;

        let ast = parse_ast(graph);

        assert_eq!(ast.declarations.len(), 1);
        let scope = &ast.declarations[0];
        assert_eq!(scope.namespace, "audio");

        assert_eq!(scope.declarations.len(), 2);

        let osc = &scope.declarations[0];
        let gain = &scope.declarations[1];

        assert_eq!(osc.node_type, "osc");
        assert_eq!(osc.alias.as_ref().unwrap(), "stereo_osc");

        assert_eq!(gain.node_type, "gain");
        assert_eq!(gain.alias.as_ref().unwrap(), "stereo_gain");

        assert_eq!(ast.connections.len(), 1);
        let conn = &ast.connections[0];

        assert_eq!(conn.source_name, "stereo_osc");
        assert_eq!(conn.sink_name, "stereo_gain");

        assert_eq!(
            conn.source_port,
            PortConnectionType::Slice { start: 0, end: 1 }
        );
        assert_eq!(
            conn.sink_port,
            PortConnectionType::Slice { start: 2, end: 4 }
        );

        assert_eq!(
            ast.sink,
            NodeSinkSource {
                name: "gain_stage".to_string()
            }
        );
    }
}
