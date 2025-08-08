use std::collections::VecDeque;
use crate::mini_graph::node::AudioNode;
use crate::mini_graph::buffer::Frame;

pub struct CombFilter<const N: usize, const C: usize> {
    ringbufs: [VecDeque<f32>; C],
    feedback: f32,
}

impl<const N: usize, const C: usize> CombFilter<N, C>{
    pub fn new(delay_len: usize, feedback: f32) -> Self {
        let ringbufs = std::array::from_fn(|_| {
            let mut buf = VecDeque::with_capacity(delay_len);
            buf.extend(std::iter::repeat(0.0).take(delay_len));
            buf
        });
        Self { ringbufs, feedback }
    }

    #[inline(always)]
    fn tick(&mut self, chan: usize, input: f32) -> f32 {
        let buf = &mut self.ringbufs[chan];
        let out = buf.pop_front().unwrap();

        let float_with_feedback = input + self.feedback * out;
        buf.push_back(float_with_feedback);
        out
    }
}

impl<const N: usize, const C: usize> AudioNode<N, C> for CombFilter<N, C> {
    #[inline(always)]
    fn process(&mut self, inputs: &[Frame<N, C>], output: &mut Frame<N, C>) {
        if self.feedback >= 1.0 {
           self.feedback = 0.98;
        }
        let input = inputs[0];
        for n in 0..N {
            for c in 0..C {
                output[c][n] = self.tick(c, input[c][n]);
            }
        }
    }
}
