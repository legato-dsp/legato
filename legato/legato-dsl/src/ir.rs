use std::ops::Mul;

use generic_array::ArrayLength;
use legato_core::engine::{builder::AddNode, graph::Connection, node::FrameSize, runtime::Runtime};
use typenum::{Prod, U2};


pub enum OffsetAlg {
    Random,
    Linear,
}

pub enum Pipes {
    Replicate(u16),
    Offset { param_name: String, range: (f32, f32), alg: OffsetAlg }
}

pub enum IR<AF, CF, C, Ci> where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    Ci: ArrayLength,
    C: ArrayLength

{
    Runtime { runtime: &'static Runtime<AF, CF, C, Ci>} ,
    AddNode { add_node: AddNode<AF, CF>, rename: Option<String>, pipes: Option<Vec<Pipes>> },
    AddConnection { connection: Connection },
    ExportParams { params: Vec<String> } // Maybe Arc<Param>?
}

