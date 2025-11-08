use crate::{
    engine::buffer::{Buffer, Frame},
    nodes::utils::ring::RingBuffer,
};
use generic_array::{sequence::GenericSequence, ArrayLength, GenericArray};
use typenum::U64;

/// A naive 2x rate adapter. Upsamples audio x2 coming in, and back
/// to audio rate on the way down.

// TODO: Polyphase and half band filters, SIMD

pub trait Resampler<const N: usize, const M: usize, C>
where
    C: ArrayLength,
{
    fn process_block(&mut self, ai: &Frame<N>, ao: &mut Frame<M>);
}

pub struct Upsample2x<C>
where
    C: ArrayLength,
{
    coeffs: Vec<f32>,
    state: GenericArray<RingBuffer, C>,
}

impl<C> Upsample2x<C>
where
    C: ArrayLength,
{
    pub fn new(coeffs: Vec<f32>) -> Self {
        let kernel_len = coeffs.len();
        Self {
            coeffs,
            state: GenericArray::generate(|_| RingBuffer::with_capacity(kernel_len)),
        }
    }
}

impl<const N: usize, const M: usize, C> Resampler<N, M, C> for Upsample2x<C>
where
    C: ArrayLength,
{
    fn process_block(&mut self, ai: &Frame<N>, ao: &mut Frame<M>) {
        debug_assert!(N * 2 == M); // Ensure that we have the correct
        debug_assert!(ai.len() == ao.len());

        // Zero insert to expand buffer, and just write to out
        for c in 0..C::USIZE {
            let input = ai[c];
            let out = &mut ao[c];
            for n in 0..N {
                out[2 * n] = input[n];
                out[(2 * n) + 1] = 0.0;
            }
        }

        // Now, out has a spectral image mirrored around the original nyquist

        // Naive FIR filter to remove spectral image
        for c in 0..C::USIZE {
            let channel_state = &mut self.state[c];

            let out = &mut ao[c];
            for x in out.iter_mut() {
                channel_state.push(*x);
                let mut y = 0.0;
                for (k, &h) in self.coeffs.iter().enumerate() {
                    y += h * channel_state.get(k);
                }
                *x = y;
            }
        }
    }
}

pub struct Downsample2x<const NX2: usize, C>
where
    C: ArrayLength,
{
    coeffs: Vec<f32>,
    state: GenericArray<RingBuffer, C>,
    filtered: GenericArray<Buffer<NX2>, C>,
}

impl<const NX2: usize, C> Downsample2x<NX2, C>
where
    C: ArrayLength,
{
    pub fn new(coeffs: Vec<f32>) -> Self {
        let kernel_len = coeffs.len();
        Self {
            coeffs,
            state: GenericArray::generate(|_| RingBuffer::with_capacity(kernel_len)),
            filtered: GenericArray::generate(|_| Buffer::SILENT),
        }
    }
}

impl<const NX2: usize, const M: usize, C> Resampler<NX2, M, C> for Downsample2x<NX2, C>
where
    C: ArrayLength,
{
    fn process_block(&mut self, ai: &Frame<NX2>, ao: &mut Frame<M>) {
        debug_assert!(NX2 / 2 == M); // Ensure that we have the correct
        debug_assert!(ai.len() == ao.len());

        // Naive FIR filter to remove frequencies above fs/4
        for c in 0..C::USIZE {
            let filter_state = &mut self.state[c];

            let input = ai[c];
            let out = &mut self.filtered[c];
            // I don't think the auto-vectorization gods can save me here
            for (n, &x) in input.iter().enumerate() {
                filter_state.push(x);
                let mut y = 0.0;
                for (k, &h) in self.coeffs.iter().enumerate() {
                    y += h * filter_state.get(k);
                }
                out[n] = y;
            }
        }

        // Decimate by 2
        for c in 0..C::USIZE {
            let input = self.filtered[c];
            let out = &mut ao[c];
            for m in 0..M {
                out[m] = input[m * 2]
            }
        }
    }
}
