use assert_no_alloc::permit_alloc;

use crate::{
    nodes::{
        Node, NodeInputs,
        ports::{PortBuilder, Ported, Ports},
    },
    runtime::{context::AudioContext, resources::SampleKey},
};

pub struct Sampler {
    sample_key: SampleKey,
    read_pos: usize,
    is_looping: bool,
    ports: Ports,
}

impl Sampler {
    pub fn new(sample_key: SampleKey, chans: usize) -> Self {
        println!("chans! {}", chans);
        Self {
            sample_key,
            read_pos: 0,
            is_looping: true,
            ports: PortBuilder::default().audio_out(chans).build(),
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
        permit_alloc(|| {
            // 128 bytes allocated in the load_full. Can we do better?
            let resources = ctx.get_resources();
            if let Some(inner) = resources.get_sample(self.sample_key) {
                let config = ctx.get_config();

                let block_size = config.audio_block_size;
                let chans = self.ports.audio_out.iter().len();

                println!("{}", chans);

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
        })
    }
}

impl Ported for Sampler {
    fn get_ports(&self) -> &Ports {
        &self.ports
    }
}
