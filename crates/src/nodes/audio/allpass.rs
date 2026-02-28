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
        // TODO: This could break
        let sr = ctx.get_config().sample_rate as f32;

        // If we have 2 input channels, delay length is channel 3, so idx 2
        let delay_length_idx = self.chans;
        let feedback_idx = self.chans + 1;

        let delay_length_port = inputs.get(delay_length_idx).and_then(|x| *x);
        let feedback_port = inputs.get(feedback_idx).and_then(|x| *x);

        // TODO: Consider branching once for modulation invariant. Likely branch prediction is good here, so may not be needed.
        // TODO: Check capacity, delay length, etc.
        for c in 0..self.chans {
            let chan_state = &mut self.delay_lines[c];

            if let Some(input) = inputs.get(c).and_then(|x| *x) {
                let output = &mut outputs[c];

                for i in 0..input.len() {
                    let delay_length_samples =
                        delay_length_port.map_or(self.delay_length_samples, |buf| {
                            let delay_length_ms = buf[i];
                            sr * (delay_length_ms / 1000.0)
                        }); // TODO: Clamp to capacity

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
        match msg {
            NodeMessage::SetParam(inner) => match (inner.param_name, inner.value) {
                ("feedback", RtValue::F32(val)) => self.feedback = val.clamp(0.0, 0.98),
                ("feedback", RtValue::U32(val)) => self.feedback = (val as f32).clamp(0.0, 0.98),
                ("delay_length", RtValue::F32(val)) => {
                    self.delay_length_samples = (val as f32).clamp(0.0, self.capacity as f32)
                }
                ("delay_length", RtValue::U32(val)) => {
                    self.delay_length_samples = (val as f32).clamp(0.0, self.capacity as f32)
                }
                _ => (),
            },
            _ => (),
        }
    }
}
