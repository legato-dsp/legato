pub mod audio_sample;
pub mod delay_line;

use std::sync::Arc;

use arc_swap::ArcSwapOption;
use slotmap::{SlotMap, new_key_type};

use crate::{nodes::NodeInputs, runtime::resources::{audio_sample::AudioSample, delay_line::DelayLine}};

// TODO: Maybe use a hashmap to get string -> index pairs,
// then use the index at runtime?
new_key_type! { pub struct DelayLineKey; }
new_key_type! { pub struct SampleKey; }

/// Resources are shared resources provided
/// to Nodes by the runtime context.
///
/// This is nice becuase it avoids some general
/// annoyances with sharing delay lines, samples, etc,

#[derive(Default)]
pub struct Resources {
    delay_lines: SlotMap<DelayLineKey, DelayLine>,
    samples: SlotMap<SampleKey, Arc<ArcSwapOption<AudioSample>>>,
}

impl Resources {
    pub fn new() -> Self {
        Self {
            delay_lines: SlotMap::default(),
            samples: SlotMap::default(),
        }
    }
    pub fn delay_write_block(&mut self, key: DelayLineKey, block: &NodeInputs) {
        let delay_line = self.delay_lines.get_mut(key).unwrap();
        delay_line.write_block(block);
    }
    #[inline(always)]
    pub fn get_delay_linear_interp(
        &mut self,
        key: DelayLineKey,
        channel: usize,
        offset: f32,
    ) -> f32 {
        let delay_line = self.delay_lines.get(key).unwrap();
        delay_line.get_delay_linear_interp(channel, offset)
    }
    pub fn add_delay_line(&mut self, delay_line: DelayLine) -> DelayLineKey {
        self.delay_lines.insert(delay_line)
    }
    pub fn add_sample_resource(&mut self, sample: Arc<ArcSwapOption<AudioSample>>) -> SampleKey {
        self.samples.insert(sample)
    }
    pub fn get_sample(&self, sample_key: SampleKey) -> Option<Arc<AudioSample>> {
        if let Some(inner) = self.samples.get(sample_key) {
            if let Some(buf) = inner.load_full() {
                return Some(buf);
            }
            return None;
        }
        None
    }
}
