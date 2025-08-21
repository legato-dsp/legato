// use crate::engine::node::Node;
// use crate::engine::audio_context::AudioContext;
// use crate::engine::buffer::{Frame};
// use crate::engine::port::{Port, PortBehavior};

// pub enum Wave {
//     Sin,
//     Saw,
//     Triangle,
//     Square,
// }

// pub struct Oscillator {
//     freq: f32,
//     phase: f32,
//     wave: Wave,
// }

// impl Oscillator {
//     pub fn new(freq: f32, phase: f32, wave: Wave) -> Self {
//         Self {
//             freq,
//             phase,
//             wave
//         }
//     }

//     pub fn set_wave_form(&mut self, wave: Wave){
//         self.wave = wave;
//     }

//     #[inline(always)]
//     fn tick_osc(&mut self, sample_rate: f32) -> f32 {
//         let sample = match self.wave {
//             Wave::Sin => sin_amp_from_phase(&self.phase),
//             Wave::Saw => saw_amp_from_phase(&self.phase),
//             Wave::Square => square_amp_from_phase(&self.phase),
//             Wave::Triangle => triangle_amp_from_phase(&self.phase),
//         };
//         self.phase += self.freq / sample_rate as f32;
//         self.phase -= (self.phase >= 1.0) as u32 as f32; 
//         sample
//     }

//     const INPUTS: [Port;2] = [
//         Port {
//             name: "FM",
//             index: 0,
//             behavior: PortBehavior::Default, 
//         },
//         Port {
//             name: "FREQ",
//             index: 1,
//             behavior: PortBehavior::Default,
//         }
//     ];

//     const OUTUTS: [Port; 1] = [
//         Port {
//             name: "AUDIO",
//             index: 0,
//             behavior: PortBehavior::Default,
//         }
//     ];
// }

// impl<'a, const N: usize> Node<'a, N> for Oscillator {
//     fn get_input_ports(&self) -> &'static [Port] { &Self::INPUTS }
//     fn get_output_ports(&self) -> &'static [Port] { &Self::OUTUTS }
//     fn process(&mut self, ctx: &AudioContext , input: Frame<'a, N>, output: &mut Frame<'a, N>) {
//         debug_assert!(input.len() == Self::INPUTS.len());
//         debug_assert!(output.len() == Self::OUTUTS.len());
//         let sample_rate = ctx.get_sample_rate();
//         for i in 0..N {
//             let sample = self.tick_osc(sample_rate);
//             for buf in output.iter_mut() {
//                 buf[i] = sample;
//             }
//         }

//     }
// }

// #[inline(always)]
// fn sin_amp_from_phase(phase: &f32) -> f32 {
//     (*phase * 2.0 * std::f32::consts::PI).sin()
// }

// #[inline(always)]
// fn saw_amp_from_phase(phase: &f32) -> f32 {
//     *phase * 2.0 - 1.0
// }

// #[inline(always)]
// fn triangle_amp_from_phase(phase: &f32) -> f32 {
//     2.0 * ((-1.0 + (*phase * 2.0)).abs() - 0.5)
// }

// #[inline(always)]
// fn square_amp_from_phase(phase: &f32) -> f32 {
//     match *phase <= 0.5 {
//         true => 1.0,
//         false => -1.0,
//     }
// }