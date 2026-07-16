use crate::{
    context::AudioContext,
    msg::{NodeMessage, RtValue},
    node::{Inputs, Node},
    persample::PerSampleNode,
    ports::{PortBuilder, Ports},
    resources::{delay::ResourceDelay, window::Window},
};

#[derive(Clone)]
pub struct DelayTap {
    /// Underlying flat allocation for all chans
    data: Box<[f32]>,
    /// One cursor per chan
    delays: Vec<ResourceDelay>,
    /// Power of 2 per-channel cap.
    cap: usize,
    delay_length_samples: f32,
    chans: usize,
    sr: f32,
    ports: Ports,
}

impl DelayTap {
    pub fn new(chans: usize, delay_length_samples: f32, cap: usize, sr: f32) -> Self {
        let cap = cap.next_power_of_two();
        let data = vec![0.0; chans * cap].into_boxed_slice();
        let delays = (0..chans)
            .map(|c| {
                ResourceDelay::new(Window {
                    start: c * cap,
                    len: cap,
                })
            })
            .collect();

        Self {
            data,
            delays,
            cap,
            delay_length_samples,
            chans,
            sr,
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_in_named(&["delay_length"])
                .audio_out(chans)
                .build(),
        }
    }
}

impl PerSampleNode for DelayTap {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        let max_capacity = self.cap as f32;

        let delay_length_samples = in_frame[self.chans]
            .map_or(self.delay_length_samples, |ms| self.sr * (ms / 1000.0))
            .clamp(1.0, max_capacity);

        for c in 0..self.chans {
            if let Some(input) = in_frame[c] {
                let base = c * self.cap;
                let chan_data = &mut self.data[base..base + self.cap];
                let delay = &mut self.delays[c];

                delay.push(chan_data, input);
                out_frame[c] = delay.get_delay_cubic(chan_data, delay_length_samples);
            }
        }
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        Node::handle_msg(self, msg);
    }
}

impl Node for DelayTap {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let sr = ctx.get_config().sample_rate as f32;

        let delay_length_idx = self.chans;
        let max_capacity = self.cap as f32;
        let delay_length_port = inputs.get(delay_length_idx).and_then(|x| *x);

        for c in 0..self.chans {
            let input = inputs[c].unwrap();
            let base = c * self.cap;
            let chan_data = &mut self.data[base..base + self.cap];
            let delay = &mut self.delays[c];
            let output = &mut outputs[c];

            for i in 0..input.len() {
                let delay_length_samples = delay_length_port
                    .map_or(self.delay_length_samples, |buf| sr * (buf[i] / 1000.0))
                    .clamp(1.0, max_capacity);

                delay.push(chan_data, input[i]);
                output[i] = delay.get_delay_cubic(chan_data, delay_length_samples);
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
    fn handle_msg(&mut self, msg: NodeMessage) {
        if let NodeMessage::SetParam(inner) = msg {
            match (inner.param_name, inner.value) {
                ("delay_length", RtValue::F32(val)) => {
                    self.delay_length_samples = val.clamp(0.0, self.cap as f32)
                }
                ("delay_length", RtValue::U32(val)) => {
                    self.delay_length_samples = (val as f32).clamp(0.0, self.cap as f32)
                }
                _ => (),
            }
        }
    }
}

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl DelayTap {
    pub fn from_params(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Self, ValidationError> {
        use std::time::Duration;
        let config = rb.get_config();
        let sr = config.sample_rate;
        let chans = p.get_usize("chans").unwrap_or(2);
        let delay_length = p
            .get_duration_ms("delay_length")
            .unwrap_or(Duration::from_millis(50));
        let delay_length_samples = sr as f32 * delay_length.as_secs_f32();
        let mut capacity = p.get_usize("capacity").unwrap_or(sr);
        if capacity < (delay_length_samples as usize) {
            capacity = (delay_length_samples as usize) * 2;
        }
        Ok(Self::new(chans, delay_length_samples, capacity, sr as f32))
    }
}

impl NodeDefinition for DelayTap {
    const NAME: &'static str = "tap";
    const DESCRIPTION: &'static str =
        "Single-tap delay line (no feedback) with cubic interpolation and modulatable delay";
    const REQUIRED_PARAMS: &'static [&'static str] = &["delay_length", "chans"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["capacity"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        Ok(Box::new(Self::from_params(rb, p)?))
    }
}
