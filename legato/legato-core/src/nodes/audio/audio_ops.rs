use generic_array::ArrayLength;
use typenum::{U0, U1};

use crate::{
    engine::{
        audio_context::AudioContext,
        buffer::Frame,
        node::{FrameSize, Node},
        port::{Mono, PortedErased, Ports, Stereo},
    },
    nodes::utils::port_utils::{generate_audio_inputs, generate_audio_outputs},
};

pub struct ApplyOp<C>
where
    C: ArrayLength,
{
    op: fn(f32, f32) -> f32,
    b: f32, // if we have an input of a, we apply op (a, b). So an input of 1.0 with a val of 0.8 with mult -> 0.8
    ports: Ports<C, C, U1, U0>,
}

impl<Ao> ApplyOp<Ao>
where
    Ao: ArrayLength,
{
    pub fn new(op: fn(f32, f32) -> f32, b: f32) -> Self {
        Self {
            op,
            b,
            ports: Ports {
                audio_inputs: Some(generate_audio_inputs()),
                audio_outputs: Some(generate_audio_outputs()),
                control_inputs: None,
                control_outputs: None,
            },
        }
    }
}

impl<AF, CF, C> Node<AF, CF> for ApplyOp<C>
where
    AF: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
{
    fn process(
        &mut self,
        ctx: &mut AudioContext<AF>,
        ai: &Frame<AF>,
        ao: &mut Frame<AF>,
        ci: &Frame<CF>,
        co: &mut Frame<CF>,
    ) {
        debug_assert_eq!(C::USIZE, ai.len());
        debug_assert_eq!(C::USIZE, ao.len());

        // TODO: Control!

        for n in 0..AF::USIZE {
            for c in 0..C::USIZE {
                let output = (self.op)(ai[c][n], self.b);
                ao[c][n] = output;
            }
        }
    }
}

impl<Ao> PortedErased for ApplyOp<Ao>
where
    Ao: ArrayLength,
{
    fn get_audio_inputs(&self) -> Option<&[crate::engine::port::AudioInputPort]> {
        self.ports.get_audio_inputs()
    }
    fn get_audio_outputs(&self) -> Option<&[crate::engine::port::AudioOutputPort]> {
        self.ports.get_audio_outputs()
    }
    fn get_control_inputs(&self) -> Option<&[crate::engine::port::ControlInputPort]> {
        self.ports.get_control_inputs()
    }
    fn get_control_outputs(&self) -> Option<&[crate::engine::port::ControlOutputPort]> {
        self.ports.get_control_outputs()
    }
}

pub type ApplyOpMono = ApplyOp<Mono>;
pub type ApplyOpStereo = ApplyOp<Stereo>;
