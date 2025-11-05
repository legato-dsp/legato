


use generic_array::ArrayLength;
use typenum::{U0, U1, U16, U2, U4, U8};

use crate::{
    engine::{audio_context::AudioContext, buffer::Frame, node::Node, port::*, runtime::Runtime},
    nodes::utils::port_utils::{generate_audio_inputs, generate_audio_outputs},
};

pub struct Subgraph<const AF: usize, const CF: usize, C>
where
    C: ArrayLength,
{
    runtime: Runtime<AF, CF, C>,
    ports: Ports<C, C, U0, U0>,
}

impl<const AF: usize, const CF: usize, C> Subgraph<AF, CF, C>
where
    C: ArrayLength,
{
    pub fn new(runtime: Runtime<AF, CF, C>) -> Self {
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