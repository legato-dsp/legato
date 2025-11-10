use generic_array::{arr, ArrayLength, GenericArray};
use typenum::{Unsigned, U0, U1};

use crate::engine::audio_context::AudioContext;
use crate::engine::node::{FrameSize, Node};
use crate::engine::port::*;
use crate::nodes::utils::port_utils::generate_audio_outputs;

pub struct Sine<Ai, Ao, Ci, Co>
where
    Ai: ArrayLength,
    Ao: ArrayLength,
    Ci: ArrayLength,
    Co: ArrayLength,
{
    freq: f32,
    phase: f32,
    ports: Ports<Ai, Ao, Ci, Co>,
}

// Note, Ao is still generic here, letting us make multiple sizes!
impl<Ao> Sine<U1, Ao, U0, U0>
where
    Ao: ArrayLength,
{
    pub fn new(freq: f32, phase: f32) -> Self {
        // FM is audio rate, frequency
        let audio_inputs = arr![AudioInputPort {
            meta: PortMeta {
                name: "fm",
                index: 0
            },
        },];

        let audio_outputs: GenericArray<AudioOutputPort, Ao> = generate_audio_outputs::<Ao>();
        let ports = Ports {
            audio_inputs: Some(audio_inputs),
            audio_outputs: Some(audio_outputs),
            control_inputs: None,
            control_outputs: None,
        };

        Self { freq, phase, ports }
    }
}

impl<AF, CF, Ao> Node<AF, CF> for Sine<U1, Ao, U0, U0>
where
    AF: FrameSize,
    CF: FrameSize,
    Ao: ArrayLength,
{
    fn process(
        &mut self,
        ctx: &mut AudioContext<AF>,
        ai: &crate::engine::buffer::Frame<AF>,
        ao: &mut crate::engine::buffer::Frame<AF>,
        _: &crate::engine::buffer::Frame<CF>,
        _: &mut crate::engine::buffer::Frame<CF>,
    ) {
        debug_assert_eq!(ai.len(), U1::USIZE);
        debug_assert_eq!(ao.len(), Ao::USIZE);
        let fs = ctx.get_sample_rate();

        for n in 0..AF::USIZE {
            let mod_amt = ai[0][n];

            let freq = self.freq + mod_amt;

            self.phase += freq / fs;
            self.phase = self.phase.fract();

            let sample = (self.phase * std::f32::consts::TAU).sin();

            for chan in ao.iter_mut() {
                chan[n] = sample;
            }
        }
    }
}

impl<Ai, Ao, Ci, Co> PortedErased for Sine<Ai, Ao, Ci, Co>
where
    Ai: ArrayLength,
    Ao: ArrayLength,
    Ci: ArrayLength,
    Co: ArrayLength,
{
    fn get_audio_inputs(&self) -> Option<&[AudioInputPort]> {
        self.ports.get_audio_inputs()
    }
    fn get_audio_outputs(&self) -> Option<&[AudioOutputPort]> {
        self.ports.get_audio_outputs()
    }
    fn get_control_inputs(&self) -> Option<&[ControlInputPort]> {
        None
    }
    fn get_control_outputs(&self) -> Option<&[ControlOutputPort]> {
        None
    }
}

pub type SineMono = Sine<U1, Mono, U0, U0>;
pub type SineStereo = Sine<U1, Stereo, U0, U0>;
pub type SineMC<C> = Sine<C, C, U0, U0>;
