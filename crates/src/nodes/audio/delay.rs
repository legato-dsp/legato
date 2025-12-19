use std::time::Duration;

use crate::{
    context::AudioContext,
    node::{Channels, Node},
    ports::{PortBuilder, Ports},
    resources::DelayLineKey,
    ring::RingBuffer,
    simd::{LANES, Vf32},
};

#[derive(Clone, Debug)]
pub struct DelayLine {
    buffers: Vec<RingBuffer>,
    write_pos: Vec<usize>,
}

impl DelayLine {
    pub fn new(capacity: usize, chans: usize) -> Self {
        let buffers = vec![RingBuffer::new(capacity); chans];
        Self {
            buffers,
            write_pos: vec![0; chans],
        }
    }
    #[inline(always)]
    pub fn get_write_pos(&self, channel: usize) -> &usize {
        &self.write_pos[channel]
    }

    #[inline(always)]
    pub fn write_block(&mut self, block: &Channels) {
        for (c, chan) in block.iter().enumerate() {
            for chunk in chan.chunks_exact(LANES) {
                self.buffers[c].push_simd(&Vf32::from_slice(chunk));
            }
        }
    }

    #[inline(always)]
    pub fn get_delay_linear_interp(&self, channel: usize, offset: f32) -> f32 {
        let buffer = &self.buffers[channel];
        buffer.get_delay_linear(offset)
    }

    #[inline(always)]
    pub fn get_delay_cubic_interp(&self, channel: usize, offset: f32) -> f32 {
        let buffer = &self.buffers[channel];
        buffer.get_delay_cubic(offset)
    }

    #[inline(always)]
    // This gives an SIMD "chunk" of size LANES after the offset
    pub fn get_delay_linear_interp_simd(&self, channel: usize, offset: Vf32) -> Vf32 {
        let buffer = &self.buffers[channel];
        buffer.get_delay_linear_simd(offset)
    }

    #[inline(always)]
    pub fn get_delay_cubic_interp_simd(&self, channel: usize, offset: Vf32) -> Vf32 {
        let buffer = &self.buffers[channel];
        buffer.get_delay_cubic_simd(offset)
    }
}

#[derive(Clone)]
pub struct DelayWrite {
    delay_line_key: DelayLineKey,
    ports: Ports,
}
impl DelayWrite {
    pub fn new(delay_line_key: DelayLineKey, chans: usize) -> Self {
        Self {
            delay_line_key,
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_out(chans) // Just for graph semantics
                .build(),
        }
    }
}

impl Node for DelayWrite {
    fn process(
        &mut self,
        ctx: &mut AudioContext,
        ai: &Channels,
        ao: &mut Channels,
    ) {
        // Single threaded, no aliasing read/writes in the graph. Reference counted so no leaks. Hopefully safe.
        let resources = ctx.get_resources_mut();
        resources.delay_write_block(self.delay_line_key, ai);

        // For graph semantics when adding connections between delays
        for chan in ao.iter_mut() {
            chan.fill(0.0);
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

#[derive(Clone)]
pub struct DelayRead {
    delay_line_key: DelayLineKey,
    delay_times: Vec<Duration>, // Different times for each channel if desired
    ports: Ports,
}
impl DelayRead {
    pub fn new(chans: usize, delay_line_key: DelayLineKey, delay_times: Vec<Duration>) -> Self {
        Self {
            delay_line_key,
            delay_times,
            ports: PortBuilder::default().audio_out(chans).build(),
        }
    }
}

impl Node for DelayRead {
    fn process(
        &mut self,
        ctx: &mut AudioContext,
        _: &Channels,
        ao: &mut Channels
    ) {
        let config = ctx.get_config();

        let block_size = config.audio_block_size;

        let resources = ctx.get_resources();

        let sr = config.sample_rate as f32;

        for (c, chan) in ao.iter_mut().enumerate() {
            let delay_time = self.delay_times[c].as_secs_f32();

            for (cidx, chunk) in chan.chunks_exact_mut(LANES).enumerate() {
                let chunk_start = LANES * cidx;

                let mut offset = [0.0; LANES];

                // Apply additional offset for each step, maybe this could also be a rotation or so.
                // This is needed, because otherwise we would just grab offsets from chunk_start for each item

                for (lane, sample) in offset.iter_mut().enumerate().take(LANES) {
                    *sample = delay_time * sr + (block_size - (chunk_start + lane)) as f32;
                }

                // Note, about 75% slower than the linear interpolation alg.
                let interpolated = resources.get_delay_cubic_interp_simd(
                    self.delay_line_key,
                    c,
                    Vf32::from_array(offset),
                );

                chunk[..].copy_from_slice(&interpolated.to_array());
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

#[cfg(test)]
mod test_delay_simd_equivalence {
    use super::*;
    use rand::Rng;

    #[test]
    fn scalar_and_simd_reads_match() {
        const CHANS: usize = 1;
        const CAP: usize = 4096;
        const BLOCK: usize = 256;

        let mut dl = DelayLine::new(CAP, CHANS);

        let mut inputs_raw = [vec![0.0; BLOCK].into(); CHANS];

        let input: &mut Channels = &mut inputs_raw;

        let mut rng = rand::rng();
        for s in &mut input[0] {
            *s = rng.random::<f32>();
        }

        dl.write_block(input);

        for _ in 0..10_000 {
            let off = rng.random::<f32>() * (CAP as f32 - 4.0);

            let s = dl.get_delay_linear_interp(0, off);

            let off_simd = Vf32::from_array(std::array::from_fn(|_| off));
            let v = dl.get_delay_linear_interp_simd(0, off_simd);

            // all SIMD lanes must match the scalar sample
            for lane in v.as_array().iter() {
                assert!(
                    (lane - s).abs() < 1e-5,
                    "SIMD mismatch: scalar={s}, simd={lane}, offset={off}"
                );
            }
        }
    }

    #[test]
    fn scalar_and_simd_writes_match() {
        const CHANS: usize = 1;
        const CAP: usize = 2048;
        const BLOCK: usize = 4096;

        let mut rb_scalar = RingBuffer::new(CAP);
        let mut rb_simd = RingBuffer::new(CAP);

        let mut inputs_raw = [vec![0.0; BLOCK].into(); CHANS];

        let input: &mut Channels = &mut inputs_raw;

        let mut rng = rand::rng();
        for s in &mut input[0] {
            *s = rng.random::<f32>();
        }

        let buf = &input[0];

        for n in 0..BLOCK {
            rb_scalar.push(buf[n]);
        }

        for chunk in buf.iter().as_slice().chunks(LANES) {
            rb_simd.push_simd(&Vf32::from_slice(chunk));
        }

        let data_scalar = rb_scalar.get_data();
        let data_simd = rb_simd.get_data();

        for i in 0..CAP {
            let a = data_scalar[i];
            let b = data_simd[i];
            assert!(
                (a - b).abs() < 1e-10,
                "data mismatch at index {}: scalar={}, simd={}",
                i,
                a,
                b
            );
        }
    }
}
