use std::collections::HashMap;

use crate::{
    ValidationError,
    ast::{NodeDeclaration, Value},
};

pub struct PipeRegistry {
    data: HashMap<String, Box<dyn Pipe>>,
}

impl PipeRegistry {
    pub fn new(data: HashMap<String, Box<dyn Pipe>>) -> Self {
        Self { data }
    }

    pub fn add(&mut self, name: String, pipe: Box<dyn Pipe>) {
        self.data.insert(name, pipe);
    }

    pub fn get(&self, name: &String) -> Result<&Box<dyn Pipe>, ValidationError> {
        self.data
            .get(name)
            .ok_or(ValidationError::PipeNotFound(format!(
                "Could not find pipe {}",
                name
            )))
    }
}

impl Default for PipeRegistry {
    fn default() -> Self {
        let mut data: HashMap<String, Box<dyn Pipe>> = HashMap::new();
        data.insert(String::from("replicate"), Box::new(Replicate {}));

        Self { data }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipeResult {
    Node(NodeDeclaration),
    Vec(Vec<NodeDeclaration>),
}

pub trait Pipe {
    fn pipe(&self, inputs: PipeResult, _props: Option<Value>) -> PipeResult {
        inputs
    }
}

// A collection of a few default pipes

struct Replicate;

impl Pipe for Replicate {
    fn pipe(&self, inputs: PipeResult, props: Option<Value>) -> PipeResult {
        match inputs {
            PipeResult::Node(n) => {
                let val = props.unwrap_or(Value::U32(2));

                match val {
                    Value::U32(i) => PipeResult::Vec(
                        (0..i)
                            .collect::<Vec<_>>()
                            .iter()
                            .map(|_| n.clone())
                            .collect(),
                    ),
                    _ => panic!("Must provide U32 to replicate"),
                }
            }
            PipeResult::Vec(_) => panic!("Must provide single node for replicate pipe."),
        }
    }
}
