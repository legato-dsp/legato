use std::f32::consts::TAU;

use crate::{
    context::AudioContext,
    msg::{NodeMessage, RtValue},
    node::{Inputs, Node},
    persample::PerSampleNode,
    ports::{PortBuilder, Ports},
};

#[derive(Clone, Debug)]
pub struct OnePole {
    a: f32,
    chans: usize,
    state: Vec<f32>,
    ports: Ports,
}

impl OnePole {
    pub fn new(cutoff: f32, chans: usize, sr: usize) -> Self {
        Self::from_coefficient(get_a_from_cutoff(sr as f32, cutoff), chans)
    }

    /// Construct directly from the pole coefficient:
    /// `y[n] = (1 - a) * x[n] + a * y[n-1]`.
    pub fn from_coefficient(a: f32, chans: usize) -> Self {
        Self {
            a,
            chans,
            state: vec![0.0; chans],
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_in_named(&["cutoff"])
                .audio_out(chans)
                .build(),
        }
    }
}

impl Node for OnePole {
    fn process(&mut self, _: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        for c in 0..self.chans {
            if let Some(input_buf) = inputs[c] {
                let chan_out = &mut outputs[c];
                let mut prev_sample = self.state[c];
                for (ins, outs) in input_buf.iter().zip(chan_out.iter_mut()) {
                    let val = ins * (1.0 - self.a) + prev_sample * self.a;
                    *outs = val;
                    prev_sample = val;
                }

                self.state[c] = prev_sample;
            }
        }
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        if let NodeMessage::SetParam(inner) = msg
            && let ("a", RtValue::F32(val)) = (inner.param_name, inner.value)
        {
            self.a = val
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl PerSampleNode for OnePole {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        for c in 0..self.chans {
            if let Some(sample) = in_frame[c] {
                let val = sample * (1.0 - self.a) + self.state[c] * self.a;
                self.state[c] = val;
                out_frame[c] = val;
            }
        }
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        Node::handle_msg(self, msg);
    }
}

fn get_a_from_cutoff(sr: f32, fc: f32) -> f32 {
    let x = TAU * fc / sr;
    let pole = x / (x + 1.0);
    1.0 - pole
}

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl OnePole {
    pub fn from_params(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Self, ValidationError> {
        let chans = p.get_usize("chans").unwrap_or(2);
        // Either a cutoff in Hz, or the pole coefficient `a` directly (handy
        // when porting designs specified as raw one-pole coefficients).
        if let Some(a) = p.get_f32("a") {
            Ok(Self::from_coefficient(a, chans))
        } else {
            let cutoff = p
                .get_f32("cutoff")
                .expect("onepole requires either `cutoff` (Hz) or `a` (pole coefficient)");
            let sr = rb.get_config().sample_rate;
            Ok(Self::new(cutoff, chans, sr))
        }
    }
}

impl NodeDefinition for OnePole {
    const NAME: &'static str = "onepole";
    const DESCRIPTION: &'static str =
        "Single-pole lowpass filter (specify `cutoff` in Hz, or the pole coefficient `a` directly)";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["cutoff", "a", "chans"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        Ok(Box::new(Self::from_params(rb, p)?))
    }
}
