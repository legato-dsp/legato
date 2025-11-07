use crate::engine::port::{AudioInputPort, AudioOutputPort, ControlInputPort, ControlOutputPort};
use crate::{
    engine::{
        audio_context::AudioContext,
        buffer::{Buffer, Frame},
        node::Node,
        port::{Ported, PortedErased},
        runtime::Runtime,
    },
    nodes::audio::resample::Resampler,
};
use generic_array::{sequence::GenericSequence, ArrayLength, GenericArray};

///  A 2X oversampler node for a subgraph. Note: Currently these
///  FIR filters are designed for 48k to 96k. You will need to design
///  your own coeffs for something more specific.
///
///  For now, Subgraph2xNode takes in a fixed C size for in and outputs.
///  This is because I want to use the graph to handle mixdowns more explicity.
///
///  Also, control is currently not resampled. This may be tweaked if there are issues.

pub struct Subgraph2xNode<const AF: usize, const SAF: usize, const CF: usize, C, Ci>
where
    C: ArrayLength,
    Ci: ArrayLength,
{
    runtime: Runtime<SAF, CF, C, Ci>,
    // Up and downsampler for oversampling
    upsampler: Box<dyn Resampler<AF, SAF, C>>,
    downsampler: Box<dyn Resampler<SAF, AF, C>>,
    // Work buffers
    upsampled: GenericArray<Buffer<SAF>, C>,
}

impl<const AF: usize, const SAF: usize, const CF: usize, C, Ci> Subgraph2xNode<AF, SAF, CF, C, Ci>
where
    C: ArrayLength,
    Ci: ArrayLength,
{
    pub fn new(
        runtime: Runtime<SAF, CF, C, Ci>,
        upsampler: Box<dyn Resampler<AF, SAF, C>>,
        downsampler: Box<dyn Resampler<SAF, AF, C>>,
    ) -> Self {
        debug_assert!(
            AF * 2 == SAF,
            "Must have 2X ratio between source and subgraph audio!"
        );
        Self {
            runtime,
            upsampler,
            downsampler,
            upsampled: GenericArray::generate(|_| Buffer::SILENT),
        }
    }
}

impl<const AF: usize, const SAF: usize, const CF: usize, C, Ci> Node<AF, CF>
    for Subgraph2xNode<AF, SAF, CF, C, Ci>
where
    C: ArrayLength,
    Ci: ArrayLength,
{
    fn process(
        &mut self,
        _: &mut AudioContext<AF>,
        ai: &Frame<AF>,
        ao: &mut Frame<AF>,
        ci: &Frame<CF>,
        _: &mut Frame<CF>,
    ) {
        debug_assert!(ai.len() == C::USIZE);
        debug_assert!(ao.len() == C::USIZE);

        // Upsample to work buffer
        self.upsampler.process_block(ai, &mut self.upsampled);
        // Process next subgraph block
        let block = self.runtime.next_block(Some((&self.upsampled, ci)));
        // Downsample and write out
        self.downsampler.process_block(block, ao);
    }
}

impl<const AF: usize, const SAF: usize, const CF: usize, C, Ci> PortedErased
    for Subgraph2xNode<AF, SAF, CF, C, Ci>
where
    C: ArrayLength,
    Ci: ArrayLength,
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
