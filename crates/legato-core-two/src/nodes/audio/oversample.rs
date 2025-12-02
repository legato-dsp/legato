// A collection of naive oversamplers. May be worth checking out halfband and polyphase filters in the future

use crate::{nodes::{Node, NodeInputs, audio::fir::FirFilter, ports::{PortBuilder, Ported, Ports}}, runtime::context::AudioContext};

pub struct Upsample<const N: usize> {
    filter: FirFilter,
    zero_stuffed: Vec<Box<[f32]>>,
    chans: usize,
    ports: Ports,
}

impl<const N: usize> Upsample<N> {
    pub fn new(buff_size: usize, chans: usize, filter: FirFilter) -> Self {
        Self {
            filter,
            chans,
            zero_stuffed: vec![vec![0.0; buff_size * N].into() ; chans],
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_out(chans)
                .build()
            ,
        }
    }
}

impl<const N: usize> Node for Upsample<N> {
    fn process(
            &mut self,
            ctx: &mut AudioContext,
            ai: &NodeInputs,
            ao: &mut NodeInputs,
            ci: &NodeInputs,
            co: &mut NodeInputs,
        ) {
        
        debug_assert_eq!(ai.len(), ao.len()); // Channels match
        debug_assert_eq!(ai[0].len() * N, ao[0].len()); // Conversion matches

        for c in 0..self.chans {
            let input = &ai[c];
            let out = &mut self.zero_stuffed[c];

            // Zero stuff the sample, this makes spectral images that must be filtered
            for (n, sample) in input.iter().enumerate() {
                let f = N;
                out[n * f] = *sample;
                for k in 1..f {
                    out[n * f + k] = 0.0;
                }
            }
        }
        // FIR filter the zero stuffed buffer
        self.filter.process(ctx, &self.zero_stuffed, ao, ci, co);
    }
}

impl<const N: usize> Ported for Upsample<N> {
    fn get_ports(&self) -> &Ports {
        &self.ports
    }
}

pub struct Downsample<const N: usize> {
    filter: FirFilter,
    filtered: Vec<Box<[f32]>>,
    chans: usize,
    ports: Ports,
}

impl<const N: usize> Downsample<N> {
    pub fn new(buff_size: usize, filter: FirFilter, chans: usize) -> Self {
        Self {
            filter,
            filtered: vec![vec![0.0; buff_size * N].into(); chans],
            chans,
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_out(chans)
                .build()
        }
    }
}

impl<const N: usize> Node for Downsample<N> {
    fn process<'a>(
            &mut self,
            ctx: &mut AudioContext,
            ai: &NodeInputs,
            ao: &mut NodeInputs,
            ci: &NodeInputs,
            co: &mut NodeInputs,
        ) {
        // Ensure that ai = ao * N
        debug_assert_eq!(ai[0].len(), ao[0].len() * N);
        // Filter the audio before decimating to prevent aliasing
        self.filter.process(ctx, ai, &mut self.filtered, ci, co);
        
        // Decimate the filtered audio
        for c in 0..self.chans {
            let input = &self.filtered[c];
            let out = &mut ao[c];
            for (m, o) in out.iter_mut().enumerate() {
                *o = input[m * N];
            }
        }
    }
}

impl<const N: usize> Ported for Downsample<N> {
    fn get_ports(&self) -> &Ports {
        &self.ports
    }
}

pub struct Oversampler<const N: usize> {
    node: Box<dyn Node + Send + 'static>,
    upsampler: Upsample<N>,
    // State for the node
    upsampled_outputs: Vec<Box<[f32]>>,
    downsampled_inputs: Vec<Box<[f32]>>,
    // Fir downsampler
    downsampler: Downsample<N>
}

impl<const N: usize> Oversampler<N> {
    pub fn new(node: Box<dyn Node + Send + 'static>, upsampler: Upsample<N>, downsampler: Downsample<N>, chans: usize, buff_size: usize) -> Self {
        Self {
            node,
            upsampler,
            downsampler,
            upsampled_outputs: vec![vec![0.0; buff_size * N].into() ; chans],
            downsampled_inputs: vec![vec![0.0; buff_size * N].into(); chans],
        }
    }
}

impl<const N: usize> Node for Oversampler<N> {
    fn process<'a>(
            &mut self,
            ctx: &mut AudioContext,
            ai: &NodeInputs,
            ao: &mut NodeInputs,
            ci: &NodeInputs,
            co: &mut NodeInputs,
        ) {
        self.upsampler.process(ctx, ai, &mut self.upsampled_outputs, ci, co);
        self.node.process(ctx, &self.upsampled_outputs, &mut self.downsampled_inputs, ci, co);
        self.downsampler.process(ctx, &self.downsampled_inputs, ao, ci, co);
    }
}

impl<const N: usize> Ported for Oversampler<N> {
    fn get_ports(&self) -> &Ports {
        &self.node.get_ports()
    }
}

// TODO: Create a filter designer rather than pasting in SciPy coeffs
pub fn upsample_by_two_factory(buff_size: usize, chans: usize) -> Upsample<2> {
    Upsample::<2>::new(buff_size, chans, FirFilter::new(CUTOFF_24K_COEFFS_FOR_96K.into(), chans))
}

pub fn downsample_by_two_factory(buff_size: usize, chans: usize) -> Downsample<2> {
    Downsample::<2>::new(buff_size, FirFilter::new(CUTOFF_24K_COEFFS_FOR_96K.into(), chans), chans)
}

pub fn oversample_by_two_factory(node: Box<dyn Node + Send + 'static>, chans: usize, buff_size: usize) -> Oversampler<2> {
    let upsampler = upsample_by_two_factory(buff_size, chans);
    let downsampler = downsample_by_two_factory(buff_size, chans);

    Oversampler::<2>::new(node, upsampler, downsampler, chans, buff_size)
}

/// A naive filter to cut at 24k at 96k rate.
const CUTOFF_24K_COEFFS_FOR_96K: [f32; 64] = [
    -0.00078997,
    -0.00106131,
    0.00019139,
    0.00186628,
    0.00118124,
    -0.00154504,
    -0.00188737,
    0.00179210,
    0.00386756,
    -0.00041068,
    -0.00518644,
    -0.00144159,
    0.00656960,
    0.00490158,
    -0.00646231,
    -0.00899469,
    0.00486494,
    0.01385281,
    -0.00056869,
    -0.01820437,
    -0.00660587,
    0.02125839,
    0.01747785,
    -0.02119668,
    -0.03247355,
    0.01592738,
    0.05352988,
    -0.00054194,
    -0.08745912,
    -0.04247219,
    0.18323183,
    0.40859913,
    0.40859913,
    0.18323183,
    -0.04247219,
    -0.08745912,
    -0.00054194,
    0.05352988,
    0.01592738,
    -0.03247355,
    -0.02119668,
    0.01747785,
    0.02125839,
    -0.00660587,
    -0.01820437,
    -0.00056869,
    0.01385281,
    0.00486494,
    -0.00899469,
    -0.00646231,
    0.00490158,
    0.00656960,
    -0.00144159,
    -0.00518644,
    -0.00041068,
    0.00386756,
    0.00179210,
    -0.00188737,
    -0.00154504,
    0.00118124,
    0.00186628,
    0.00019139,
    -0.00106131,
    -0.00078997,
];






