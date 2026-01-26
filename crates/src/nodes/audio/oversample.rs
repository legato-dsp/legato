// TODO: Rewrite with flat buffer, half band filter

// // A collection of naive oversamplers. May be worth checking out halfband and polyphase filters in the future

// use crate::{
//     context::AudioContext,
//     node::{Inputs, LegatoNode, Node},
//     nodes::audio::fir::FirFilter,
//     ports::{PortBuilder, Ports},
//     runtime::MAX_INPUTS,
// };

// #[derive(Clone)]
// pub struct Upsample<const N: usize> {
//     filter: FirFilter,
//     zero_stuffed: Vec<Box<[f32]>>,
//     chans: usize,
//     ports: Ports,
// }

// impl<const N: usize> Upsample<N> {
//     pub fn new(buff_size: usize, chans: usize, filter: FirFilter) -> Self {
//         Self {
//             filter,
//             chans,
//             zero_stuffed: vec![vec![0.0; buff_size * N].into(); chans],
//             ports: PortBuilder::default()
//                 .audio_in(chans)
//                 .audio_out(chans)
//                 .build(),
//         }
//     }
// }

// impl<const N: usize> Node for Upsample<N> {
//     fn process(&mut self, ctx: &mut AudioContext, ai: &Inputs, ao: &mut [&mut [f32]]) {
//         if ai.is_empty() {
//             return;
//         };

//         if let Some(inner) = ai[0] {
//             debug_assert_eq!(inner.len() * N, ao[0].len());
//         }

//         for c in 0..self.chans {
//             let out = &mut self.zero_stuffed[c];

//             if let Some(input) = &ai[c] {
//                 // Zero stuff the sample, this makes spectral images that must be filtered
//                 for (n, sample) in input.iter().enumerate() {
//                     out[n * N] = *sample;
//                     for k in 1..N {
//                         out[n * N + k] = 0.0;
//                     }
//                 }
//             }
//         }

//         let output_size = self.ports().audio_out.len();

//         let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];

//         self.zero_stuffed.iter().enumerate().for_each(|(c, x)| {
//             if ai[c].is_some() {
//                 inputs[c] = Some(x);
//             }
//         });

//         // FIR filter the zero stuffed buffer
//         self.filter.process(ctx, &inputs[..output_size], ao);
//     }
//     fn ports(&self) -> &Ports {
//         &self.ports
//     }
// }
// #[derive(Clone)]
// pub struct Downsample<const N: usize> {
//     filter: FirFilter,
//     filtered: Vec<Box<[f32]>>,
//     chans: usize,
//     ports: Ports,
// }

// impl<const N: usize> Downsample<N> {
//     pub fn new(buff_size: usize, filter: FirFilter, chans: usize) -> Self {
//         Self {
//             filter,
//             filtered: vec![vec![0.0; buff_size * N].into(); chans],
//             chans,
//             ports: PortBuilder::default()
//                 .audio_in(chans)
//                 .audio_out(chans)
//                 .build(),
//         }
//     }
// }

// impl<const N: usize> Node for Downsample<N> {
//     fn process<'a>(&mut self, ctx: &mut AudioContext, ai: &Inputs, ao: &mut [&mut [f32]]) {
//         // Ensure that ai = ao * N
//         if let Some(inner) = ai[0] {
//             debug_assert_eq!(inner.len(), ao[0].len() * N);
//         }

//         // Filter the audio before decimating to prevent aliasing
//         self.filter.process(ctx, ai, self.filtered.as_mut_slice());

//         // Decimate the filtered audio
//         for c in 0..self.chans {
//             let input = &self.filtered[c];
//             let out = &mut ao[c];
//             for (m, o) in out.iter_mut().enumerate() {
//                 *o = input[m * N];
//             }
//         }
//     }
//     fn ports(&self) -> &Ports {
//         &self.ports
//     }
// }

// #[derive(Clone)]
// pub struct Oversampler<const N: usize> {
//     node: LegatoNode,
//     upsampler: Upsample<N>,
//     // State for the node
//     upsampled_outputs: Vec<Box<[f32]>>,
//     downsampled_inputs: Vec<Box<[f32]>>,
//     // Fir downsampler
//     downsampler: Downsample<N>,
//     ports: Ports,
// }

// impl<const N: usize> Oversampler<N> {
//     pub fn new(
//         node: LegatoNode,
//         upsampler: Upsample<N>,
//         downsampler: Downsample<N>,
//         chans: usize,
//         buff_size: usize,
//     ) -> Self {
//         let node_ports = node.get_node().ports().clone();
//         Self {
//             node,
//             upsampler,
//             downsampler,
//             upsampled_outputs: vec![vec![0.0; buff_size * N].into(); chans],
//             downsampled_inputs: vec![vec![0.0; buff_size * N].into(); chans],
//             ports: node_ports,
//         }
//     }
// }

// impl<const N: usize> Node for Oversampler<N> {
//     fn process<'a>(&mut self, ctx: &mut AudioContext, ai: &Inputs, ao: &mut [&mut [f32]]) {
//         let config = ctx.get_config();

//         let sr = config.sample_rate;
//         let block_size = config.block_size;

//         self.upsampler.process(ctx, ai, &mut self.upsampled_outputs);

//         let mut node_inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];

//         if !ai.is_empty() {
//             self.upsampled_outputs
//                 .iter()
//                 .enumerate()
//                 .for_each(|(c, x)| {
//                     if ai[c].is_some() {
//                         node_inputs[c] = Some(x);
//                     }
//                 });
//         }

//         // TODO: Better pattern than this
//         ctx.set_sample_rate(sr * N);
//         ctx.set_block_size(block_size * N);

//         self.node
//             .get_node_mut()
//             .process(ctx, &node_inputs, &mut self.downsampled_inputs);

//         ctx.set_sample_rate(sr);
//         ctx.set_block_size(block_size);

//         let mut downsampler_node_inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];

//         self.downsampled_inputs
//             .iter()
//             .enumerate()
//             .for_each(|(c, x)| {
//                 downsampler_node_inputs[c] = Some(x);
//             });

//         let out_chans = self.ports().audio_out.len();

//         self.downsampler
//             .process(ctx, &downsampler_node_inputs[..out_chans], ao);
//     }
//     fn ports(&self) -> &Ports {
//         &self.ports
//     }
// }

// // TODO: Create a filter designer rather than pasting in SciPy coeffs
// pub fn upsample_by_two_factory(buff_size: usize, chans: usize) -> Upsample<2> {
//     Upsample::<2>::new(
//         buff_size,
//         chans,
//         FirFilter::new(CUTOFF_24K_COEFFS_FOR_96K.into(), chans),
//     )
// }

// pub fn downsample_by_two_factory(buff_size: usize, chans: usize) -> Downsample<2> {
//     Downsample::<2>::new(
//         buff_size,
//         FirFilter::new(CUTOFF_24K_COEFFS_FOR_96K.into(), chans),
//         chans,
//     )
// }

// pub fn oversample_by_two_factory(
//     node: LegatoNode,
//     chans: usize,
//     buff_size: usize,
// ) -> Oversampler<2> {
//     let upsampler = upsample_by_two_factory(buff_size, chans);
//     let downsampler = downsample_by_two_factory(buff_size, chans);

//     Oversampler::<2>::new(node, upsampler, downsampler, chans, buff_size)
// }

// /// A naive filter to cut at 24k at 96k rate.
// const CUTOFF_24K_COEFFS_FOR_96K: [f32; 64] = [
//     -0.00078997,
//     -0.00106131,
//     0.00019139,
//     0.00186628,
//     0.00118124,
//     -0.00154504,
//     -0.00188737,
//     0.00179210,
//     0.00386756,
//     -0.00041068,
//     -0.00518644,
//     -0.00144159,
//     0.00656960,
//     0.00490158,
//     -0.00646231,
//     -0.00899469,
//     0.00486494,
//     0.01385281,
//     -0.00056869,
//     -0.01820437,
//     -0.00660587,
//     0.02125839,
//     0.01747785,
//     -0.02119668,
//     -0.03247355,
//     0.01592738,
//     0.05352988,
//     -0.00054194,
//     -0.08745912,
//     -0.04247219,
//     0.18323183,
//     0.40859913,
//     0.40859913,
//     0.18323183,
//     -0.04247219,
//     -0.08745912,
//     -0.00054194,
//     0.05352988,
//     0.01592738,
//     -0.03247355,
//     -0.02119668,
//     0.01747785,
//     0.02125839,
//     -0.00660587,
//     -0.01820437,
//     -0.00056869,
//     0.01385281,
//     0.00486494,
//     -0.00899469,
//     -0.00646231,
//     0.00490158,
//     0.00656960,
//     -0.00144159,
//     -0.00518644,
//     -0.00041068,
//     0.00386756,
//     0.00179210,
//     -0.00188737,
//     -0.00154504,
//     0.00118124,
//     0.00186628,
//     0.00019139,
//     -0.00106131,
//     -0.00078997,
// ];
