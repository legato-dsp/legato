use crate::simd::LANES;

/// The config that determines the interior application
/// sample rate, as well as a few other settings.
///
/// Note: The audio block size is the internal graph rate.
/// This does not need to match the audio callback rate of your end device, as you can adapt if needed
/// with a ringbuffer, double buffer, etc.
///
/// In summary, depending on your latency requirements,
/// you may need to change the blocksize somewhat.
pub enum BlockSize {
    Block64,
    Block128,
    Block256,
    Block512,
    Block1024,
    Block2048,
    Block4096,
}

impl BlockSize {
    fn to_usize(&self) -> usize {
        match self {
            BlockSize::Block64 => 64,
            BlockSize::Block128 => 128,
            BlockSize::Block256 => 256,
            BlockSize::Block512 => 512,
            BlockSize::Block1024 => 1024,
            BlockSize::Block2048 => 2048,
            BlockSize::Block4096 => 4096,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub sample_rate: usize,
    pub block_size: usize,
    pub channels: usize,
    pub initial_graph_capacity: usize,
}

impl Config {
    pub fn new(
        sr: usize,
        block_size: BlockSize,
        channels: usize,
        initial_graph_capacity: usize,
    ) -> Self {
        let block_size = block_size.to_usize();
        Self {
            sample_rate: sr,
            block_size,
            channels,
            initial_graph_capacity,
        }
    }
    pub fn validate(&self) {
        assert!(self.block_size.is_multiple_of(LANES));
    }
}
