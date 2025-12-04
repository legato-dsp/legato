use std::sync::{Arc};

use assert_no_alloc::permit_alloc;

use crate::{
    nodes::{
        Node, NodeInputs,
        ports::{PortBuilder, Ported, Ports},
    },
    runtime::{context::AudioContext, resources::{SampleKey, audio_sample::{AudioSample}}},
};

pub struct Sampler {
    sample_key: SampleKey,
    read_pos: usize,
    is_looping: bool,
    ports: Ports,
    sample: Option<Arc<AudioSample>>,
    sample_version: u64
}

impl Sampler {
    pub fn new(sample_key: SampleKey, chans: usize) -> Self {
        Self {
            sample_key,
            read_pos: 0,
            is_looping: true,
            ports: PortBuilder::default().audio_out(chans).build(),
            sample: None,
            sample_version: 0
        }
    }
}

impl Node for Sampler {
    fn process<'a>(
        &mut self,
        ctx: &mut AudioContext,
        _: &NodeInputs,
        ao: &mut NodeInputs,
        _: &NodeInputs,
        _: &mut NodeInputs,
    ) {
        let resources = ctx.get_resources();
        // Check for sample update by seeing if the handle and local version match
        // This is all done rather than directly using the swap option, because Arc has a small allocation.
        if let Some(sample_handle) = resources.get_sample(self.sample_key) {
            let handle_version = sample_handle.sample_version.load(std::sync::atomic::Ordering::Acquire);
            if let Some(ref mut self_sample) = self.sample {
                if self.sample_version != handle_version {
                    // Permit small Arc alloc on sample change. Open to alternatives, maybe a heapless arc swap exploration?
                    permit_alloc(|| {
                        if let Some(handle_sample) = sample_handle.sample.load_full() {
                            *self_sample = handle_sample.clone();
                    }});
                    self.sample_version = handle_version;
                }
            }
            else {
                permit_alloc(|| {
                    if let Some(handle_sample) = sample_handle.sample.load_full() {
                        self.sample = Some(handle_sample.clone());
                        self.sample_version = handle_version;
                }});
            }
        }

        if let Some(sample) = &self.sample {
            let inner = &sample;
            let config = ctx.get_config();

            let block_size = config.audio_block_size;
            let chans = self.ports.audio_out.iter().len();

            let buf = inner.data();

            let len = buf[0].len();

            for n in 0..block_size {
                let i = self.read_pos + n;
                for c in 0..chans {
                    ao[c][n] = if i < len {
                        buf[c][i]
                    } else if self.is_looping {
                        buf[c][i % len]
                    } else {
                        0.0
                    };
                }
            }
            self.read_pos = if self.is_looping {
                (self.read_pos + block_size) % len // If we're looping, wrap around
            } else {
                (self.read_pos + block_size).min(len) // If we're not looping, cap at the end
            };
        }
    }
}

impl Ported for Sampler {
    fn get_ports(&self) -> &Ports {
        &self.ports
    }
}
