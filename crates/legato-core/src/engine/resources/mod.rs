pub mod audio_sample;

use std::sync::Arc;

use arc_swap::ArcSwapOption;
use slotmap::{SlotMap, new_key_type};

use crate::{
    engine::{buffer::Frame, node::FrameSize, resources::audio_sample::AudioSample},
    nodes::audio::delay::DelayLineErased,
};

// TODO: Maybe use a hashmap to get string -> index pairs,
// then use the index at runtime?
new_key_type! { pub struct DelayLineKey; }
new_key_type! { pub struct SampleKey; }

/// Resources are shared resources provided
/// to Nodes by the runtime context.
///
/// This is nice becuase it avoids some general
/// annoyances with sharing delay lines, samples, etc,

pub struct Resources<N>
where
    N: FrameSize + Send + Sync + 'static,
{
    delay_lines: SlotMap<DelayLineKey, Box<dyn DelayLineErased<N>>>,
    samples: SlotMap<SampleKey, Arc<ArcSwapOption<AudioSample>>>,
}

impl<N> Resources<N>
where
    N: FrameSize + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            delay_lines: SlotMap::default(),
            samples: SlotMap::default(),
        }
    }
    pub fn delay_write_block(&mut self, key: DelayLineKey, block: &Frame<N>) {
        let delay_line = self.delay_lines.get_mut(key).unwrap();
        delay_line.write_block_erased(block);
    }
    #[inline(always)]
    pub fn get_delay_linear_interp(
        &mut self,
        key: DelayLineKey,
        channel: usize,
        offset: f32,
    ) -> f32 {
        let delay_line = self.delay_lines.get(key).unwrap();
        delay_line.get_delay_linear_interp_erased(channel, offset)
    }
    pub fn add_delay_line(
        &mut self,
        delay_line: Box<dyn DelayLineErased<N> + Send + 'static>,
    ) -> DelayLineKey {
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
