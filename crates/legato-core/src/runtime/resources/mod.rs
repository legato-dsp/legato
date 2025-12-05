pub mod audio_sample;

use std::sync::Arc;

use slotmap::{SlotMap, new_key_type};

use crate::{
    nodes::{NodeInputs, audio::delay::DelayLine},
    runtime::{lanes::Vf32, resources::audio_sample::{AudioSample, AudioSampleHandle, AudioSampleRef}},
};

use std::sync::atomic::Ordering::AcqRel;

new_key_type! { pub struct DelayLineKey; }
new_key_type! { pub struct SampleKey; }

/// Resources are shared resources provided
/// to Nodes by the runtime context.
///
/// This is nice becuase it avoids some general
/// annoyances with sharing delay lines, samples, etc,

#[derive(Default, Clone)]
pub struct Resources {
    delay_lines: SlotMap<DelayLineKey, DelayLine>,
    sample_handles: SlotMap<SampleKey, Arc<AudioSampleHandle>>,
}

impl Resources {
    pub fn new() -> Self {
        Self {
            delay_lines: SlotMap::default(),
            sample_handles: SlotMap::default(),
        }
    }
    pub fn delay_write_block(&mut self, key: DelayLineKey, block: &NodeInputs) {
        let delay_line = self.delay_lines.get_mut(key).unwrap();
        delay_line.write_block(block);
    }
    // Get delays with interpolation
    #[inline(always)]
    pub fn get_delay_linear_interp(&self, key: DelayLineKey, channel: usize, offset: f32) -> f32 {
        let delay_line = self.delay_lines.get(key).unwrap();
        delay_line.get_delay_linear_interp(channel, offset)
    }
    #[inline(always)]
    pub fn get_delay_linear_interp_simd(
        &self,
        key: DelayLineKey,
        channel: usize,
        offset: Vf32,
    ) -> Vf32 {
        let delay_line = self.delay_lines.get(key).unwrap();
        delay_line.get_delay_linear_interp_simd(channel, offset)
    }
    #[inline(always)]
    pub fn get_delay_cubic_interp(&self, key: DelayLineKey, channel: usize, offset: f32) -> f32 {
        let delay_line = self.delay_lines.get(key).unwrap();
        delay_line.get_delay_cubic_interp(channel, offset)
    }
    #[inline(always)]
    pub fn get_delay_cubic_interp_simd(
        &self,
        key: DelayLineKey,
        channel: usize,
        offset: Vf32,
    ) -> Vf32 {
        let delay_line = self.delay_lines.get(key).unwrap();
        delay_line.get_delay_cubic_interp_simd(channel, offset)
    }

    pub fn add_delay_line(&mut self, delay_line: DelayLine) -> DelayLineKey {
        self.delay_lines.insert(delay_line)
    }

    pub fn add_sample_resource(&mut self, sample: Arc<AudioSampleHandle>) -> SampleKey {
        self.sample_handles.insert(sample)
    }

    pub fn get_sample(&self, sample_key: SampleKey) -> Option<&Arc<AudioSampleHandle>> {
        self.sample_handles.get(sample_key)
    }
}
