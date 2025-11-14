use std::collections::HashMap;
use legato_core::engine::node::Node;
use legato_core::engine::{node::FrameSize};

use crate::ast::{Value};

// pub enum OffsetAlg {
//     Random,
//     Linear,
// }

// pub enum Pipes {
//     Replicate(u16),
//     Offset { param_name: String, range: (f32, f32), alg: OffsetAlg }
// }

// pub enum IR<AF, CF, C, Ci> where
//     AF: FrameSize + Mul<U2>,
//     Prod<AF, U2>: FrameSize,
//     CF: FrameSize,
//     Ci: ArrayLength,
//     C: ArrayLength
// {
//     Runtime { runtime: &'static Runtime<AF, CF, C, Ci>} ,
//     AddNode { add_node: AddNode<AF, CF>, rename: Option<String>, pipes: Option<Vec<Pipes>> },
//     AddConnection { connection: Connection },
//     ExportParams { params: Vec<String> } // Maybe Arc<Param>?
// }

/// This trait will let us wrap our existing nodes, future nodes,
/// user space nodes, etc. into an abstraction that can be lowered
/// from the AST to our IR.
///
/// A bit more simply: I would like more people than myself,
/// to be able to make nodes that can be recognized by our DSL
///
/// It assumes that a factory function that builds nodes exists,
/// and it assumes that the params passed will resemble the params
/// from the AST. So, that tends to be our "object" type, which
/// under the hood is a BinaryTree of string "value" pairs.
trait NodeFactory<AF, CF> where AF: FrameSize, CF: FrameSize
{
    fn build_node(
        &self,
        ident: String,
        params: Option<Value>, // The params from the AST
    ) -> Result<Box<dyn Node<AF, CF>>, IRNodeBuilderError>;
}

enum IRNodeBuilderError {
    NamespaceNotFound,
    NodeNotFound
}

struct IRNodeBuilder<AF, CF> where AF: FrameSize, CF: FrameSize

{
    // A namespace is just a certain scope that has it's own node factory.
    // For instance, "IO" will have some factory that we can use to build IO nodes.
    // With this, users can add their own scopes easily!
    namespaces: HashMap<String, Box<dyn NodeFactory<AF, CF>>>,
}


// TODO
impl<AF, CF> IRNodeBuilder<AF, CF> where AF: FrameSize, CF: FrameSize
{
    pub fn new() -> Self {
        let mut namespaces = HashMap::new();
        namespaces.insert("audio".to_string(), Box::new(AudioFactory::default()) as Box<dyn NodeFactory<AF, CF>>);
        Self {
            namespaces
        }
    }
    pub fn add_namespace(&mut self, name: String, factory: Box<dyn NodeFactory<AF, CF>>) {
        self.namespaces.insert(name, factory);
    }
    pub fn build_node(&self, namespace: String, node: String, params: Option<Value>) -> Result<Box<dyn Node<AF, CF>>, IRNodeBuilderError>{
        if let Some(namespace) = self.namespaces.get(&namespace) {
            return Ok(namespace.build_node(node, params)?);
        }
        Err(IRNodeBuilderError::NamespaceNotFound)
    }
}

#[derive(Default)]
struct AudioFactory;

impl<AF, CF> NodeFactory<AF, CF> for AudioFactory where AF: FrameSize, CF: FrameSize  {
    fn build_node(
        &self,
        ident: String,
        params: Option<Value>, 
    ) -> Result<Box<dyn Node<AF, CF>>, IRNodeBuilderError> {
        todo!()
    }
}



