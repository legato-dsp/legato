// use std::ops::Add;

// use generic_array::sequence::GenericSequence;
// use generic_array::{arr, ArrayLength, GenericArray};
// use typenum::{Sum, Unsigned, U0, U2};

// use crate::engine::audio_context::AudioContext;
// use crate::engine::node::Node;
// use crate::engine::port::{Mono, Port, MultipleInputBehavior, PortRate, PortedErased, Ports, Stereo};

// pub enum Wave {
//     Sin,
//     Saw,
//     Triangle,
//     Square,
// }

// pub struct Oscillator<Ai, Ci, O>
// where
//     Ai: Unsigned + Add<Ci>,
//     Ci: Unsigned,
//     O: Unsigned + ArrayLength,
//     Sum<Ai, Ci>: Unsigned + ArrayLength,
// {
//     freq: f32,
//     phase: f32,
//     wave: Wave,
//     ports: Ports<Sum<Ai, Ci>, O>,
// }

// type AudioIn = U0;
// type ControlIn = U2;

// impl<O> Oscillator<AudioIn, ControlIn, O>
// where
//     O: Unsigned + ArrayLength,
//     Sum<U0, U2>: Unsigned + ArrayLength,
// {
//     pub fn new(freq: f32, phase: f32, wave: Wave) -> Self {
//         let inputs = arr![
//             Port {
//                 name: "fm",
//                 index: 0,
//                 behavior: MultipleInputBehavior::Default,
//                 rate: PortRate::Audio
//             },
//             Port {
//                 name: "freq",
//                 index: 1,
//                 behavior: MultipleInputBehavior::Default,
//                 rate: PortRate::Control
//             },
//         ];

//         let outputs: GenericArray<Port, O> = GenericArray::generate(|i| Port {
//             name: match O::USIZE {
//                 1 => "out",
//                 2 => {
//                     if i == 0 {
//                         "l"
//                     } else {
//                         "r"
//                     }
//                 }
//                 _ => "out",
//             },
//             index: i,
//             behavior: MultipleInputBehavior::Default,
//             rate: PortRate::Audio,
//         });

//         let ports = Ports { inputs, outputs };

//         Self {
//             freq,
//             phase,
//             wave,
//             ports,
//         }
//     }

//     pub fn default() -> Self {
//         Self::new(440.0, 0.0, Wave::Sin)
//     }

//     pub fn set_wave_form(&mut self, wave: Wave) {
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
//         self.phase += self.freq / sample_rate;
//         self.phase -= (self.phase >= 1.0) as u32 as f32;
//         sample
//     }
// }

// impl<const AF: usize, const CF: usize, O> Node<AF, CF> for Oscillator<U0, U2, O>
// where
//     O: Unsigned + ArrayLength,
// {
//     fn process(
//         &mut self,
//         ctx: &AudioContext,
//         ai: &crate::engine::buffer::Frame<AF>,
//         ao: &mut crate::engine::buffer::Frame<AF>,
//         ci: &crate::engine::buffer::Frame<CF>,
//         co: &mut crate::engine::buffer::Frame<CF>,
//     ) {
//         debug_assert_eq!(ai.len(), U2::USIZE);
//         debug_assert_eq!(ao.len(), O::USIZE);
//         let sample_rate = ctx.get_sample_rate();
//         for i in 0..AF {
//             let sample = self.tick_osc(sample_rate);
//             for buf in ao.iter_mut() {
//                 buf[i] = sample;
//             }
//         }
//     }
// }

// impl<Ai, Ci, O> PortedErased for Oscillator<Ai, Ci, O>
// where
//     Ai: Unsigned + Add<Ci>,
//     Ci: Unsigned,
//     O: Unsigned + ArrayLength,
//     Sum<Ai, Ci>: Unsigned + ArrayLength,
// {
//     fn get_inputs(&self) -> &[Port] {
//         self.ports.get_inputs()
//     }
//     fn get_outputs(&self) -> &[Port] {
//         self.ports.get_outputs()
//     }
// }

// pub type OscMono = Oscillator<AudioIn, ControlIn, Mono>;
// pub type OscStereo = Oscillator<AudioIn, ControlIn, Stereo>;
// pub type OscMC<C> = Oscillator<C, ControlIn, C>;

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


