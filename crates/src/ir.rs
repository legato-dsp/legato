use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use crate::builder::ValidationError;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    U32(u32),
    I32(i32),
    F32(f32),
    Bool(bool),
    Ident(String),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
    Template(String),
}

pub type Object = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ASTPipe {
    pub name: String,
    pub params: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Default)]
/// The definitions needed to define a node.
pub struct NodeDeclaration {
    pub node_type: String,
    pub alias: Option<String>,
    pub params: Option<Object>,
    pub pipes: Vec<ASTPipe>,
}

#[derive(Debug, Clone, PartialEq, Default)]
/// A namespace for a node, as well as all of the definitions
/// for the specific namepspace.
pub struct DeclarationScope {
    pub namespace: String,
    pub declarations: Vec<NodeDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Default)]
/// The AST is currently stored separately from the IR
///
/// This allows additional steps to lower into the IR,
/// and leaves the IR as it's own unit that can later
/// be easily serialized.
///
/// Right now, the biggest transformation that occurs before
/// we reach the IR, is the macro step, where we define and inline
/// node definitions. Originally, this was intended to be subgraphs,
/// but these would have worse cache locality, so macros became a better fit.
pub struct Ast {
    pub declarations: Vec<DeclarationScope>,
    pub connections: Vec<Connection>,
    pub macros: Vec<Macro>,
    // The exit point that the runtime/executor delivers samples from
    pub sink: String,
}

impl From<Ast> for IR {
    fn from(ast: Ast) -> Self {
        lower_ast_to_ir(ast)
    }
}

fn lower_ast_to_ir(ast: Ast) -> IR {
    let mut macro_registry = BTreeMap::<String, &Macro>::new();

    ast.macros.iter().for_each(|x| {
        macro_registry.insert(x.name.clone(), x);
    });

    let mut declarations: Vec<DeclarationScope> = Vec::new();
    let mut connections: Vec<Connection> = Vec::new();

    for scope in ast.declarations {
        let mut scope_declarations: Vec<NodeDeclaration> = Vec::new();
        for decl in scope.declarations {
            // If macro exists, expand
            if let Some(m) = macro_registry.get(&decl.node_type) {
            }
            // Otherwise if not, just add it
            else {
                scope_declarations.push(decl);
            }
        }
        // If it's not empty, this isn't good, something could not resolve
        if !scope_declarations.is_empty() {
            panic!("Could not fully expand or find all scope definitions!");
        }
        declarations.push(DeclarationScope {
            namespace: scope.namespace,
            declarations: scope_declarations,
        });
    }

    IR {
        declarations,
        connections,
        sink: ast.sink,
    }
}

fn inline_node()

#[derive(Debug, Clone, PartialEq, Default)]
/// The IR for a Legato graph. This should be relatively easy to serialize
/// down the line. Note: At this point, this does not allow for user defined
/// nodes, although user defined macros should be inlined at this point.
pub struct IR {
    pub declarations: Vec<DeclarationScope>,
    pub connections: Vec<Connection>,
    pub sink: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Port {
    Named(String),
    Index(usize),
    Slice(usize, usize),
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Endpoint {
    pub node: String,
    pub port: Port,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub source: Endpoint,
    pub sink: Endpoint,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Macro {
    name: String,
    default_params: Option<Object>,
    virtual_ports_in: Vec<String>,
    declarations: Vec<DeclarationScope>,
    connections: Vec<Connection>,
    sink: String,
}

// Logic for validation DSL params
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
            Some(Value::String(s)) => Some(s.clone()),
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
            Some(Value::Object(o)) => Some(o.clone()),
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

#[macro_export]
macro_rules! object {
    () => {
        BTreeMap::new()
    };
    ( $($key:expr => template $val:expr),* $(,)? ) => {
        {
            let mut _map = BTreeMap::new();
            $(
                _map.insert($key.to_string(), $crate::ir::Value::Template($val.to_string()));
            )*
            _map
        }
    };
    ( $($key:expr => $value:expr),* $(,)? ) => {
        {
            let mut _map = BTreeMap::new();
            $(
                _map.insert($key.to_string(), $crate::ir::Value::from($value));
            )*
            _map
        }
    };
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::F32(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::I32(v)
    }
}
impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Value::U32(v)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}
impl From<BTreeMap<String, Value>> for Value {
    fn from(v: BTreeMap<String, Value>) -> Self {
        Value::Object(v)
    }
}

impl<T> From<Vec<T>> for Value
where
    T: Into<Value>,
{
    fn from(v: Vec<T>) -> Self {
        Value::Array(v.into_iter().map(|x| x.into()).collect())
    }
}

pub struct Template(pub String);

impl From<Template> for Value {
    fn from(t: Template) -> Self {
        Value::Template(t.0)
    }
}

impl<'a> From<&'a Object> for DSLParams<'a> {
    fn from(value: &'a Object) -> Self {
        DSLParams(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuildAstError {
    ConstructionError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn basic_single_instantiation() {
        let expected_ir = IR {
            declarations: vec![
                DeclarationScope {
                    namespace: "audio".into(),
                    declarations: vec![NodeDeclaration {
                        alias: Some("osc_one".into()),
                        node_type: "sine".into(),
                        params: Some(object! {
                            "freq" => 440.0,
                        }),
                        pipes: vec![],
                    }],
                },
                DeclarationScope {
                    namespace: "audio".into(),
                    declarations: vec![NodeDeclaration {
                        alias: None,
                        node_type: "adsr".into(),
                        params: Some(object! {
                            "attack" => 300.0,
                            "decay" => 600.0,
                            "sustain" => 0.5,
                            "release" => 800.0
                        }),
                        pipes: vec![],
                    }],
                },
            ],
            connections: vec![Connection {
                source: Endpoint {
                    node: "osc_one".into(),
                    port: Port::None,
                },
                sink: Endpoint {
                    node: "adsr".into(),
                    port: Port::Named("gate".into()),
                },
            }],
            sink: "adsr".into(),
        };

        let voice_macro = Macro {
            name: "voice".into(),
            default_params: Some(object! {
                "freq" => 440.0,
                "attack" => 300.0,
                "decay" => 600.0,
                "sustain" => 0.5,
                "release" => 800.0
            }),
            virtual_ports_in: vec!["gate".into(), "freq_in".into()],
            declarations: vec![
                DeclarationScope {
                    namespace: "audio".into(),
                    declarations: vec![NodeDeclaration {
                        alias: Some("osc_one".into()),
                        node_type: "sine".into(),
                        params: Some(object! {
                            "freq" => Template("$freq".into()),
                        }),
                        pipes: vec![],
                    }],
                },
                DeclarationScope {
                    namespace: "audio".into(),
                    declarations: vec![NodeDeclaration {
                        alias: None,
                        node_type: "adsr".into(),
                        params: Some(object! {
                            "attack" => Template("$attack".into()),
                            "decay" => Template("$decay".into()),
                            "sustain" => Template("$sustain".into()),
                            "release" => Template("$release".into())
                        }),
                        pipes: vec![],
                    }],
                },
            ],
            connections: vec![Connection {
                source: Endpoint {
                    node: "osc_one".into(),
                    port: Port::None,
                },
                sink: Endpoint {
                    node: "adsr".into(),
                    port: Port::Named("gate".into()),
                },
            }],
            sink: "adsr".into(),
        };

        let ast = Ast {
            macros: vec![voice_macro],
            declarations: vec![DeclarationScope {
                namespace: "user".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "voice".into(),
                    alias: Some("lead".into()),
                    params: Some(object! { "freq" => 880.0 }),
                    ..Default::default()
                }],
            }],
            ..Default::default()
        };

        let lowered: IR = ast.into();

        // Verify name was established from new name
        let node = &ir.declarations[0].declarations[0];
        assert_eq!(node.alias, Some("lead".into()));

        // Verify parameter was instantiated correctly
        let freq_val = node.params.as_ref().unwrap().get("f").unwrap();
        assert_eq!(freq_val, &Value::F32(880.0));

        assert_eq!(lowered, expected_ir);
    }
}
