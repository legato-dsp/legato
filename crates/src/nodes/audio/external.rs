use crate::{
    context::AudioContext,
    node::{Inputs, Node},
    ports::{PortBuilder, Ports},
    resources::AudioInputKey,
};

#[derive(Clone)]
pub struct ExternalInput {
    key: AudioInputKey,
    ports: Ports,
}

impl ExternalInput {
    pub fn new(chans: usize, key: AudioInputKey) -> Self {
        Self {
            key,
            ports: PortBuilder::default().audio_out(chans).build(),
        }
    }
}

impl Node for ExternalInput {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, outputs: &mut [&mut [f32]]) {
        let resources = ctx.get_resources_mut();

        // Simply copy the audio out
        for (i, chan_out) in outputs.iter_mut().enumerate() {
            let incoming_chan = resources.get_audio_input_chan(self.key, i);
            chan_out.copy_from_slice(incoming_chan);
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}
