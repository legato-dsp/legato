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

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl NodeDefinition for ExternalInput {
    const NAME: &'static str = "external";
    const DESCRIPTION: &'static str = "Receives audio from an external hardware interface";
    const REQUIRED_PARAMS: &'static [&'static str] = &["interface_name", "chans"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let interface_name = p.get_str("interface_name").expect(
            "Must pass in the name the interface was defined with to the audio_input node!",
        );
        let chans = p
            .get_usize("chans")
            .expect("Must provide chans to audio_input");
        let key = rb.get_audio_input_key(&interface_name).unwrap_or_else(|_| {
            panic!(
                "Could not find AudioInputKey for interface {}",
                interface_name,
            )
        });
        Ok(Box::new(Self::new(chans, key)))
    }
}
