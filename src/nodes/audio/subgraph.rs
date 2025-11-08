use std::marker::PhantomData;

use crate::engine::port::{AudioInputPort, AudioOutputPort, ControlInputPort, ControlOutputPort};
use crate::engine::runtime::{Runtime, RuntimeErased};
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
use generic_array::{sequence::GenericSequence, ArrayLength, GenericArray};
use typenum::U64;

///  A 2X oversampler node for a subgraph. Note: Currently these
///  FIR filters are designed for 48k to 96k. You will need to design
///  your own coeffs for something more specific.
///
///  For now, Subgraph2xNode takes in a fixed C size for in and outputs.
///  This is because I want to use the graph to handle mixdowns more explicity.
///
///  Also, control is currently not resampled. This may be tweaked if there are issues.

pub struct Oversample2X<const AF: usize, const SAF: usize, const CF: usize, C>
where
    C: ArrayLength,
{
    runtime: Box<dyn RuntimeErased<SAF, CF> + Send + 'static>,
    // Up and downsampler for oversampling
    upsampler: Box<dyn Resampler<AF, SAF, C> + Send + 'static>,
    downsampler: Box<dyn Resampler<SAF, AF, C> + Send + 'static>,
    // Work buffers
    upsampled_ai: GenericArray<Buffer<SAF>, C>,
}

impl<const AF: usize, const SAF: usize, const CF: usize, C> Oversample2X<AF, SAF, CF, C>
where
    C: ArrayLength,
{
    pub fn new(
        runtime: Box<dyn RuntimeErased<SAF, CF> + Send + 'static>,
        upsampler: Box<dyn Resampler<AF, SAF, C> + Send + 'static>,
        downsampler: Box<dyn Resampler<SAF, AF, C> + Send + 'static>,
    ) -> Self {
        debug_assert!(
            AF * 2 == SAF,
            "Must have 2X ratio between source and subgraph audio!"
        );
        Self {
            runtime,
            upsampler,
            downsampler,
            upsampled_ai: GenericArray::generate(|_| Buffer::SILENT),
        }
    }
}

impl<const AF: usize, const SAF: usize, const CF: usize, C> Node<AF, CF>
    for Oversample2X<AF, SAF, CF, C>
where
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
        debug_assert!(ai.len() == C::USIZE);
        debug_assert!(ao.len() == C::USIZE);

        // Upsample to work buffer
        self.upsampler.process_block(ai, &mut self.upsampled_ai);
        // Process next subgraph block
        let res = self
            .runtime
            .next_block(Some((self.upsampled_ai.as_slice(), ci)));
        // Downsample and write out
        self.downsampler.process_block(&res, ao);
    }
}

impl<const AF: usize, const SAF: usize, const CF: usize, C> PortedErased
    for Oversample2X<AF, SAF, CF, C>
where
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

pub fn build_2x_oversample<const AF: usize, const SAF: usize, const CF: usize, C, Ci>(
    runtime: Box<dyn RuntimeErased<SAF, CF> + Send + 'static>,
) -> Box<dyn Node<AF, CF> + Send + 'static>
where
    C: ArrayLength,
    Ci: ArrayLength,
{
    // Reuse the same prototype kernel for both directions (simple but fine to start).
    let upsampler = Box::new(Upsample2x::<C>::new(coeffs.to_vec()));
    let downsampler = Box::new(Downsample2x::<SAF, C>::new(coeffs.to_vec()));

    Box::new(Oversample2X::<AF, SAF, CF, C>::new(
        runtime,
        upsampler,
        downsampler,
    ))
}

// 64 tap remez exchange FIR filter that may be decent for 2x oversampling
const coeffs: GenericArray<f32, U64> = GenericArray::from_array([
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
