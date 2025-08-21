use crate::engine::{audio_context::AudioContext, buffer::Frame, port::{Port, Ported}};

pub trait Node<const N: usize>: Ported {
    fn process(&mut self, ctx: &AudioContext, inputs: &Frame<N>, output: &mut Frame<N>){}

}

