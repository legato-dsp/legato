use std::f32::consts::TAU;

use crate::{
    context::AudioContext,
    msg::{NodeMessage, RtValue},
    node::{Inputs, Node},
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
        let a = get_a_from_cutoff(sr as f32, cutoff);
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
        match msg {
            NodeMessage::SetParam(inner) => match (inner.param_name, inner.value) {
                ("a", RtValue::F32(val)) => self.a = val,
                _ => (),
            },
            _ => (),
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

fn get_a_from_cutoff(sr: f32, fc: f32) -> f32 {
    let x = TAU * fc / sr;
    let pole = x / (x + 1.0);
    1.0 - pole
}
