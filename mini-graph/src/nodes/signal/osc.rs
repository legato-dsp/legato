use crate::{Port, PortError, Inputs, AudioContext, ReadPort, Frame, Node};
use mini_graph_macros::{Port};

#[derive(Port, PartialEq, Debug)]
enum OscillatorInputs {
    Freq,
    FM,
}

pub enum Wave {
    SinWave,
    SawWave,
    TriangleWave,
    SquareWave,
}

struct Oscillator {
    wave: Wave,
    freq: f32,
    phase: f32,
}

impl Oscillator {
    #[inline(always)]
    fn tick_osc_with_freq(&mut self, freq: f32, ctx: &AudioContext) -> f32 {
        let sample = match self.wave {
            Wave::SinWave      => sin_amp_from_phase(&self.phase),
            Wave::SawWave      => saw_amp_from_phase(&self.phase),
            Wave::SquareWave   => square_amp_from_phase(&self.phase),
            Wave::TriangleWave => triangle_amp_from_phase(&self.phase),
        };
        self.phase += freq / ctx.sample_rate;
        if self.phase >= 1.0 { self.phase -= 1.0; }
        sample
    }
}

impl<const N: usize, const C: usize> Node<N, C> for Oscillator {
    type InputPorts = OscillatorInputs;

    fn process(&mut self, ctx: &AudioContext, inputs: &Inputs<N, C>, output: &mut Frame<N, C>) {
        let freq_buf = inputs.get_buf(OscillatorInputs::Freq, 0);
        let fm_buf   = inputs.get_buf(OscillatorInputs::FM,   0);

        match (freq_buf, fm_buf) {
            (None, None) => {
                let base = self.freq;
                for i in 0..N {
                    let s = self.tick_osc_with_freq(base, ctx);
                    for ch in 0..C { output[ch][i] = s; }
                }
            }
            (Some(freq), None) => {
                for i in 0..N {
                    let s = self.tick_osc_with_freq(freq[i], ctx);
                    for ch in 0..C { output[ch][i] = s; }
                }
            }
            (None, Some(fm)) => {
                let base = self.freq;
                for i in 0..N {
                    let s = self.tick_osc_with_freq(base + fm[i], ctx);
                    for ch in 0..C { output[ch][i] = s; }
                }
            }
            (Some(freq), Some(fm)) => {
                for i in 0..N {
                    let s = self.tick_osc_with_freq(freq[i] + fm[i], ctx);
                    for ch in 0..C { output[ch][i] = s; }
                }
            }
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