use std::time::Duration;

use crate::{nodes::{Node, NodeInputs, ports::{PortBuilder, Ported, Ports}}, runtime::{context::AudioContext, resources::DelayLineKey}, utils::interpolation::lerp};



#[derive(Clone)]
pub struct DelayLine {
    buffers: Vec<Box<[f32]>>,
    capacity: usize,
    write_pos: Vec<usize>,
    chans: usize,
}

impl DelayLine {
    pub fn new(capacity: usize, chans: usize) -> Self {
        let buffers = vec![vec![0.0; capacity].into(); chans];
        Self {
            buffers,
            capacity,
            write_pos: vec![0, chans],
            chans
        }
    }
    #[inline(always)]
    pub fn get_write_pos(&self, channel: usize) -> &usize {
        &self.write_pos[channel]
    }
    pub fn write_block(&mut self, block: &NodeInputs) {
        // We're assuming single threaded, with the graph firing in order, so no aliasing writes
        // Our first writing block is whatever capacity is leftover from the writing position
        // Our maximum write size is the block N
        // Our second write size is whatever leftover from N we still have

        let block_size = block.get(0).iter().len();

        for c in 0..self.chans {
            let first_write_size = (self.capacity - self.write_pos[c]).min(block_size);
            let second_write_size = block_size - first_write_size;

            let buf = &mut self.buffers[c];
            buf[self.write_pos[c]..self.write_pos[c] + first_write_size]
                .copy_from_slice(&block[c][0..first_write_size]);
            // TODO: Maybe some sort of mask?
            if second_write_size > 0 {
                buf[0..second_write_size].copy_from_slice(
                    &block[c][first_write_size..first_write_size + second_write_size],
                );
            }
            self.write_pos[c] = (self.write_pos[c] + block_size) % self.capacity;
        }
    }
    /// This uses f32 sample indexes, as we allow for interpolated values
    #[inline(always)]
    pub fn get_delay_linear_interp(&self, channel: usize, offset: f32) -> f32 {
        // Get the remainder of the difference of the write position and fractional sample index we need
        let read_pos = (self.write_pos[channel] as f32 - offset).rem_euclid(self.capacity as f32);

        let pos_floor = read_pos.floor() as usize;
        let pos_floor = pos_floor.min(self.capacity - 1); // clamp to valid index

        let next_sample = (pos_floor + 1) % self.capacity; // TODO: can we have some sort of mask if we make the delay a power of 2?

        let buffer = &self.buffers[channel];

        lerp(
            buffer[pos_floor],
            buffer[next_sample],
            read_pos - pos_floor as f32,
        )
    }
}

pub struct DelayWrite {
    delay_line_key: DelayLineKey,
    ports: Ports,
}
impl DelayWrite
{
    pub fn new(delay_line_key: DelayLineKey, chans: usize) -> Self {
        Self {
            delay_line_key,
            ports: PortBuilder::default()
                .audio_in(chans)
                .build()
        }
    }
}

impl Node for DelayWrite
{
    fn process(
        &mut self,
        ctx: &mut AudioContext,
        ai: &NodeInputs,
        _: &mut NodeInputs,
        _: &NodeInputs,
        _: &mut NodeInputs,
    ) {
        // Single threaded, no aliasing read/writes in the graph. Reference counted so no leaks. Hopefully safe.
        ctx.write_block(self.delay_line_key, ai);
    }
}

impl Ported for DelayWrite {
    fn get_ports(&self) -> &Ports {
        &self.ports
    }
}

pub struct DelayRead
{
    delay_line_key: DelayLineKey,
    delay_times: Vec<Duration>, // Different times for each channel if desired
    ports: Ports,
}
impl DelayRead
{
    pub fn new(chans: usize, delay_line_key: DelayLineKey, delay_times: Vec<Duration>) -> Self {

        Self {
            delay_line_key,
            delay_times,
            ports: PortBuilder::default()
                .audio_out(chans)
                .build()
        }
    }
}

impl Node for DelayRead
{
    fn process(
        &mut self,
        ctx: &mut AudioContext,
        _: &NodeInputs,
        ao: &mut NodeInputs,
        _: &NodeInputs,
        _: &mut NodeInputs,
    ) {

        let config = ctx.get_config();

        let block_size = config.audio_block_size;
        let chans = ao.len();

        debug_assert_eq!(block_size, ao.len());

        for n in 0..block_size {
            for c in 0..chans {
                let offset = (self.delay_times[c].as_secs_f32() * config.sample_rate as f32)
                    + (block_size - n) as f32;
                // Read delay line based on per channel delay time. Must cast to sample index.
                ao[c][n] = ctx.get_delay_linear_interp(self.delay_line_key, c, offset)
            }
        }
    }
}

impl Ported for DelayRead {
    fn get_ports(&self) -> &Ports {
        &self.ports
    }
}