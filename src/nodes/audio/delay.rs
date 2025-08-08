use crate::mini_graph::{buffer::Frame, node::{DelayTapNode, DelayWriteNode}};

/// For now, we are assuming that the delay line is single threaded,
/// and since we have readers after the writer, we are using unsafe 
/// as there will not be aliasing reads during writing.

pub fn lerp(v0: f32,  v1: f32, t: f32) -> f32 {
    (1.0 - t) * v0 + t * v1
}

#[derive(Clone)]
pub struct DelayLine<const N: usize, const C: usize> {
    buffers: [Vec<f32>; C],
    capacity: usize,
    write_pos: usize,
}

impl<const N: usize, const C: usize> DelayLine<N, C> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity >= N);
        let buffers = std::array::from_fn(|_| vec![0.0; capacity]);
        Self {
            buffers,
            capacity: capacity,
            write_pos: 0
        }
    }
    #[inline(always)]
    pub fn get_write_pos(&self) -> &usize {
        &self.write_pos
    }
    #[inline(always)]
    pub fn write_block(&mut self, block: &Frame<N, C>) {
        // We're assuming single threaded, with the graph firing in order, so no aliasing writes
        // Our first writing block is whatever capacity is leftover from the writing position
        // Our maximum write size is the block N
        let first_write_size = (self.capacity - self.write_pos).min(N);
        // Our second write size is whatever leftover from N we still have
        let second_write_size = N - first_write_size;

        for c in 0..C {
            let buf = &mut self.buffers[c];
            buf[self.write_pos..self.write_pos + first_write_size].copy_from_slice(&block[c][0..first_write_size]);
            if second_write_size > 0 {
                buf[0..second_write_size].copy_from_slice(&block[c][first_write_size..first_write_size + second_write_size]);
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

        lerp(buffer[pos_floor], buffer[next_sample], read_pos - pos_floor as f32)
    }
}

pub struct DelayWrite<const N: usize, const C: usize> {
    name: &'static str,
    delay_index: usize
}

impl<const N: usize, const C: usize> DelayWrite<N, C>{
    pub fn new(name: &'static str, delay_index: usize) -> Self {
        Self {
            name,
            delay_index
        }
    }
}

impl<const N: usize, const C: usize> DelayWriteNode<N, C> for DelayWrite<N, C>{
    fn process(&mut self, inputs: &[Frame<N, C>], delay_line: &mut DelayLine<N, C>) {
        if let Some(input) = inputs.get(0){
            delay_line.write_block(input);
        }
    }
    #[inline(always)]
    fn get_delay_line_index(&self) -> &usize {
        &self.delay_index
    }
}

pub struct DelayTap<const N: usize, const C: usize> {
    gain: f32,
    delay_index: usize,
    sample_offset: f32, // Tap size, in samples

}
impl<const N: usize, const C: usize> DelayTap<N, C>{
    pub fn new(sample_offset: f32, delay_index: usize, gain: f32) -> Self {
        Self {
            gain,
            delay_index,
            sample_offset
        }
    }
}
impl<const N: usize, const C: usize> DelayTapNode<N, C> for DelayTap<N, C>{
    fn process(&mut self, delay_line: &DelayLine<N, C>, output: &mut Frame<N, C>) {
        for n in 0..N {
            for c in 0..C {
                let dynamic_offset = self.sample_offset + (N as f32 - n as f32);
                output[c][n] = delay_line.get_delay_linear_interp(c, dynamic_offset) * self.gain;
            }
        }
    }
    fn get_delay_line_index(&self) -> &usize {
        &self.delay_index
    }
}

// Currently single threaded, no multiple aliases, but this needs to be refactored
unsafe impl<const N: usize, const C: usize> Send for DelayLine<N, C> {}
unsafe impl<const N: usize, const C: usize> Send for DelayWrite<N, C> {}
unsafe impl<const N: usize, const C: usize> Send for DelayTap<N, C> {}