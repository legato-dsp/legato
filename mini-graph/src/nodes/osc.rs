use crate::engine::node::Node;
use crate::engine::audio_context::AudioContext;
use crate::engine::buffer::Frame;
use crate::engine::port::{Port, PortBehavior};

pub enum Wave {
    Sin,
    Saw,
    Triangle,
    Square,
}

pub struct Oscillator {
    phase: f32,
    wave: Wave,
}

impl Oscillator {
    const INPUTS: [Port;2] = [
        Port {
            name: "FM",
            index: 0,
            behavior: PortBehavior::Default, 
        },
        Port {
            name: "FREQ",
            index: 1,
            behavior: PortBehavior::Default,
        }
    ];

    const OUTUTS: [Port; 1] = [
        Port {
            name: "AUDIO",
            index: 0,
            behavior: PortBehavior::Default,
        }
    ];

    #[inline(always)]
    fn tick(&mut self) -> f32 {
        todo!()
    }
}

impl<const N: usize, const C: usize> Node<N, C> for Oscillator {
    fn get_input_ports(&self) -> &'static [Port] { &Self::INPUTS }
    fn get_output_ports(&self) -> &'static [Port] { &Self::OUTUTS }
    fn process(&mut self, ctx: &AudioContext , inputs: &[Frame<N, C>], output: &mut Frame<N,C>) {
        
    }
}