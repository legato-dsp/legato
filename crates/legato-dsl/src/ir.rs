use std::collections::HashMap;
use std::ops::Mul;

use generic_array::ArrayLength;
use legato_core::engine::node::Node;
use legato_core::engine::{builder::AddNode, graph::Connection, node::FrameSize, runtime::Runtime};
use typenum::{Prod, U2};

use crate::ast::Object;

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
/// under the hood is BinaryTree of string "value" pairs.
///
/// We don't care about node names at this level, we are just assembling
/// nodes here.
trait NodeFactory<AF, CF>
where
    AF: FrameSize,
    CF: FrameSize,
{
    fn lower_to_ir(
        &self,
        factory: Box<dyn Fn() -> Box<dyn Node<AF, CF>>>,
        params: Object, // The params from the AST
    ) -> Box<dyn Node<AF, CF>>;
}

struct IRBuilder<AF, CF>
where
    AF: FrameSize,
    CF: FrameSize,
{
    // A namespace is just a certain scope that has it's own node factory.
    // For instance, "IO" will have some factory that we can use to build IO nodes.
    // With this, users can add their own scopes easily!
    namespace: HashMap<String, Box<dyn NodeFactory<AF, CF>>>,
}

// TODO
impl<AF, CF> IRBuilder<AF, CF>
where
    AF: FrameSize,
    CF: FrameSize,
{
    pub fn new() -> Self {
        Self {
            namespace: HashMap::new(),
        }
    }
}
