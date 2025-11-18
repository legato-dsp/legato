use std::ops::Mul;

use crate::engine::node::FrameSize;
use crate::engine::port::{AudioInputPort, AudioOutputPort, ControlInputPort, ControlOutputPort};
use crate::engine::runtime::RuntimeErased;
use crate::nodes::audio::resample::{Downsample2x, Upsample2x};
use crate::{
    engine::{
        audio_context::AudioContext,
        buffer::{Buffer, Frame},
        node::Node,
        port::PortedErased,
    },
    nodes::audio::resample::Resampler,
};
use generic_array::{ArrayLength, GenericArray, sequence::GenericSequence};
use typenum::{Prod, U2};

// Maybe I should not have been so harsh on C++ templates...
// I have stared into the abyss, and the abyss said back "<<AF as Mul<UInt<UInt<UTerm, B1>, B0>>>::Output as PartialDiv<UInt<UInt<UTerm, B1>, B0>>>::Output"

///  A 2X oversampler node for a subgraph. Note: Currently these
///  FIR filters are designed for 48k to 96k. You will need to design
///  your own coeffs for something more specific.
///
///  For now, Subgraph2xNode takes in a fixed C size for in and outputs.
///  This is because I want to use the graph to handle mixdowns more explicity.
///
///  Also, control is currently not resampled. This may be tweaked if there are issues.
pub struct Oversample2X<AF, CF, C>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
{
    runtime: Box<dyn RuntimeErased<Prod<AF, U2>, CF> + Send + 'static>,
    // Up and downsampler for oversampling
    upsampler: Upsample2x<AF, C>,
    downsampler: Downsample2x<AF, C>,
    // Work buffers
    upsampled_ai: GenericArray<Buffer<Prod<AF, U2>>, C>,
}

impl<AF, CF, C> Oversample2X<AF, CF, C>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
{
    pub fn new(runtime: Box<dyn RuntimeErased<Prod<AF, U2>, CF> + Send + 'static>) -> Self {
        Self {
            runtime,
            upsampler: Upsample2x::new(CUTOFF_24K_COEFFS_FOR_96K.to_vec()),
            downsampler: Downsample2x::new(CUTOFF_24K_COEFFS_FOR_96K.to_vec()), // TODO: Fine tune these filters
            upsampled_ai: GenericArray::generate(|_| Buffer::silent()),
        }
    }
}

impl<AF, CF, C> Node<AF, CF> for Oversample2X<AF, CF, C>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
{
    fn process(
        &mut self,
        _: &mut AudioContext<AF>,
        ai: &Frame<AF>,
        ao: &mut Frame<AF>,
        ci: &Frame<CF>,
        co: &mut Frame<CF>,
    ) {
        // Upsample inputs
        self.upsampler.process_block(ai, &mut self.upsampled_ai);

        let upsampled_slice = self.upsampled_ai.as_slice();
        // Process next subgraph block
        let res: &Frame<Prod<AF, U2>> = self.runtime.next_block(Some((upsampled_slice, ci)));
        // Downsample and write out
        self.downsampler.process_block(res, ao);
    }
}

impl<AF, CF, C> PortedErased for Oversample2X<AF, CF, C>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
{
    fn get_audio_inputs(&self) -> Option<&[AudioInputPort]> {
        self.runtime.get_audio_inputs()
    }
    fn get_audio_outputs(&self) -> Option<&[AudioOutputPort]> {
        self.runtime.get_audio_outputs()
    }
    fn get_control_inputs(&self) -> Option<&[ControlInputPort]> {
        self.runtime.get_control_inputs()
    }
    fn get_control_outputs(&self) -> Option<&[ControlOutputPort]> {
        self.runtime.get_control_outputs()
    }
}

const CUTOFF_24K_COEFFS_FOR_96K: [f32; 64] = [
    -0.00078997,
    -0.00106131,
    0.00019139,
    0.00186628,
    0.00118124,
    -0.00154504,
    -0.00188737,
    0.00179210,
    0.00386756,
    -0.00041068,
    -0.00518644,
    -0.00144159,
    0.00656960,
    0.00490158,
    -0.00646231,
    -0.00899469,
    0.00486494,
    0.01385281,
    -0.00056869,
    -0.01820437,
    -0.00660587,
    0.02125839,
    0.01747785,
    -0.02119668,
    -0.03247355,
    0.01592738,
    0.05352988,
    -0.00054194,
    -0.08745912,
    -0.04247219,
    0.18323183,
    0.40859913,
    0.40859913,
    0.18323183,
    -0.04247219,
    -0.08745912,
    -0.00054194,
    0.05352988,
    0.01592738,
    -0.03247355,
    -0.02119668,
    0.01747785,
    0.02125839,
    -0.00660587,
    -0.01820437,
    -0.00056869,
    0.01385281,
    0.00486494,
    -0.00899469,
    -0.00646231,
    0.00490158,
    0.00656960,
    -0.00144159,
    -0.00518644,
    -0.00041068,
    0.00386756,
    0.00179210,
    -0.00188737,
    -0.00154504,
    0.00118124,
    0.00186628,
    0.00019139,
    -0.00106131,
    -0.00078997,
];
