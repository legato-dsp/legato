use std::ops::Add;

use generic_array::ArrayLength;
use typenum::{Sum, Unsigned};

use crate::engine::{audio_context::AudioContext, buffer::Frame, port::{Port, Ported}};

pub trait Node<const N: usize, Ai, Ci, O>: Ported<Ai, Ci, O>
where
    Ai: Unsigned + Add<Ci>,
    Ci: Unsigned,
    O: Unsigned + ArrayLength,
    Sum<Ai, Ci>: Unsigned + ArrayLength,
{
    fn process(&mut self, ctx: &AudioContext, inputs: &Frame<N>, output: &mut Frame<N>){}
}

