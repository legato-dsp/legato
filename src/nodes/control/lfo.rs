use std::f32::consts::PI;

use crate::mini_graph::node::Node;
use crate::mini_graph::buffer::Frame;

pub struct Lfo {
    freq: f32,
    min: f32,
    max: f32,
    phase: f32,
    sample_rate: f32
}
impl Lfo {
    pub fn new(freq: f32, min: f32, max: f32, phase: f32, sample_rate: f32) -> Self {
        Self {
            freq,
            min,
            max,
            phase,
            sample_rate
        }
    }
    #[inline(always)]
    fn tick(&mut self) -> f32 {
        let amp = (self.max - self.min) * 0.5;
        let offset = (self.max + self.min) * 0.5;

        let sample = (self.phase * 2.0 * PI).sin() * amp + offset;
        self.phase += self.freq / self.sample_rate;

        sample
    }
}

impl<const N: usize, const C: usize> Node<N, C> for Lfo {
    fn process(&mut self, _: &[Frame<N, C>], output: &mut Frame<N, C>) {
        for n in 0..N {
            let sample = self.tick();
            for c in 0..C {
                output[c][n] = sample;
            }
        }
    }
}