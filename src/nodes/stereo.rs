use crate::engine::{buffer::Buffer, node::Node, port::{Port, PortBehavior, PortedErased}};
use crate::engine::audio_context::AudioContext;

#[derive(Default)]
pub struct Stereo {}

impl<const N: usize> Node<N> for Stereo {
    fn process(&mut self, ctx: &AudioContext, inputs: &[Buffer<N>], outputs: &mut [Buffer<N>]) {
        debug_assert_eq!(inputs.len(), 1);
        debug_assert_eq!(outputs.len(), 2);

        for n in 0..N {
            for c in 0..1 {
                outputs[c][n] = inputs[0][n];
            }
        }
    }
}

impl PortedErased for Stereo {
    fn get_inputs(&self) -> &[Port] {
        &[
            Port {name: "AUDIO", index: 0, behavior: PortBehavior::Default }
        ]        
    }
    fn get_outputs(&self) -> &[Port] {
                &[
            Port {name: "L", index: 0, behavior: PortBehavior::Default },
            Port {name: "R", index: 0, behavior: PortBehavior::Default }
        ]   
    }
}