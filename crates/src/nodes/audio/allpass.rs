use crate::{
    context::AudioContext,
    msg::{NodeMessage, RtValue},
    node::{Inputs, Node},
    ports::{PortBuilder, Ports},
    ring::RingBuffer,
};

#[derive(Clone, Debug)]
pub struct Allpass {
    feedback: f32,
    delay_length_samples: f32,
    delay_lines: Vec<RingBuffer>,
    capacity: usize,
    chans: usize,
    ports: Ports,
}

impl Allpass {
    pub fn new(chans: usize, feedback: f32, delay_length_samples: f32, capacity: usize) -> Self {
        let delay_lines = vec![RingBuffer::new(capacity); chans];

        Self {
            feedback: feedback.clamp(0.0, 0.98),
            delay_length_samples,
            delay_lines,
            capacity,
            chans,
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_in_named(&["delay_length", "feedback"])
                .audio_out(chans)
                .build(),
        }
    }
}

impl Node for Allpass {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let sr = ctx.get_config().sample_rate as f32;

        // If we have 2 input channels, delay length is channel 3, so idx 2
        let delay_length_idx = self.chans;
        let feedback_idx = self.chans + 1;

        let max_capacity = self.capacity as f32;

        let delay_length_port = inputs.get(delay_length_idx).and_then(|x| *x);
        let feedback_port = inputs.get(feedback_idx).and_then(|x| *x);

        // TODO: Consider branching once for modulation invariant. Likely branch prediction is good here, so may not be needed.
        for (c, chan_state) in &mut self.delay_lines.iter_mut().enumerate() {
            if let Some(input) = inputs.get(c).and_then(|x| *x) {
                let output = &mut outputs[c];

                for i in 0..input.len() {
                    let delay_length_samples = delay_length_port
                        .map_or(self.delay_length_samples, |buf| {
                            let delay_length_ms = buf[i];
                            sr * (delay_length_ms / 1000.0)
                        })
                        .clamp(1.0, max_capacity);

                    let feedback = feedback_port
                        .map_or(self.feedback, |buf| buf[i])
                        .clamp(0.0, 0.98);

                    let delayed = chan_state.get_delay_cubic(delay_length_samples);
                    let write = input[i] + feedback * delayed;

                    chan_state.push(write);
                    output[i] = delayed - write * feedback;
                }
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
    fn handle_msg(&mut self, msg: NodeMessage) {
        if let NodeMessage::SetParam(inner) = msg {
            match (inner.param_name, inner.value) {
                ("feedback", RtValue::F32(val)) => self.feedback = val.clamp(0.0, 0.98),
                ("feedback", RtValue::U32(val)) => self.feedback = (val as f32).clamp(0.0, 0.98),
                ("delay_length", RtValue::F32(val)) => {
                    self.delay_length_samples = val.clamp(0.0, self.capacity as f32)
                }
                ("delay_length", RtValue::U32(val)) => {
                    self.delay_length_samples = (val as f32).clamp(0.0, self.capacity as f32)
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

impl NodeDefinition for Allpass {
    const NAME: &'static str = "allpass";
    const DESCRIPTION: &'static str = "Allpass filter with configurable delay length and feedback";
    const REQUIRED_PARAMS: &'static [&'static str] = &["delay_length", "feedback", "chans"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["capacity"];

    fn create(rb: &mut ResourceBuilderView, p: &DSLParams) -> Result<Box<dyn DynNode>, ValidationError> {
        use std::time::Duration;
        let config = rb.get_config();
        let sr = config.sample_rate;
        let chans = p.get_usize("chans").unwrap_or(2);
        let delay_length = p
            .get_duration_ms("delay_length")
            .unwrap_or(Duration::from_millis(200));
        let delay_length_samples = sr as f32 * delay_length.as_secs_f32();
        let feedback = p.get_f32("feedback").unwrap_or(0.5);
        let mut capacity = p.get_usize("capacity").unwrap_or(sr);
        if capacity < (delay_length_samples as usize) {
            capacity = (delay_length_samples as usize) * 2;
        }
        Ok(Box::new(Self::new(chans, feedback, delay_length_samples, capacity)))
    }
}
