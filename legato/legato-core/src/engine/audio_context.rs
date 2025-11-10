use generic_array::ArrayLength;
use slotmap::{new_key_type, SlotMap};

use crate::{engine::buffer::Frame, nodes::audio::delay::DelayLineErased};

new_key_type! { pub struct DelayLineKey; }

pub struct AudioContext<N>
where
    N: ArrayLength + Send + Sync + 'static,
{
    sample_rate: f32, // avoiding frequent casting
    control_rate: f32,
    delay_lines: SlotMap<DelayLineKey, Box<dyn DelayLineErased<N>>>,
}

impl<N> AudioContext<N>
where
    N: ArrayLength + Send + Sync + 'static,
{
    pub fn new(sample_rate: f32, control_rate: f32) -> Self {
        Self {
            sample_rate,
            control_rate,
            delay_lines: SlotMap::default(),
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
    pub fn write_block(&mut self, key: DelayLineKey, block: &Frame<N>) {
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
}
