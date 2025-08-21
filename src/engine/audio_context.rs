

pub struct AudioContext {
    sample_rate: f32, // avoiding frequent casting
}
impl AudioContext {
    #[inline(always)]
    pub fn get_sample_rate(&self) -> f32 {
        self.sample_rate
    }
}