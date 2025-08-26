use crate::engine::node::Node;
use crate::engine::audio_context::AudioContext;
use crate::engine::buffer::{Buffer, Frame};
use crate::engine::port::{Port, PortBehavior, Ported};

pub struct Stereo {}

impl Stereo {
    const INPUTS: [Port; 1] = [
        Port {
            name: "MONO",
            index: 0,
            behavior: PortBehavior::Default,
        }
    ];
    const OUTPUTS: [Port; 2] = [
        Port {
            name: "L",
            index: 0,
            behavior: PortBehavior::Default,
        },
        Port {
            name: "R",
            index: 1,
            behavior: PortBehavior::Default,
        }
    ];
}

impl<const N: usize> Node<N> for Stereo {
    fn process(&mut self, ctx: &AudioContext, inputs: &Frame<N>, output: &mut Frame<N>) {
        for n in 0..N {
            for c in 0..1 {
                output[c][n] = inputs[0][n];
            }
        }
    }
}

impl Ported for Stereo {
    fn get_input_ports(&self) -> &'static [Port] {
        &Self::INPUTS
    }
    fn get_output_ports(&self) -> &'static [Port] {
        &Self::OUTPUTS
    }
}