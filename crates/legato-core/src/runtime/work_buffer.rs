pub struct WorkBuffer {
    data: Vec<f32>,
    channel_slices: Vec<&'static mut [f32]>,
    channels: usize,
    block_size: usize,
}

impl WorkBuffer {
    pub fn new(block_size: usize, chans: usize) -> Self {
        let mut data = vec![0.0; block_size * chans]; // Flat storage, where N is block size -> [ L*N, R*N, etc. ]
        let mut channel_slices = Vec::with_capacity(chans);

        for chan in 0..chans {
            let start = block_size * chan;
            let end = start + block_size;

            let slice = &mut data[start..end] as *mut [f32];
            channel_slices.push(unsafe { &mut *slice });
        }

        Self {
            data,
            channel_slices,
            channels: chans,
            block_size,
        }
    }
    pub fn slice_to_chans_mut(&mut self) -> &mut Vec<&'static mut [f32]> {
        &mut self.channel_slices
    }
}
