use std::simd::{StdFloat, num::SimdFloat};

use crate::{
    nodes::{
        Node, NodeInputs,
        ports::{PortBuilder, Ported, Ports},
    },
    runtime::{
        context::AudioContext,
        lanes::{LANES, Vf32},
    },
    utils::ringbuffer::RingBuffer,
};

// A semi-naitve FIR filter. In the future, it will be
// nice to have one in the frequency domain as well.
//
// It's also worth noting that keeping the chunks and state
// in an interleaved format could potentially be faster?
pub struct FirFilter {
    coeffs: Vec<f32>,
    state: Vec<RingBuffer>,
    ports: Ports,
}

impl FirFilter {
    pub fn new(coeffs: Vec<f32>, chans: usize) -> Self {
        let coeffs_len = coeffs.len();
        Self {
            coeffs,
            state: vec![RingBuffer::new(coeffs_len); chans],
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_out(chans)
                .build(),
        }
    }
}

impl Node for FirFilter {
    fn process(
        &mut self,
        _: &mut AudioContext,
        ai: &NodeInputs,
        ao: &mut NodeInputs,
        _: &NodeInputs,
        _: &mut NodeInputs,
    ) {
        // These checks are important because we are using this elsewhere for oversampling
        debug_assert_eq!(ai.len(), ao.len());
        debug_assert_eq!(ai[0].len(), ao[0].len());

        for ((input, out), state) in ai.iter().zip(ao.iter_mut()).zip(self.state.iter_mut()) {
            for (n, x) in input.iter().enumerate() {
                state.push(*x);

                let mut y = Vf32::splat(0.0);

                for (k, h) in self.coeffs.chunks_exact(LANES).enumerate() {
                    let a = Vf32::from_slice(&h);
                    let b = state.get_chunk_simd(k * LANES);
                    y = a.mul_add(b, y)
                }

                let start = self.coeffs.chunks_exact(LANES).len() * LANES;

                let mut scalar = y.reduce_sum();

                for (k, h) in self.coeffs[start..].iter().enumerate() {
                    scalar += h * state.get_offset(k + start);
                }

                out[n] = scalar;
            }
        }
    }
}

impl Ported for FirFilter {
    fn get_ports(&self) -> &Ports {
        &self.ports
    }
}
