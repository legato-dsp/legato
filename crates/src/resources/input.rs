use slotmap::new_key_type;

new_key_type! { pub struct AudioInputKey; }

/// Planar audio handed to the graph from outside, read by `ExternalInput` nodes.
///
/// Either fed from another thread through a ring of interleaved frames (e.g. a cpal
/// input stream), or written directly by the caller on the audio thread (e.g. a plugin host).
pub struct AudioInput {
    data: Box<[f32]>, // chans * block_size
    chans: usize,
    block_size: usize,
    feeder: Option<rtrb::Consumer<f32>>,
}

impl AudioInput {
    pub fn new(chans: usize, block_size: usize, consumer: rtrb::Consumer<f32>) -> Self {
        Self {
            feeder: Some(consumer),
            ..Self::host_fed(chans, block_size)
        }
    }

    pub fn host_fed(chans: usize, block_size: usize) -> Self {
        Self {
            data: vec![0.0; chans * block_size].into(),
            chans,
            block_size,
            feeder: None,
        }
    }

    /// Pull one block of interleaved frames from the feeder, if any, deinterleaving into `data`.
    pub fn drain(&mut self) {
        let Some(consumer) = &mut self.feeder else {
            return;
        };
        let expected = self.chans * self.block_size;
        let available = consumer.slots();

        if available >= expected {
            let chunk = consumer
                .read_chunk(expected)
                .expect("slots() reported enough room but read_chunk failed");
            let (first, second) = chunk.as_slices();
            for (i, &sample) in first.iter().chain(second).enumerate() {
                let (frame, chan) = (i / self.chans, i % self.chans);
                self.data[chan * self.block_size + frame] = sample;
            }
            chunk.commit_all();
        } else {
            // Underrun, discard bad data TODO: Reporting?
            self.data.fill(0.0);
            if available > 0 {
                let chunk = consumer
                    .read_chunk(available)
                    .expect("slots() reported enough room but read_chunk failed");
                chunk.commit_all();
            }
        }
    }

    /// Copy `src` into `channel` starting at frame `offset`.
    #[inline]
    pub fn write(&mut self, channel: usize, offset: usize, src: &[f32]) {
        assert!(channel < self.chans, "channel index out of range");
        let start = channel * self.block_size + offset;
        self.data[start..start + src.len()].copy_from_slice(src);
    }

    #[inline]
    pub fn chans(&self) -> usize {
        self.chans
    }

    /// The full flat non-interleaved buffer, note: this is not a per channel abstraction.
    #[inline]
    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }

    /// A single channel's samples.
    #[inline]
    pub fn channel(&self, channel: usize) -> &[f32] {
        assert!(channel < self.chans, "channel index out of range");
        let start = channel * self.block_size;
        &self.data[start..start + self.block_size]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_deinterleaves_ring_frames() {
        let (mut p, c) = rtrb::RingBuffer::new(16);
        let mut input = AudioInput::new(2, 4, c);
        for s in [0.0, 10.0, 1.0, 11.0, 2.0, 12.0, 3.0, 13.0] {
            p.push(s).unwrap();
        }
        input.drain();
        assert_eq!(input.channel(0), &[0.0, 1.0, 2.0, 3.0]);
        assert_eq!(input.channel(1), &[10.0, 11.0, 12.0, 13.0]);
    }

    #[test]
    fn drain_zeroes_on_underrun() {
        let (mut p, c) = rtrb::RingBuffer::new(16);
        let mut input = AudioInput::new(1, 4, c);
        input.write(0, 0, &[1.0; 4]);
        p.push(5.0).unwrap();
        input.drain();
        assert_eq!(input.channel(0), &[0.0; 4]);
    }

    #[test]
    fn host_fed_drain_keeps_written_data() {
        let mut input = AudioInput::host_fed(1, 4);
        input.write(0, 1, &[1.0, 2.0]);
        input.drain();
        assert_eq!(input.channel(0), &[0.0, 1.0, 2.0, 0.0]);
    }
}
