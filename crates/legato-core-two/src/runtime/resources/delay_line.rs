use crate::{nodes::NodeInputs, utils::interpolation::lerp};

#[derive(Clone)]
pub struct DelayLine {
    buffers: Vec<Vec<f32>>,
    capacity: usize,
    write_pos: Vec<usize>,
}

impl DelayLine {
    pub fn new(capacity: usize, chans: usize) -> Self {
        let buffers = vec![vec![0.0; capacity]; chans];
        Self {
            buffers,
            capacity,
            write_pos: vec![0; chans],
        }
    }
    #[inline(always)]
    pub fn get_write_pos(&self, channel: usize) -> &usize {
        &self.write_pos[channel]
    }
    pub fn write_block(&mut self, block: &NodeInputs) {
        let block_size = block[0].len();
        for (c, _) in block.iter().enumerate() {
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
