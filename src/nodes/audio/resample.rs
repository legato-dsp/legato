use crate::{
    engine::buffer::{Buffer, Frame},
    nodes::utils::ring::RingBuffer,
};
use generic_array::{sequence::GenericSequence, ArrayLength, GenericArray};
use typenum::U64;

// 64 tap remez exchange FIR filter that may be decent for 2x oversampling
const KERNEL: GenericArray<f32, U64> = GenericArray::from_array([
    0.003_933_759,
    -0.011_818_053,
    0.002_154_722_3,
    -0.005_534_518_5,
    0.003_771_703_7,
    -0.002_953_569_9,
    0.001_023_558_4,
    0.001_198_530_2,
    -0.003_788_925_5,
    0.006_412_682,
    -0.008_801_702,
    0.010_626_581_5,
    -0.011_581_174,
    0.011_388_29,
    -0.009_857_22,
    0.006_911_201_4,
    -0.002_581_814_3,
    -0.002_904_066_1,
    0.009_205_313,
    -0.015_809_234,
    0.022_087_993,
    -0.027_300_153,
    0.030_679_975,
    -0.031_397_417,
    0.028_610_898,
    -0.021_431_202,
    0.008_795_602,
    0.010_915_615,
    -0.040_906_29,
    0.089_592_04,
    -0.188_738_03,
    0.628_654_1,
    0.628_654_1,
    -0.188_738_03,
    0.089_592_04,
    -0.040_906_29,
    0.010_915_615,
    0.008_795_602,
    -0.021_431_202,
    0.028_610_898,
    -0.031_397_417,
    0.030_679_975,
    -0.027_300_153,
    0.022_087_993,
    -0.015_809_234,
    0.009_205_313,
    -0.002_904_066_1,
    -0.002_581_814_3,
    0.006_911_201_4,
    -0.009_857_22,
    0.011_388_29,
    -0.011_581_174,
    0.010_626_581_5,
    -0.008_801_702,
    0.006_412_682,
    -0.003_788_925_5,
    0.001_198_530_2,
    0.001_023_558_4,
    -0.002_953_569_9,
    0.003_771_703_7,
    -0.005_534_518_5,
    0.002_154_722_3,
    -0.011_818_053,
    0.003_933_759,
]);

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

impl<const N: usize, const M: usize, C> Resampler<N, M, C> for Upsample2x<C>
where
    C: ArrayLength,
{
    fn process_block(&mut self, ai: &Frame<N>, ao: &mut Frame<M>) {
        debug_assert!(N * 2 == M); // Ensure that we have the correct
        debug_assert!(ai.len() == ao.len() * 2);

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

pub struct Downsample2X<const NX2: usize, C>
where
    C: ArrayLength,
{
    coeffs: Vec<f32>,
    state: GenericArray<RingBuffer, C>,
    filtered: GenericArray<Buffer<NX2>, C>,
}

impl<const NX2: usize, C> Downsample2X<NX2, C>
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

impl<const NX2: usize, const M: usize, C> Resampler<NX2, M, C> for Downsample2X<NX2, C>
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
