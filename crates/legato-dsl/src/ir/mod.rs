use std::ops::Mul;

use generic_array::ArrayLength;
use legato_core::{
    application::Application,
    engine::{builder::AddNode, node::FrameSize, runtime::Runtime},
};
use typenum::{Prod, U2};

use crate::{ast::Ast, ir::registry::LegatoRegistryContainer};

pub mod params;
pub mod registry;
/// ValidationError covers logical issues
/// when lowering from the AST to the IR.
///
/// Typically, these might be bad parameters,
/// bad values, nodes that don't exist, etc.
#[derive(Clone, PartialEq, Debug)]
pub enum ValidationError {
    NodeNotFound(String),
    NamespaceNotFound(String),
    InvalidParameter(String),
    MissingRequiredParameters(String),
    MissingRequiredParameter(String),

}

pub struct IR<AF, CF>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
{
    add_node_instructions: Vec<AddNode<AF, CF>>,
}

impl<AF, CF> From<Ast> for IR<AF, CF>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
{
    fn from(ast: Ast) -> Self {
        let registry = LegatoRegistryContainer::new();
        let add_node_instructions = ast.declarations.iter().map(|x| )
    }
}






pub fn build_runtime<AF, CF, C, Ci>() -> Runtime<AF, CF, C, Ci>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
    Ci: ArrayLength,
{
    todo!()
}

pub fn build_application<AF, CF, C, Ci>() -> Application<AF, CF>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
{
    todo!()
}
