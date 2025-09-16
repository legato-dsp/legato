use generic_array::{arr, ArrayLength, GenericArray};
use typenum::{Unsigned, U0, U2};

use crate::engine::audio_context::AudioContext;
use crate::engine::node::Node;
use crate::engine::port::*;
use crate::nodes::utils::generate_audio_outputs;
pub enum Wave {
    Sin,
    Saw,
    Triangle,
    Square,
}

pub struct Oscillator<Ai, Ao, Ci, Co>
where
    Ai: ArrayLength,
    Ao: ArrayLength,
    Ci: ArrayLength,
    Co: ArrayLength,
{
    freq: f32,
    phase: f32,
    wave: Wave,
    ports: Ports<Ai, Ao, Ci, Co>,
}

// Note, Ao is still generic here, letting us make multiple sizes!
impl<Ao> Oscillator<U2, Ao, U0, U0>
where
    Ao: ArrayLength,
{
    pub fn new(freq: f32, phase: f32, wave: Wave) -> Self {
        // FM is audio rate, frequency
        let audio_inputs = arr![
            AudioInputPort {
                meta: PortMeta {
                    name: "fm",
                    index: 0
                },
                input_behavior: MultipleInputBehavior::Default,
                resample: UpsampleAlg::Lerp
            },
            AudioInputPort {
                meta: PortMeta {
                    name: "pm",
                    index: 1
                },
                input_behavior: MultipleInputBehavior::Default,
                resample: UpsampleAlg::Lerp
            },
        ];

        let audio_outputs: GenericArray<AudioOutputPort, Ao> = generate_audio_outputs::<Ao>();
        let ports = Ports {
            audio_inputs: Some(audio_inputs),
            audio_outputs: Some(audio_outputs),
            control_inputs: None,
            control_outputs: None,
        };

        Self {
            freq,
            phase,
            wave,
            ports,
        }
    }

    pub fn default() -> Self {
        Self::new(440.0, 0.0, Wave::Sin)
    }

    pub fn set_wave_form(&mut self, wave: Wave) {
        self.wave = wave;
    }

    #[inline(always)]
    fn tick_osc(&mut self, sample_rate: f32) -> f32 {
        let sample = match self.wave {
            Wave::Sin => sin_amp_from_phase(&self.phase),
            Wave::Saw => saw_amp_from_phase(&self.phase),
            Wave::Square => square_amp_from_phase(&self.phase),
            Wave::Triangle => triangle_amp_from_phase(&self.phase),
        };
        self.phase += self.freq / sample_rate;
        self.phase -= (self.phase >= 1.0) as u32 as f32;
        sample
    }
}

impl<const AF: usize, const CF: usize, Ao> Node<AF, CF> for Oscillator<U2, Ao, U0, U0>
where
    Ao: ArrayLength,
{
    fn process(
        &mut self,
        ctx: &AudioContext,
        ai: &crate::engine::buffer::Frame<AF>,
        ao: &mut crate::engine::buffer::Frame<AF>,
        _: &crate::engine::buffer::Frame<CF>,
        _: &mut crate::engine::buffer::Frame<CF>,
    ) {
        debug_assert_eq!(ai.len(), U2::USIZE);
        debug_assert_eq!(ao.len(), Ao::USIZE);
        let sample_rate = ctx.get_sample_rate();
        for i in 0..AF {
            let sample = self.tick_osc(sample_rate);
            for buf in ao.iter_mut() {
                buf[i] = sample;
            }
        }
    }
}

impl<Ai, Ao, Ci, Co> PortedErased for Oscillator<Ai, Ao, Ci, Co>
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

type AudioIn = U2;

pub type OscMono = Oscillator<AudioIn, Mono, U0, U0>;
pub type OscStereo = Oscillator<AudioIn, Stereo, U0, U0>;
pub type OscMC<C> = Oscillator<C, C, U0, U0>;

#[inline(always)]
fn sin_amp_from_phase(phase: &f32) -> f32 {
    (*phase * 2.0 * std::f32::consts::PI).sin()
}

#[inline(always)]
fn saw_amp_from_phase(phase: &f32) -> f32 {
    *phase * 2.0 - 1.0
}

#[inline(always)]
fn triangle_amp_from_phase(phase: &f32) -> f32 {
    2.0 * ((-1.0 + (*phase * 2.0)).abs() - 0.5)
}

#[inline(always)]
fn square_amp_from_phase(phase: &f32) -> f32 {
    match *phase <= 0.5 {
        true => 1.0,
        false => -1.0,
    }
}
