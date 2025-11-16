use legato_core::engine::node::FrameSize;
use legato_core::engine::node::Node;
use std::collections::HashMap;

use crate::ast::Value;

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
