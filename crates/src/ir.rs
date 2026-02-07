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
}

pub type Object = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ASTPipe {
    pub name: String,
    pub params: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodeDeclaration {
    pub node_type: String,
    pub alias: Option<String>,
    pub params: Option<Object>,
    pub pipes: Vec<ASTPipe>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeclarationScope {
    pub namespace: String,
    pub declarations: Vec<NodeDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ast {
    pub declarations: Vec<DeclarationScope>,
    pub connections: Vec<Connection>,
    // When chaining executors/graphs, this is the entry point
    pub source: Option<String>,
    // The exit point that the runtime/executor delivers samples from
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

impl<'a> From<&'a Object> for DSLParams<'a> {
    fn from(value: &'a Object) -> Self {
        DSLParams(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuildAstError {
    ConstructionError(String),
}
