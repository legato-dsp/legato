use generic_array::{sequence::GenericSequence, ArrayLength, GenericArray};
use typenum::{U0, U1, U2};

use crate::{
    engine::{
        audio_context::AudioContext,
        buffer::Frame,
        node::{FrameSize, Node},
        port::*,
    },
    nodes::utils::{
        port_utils::{generate_audio_inputs, generate_audio_outputs},
        ring::RingBuffer,
    },
};

/// A naive FIR filter implementation.
///
/// TODO: A ring buffer and per sample is convenient,
/// but this would likely have much better performance
/// if we rewrite this as a fixed size down the line, for
/// auto-vectorization, and maybe eventually
/// some SIMD/intrinsics.
///
/// My "dream DX", is something like a ringbuffer that we can use
/// in multiple places, that gives packed SIMD values,
/// and an optional remainder as well. Bonus points for doing
/// SIMD linear interp, hermite interp, etc.
///
/// We can probably easily use FMA operations here as well down the line,
/// but I will take a peak at this once the functionality is in place.
///
/// It's also worth noting that this operation in the
/// time domain is O(n * m).
///
/// This is only a good approach with small kernels,
/// for larger kernels, the operation is better in the frequency
/// domain, which we might implement later. But, doing FFT on the
/// input has a larger overhead until we reach fairly large kernel sizes.
///
/// If you have a kernel with 100+ taps, you should start considering
/// a frequency domain implementation, which we will hopefully get to soon!
///
/// However, for really large kernels i.e convolution hall reverb,
/// there is a bit of complixity with the overlapping and
/// partioning logic required. I would appreciate some help
/// designing a generalized solution for the above frequency domain
/// problem. From my understanding, its something along the lines
/// of splitting a long impulse response into a bunch of smaller power of 2
/// chunks, and having a spectral delay line that then synchronizes these.
///
/// For designing FIR filters, I have really been enjoying Numpy/SciPy.
/// When you use the UV manager suddeny I don't mind working with Python again.

pub struct FirFilter<C>
where
    C: ArrayLength,
{
    coeffs: Vec<f32>,
    state: GenericArray<RingBuffer, C>,
    ports: Ports<C, C, U0, U0>,
}

impl<C> FirFilter<C>
where
    C: ArrayLength,
{
    pub fn new(coeffs: Vec<f32>) -> Self {
        let length = coeffs.len();
        Self {
            coeffs,
            state: GenericArray::generate(|_| RingBuffer::with_capacity(length)),
            ports: Ports {
                audio_inputs: Some(generate_audio_inputs()),
                audio_outputs: Some(generate_audio_outputs()),
                control_inputs: None,
                control_outputs: None,
            },
        }
    }
}

impl<C, AF, CF> Node<AF, CF> for FirFilter<C>
where
    AF: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
{
    fn process(
        &mut self,
        _: &mut AudioContext<AF>,
        ai: &Frame<AF>,
        ao: &mut Frame<AF>,
        _: &Frame<CF>,
        _: &mut Frame<CF>,
    ) {
        for c in 0..C::USIZE {
            let channel_state = &mut self.state[c];

            let input = &ai[c];
            let out = &mut ao[c];
            // I don't think the auto-vectorization gods can save me here
            for (n, x) in input.iter().enumerate() {
                channel_state.push(*x);
                let mut y = 0.0;
                for (k, &h) in self.coeffs.iter().enumerate() {
                    y += h * channel_state.get(k);
                }
                out[n] = y;
            }
        }
    }
}

impl<C> PortedErased for FirFilter<C>
where
    C: ArrayLength,
{
    fn get_audio_inputs(&self) -> Option<&[AudioInputPort]> {
        self.ports.get_audio_inputs()
    }
    fn get_audio_outputs(&self) -> Option<&[AudioOutputPort]> {
        self.ports.get_audio_outputs()
    }
    fn get_control_inputs(&self) -> Option<&[ControlInputPort]> {
        self.ports.get_control_inputs()
    }
    fn get_control_outputs(&self) -> Option<&[ControlOutputPort]> {
        self.ports.get_control_outputs()
    }
}

pub type FirMono = FirFilter<U1>;
pub type FirStereo = FirFilter<U2>;
