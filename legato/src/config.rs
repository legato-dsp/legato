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
/// 
/// The control rate is by default 1/32 of the audio rate.
/// So, this is not suitable for say audio rate FM, but it is reasonable for changing parameters.
/// 
/// If you need smoothing, try using a lowpass filter or some averaging filter to prevent any sharp changes.
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
    pub control_rate: usize,
    pub audio_block_size: usize,
    pub control_block_size: usize,
    pub channels: usize,
    pub initial_graph_capacity: usize,
}

impl Config {
    pub fn new(sr: usize, cr: usize, block_size: BlockSize, channels: usize, initial_graph_capacity: usize) -> Self {
        let audio_block_size = block_size.to_usize();
        Self {
            sample_rate: sr,
            control_rate: cr,
            audio_block_size: audio_block_size,
            control_block_size: audio_block_size / 32,
            channels,
            initial_graph_capacity
        }
    }
    pub fn validate(&self) {
        assert!(self.audio_block_size % LANES == 0);
        assert!(self.control_block_size % LANES == 0);
    }
}
