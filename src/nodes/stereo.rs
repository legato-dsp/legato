// use crate::engine::audio_context::AudioContext;
// use crate::engine::port::PortRate;
// use crate::engine::{
//     node::Node,
//     port::{Port, MultipleInputBehavior, PortedErased},
// };

// pub struct Stereo {
//     ports: StereoPorts,
// }

// impl Default for Stereo {
//     fn default() -> Self {
//         Self {
//             ports: StereoPorts::new(),
//         }
//     }
// }

// impl<'a, const AF: usize, const CF: usize> Node<AF, CF> for Stereo {
//     fn process(
//         &mut self,
//         ctx: &AudioContext,
//         ai: &crate::engine::buffer::Frame<AF>,
//         ao: &mut crate::engine::buffer::Frame<AF>,
//         ci: &crate::engine::buffer::Frame<CF>,
//         co: &mut crate::engine::buffer::Frame<CF>,
//     ) {
//         debug_assert_eq!(ai.len(), 1);
//         debug_assert_eq!(ao.len(), 2);

//         for n in 0..AF {
//             for c in 0..2 {
//                 ao[c][n] = ai[0][n];
//             }
//         }
//     }
// }

// struct StereoPorts {
//     inputs: [Port; 1],
//     outputs: [Port; 2],
// }
// impl StereoPorts {
//     pub fn new() -> Self {
//         Self {
//             inputs: [Port {
//                 name: "audio",
//                 index: 0,
//                 behavior: MultipleInputBehavior::Default,
//                 rate: PortRate::Audio,
//             }],
//             outputs: [
//                 Port {
//                     name: "l",
//                     index: 0,
//                     behavior: MultipleInputBehavior::Default,
//                     rate: PortRate::Audio,
//                 },
//                 Port {
//                     name: "r",
//                     index: 0,
//                     behavior: MultipleInputBehavior::Default,
//                     rate: PortRate::Audio,
//                 },
//             ],
//         }
//     }
// }

// impl PortedErased for Stereo {
//     fn get_inputs(&self) -> &[Port] {
//         &self.ports.inputs
//     }
//     fn get_outputs(&self) -> &[Port] {
//         &self.ports.outputs
//     }
// }
