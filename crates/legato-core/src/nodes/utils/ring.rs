/// Ringbuffer utility. Note, this is a
/// ring buffer in the traditional sense, not some
/// sort of spsc queue implementation. For that, I would
/// suggest something like heapless, crossbeam, etc.
pub struct RingBuffer {
    data: Vec<f32>,
    write_index: usize,
}
impl RingBuffer {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: vec![0.0; capacity],
            write_index: 0,
        }
    }
    #[inline(always)]
    pub fn get(&self, k: usize) -> f32 {
        let len = self.data.len();
        let idx = (self.write_index + len - 1 - k) % len;
        self.data[idx]
    }
    #[inline(always)]
    pub fn push(&mut self, val: f32) {
        self.data[self.write_index] = val;
        self.write_index = (self.write_index + 1) % self.data.len()
    }
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn clear(&mut self) {
        self.data.fill(0.0);
        self.write_index = 0;
    }
}
