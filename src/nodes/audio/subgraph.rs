


use generic_array::{ArrayLength, GenericArray};
use typenum::{U0, U1, U16, U2, U4, U8};

use crate::{
    engine::{audio_context::AudioContext, buffer::Frame, node::Node, port::*, runtime::Runtime},
    nodes::utils::{port_utils::{generate_audio_inputs, generate_audio_outputs}, ring::RingBuffer},
};

/// Establishing a trait for a resampler for a subgraph. This contains logic for 
/// both zero stuffing and filtering, as well as filtering + decimation. In the 
/// future, a more efficient structure, like a halfband or polyphase filter 
/// may be required, but at this time I have not built much intuition on these
/// topics. 
/// 
/// Also, for the time being, I am just writing 2X over and downsamplers,
/// I will make it so that you can atleast pass in FIR coeffs if you 
/// want to fine tune your resampling logic.
trait Resampler<const AF: usize, const SF: usize> {
    fn upsample(&self, ain: &Frame<AF>, aout: &mut Frame<SF>);
    fn downsample(&self, ain: &Frame<AF>, aout: &mut Frame<SF>);
}

struct Oversample2X<C> where C: ArrayLength {
    coeffs: Vec<f32>,
    buffers: GenericArray<RingBuffer, C>>,
}
impl<C> Oversample2X<C> where C: ArrayLength {
    fn new(coeffs: Vec<f32>) -> Self {
        Self {
            coeffs
        }
    }
}

impl Resampler for Oversample2X {
}


// TODO: Not currently thinking about how we would say move from
// 5 -> 2 channel audio. I am currently more concerned with changing
// sample rates.

/// Subgraph node that allows for changing sample rates. In the future,
/// this will also take care of channels, but, I think I want to leave
/// that in control of the user created graph for the time being. 
/// 
/// It's important to note that we need two sets of constants here,
/// we need block sizes for the subgraph runtime ()
pub struct Subgraph<const AF: usize, const CF: usize, const SAF: usize, const SCF: usize, Ai, Ao>
where
    Ai: ArrayLength,
    Ao: ArrayLength
{
    runtime: Runtime<AF, CF, Ao>, // Assume that our runtime gives us Ao::USIZE 
    ports: Ports<Ai, Ao, U0, U0>, // Maybe in the future we expose control as well
    upsample: Option<Box<dyn Resampler<AF, SAF>>>,
    downsample: Option<Box<dyn Resampler<SAF, AF>>>,
}

impl<const AF: usize, const CF: usize, const SAF: usize, const SCF: usize, Ai, Ao> Subgraph<AF, CF, SAF, SCF, Ai, Ao>
where
    Ai: ArrayLength,
    Ao: ArrayLength
{
    pub fn new(runtime: Runtime<AF, CF, Ao>) -> Self {
        Self {
            runtime,
            ports: Ports {
                audio_inputs: Some(generate_audio_inputs()),
                audio_outputs: Some(generate_audio_outputs()),
                control_inputs: None,
                control_outputs: None,
            },
        }
    }
}

impl<const AF: usize, const CF: usize, C> Node<AF, CF> for Subgraph<AF, CF, C>
where
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
        debug_assert_eq!(ai.len(), C::USIZE);
        debug_assert_eq!(ao.len(), C::USIZE);


        
    }
}

impl<const AF: usize, const CF: usize, C> PortedErased for Subgraph<AF, CF, C>
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