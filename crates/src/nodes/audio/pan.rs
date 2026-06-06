use crate::{
    builder::{ResourceBuilderView, ValidationError},
    context::AudioContext,
    dsl::ir::DSLParams,
    msg::{NodeMessage, RtValue},
    node::{DynNode, Inputs, Node},
    ports::{PortBuilder, Ports},
    spec::NodeDefinition,
};

#[derive(Clone)]
pub struct Pan {
    pan: f32, // 0.0 = left, 0.5 = center, 1.0 = right
    ports: Ports,
}

impl Pan {
    pub fn new(pan: f32) -> Self {
        Self {
            pan: pan.clamp(0.0, 1.0),
            ports: PortBuilder::default()
                .audio_in(1)
                .audio_in_named(&["pan"])
                .audio_out(2)
                .build(),
        }
    }
}

impl Node for Pan {
    fn process(&mut self, _ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let input = inputs
            .first()
            .and_then(|x| *x)
            .expect("No mono input for pan node!");

        let pan_port = inputs.get(1).and_then(|x| *x);

        let (left, right) = outputs.split_at_mut(1);

        for i in 0..input.len() {
            let pan = pan_port.map_or(self.pan, |buf| buf[i]).clamp(0.0, 1.0);

            let angle = pan * std::f32::consts::FRAC_PI_2;
            left[0][i] = input[i] * angle.cos();
            right[0][i] = input[i] * angle.sin();
        }
    }
    fn handle_msg(&mut self, msg: crate::msg::NodeMessage) {
        if let NodeMessage::SetParam(payload) = msg {
            match (payload.param_name, payload.value) {
                ("pan", RtValue::F32(val)) => self.pan = val.clamp(0.0, 1.0),
                _ => unreachable!("Invalid parameter and value passed"),
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl NodeDefinition for Pan {
    const NAME: &'static str = "pan";
    const DESCRIPTION: &'static str = "A mono to stereo panning node. 0.0 is left, 1.0 is right.";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["pan"];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let pan = p.get_f32("freq").unwrap_or(0.5);

        Ok(Box::new(Self::new(pan)))
    }
}
