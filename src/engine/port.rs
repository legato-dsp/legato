use std::{ops::Add};
use generic_array::{ArrayLength, GenericArray};
use typenum::{Sum, Unsigned, U1, U2};

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum PortBehavior {
    Default, // Input: Take the first sample, Output: Fill the frame
    Sum,
    SumNormalized,
    Mute,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Port {
    pub name: &'static str,
    pub index: usize,
    pub behavior: PortBehavior,
}

pub struct Ports<Ai, Ci, O>
where
    Ai: Unsigned + Add<Ci>,
    Ci: Unsigned,
    O: Unsigned + ArrayLength,
    Sum<Ai, Ci>: Unsigned + ArrayLength,
{
    pub inputs: GenericArray<Port, Sum<Ai, Ci>>,
    pub outputs: GenericArray<Port, O>,
}

pub trait Ported<Ai, Ci, O>
where
    Ai: Unsigned + Add<Ci>,
    Ci: Unsigned,
    O: Unsigned + ArrayLength,
    Sum<Ai, Ci>: Unsigned + ArrayLength,
{
    fn get_ports(&self) -> Ports<Ai, Ci, O>;
}

pub type Mono = U1;
pub type Stereo = U2;