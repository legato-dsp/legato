use std::{cmp::max, mem::MaybeUninit};

use halfband::fir::{Downsampler16, Upsampler16};

use crate::{
    context::AudioContext,
    executor::MAX_ARITY,
    node::{Inputs, LegatoNode, Node},
    ports::{PortBuilder, Ports},
};

const OVERSAMPLE_K: usize = 2;

#[derive(Debug)]
pub struct Oversampler2X {
    node: LegatoNode,
    upsamplers: Box<[Upsampler16]>,
    downsamplers: Box<[Downsampler16]>,
    // Flat work buffer, so buffer_size * upsample * chans
    upsampled: Box<[f32]>,
    node_outputs: Box<[f32]>,
    chans: usize,
    ports: Ports,
}

impl Oversampler2X {
    pub fn new(node: LegatoNode, buffer_size: usize) -> Self {
        let ports = node.get_node().ports();

        let chans = max(ports.audio_in.len(), ports.audio_out.len());

        let new_ports = PortBuilder::default()
            .audio_in(chans)
            .audio_in(chans)
            .build();

        let upsamplers = (0..chans)
            .map(|_| Upsampler16::default())
            .collect::<Vec<Upsampler16>>()
            .into();

        let downsamplers = (0..chans)
            .map(|_| Downsampler16::default())
            .collect::<Vec<Downsampler16>>()
            .into();

        Self {
            node,
            upsamplers,
            downsamplers,
            upsampled: vec![0.0; buffer_size * OVERSAMPLE_K * chans].into(),
            node_outputs: vec![0.0; buffer_size * OVERSAMPLE_K * chans].into(),
            chans,
            ports: new_ports,
        }
    }
}

/// Upsampler and Downsampler are not clone.
///
/// So, we lose state, but likely that is not what we want anyways.
impl Clone for Oversampler2X {
    fn clone(&self) -> Self {
        let upsamplers = (0..self.chans)
            .map(|_| Upsampler16::default())
            .collect::<Vec<Upsampler16>>()
            .into();

        let downsamplers = (0..self.chans)
            .map(|_| Downsampler16::default())
            .collect::<Vec<Downsampler16>>()
            .into();

        Self {
            node: self.node.clone(),
            upsamplers,
            downsamplers,
            upsampled: self.upsampled.clone(),
            node_outputs: self.node_outputs.clone(),
            chans: self.chans,
            ports: self.ports.clone(),
        }
    }
}

impl Node for Oversampler2X {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let cfg = ctx.get_config();

        let block_size = cfg.block_size;
        let sample_rate = cfg.sample_rate;

        assert!(self.upsampled.len() == self.chans * block_size * OVERSAMPLE_K);

        // Used to construct slices for oversampling
        let mut node_inputs: [Option<&[f32]>; MAX_ARITY] = [None; MAX_ARITY];
        let mut has_inputs: [bool; MAX_ARITY] = [false; MAX_ARITY];

        // Upsample audio into flat buffer slices per chan
        for (c, chan_in_outer) in inputs.iter().enumerate() {
            if let Some(chan_in) = chan_in_outer {
                has_inputs[c] = true;
                let start = block_size * OVERSAMPLE_K * c;
                let end = start + block_size * OVERSAMPLE_K;

                let chan_mut = &mut self.upsampled[start..end];

                self.upsamplers[c].process_block(chan_in, chan_mut);
            }
        }

        for (c, (input_chan, has_input_chan)) in node_inputs
            .iter_mut()
            .zip(has_inputs.iter())
            .take(self.chans)
            .enumerate()
        {
            if *has_input_chan {
                let start = block_size * OVERSAMPLE_K * c;
                let end = start + block_size * OVERSAMPLE_K;

                let chan_in = &self.upsampled[start..end];

                *input_chan = Some(chan_in);
            }
        }

        self.node_outputs.fill(0.0);

        let mut node_outputs_raw = slice_node_ports_mut(
            &mut self.node_outputs,
            0,
            block_size * OVERSAMPLE_K,
            self.chans,
        );

        let outputs_for_node: &mut [&mut [f32]] = unsafe {
            &mut *(&mut node_outputs_raw[..self.chans] as *mut [MaybeUninit<&mut [f32]>]
                as *mut [&mut [f32]])
        };

        // TODO: This is stupid, find a different pattern

        ctx.set_block_size(block_size * OVERSAMPLE_K);
        ctx.set_sample_rate(sample_rate * OVERSAMPLE_K);

        self.node
            .get_node_mut()
            .process(ctx, &inputs, outputs_for_node);

        ctx.set_block_size(block_size);
        ctx.set_sample_rate(sample_rate);

        for c in 0..self.chans {
            let downsampler = &mut self.downsamplers[c];

            let chan_out = &mut outputs[c];

            downsampler.process_block(outputs_for_node[c], chan_out);
        }
    }

    fn ports(&self) -> &Ports {
        &self.node.get_node().ports()
    }
}

#[inline(always)]
fn slice_node_ports_mut<'a>(
    buffer: &'a mut [f32],
    offset: usize,
    block_size: usize,
    chans: usize,
) -> [MaybeUninit<&'a mut [f32]>; MAX_ARITY] {
    let end = (block_size * chans) + offset;

    let node_buffer = &mut buffer[offset..end];

    let slices = node_buffer.chunks_exact_mut(block_size);

    assert_eq!(slices.len(), chans);

    let mut outputs_raw: [MaybeUninit<&mut [f32]>; MAX_ARITY] =
        { [const { MaybeUninit::<&mut [f32]>::uninit() }; MAX_ARITY] };

    for (i, slice) in slices.enumerate() {
        outputs_raw[i] = MaybeUninit::new(slice);
    }

    outputs_raw
}
