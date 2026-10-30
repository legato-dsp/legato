use crate::{
    builder::{ResourceBuilderView, ValidationError},
    context::AudioContext,
    dsl::ir::DSLParams,
    msg::{NodeMessage, RtValue},
    node::{DynNode, Inputs, Node},
    persample::PerSampleNode,
    ports::{PortBuilder, Ports},
    spec::NodeDefinition,
};

/// Outputs `val` on every channel.
#[derive(Clone)]
pub struct Constant {
    val: f32,
    ports: Ports,
}

impl Constant {
    pub fn new(val: f32, chans: usize) -> Self {
        Self {
            val,
            ports: PortBuilder::default().control_out(chans).build(),
        }
    }

    pub fn from_params(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Self, ValidationError> {
        let val = p.get_f32("val")?.unwrap_or(0.0);
        let chans = p.get_usize("chans")?.unwrap_or(1);
        Ok(Self::new(val, chans))
    }
}

impl Node for Constant {
    fn process(&mut self, _: &mut AudioContext, _: &Inputs, outputs: &mut [&mut [f32]]) {
        for out in outputs.iter_mut() {
            out.fill(self.val);
        }
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        if let NodeMessage::SetParam(payload) = msg
            && let ("val", RtValue::F32(val)) = (payload.param_name, payload.value)
        {
            self.val = val;
        }
    }

    fn runtime_params(&self) -> Option<Vec<&str>> {
        Some(vec!["val"])
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl PerSampleNode for Constant {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, _: &[Option<f32>], out_frame: &mut [f32]) {
        out_frame.fill(self.val);
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        Node::handle_msg(self, msg);
    }

    fn runtime_params(&self) -> Option<Vec<&str>> {
        Node::runtime_params(self)
    }
}

impl NodeDefinition for Constant {
    const NAME: &'static str = "constant";
    const DESCRIPTION: &'static str =
        "Outputs a fixed value, settable at runtime with a `val` message";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["val", "chans"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        Ok(Box::new(Self::from_params(rb, p)?))
    }
}
