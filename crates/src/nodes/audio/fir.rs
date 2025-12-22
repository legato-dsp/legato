use std::simd::{StdFloat, num::SimdFloat};

use crate::{
    context::AudioContext,
    node::{Channels, Inputs, Node},
    ports::{PortBuilder, Ports},
    ring::RingBuffer,
    simd::{LANES, Vf32},
};

// A semi-naitve FIR filter. In the future, it will be
// nice to have one in the frequency domain as well.
//
// It's also worth noting that keeping the chunks and state
// in an interleaved format could potentially be faster?
#[derive(Clone)]
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
    fn process(&mut self, _: &mut AudioContext, ai: &Inputs, ao: &mut Channels) {
        // These checks are important because we are using this elsewhere for oversampling
        if let Some(inner) = ai[0] {
            // Channel alignment
            debug_assert_eq!(ai.len(), ao.len());
            // Block alignment
            debug_assert_eq!(inner.len(), inner.len());
        }

        for ((chan_in, out), state) in ai.iter().zip(ao.iter_mut()).zip(self.state.iter_mut()) {
            if let Some(input) = chan_in {
                for (n, x) in input.iter().enumerate() {
                    state.push(*x);

                    let mut y = Vf32::splat(0.0);

                    for (k, h) in self.coeffs.chunks_exact(LANES).enumerate() {
                        let a = Vf32::from_slice(h);
                        let b = state.get_chunk_by_offset(k * LANES);
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
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

#[cfg(test)]
mod test {
    use std::simd::{StdFloat, num::SimdFloat};

    use crate::{
        nodes::audio::fir::FirFilter,
        simd::{LANES, Vf32},
    };

    impl FirFilter {
        fn process_mono_block(&mut self, input: &[f32]) -> Vec<f32> {
            assert_eq!(self.state.len(), 1);

            let mut out = vec![0.0; input.len()];
            let state = &mut self.state[0];

            for (n, x) in input.iter().enumerate() {
                state.push(*x);

                let mut y = Vf32::splat(0.0);

                for (chunk_idx, h_chunk) in self.coeffs.chunks_exact(LANES).enumerate() {
                    let a = Vf32::from_slice(h_chunk);
                    let b = state.get_chunk_by_offset(chunk_idx * LANES);
                    y = a.mul_add(b, y);
                }

                let start = self.coeffs.chunks_exact(LANES).len() * LANES;

                let mut scalar = y.reduce_sum();
                for (k, h) in self.coeffs[start..].iter().enumerate() {
                    scalar += h * state.get_offset(k + start);
                }

                out[n] = scalar;
            }

            out
        }
    }

    fn fir_scalar(coeffs: &[f32], input: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0; input.len()];
        for n in 0..input.len() {
            let mut acc = 0.0;
            for (k, &h) in coeffs.iter().enumerate() {
                if n >= k {
                    acc += h * input[n - k];
                }
            }
            out[n] = acc;
        }
        out
    }

    #[test]
    fn fir_impulse_response_matches_coeffs() {
        // With an impulse of one at the start, we should match the coeffs
        let coeffs = vec![
            0.1, -0.2, 0.3, -0.4, 0.5, -0.6, 0.7, 1.3, 0.3, 0.19, 1.9, 0.6, 7.4,
        ];

        let mut fir = FirFilter::new(coeffs.clone(), 1);

        let len = coeffs.len() + 4;
        let mut input = vec![0.0; len];
        input[0] = 1.0;

        let out = fir.process_mono_block(&input);

        for (i, &h) in coeffs.iter().enumerate() {
            assert!(
                (out[i] - h).abs() < 1e-6,
                "Impulse response mismatch at {}: got {}, expected {}",
                i,
                out[i],
                h
            );
        }
    }

    #[test]
    fn fir_matches_scalar_reference_for_random_signal() {
        use rand::rngs::StdRng;
        use rand::{Rng, SeedableRng};

        let mut rng = StdRng::seed_from_u64(1337);

        let coeffs_len = LANES * 2 + 3; // Awkward number to test wrapping and edge cases
        let mut coeffs = Vec::with_capacity(coeffs_len);
        for _ in 0..coeffs_len {
            coeffs.push(rng.random_range(-1.0..1.0));
        }

        let input_len = 256;
        let mut input = Vec::with_capacity(input_len);
        for _ in 0..input_len {
            input.push(rng.random_range(-1.0..1.0));
        }

        let ref_out = fir_scalar(&coeffs, &input);

        let mut fir = FirFilter::new(coeffs.clone(), 1);
        let simd_out = fir.process_mono_block(&input);

        assert_eq!(ref_out.len(), simd_out.len());

        for (n, (a, b)) in ref_out.iter().zip(simd_out.iter()).enumerate() {
            let diff = (a - b).abs();
            assert!(
                diff < 1e-5,
                "Output mismatch at sample {}: scalar={}, simd={}, diff={}",
                n,
                a,
                b,
                diff
            );
        }
    }
}
