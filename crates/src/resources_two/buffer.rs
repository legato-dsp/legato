use std::sync::Arc;

/// Resource buffers are designed for interacting with data
/// loaded from another thread. This could be samples, LUT, etc.
///
/// For audio, this should already be the traditional graph audio rate.
///
/// Note: These are not internally mutable. For this, you could use the
/// internal preallocated resource buffer, but you would then have to do a
/// relatively expensive copy that may not be realtime safe with large buffers.
#[derive(Clone)]
pub struct ExternalBuffer {
    pub data: Arc<[f32]>,
    pub num_channels: usize,
}

impl ExternalBuffer {
    #[inline(always)]
    pub fn channel(&self, idx: usize) -> &[f32] {
        let stride = self.data.len() / self.num_channels;
        let start = idx * stride;
        &self.data[start..start + stride]
    }
}
