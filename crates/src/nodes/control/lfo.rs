use crate::{
    builder::{ResourceBuilderView, ValidationError},
    context::AudioContext,
    dsl::ir::DSLParams,
    msg::NodeMessage,
    node::{DynNode, Inputs, Node},
    nodes::audio::sine::Sine,
    ports::{PortBuilder, Ports},
    spec::NodeDefinition,
};

/// A sine oscillator with a control-kind output, for use as a modulation source.
///
/// Delegates DSP to an inner `Sine`; only the port kinds differ so that
/// `lfo >> map` auto-maps as a same-kind control wire.
#[derive(Clone)]
pub struct Lfo {
    inner: Sine,
    ports: Ports,
}

impl Lfo {
    pub fn from_params(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Self, ValidationError> {
        let inner = Sine::from_params(rb, p)?;
        Ok(Self {
            inner,
            ports: PortBuilder::default()
                .control_in_named(&["freq"])
                .control_out(1)
                .build(),
        })
    }
}

impl Node for Lfo {
    fn process(&mut self, ctx: &mut AudioContext, ai: &Inputs, ao: &mut [&mut [f32]]) {
        self.inner.process(ctx, ai, ao);
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        Node::handle_msg(&mut self.inner, msg);
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl NodeDefinition for Lfo {
    const NAME: &'static str = "lfo";
    const DESCRIPTION: &'static str = "Sine modulation source with a control-kind output";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["freq", "quality", "phase"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        Ok(Box::new(Self::from_params(rb, p)?))
    }
}
