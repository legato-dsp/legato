use std::sync::Arc;

use arc_swap::ArcSwapOption;

use crate::{
    nodes::NodeInputs,
    runtime::{
        lanes::LANES,
        resources::{DelayLineKey, Resources, SampleKey, audio_sample::AudioSample},
    },
};

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
}

impl Config {
    pub fn new(sr: usize, cr: usize, block_size: BlockSize, channels: usize) -> Self {
        let audio_block_size = block_size.to_usize();
        Self {
            sample_rate: sr,
            control_rate: cr,
            audio_block_size: audio_block_size,
            control_block_size: audio_block_size / 32,
            channels,
        }
    }
    pub fn validate(&self) {
        assert!(self.audio_block_size % LANES == 0);
        assert!(self.control_block_size % LANES == 0);
    }
}

pub struct AudioContext {
    config: Config,
    resources: Resources,
}

impl AudioContext {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            resources: Resources::default(),
        }
    }
    pub fn get_config(&self) -> Config {
        self.config.clone()
    }
    pub fn get_resources(&self) -> &Resources {
        &self.resources
    }
    pub fn get_resources_mut(&mut self) -> &mut Resources {
        &mut self.resources
    }
}
