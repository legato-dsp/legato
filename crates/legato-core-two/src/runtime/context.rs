use crate::runtime::lanes::LANES;

pub struct Config {
    pub sample_rate: usize,
    pub control_rate: usize,
    pub audio_block_size: usize,
    pub control_block_size: usize,
}

impl Config {
    pub fn validate(&self) {
        assert!(self.audio_block_size % LANES == 0);
        assert!(self.control_block_size % LANES == 0);
    }
}

pub struct AudioContext {
    config: Config
}

impl AudioContext {
    pub fn get_config(&self) -> &Config {
        &self.config
    }
}