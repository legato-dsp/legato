use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use crate::mini_graph::node::Node;
use crate::mini_graph::buffer::{self, Frame};

/// For now, we are assuming that the delay line is single threaded,
/// and since we have readers after the writer, we are using unsafe 
/// as there will not be aliasing reads during writing.

pub fn lerp(v0: f32,  v1: f32, t: f32) -> f32 {
    (1.0 - t) * v0 + t * v1
}

struct DelayLineInner<const C: usize> {
    buffers: [Vec<f32>; C],
    capacity: usize,
    write_pos: usize,
}

#[derive(Clone)]
pub struct DelayLine<const N: usize, const C: usize> {
    inner: Rc<UnsafeCell<DelayLineInner<C>>>,
}

impl<const N: usize, const C: usize> DelayLine<N, C> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity >= N);
        let buffers = std::array::from_fn(|_| vec![0.0; capacity]);
        let inner = DelayLineInner {
            buffers,
            capacity: capacity,
            write_pos: 0
        };
        Self {
            inner: Rc::new(UnsafeCell::new(inner))
        }
    }
    #[inline(always)]
    pub fn get_write_pos(&self) -> usize {
        let inner = unsafe { &*self.inner.get() };
        inner.write_pos
    }
    #[inline(always)]
    pub fn write_block(&self, block: &Frame<N, C>) {
        // We're assuming single threaded, with the graph firing in order, so no aliasing writes
        let inner = unsafe { &mut *self.inner.get() };
        // Our first writing block is whatever capacity is leftover from the writing position
        // Our maximum write size is the block N
        let first_write_size = (inner.capacity - inner.write_pos).min(N);
        // Our second write size is whatever leftover from N we still have
        let second_write_size = (N - first_write_size);

        for c in 0..C {
            let buf = &mut inner.buffers[c];
            buf[inner.write_pos..inner.write_pos + first_write_size].copy_from_slice(&block[c][0..first_write_size]);
            if second_write_size > 0 {
                buf[0..second_write_size].copy_from_slice(&block[c][first_write_size..first_write_size + second_write_size]);
            }
        }
        inner.write_pos = (inner.write_pos + N) % inner.capacity;
    }
    // Note: both of these functions use f32 sample indexes, as we allow for interpolated values
    #[inline(always)]
    pub fn get_delay_linear_interp(&self, channel: usize, offset: f32) -> f32 { 
        let inner = unsafe { & *self.inner.get() };

        // Get the remainder of the difference of the write position and fractional sample index we need
        let read_pos = (inner.write_pos as f32 - offset).rem_euclid(inner.capacity as f32);

        let pos_floor = read_pos.floor() as usize;
        let next_sample = (pos_floor + 1) % inner.capacity;

        let buffer = &inner.buffers[channel];

        lerp(buffer[pos_floor], buffer[next_sample], read_pos - pos_floor as f32)
    }
}

pub struct DelayWrite<const N: usize, const C: usize> {
    inner: DelayLine<N, C>,
}

impl<const N: usize, const C: usize> DelayWrite<N, C>{
    pub fn new(inner: DelayLine<N, C>) -> Self {
        Self {
            inner
        }
    }
}

impl<const N: usize, const C: usize> Node<N, C> for DelayWrite<N, C>{
    fn process(&mut self, inputs: &[Frame<N, C>], _: &mut Frame<N, C>) {
        if let Some(input) = inputs.get(0){
            self.inner.write_block(input);
        }
    }
}

pub struct DelayTap<const N: usize, const C: usize> {
    inner: DelayLine<N, C>,
    sample_offset: f32, // Tap size, in samples
    gain: f32,
}
impl<const N: usize, const C: usize> DelayTap<N, C>{
    pub fn new(inner: DelayLine<N, C>, sample_offset: f32, gain: f32) -> Self {
        Self {
            inner,
            sample_offset,
            gain
        }
    }
}
impl<const N: usize, const C: usize> Node<N, C> for DelayTap<N, C>{
    fn process(&mut self, _: &[Frame<N, C>], output: &mut Frame<N, C>) {
        for n in 0..N {
            for c in 0..C {
                let dynamic_offset = self.sample_offset + (N as f32 - n as f32);
                output[c][n] = self.inner.get_delay_linear_interp(c, dynamic_offset) * self.gain;
            }
        }
    }
}

// Currently single threaded, no multiple aliases, but this needs to be refactored
unsafe impl<const N: usize, const C: usize> Send for DelayLine<N, C> {}
unsafe impl<const N: usize, const C: usize> Send for DelayWrite<N, C> {}
unsafe impl<const N: usize, const C: usize> Send for DelayTap<N, C> {}