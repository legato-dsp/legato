use std::time::Duration;

use generic_array::{sequence::GenericSequence, ArrayLength, GenericArray};
use typenum::U0;

use crate::{
    engine::{
        audio_context::{AudioContext, DelayLineKey},
        buffer::Frame,
        node::Node,
        port::{
            AudioInputPort, AudioOutputPort, ControlInputPort, ControlOutputPort, Mono,
            PortedErased, Ports, Stereo,
        },
    },
    nodes::utils::{generate_audio_inputs, generate_audio_outputs},
};

/// Fairly hard to meet the requirements of a PureData/MaxMSP style delay line.
///
/// Perhaps something like a global delay context is a safer solution.

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
    write_pos: GenericArray<usize, C>,
}

// Erasing delay line so we can store in a global context
pub trait DelayLineErased<const N: usize>: Send + Sync {
    fn get_write_pos(&self, channel: usize) -> &usize;
    fn write_block(&mut self, block: &Frame<N>);
    fn get_delay_linear_interp(&self, channel: usize, offset: f32) -> f32;
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
            write_pos: GenericArray::generate(|_| 0),
        }
    }
    #[inline(always)]
    pub fn get_write_pos(&self, channel: usize) -> &usize {
        &self.write_pos[channel]
    }
    #[inline(always)]
    pub fn write_block(&mut self, block: &Frame<N>) {
        // We're assuming single threaded, with the graph firing in order, so no aliasing writes
        // Our first writing block is whatever capacity is leftover from the writing position
        // Our maximum write size is the block N
        // Our second write size is whatever leftover from N we still have

        for c in 0..C::USIZE {
            let first_write_size = (self.capacity - self.write_pos[c]).min(N);
            let second_write_size = N - first_write_size;

            let buf = &mut self.buffers[c];
            buf[self.write_pos[c]..self.write_pos[c] + first_write_size]
                .copy_from_slice(&block[c][0..first_write_size]);
            // TODO: Maybe some sort of mask?
            if second_write_size > 0 {
                buf[0..second_write_size].copy_from_slice(
                    &block[c][first_write_size..first_write_size + second_write_size],
                );
            }
            self.write_pos[c] = (self.write_pos[c] + N) % self.capacity;
        }
    }
    // Note: both of these functions use f32 sample indexes, as we allow for interpolated values
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

impl<const N: usize, C> DelayLineErased<N> for DelayLine<N, C>
where
    C: ArrayLength,
{
    fn get_delay_linear_interp(&self, channel: usize, offset: f32) -> f32 {
        self.get_delay_linear_interp(channel, offset)
    }
    fn get_write_pos(&self, channel: usize) -> &usize {
        self.get_write_pos(channel)
    }
    fn write_block(&mut self, block: &Frame<N>) {
        self.write_block(block)
    }
}

pub struct DelayWrite<const AF: usize, Ai>
where
    Ai: ArrayLength,
{
    delay_line_key: DelayLineKey,
    ports: Ports<Ai, U0, U0, U0>,
}
impl<const AF: usize, Ai> DelayWrite<AF, Ai>
where
    Ai: ArrayLength,
{
    pub fn new(delay_line_key: DelayLineKey) -> Self {
        Self {
            delay_line_key,
            ports: Ports {
                audio_inputs: Some(generate_audio_inputs()),
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
        ctx: &mut AudioContext<AF>,
        ai: &Frame<AF>,
        _: &mut Frame<AF>,
        _: &Frame<CF>,
        _: &mut Frame<CF>,
    ) {
        // Single threaded, no aliasing read/writes in the graph. Reference counted so no leaks. Hopefully safe.
        ctx.write_block(self.delay_line_key, ai);
    }
}

impl<const AF: usize, Ai> PortedErased for DelayWrite<AF, Ai>
where
    Ai: ArrayLength,
{
    fn get_audio_inputs(&self) -> Option<&[crate::engine::port::AudioInputPort]> {
        self.ports.get_audio_inputs()
    }
    fn get_audio_outputs(&self) -> Option<&[crate::engine::port::AudioOutputPort]> {
        self.ports.get_audio_outputs()
    }
    fn get_control_inputs(&self) -> Option<&[crate::engine::port::ControlInputPort]> {
        self.ports.get_control_inputs()
    }
    fn get_control_outputs(&self) -> Option<&[crate::engine::port::ControlOutputPort]> {
        self.ports.get_control_outputs()
    }
}

pub struct DelayRead<const AF: usize, Ao>
where
    Ao: ArrayLength,
{
    delay_line_key: DelayLineKey,
    delay_times: GenericArray<Duration, Ao>, // Different times for each channel if desired
    ports: Ports<U0, Ao, U0, U0>,
}
impl<const AF: usize, Ao> DelayRead<AF, Ao>
where
    Ao: ArrayLength,
{
    pub fn new(delay_line_key: DelayLineKey, delay_times: GenericArray<Duration, Ao>) -> Self {
        Self {
            delay_line_key,
            delay_times,
            ports: Ports {
                audio_inputs: None,
                audio_outputs: Some(generate_audio_outputs()),
                control_inputs: None, // TODO: modulate delay times per channel
                control_outputs: None,
            },
        }
    }
}

impl<const AF: usize, const CF: usize, Ao> Node<AF, CF> for DelayRead<AF, Ao>
where
    Ao: ArrayLength,
{
    fn process(
        &mut self,
        ctx: &mut AudioContext<AF>,
        _: &Frame<AF>,
        ao: &mut Frame<AF>,
        _: &Frame<CF>,
        _: &mut Frame<CF>,
    ) {
        debug_assert_eq!(Ao::USIZE, ao.len());
        for n in 0..AF {
            for c in 0..Ao::USIZE {
                let offset =
                    (self.delay_times[c].as_secs_f32() * ctx.get_sample_rate()) + (AF - n) as f32;
                // Read delay line based on per channel delay time. Must cast to sample index.
                ao[c][n] = ctx.get_delay_linear_interp(self.delay_line_key, c, offset)
            }
        }
    }
}

impl<const AF: usize, Ao> PortedErased for DelayRead<AF, Ao>
where
    Ao: ArrayLength,
{
    fn get_audio_inputs(&self) -> Option<&[AudioInputPort]> {
        self.ports.get_audio_inputs()
    }
    fn get_audio_outputs(&self) -> Option<&[AudioOutputPort]> {
        self.ports.get_audio_outputs()
    }
    fn get_control_inputs(&self) -> Option<&[ControlInputPort]> {
        self.ports.get_control_inputs()
    }
    fn get_control_outputs(&self) -> Option<&[ControlOutputPort]> {
        self.ports.get_control_outputs()
    }
}

pub type DelayReadMono<const AF: usize> = DelayRead<AF, Mono>;
pub type DelayReadStereo<const AF: usize> = DelayRead<AF, Stereo>;

pub type DelayWriteMono<const AF: usize> = DelayWrite<AF, Mono>;
pub type DelayWriteStereo<const AF: usize> = DelayWrite<AF, Stereo>;
