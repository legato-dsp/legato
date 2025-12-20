use std::sync::Arc;

use slotmap::{SlotMap, new_key_type};

use crate::{
    node::{Channels, Inputs}, nodes::audio::delay::DelayLine, params::{ParamError, ParamKey, ParamMeta, ParamStore, ParamStoreBuilder, ParamStoreFrontend}, sample::AudioSampleHandle, simd::Vf32
};

new_key_type! { pub struct DelayLineKey; }
new_key_type! { pub struct SampleKey; }

#[derive(Debug, Clone, Default)]
pub struct ResourceBuilder {
    delay_lines: SlotMap<DelayLineKey, DelayLine>,
    sample_handles: SlotMap<SampleKey, Arc<AudioSampleHandle>>,
    param_builder: ParamStoreBuilder
}

impl ResourceBuilder {
    pub fn add_delay_line(&mut self, delay_line: DelayLine) -> DelayLineKey {
        self.delay_lines.insert(delay_line)
    }

    pub fn add_sample_resource(&mut self, sample: Arc<AudioSampleHandle>) -> SampleKey {
        self.sample_handles.insert(sample)
    }

    pub fn add_param(&mut self, unique_name: String, meta: ParamMeta) -> ParamKey {
        self.param_builder.add_param(unique_name, meta)
    }

    pub fn build(self) -> (Resources, ParamStoreFrontend) {
        let (frontend, store) = self.param_builder.build();

        let resources = Resources::new(
            self.delay_lines,
            self.sample_handles,
            store
        );

        (resources, frontend)
    }
}

/// Resources are shared resources provided
/// to Nodes by the runtime context.
///
/// This is convenient becuase it avoids some general
/// annoyances with sharing delay lines, samples, etc.

#[derive(Default, Clone, Debug)]
pub struct Resources {
    delay_lines: SlotMap<DelayLineKey, DelayLine>,
    sample_handles: SlotMap<SampleKey, Arc<AudioSampleHandle>>,
    param_store: ParamStore
}

impl Resources {
    pub fn new(delay_lines: SlotMap<DelayLineKey, DelayLine>, sample_handles: SlotMap<SampleKey, Arc<AudioSampleHandle>>, param_store: ParamStore) -> Self {
        Self {
            delay_lines,
            sample_handles,
            param_store
        }
    }

    #[inline(always)]
    pub fn delay_write_block(&mut self, key: DelayLineKey, block: &Inputs) {
        let delay_line = self.delay_lines.get_mut(key).unwrap();
        delay_line.write_block(block);
    }

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

    #[inline(always)]
    pub fn get_sample(&self, sample_key: SampleKey) -> Option<&Arc<AudioSampleHandle>> {
        self.sample_handles.get(sample_key)
    }

    #[inline(always)]
    pub fn get_param(&self, param_key: &ParamKey) -> Result<f32, ParamError> {
        self.param_store.get(param_key)
    }

    #[inline(always)]
    pub unsafe fn get_param_unchecked(&self, param_key: &ParamKey) -> f32 {
        unsafe { self.param_store.get_unchecked(param_key) }
    }
}
