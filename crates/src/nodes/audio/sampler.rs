use crate::{
    context::AudioContext,
    node::{Inputs, Node},
    ports::{PortBuilder, Ports},
    resources::ExternalBufferKey,
};

#[derive(Clone)]
pub struct Sampler {
    sample_key: ExternalBufferKey,
    read_pos: usize,
    is_looping: bool,
    ports: Ports,
}

impl Sampler {
    pub fn new(sample_key: ExternalBufferKey, chans: usize) -> Self {
        Self {
            sample_key,
            read_pos: 0,
            is_looping: true,
            ports: PortBuilder::default().audio_out(chans).build(),
        }
    }
}

impl Node for Sampler {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, ao: &mut [&mut [f32]]) {
        let config = ctx.get_config();
        let block_size = config.block_size;
        let resources = ctx.get_resources();

        let Some(buffer) = resources.get_external_buffer(self.sample_key) else {
            // No sample loaded yet — output silence.
            for chan in ao.iter_mut() {
                chan.fill(0.0);
            }
            return;
        };

        // Derive sample length from any channel (all channels share the same stride).
        let len = buffer.data.len() / buffer.num_channels.max(1);

        if len == 0 {
            for chan in ao.iter_mut() {
                chan.fill(0.0);
            }
            return;
        }

        for (c, chan_out) in ao.iter_mut().enumerate() {
            // channel() gives us the planar slice for channel c.
            let src = buffer.channel(c);
            for (n, out) in chan_out.iter_mut().enumerate() {
                let i = self.read_pos + n;
                *out = if i < len {
                    src[i]
                } else if self.is_looping {
                    src[i % len]
                } else {
                    0.0
                };
            }
        }

        self.read_pos = if self.is_looping {
            (self.read_pos + block_size) % len
        } else {
            (self.read_pos + block_size).min(len)
        };
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}
