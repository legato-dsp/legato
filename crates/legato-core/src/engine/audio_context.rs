use std::sync::Arc;

use arc_swap::ArcSwapOption;

use crate::{
    engine::{
        buffer::Frame,
        node::FrameSize,
        resources::{DelayLineKey, Resources, SampleKey, audio_sample::AudioSample},
    },
    nodes::audio::delay::DelayLineErased,
};


pub struct AudioContext<N>
where
    N: FrameSize + Send + Sync + 'static,
{
    sample_rate: f32, // avoiding frequent casting
    control_rate: f32,
    resources: Resources<N>,
}

impl<N> AudioContext<N>
where
    N: FrameSize + Send + Sync + 'static,
{
    pub fn new(sample_rate: f32, control_rate: f32) -> Self {
        Self {
            sample_rate,
            control_rate,
            resources: Resources::new(),
        }
    }
    #[inline(always)]
    pub fn get_sample_rate(&self) -> f32 {
        self.sample_rate
    }
    #[inline(always)]
    pub fn get_control_rate(&self) -> f32 {
        self.control_rate
    }
    // Operations for resources
    pub fn write_block(&mut self, key: DelayLineKey, block: &Frame<N>) {
        self.resources.delay_write_block(key, block)
    }
    #[inline(always)]
    pub fn get_delay_linear_interp(
        &mut self,
        key: DelayLineKey,
        channel: usize,
        offset: f32,
    ) -> f32 {
        self.resources.get_delay_linear_interp(key, channel, offset)
    }
    pub fn add_delay_line(
        &mut self,
        delay_line: Box<dyn DelayLineErased<N> + Send + 'static>,
    ) -> DelayLineKey {
        self.resources.add_delay_line(delay_line)
    }
    pub fn get_sample(&self, sample_key: SampleKey) -> Option<Arc<AudioSample>> {
        self.resources.get_sample(sample_key)
    }
    // Note, the sample does not have to live at this point.
    // This is creating an Arc<ArcSwapOption> that can load samples at runtime
    pub fn add_sample_resource(&mut self, sample: Arc<ArcSwapOption<AudioSample>>) -> SampleKey {
        self.resources.add_sample_resource(sample)
    }
}
