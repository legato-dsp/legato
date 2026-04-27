use slotmap::new_key_type;

new_key_type! { pub struct AudioInputKey; }

/// We use this struct to hold and pass in data from external input threads.
///
/// Then, nodes can simply call for the slice or specific channel in data.
pub struct AudioInput {
    data: Box<[f32]>, // chans * block_size
    chans: usize,
    block_size: usize,
    consumer: rtrb::Consumer<f32>,
}

impl AudioInput {
    pub fn new(chans: usize, block_size: usize, consumer: rtrb::Consumer<f32>) -> Self {
        Self {
            data: vec![0.0; chans * block_size].into(),
            chans,
            block_size,
            consumer,
        }
    }

    pub fn drain(&mut self) {
        let expected = self.chans * self.block_size;
        let available = self.consumer.slots();

        // Here, we just copy the two slices if we have enough data
        if available >= expected {
            let chunk = self
                .consumer
                .read_chunk(expected)
                .expect("slots() reported enough room but read_chunk failed");
            let (first, second) = chunk.as_slices();
            let mid = first.len();
            self.data[..mid].copy_from_slice(first);
            self.data[mid..].copy_from_slice(second);
            chunk.commit_all();
        } else {
            // Underrun, discard bad data TODO: Reporting?
            self.data.fill(0.0);
            if available > 0 {
                let chunk = self
                    .consumer
                    .read_chunk(available)
                    .expect("slots() reported enough room but read_chunk failed");
                chunk.commit_all();
            }
        }
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
