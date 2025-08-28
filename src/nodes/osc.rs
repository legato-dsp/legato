use std::ops::Add;

use generic_array::{ArrayLength, arr};
use typenum::{Sum, Unsigned, U0, U2};

use crate::engine::node::Node;
use crate::engine::audio_context::AudioContext;
use crate::engine::buffer::{Buffer};
use crate::engine::port::{Mono, Port, PortBehavior, Ports, Stereo};

pub enum Wave {
    Sin,
    Saw,
    Triangle,
    Square,
}

pub struct Oscillator<Ai, Ci, O>
where
    Ai: Unsigned + Add<Ci>,
    Ci: Unsigned,
    O: Unsigned + ArrayLength,
    Sum<Ai, Ci>: Unsigned + ArrayLength,
{
    freq: f32,
    phase: f32,
    wave: Wave,
    ports: Ports<Ai, Ci, O>
}


type AudioIn = U0;
type ControlIn = U2;

type OscMono = Oscillator<AudioIn, ControlIn, Mono>;
type OscStereo = Oscillator<AudioIn, ControlIn, Stereo>;

impl Ports<AudioIn, ControlIn, Mono> {
    fn new() -> Self {
        let inputs = arr![
            Port { name: "fm",   index: 0, behavior: PortBehavior::Default },
            Port { name: "freq", index: 1, behavior: PortBehavior::Default },
        ];
        let outputs = arr![
            Port { name: "audio", index: 0, behavior: PortBehavior::Default }
        ];
        Self {
            inputs,
            outputs
        }
    }
}

impl Ports<AudioIn, ControlIn, Stereo> {
    fn new() -> Self {
        let inputs = arr![
            Port { name: "fm",   index: 0, behavior: PortBehavior::Default },
            Port { name: "freq", index: 1, behavior: PortBehavior::Default },
        ];
        let outputs = arr![
            Port { name: "L", index: 0, behavior: PortBehavior::Default },
            Port { name: "R", index: 1, behavior: PortBehavior::Default }
        ];
        Self {
            inputs,
            outputs
        }
    }
}

impl<Ai, Ci, O> Oscillator<Ai, Ci, O>
where
    Ai: Unsigned + Add<Ci>,
    Ci: Unsigned,
    O: Unsigned + ArrayLength,
    Sum<Ai, Ci>: Unsigned + ArrayLength,
{
    pub fn new(freq: f32, phase: f32, wave: Wave, ports: Ports<Ai, Ci, O>) -> Self {
        Self {
            freq,
            phase,
            wave,
            ports
        }
    }

    pub fn set_wave_form(&mut self, wave: Wave){
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
        self.phase += self.freq / sample_rate as f32;
        self.phase -= (self.phase >= 1.0) as u32 as f32; 
        sample
    }
}

impl<const N: usize, Ai, Ci, O> Node<N, Ai, Ci, O> for Oscillator<Ai, Ci, O>
where 
    Ai: Unsigned + Add<Ci>,
    Ci: Unsigned,
    O: Unsigned + ArrayLength,
    Sum<Ai, Ci>: Unsigned + ArrayLength
{
    fn process(&mut self, ctx: &AudioContext , input: &[Buffer<N>], output: &mut [Buffer<N>]) {
        debug_assert_eq!(input.len(), <Sum<Ai, Ci>>::USIZE );
        debug_assert_eq!(output.len(), O::USIZE);
        let sample_rate = ctx.get_sample_rate();
        for i in 0..N {
            let sample = self.tick_osc(sample_rate);
            for buf in output.iter_mut() {
                buf[i] = sample;
            }
        }
    }
}

impl Default for OscMono {
    fn default() -> Self {
        let ports = Ports::<AudioIn, ControlIn, Mono>::new();
        Self {
            freq: 440.0,
            phase: 0.0,
            wave: Wave::Sin,
            ports
        }
    }
}

impl Default for OscStereo {
    fn default() -> Self {
        let ports = Ports::<AudioIn, ControlIn, Stereo>::new();
        Self {
            freq: 440.0,
            phase: 0.0,
            wave: Wave::Sin,
            ports
        }
    }
}

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