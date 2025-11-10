use crate::{
    engine::{
        buffer::{Buffer, Frame},
        node::FrameSize,
    },
    nodes::utils::ring::RingBuffer,
};
use generic_array::{sequence::GenericSequence, ArrayLength, GenericArray};
use std::ops::Mul;
use typenum::{Prod, U2};

/// A naive 2x rate adapter. Upsamples audio x2 coming in, and back
/// to audio rate on the way down.

// TODO: Polyphase and half band filters, SIMD

pub trait Resampler<N, M, C>
where
    N: FrameSize,
    M: FrameSize,
{
    fn process_block(&mut self, ai: &Frame<N>, ao: &mut Frame<M>);
}

pub struct Upsample2x<N, C>
where
    N: FrameSize + Mul<U2>,
    Prod<N, U2>: FrameSize,
    C: ArrayLength,
{
    coeffs: Vec<f32>,
    zero_stuffed: GenericArray<Buffer<Prod<N, U2>>, C>,
    state: GenericArray<RingBuffer, C>,
}

impl<N, C> Upsample2x<N, C>
where
    N: FrameSize + Mul<U2>,
    Prod<N, U2>: FrameSize,
    C: ArrayLength,
{
    pub fn new(coeffs: Vec<f32>) -> Self {
        let kernel_len = coeffs.len();
        Self {
            coeffs,
            zero_stuffed: GenericArray::generate(|_| Buffer::silent()),
            state: GenericArray::generate(|_| RingBuffer::with_capacity(kernel_len)),
        }
    }
}

impl<N, C> Resampler<N, Prod<N, U2>, C> for Upsample2x<N, C>
where
    N: FrameSize + Mul<U2>,
    Prod<N, U2>: FrameSize,
    C: ArrayLength,
{
    fn process_block(&mut self, ai: &Frame<N>, ao: &mut Frame<Prod<N, U2>>) {
        debug_assert!(ai.len() == ao.len());

        // Zero insert to expand buffer, and just write to out
        for c in 0..C::USIZE {
            let input = &ai[c];
            let out = &mut self.zero_stuffed[c];
            for n in 0..N::USIZE {
                out[2 * n] = input[n];
                out[(2 * n) + 1] = 0.0;
            }
        }

        // Now, out has a spectral image mirrored around the original nyquist

        // Naive FIR filter to remove spectral image
        for c in 0..C::USIZE {
            let channel_state = &mut self.state[c];

            let zero_in = &self.zero_stuffed[c];
            let out = &mut ao[c];

            for (i, x) in zero_in.iter().enumerate() {
                channel_state.push(*x);
                let mut y = 0.0;
                for (k, &h) in self.coeffs.iter().enumerate() {
                    y += h * channel_state.get(k);
                }
                out[i] = y;
            }
        }
    }
}

pub struct Downsample2x<N, C>
where
    N: FrameSize + Mul<U2>,
    Prod<N, U2>: FrameSize,
    C: ArrayLength,
{
    coeffs: Vec<f32>,
    state: GenericArray<RingBuffer, C>,
    filtered: GenericArray<Buffer<Prod<N, U2>>, C>,
}

impl<N, C> Downsample2x<N, C>
where
    N: FrameSize + Mul<U2>,
    Prod<N, U2>: FrameSize,
    C: ArrayLength,
{
    pub fn new(coeffs: Vec<f32>) -> Self {
        let kernel_len = coeffs.len();
        Self {
            coeffs,
            state: GenericArray::generate(|_| RingBuffer::with_capacity(kernel_len)),
            filtered: GenericArray::generate(|_| Buffer::silent()),
        }
    }
}

/// Downsampler, it's worth noting that N is the traditional audio rate
impl<N, C> Resampler<Prod<N, U2>, N, C> for Downsample2x<N, C>
where
    N: FrameSize + Mul<U2>,
    Prod<N, U2>: FrameSize,
    C: ArrayLength,
{
    fn process_block(&mut self, ai: &Frame<Prod<N, U2>>, ao: &mut Frame<N>) {
        debug_assert!(ai.len() == ao.len());

        // Naive FIR filter to remove frequencies above fs/4
        for c in 0..C::USIZE {
            let filter_state = &mut self.state[c];

            let input = &ai[c];
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
            let input = &self.filtered[c];
            let out = &mut ao[c];
            for (m, o) in out.iter_mut().enumerate() {
                *o = input[m * 2];
            }
        }
    }
}
