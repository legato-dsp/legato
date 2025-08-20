use crate::engine::{audio_context::AudioContext, buffer::Frame, port::Port};



pub trait Node<const N: usize, const C: usize> {
    fn process(&mut self, ctx: &AudioContext, inputs: &[Frame<N, C>], output: &mut Frame<N,C>){}
    fn get_input_ports(&self) -> &'static [Port];
    fn get_output_ports(&self) -> &'static [Port];
}

