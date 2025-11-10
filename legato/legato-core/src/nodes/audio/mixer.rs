use generic_array::ArrayLength;
use typenum::{U0, U1, U16, U2, U4, U8};

use crate::{
    engine::{
        audio_context::AudioContext,
        buffer::Frame,
        node::{FrameSize, Node},
        port::*,
    },
    nodes::utils::port_utils::{generate_audio_inputs, generate_audio_outputs},
};

pub struct Mixer<Ai, Ao>
where
    Ai: ArrayLength,
    Ao: ArrayLength,
{
    ports: Ports<Ai, Ao, U0, U0>,
}

impl<Ai, Ao> Mixer<Ai, Ao>
where
    Ai: ArrayLength,
    Ao: ArrayLength,
{
    pub fn default() -> Self {
        Self {
            ports: Ports {
                audio_inputs: Some(generate_audio_inputs()),
                audio_outputs: Some(generate_audio_outputs()),
                control_inputs: None,
                control_outputs: None,
            },
        }
    }
}

impl<AF, CF, Ai, Ao> Node<AF, CF> for Mixer<Ai, Ao>
where
    AF: FrameSize,
    CF: FrameSize,
    Ai: ArrayLength,
    Ao: ArrayLength,
{
    fn process(
        &mut self,
        _: &mut AudioContext<AF>,
        ai: &Frame<AF>,
        ao: &mut Frame<AF>,
        _: &Frame<CF>,
        _: &mut Frame<CF>,
    ) {
        // For instance, we can have a stereo mixer with 2 stereo tracks.
        // This would then be mapped like so [[L][R][L][R]].
        // We sum them all up to the desired outputs.
        debug_assert_eq!(ai.len(), Ai::USIZE);
        debug_assert_eq!(ao.len(), Ao::USIZE);

        let tracks = Ai::USIZE / Ao::USIZE;
        let divisor = (tracks as f32).sqrt();

        for buffer in ao.iter_mut() {
            buffer.fill(0.0);
        }

        for n in 0..AF::USIZE {
            for c in 0..Ai::USIZE {
                let index = c % Ao::USIZE;
                ao[index][n] += ai[c][n] / divisor;
            }
        }
    }
}

impl<Ai, Ao> PortedErased for Mixer<Ai, Ao>
where
    Ai: ArrayLength,
    Ao: ArrayLength,
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

pub type StereoMixer = Mixer<U2, U2>;
pub type StereoToMonoMixer = Mixer<U2, U1>;
pub type FourToMonoMixer = Mixer<U4, U1>;

pub type TwoTrackStereoMixer = Mixer<U4, U2>;
pub type FourTrackStereoMixer = Mixer<U8, U2>;
pub type EightTrackStereoMixer = Mixer<U16, U2>;

pub type TwoTrackMonoMixer = Mixer<U2, U1>;
