use std::{cell::UnsafeCell, sync::Arc};

use generic_array::{sequence::GenericSequence, ArrayLength, GenericArray};
use typenum::U0;

use crate::{
    engine::{
        buffer::Frame,
        graph::AudioNode,
        node::Node,
        port::{MultipleInputBehavior, Ports, UpsampleAlg},
    },
    nodes::utils::{generate_audio_inputs, generate_audio_outputs},
};

pub fn lerp(v0: f32, v1: f32, t: f32) -> f32 {
    (1.0 - t) * v0 + t * v1
}

#[derive(Clone)]
pub struct DelayLine<const N: usize, C>
where
    C: ArrayLength,
{
    buffers: GenericArray<Vec<f32>, C>,
    capacity: usize,
    write_pos: usize,
}

impl<const N: usize, C> DelayLine<N, C>
where
    C: ArrayLength,
{
    pub fn new(capacity: usize) -> Self {
        let buffers = GenericArray::generate(|_| vec![0.0; capacity]);
        Self {
            buffers,
            capacity: capacity,
            write_pos: 0,
        }
    }
    #[inline(always)]
    pub fn get_write_pos(&self) -> &usize {
        &self.write_pos
    }
    #[inline(always)]
    pub fn write_block(&mut self, block: &Frame<N>) {
        // We're assuming single threaded, with the graph firing in order, so no aliasing writes
        // Our first writing block is whatever capacity is leftover from the writing position
        // Our maximum write size is the block N
        let first_write_size = (self.capacity - self.write_pos).min(N);
        // Our second write size is whatever leftover from N we still have
        let second_write_size = N - first_write_size;

        for c in 0..C::USIZE {
            let buf = &mut self.buffers[c];
            buf[self.write_pos..self.write_pos + first_write_size]
                .copy_from_slice(&block[c][0..first_write_size]);
            // TODO: Maybe some sort of mask?
            if second_write_size > 0 {
                buf[0..second_write_size].copy_from_slice(
                    &block[c][first_write_size..first_write_size + second_write_size],
                );
            }
        }
        self.write_pos = (self.write_pos + N) % self.capacity;
    }
    // Note: both of these functions use f32 sample indexes, as we allow for interpolated values
    #[inline(always)]
    pub fn get_delay_linear_interp(&self, channel: usize, offset: f32) -> f32 {
        // Get the remainder of the difference of the write position and fractional sample index we need
        let read_pos = (self.write_pos as f32 - offset).rem_euclid(self.capacity as f32);

        let pos_floor = read_pos.floor() as usize;
        let next_sample = (pos_floor + 1) % self.capacity;

        let buffer = &self.buffers[channel];

        lerp(
            buffer[pos_floor],
            buffer[next_sample],
            read_pos - pos_floor as f32,
        )
    }
}

struct DelayWrite<const AF: usize, Ai>
where
    Ai: ArrayLength,
{
    delay_line: Arc<UnsafeCell<DelayLine<AF, Ai>>>,
    ports: Ports<Ai, U0, U0, U0>,
}
impl<const AF: usize, Ai> DelayWrite<AF, Ai>
where
    Ai: ArrayLength,
{
    pub fn new(delay_line: Arc<UnsafeCell<DelayLine<AF, Ai>>>) -> Self {
        Self {
            delay_line,
            ports: Ports {
                audio_inputs: Some(generate_audio_inputs(
                    MultipleInputBehavior::Default,
                    UpsampleAlg::Lerp,
                )),
                audio_outputs: None,
                control_inputs: None,
                control_outputs: None,
            },
        }
    }
}

impl<const AF: usize, const CF: usize, Ai> Node<AF, CF> for DelayWrite<AF, Ai>
where
    Ai: ArrayLength,
{
    fn process(
        &mut self,
        ctx: &crate::engine::audio_context::AudioContext,
        ai: &Frame<AF>,
        ao: &mut Frame<AF>,
        ci: &Frame<CF>,
        co: &mut Frame<CF>,
    ) {
    }
}

struct DelayRead<const AF: usize, Ao>
where
    Ao: ArrayLength,
{
    delay_line: Arc<UnsafeCell<DelayLine<AF, Ao>>>,
    delay_times: GenericArray<f32, Ao>, // Different times for each channel if desired
    ports: Ports<U0, Ao, U0, U0>,
}
impl<const AF: usize, Ao> DelayRead<AF, Ao>
where
    Ao: ArrayLength,
{
    pub fn new(delay_line: Arc<UnsafeCell<DelayLine<AF, Ao>>>) -> Self {
        Self {
            delay_line,
            delay_times: GenericArray::generate(|_| 0.0),
            ports: Ports {
                audio_inputs: None,
                audio_outputs: Some(generate_audio_outputs()),
                control_inputs: None,
                control_outputs: None,
            },
        }
    }
}
