use crate::{
    context::AudioContext,
    msg::{NodeMessage, RtValue},
    node::{Inputs, Node},
    ports::{PortBuilder, Ports},
};

/// A single step in the sequencer.
#[derive(Clone, Debug)]
pub struct SequencerStep {
    pub freq: f32,
    pub vel: f32,
    /// 0.0 is muted, 1.0 fires
    pub gate: f32,
    /// Portion of the step duration the gate is held high. Range [0.0, 1.0].
    pub length: f32,
}

impl Default for SequencerStep {
    fn default() -> Self {
        Self {
            freq: 440.0,
            vel: 0.0,
            gate: 0.0,
            length: 0.0,
        }
    }
}

const MAXIMUM_SIZE: usize = 256;

#[derive(Clone)]
pub struct StepSequencer {
    steps: Box<[SequencerStep]>,
    num_steps: usize, // Essentially, we take the first 0..num_steps, so we can preallocate the max step size
    ports: Ports,
}

impl StepSequencer {
    pub fn new(num_steps: usize) -> Self {
        let ports = PortBuilder::default()
            .audio_in_named(&["phasor"])
            .audio_out_named(&["freq", "vel", "gate"])
            .build();

        Self {
            steps: vec![SequencerStep::default(); MAXIMUM_SIZE].into(),
            num_steps,
            ports,
        }
    }

    #[inline(always)]
    fn step_index(&self, phase: f32) -> usize {
        let num_steps = self.steps.len();
        // map range of phasor (0,1) to (0,num_steps)
        let idx = (phase.min(0.999_999) * num_steps as f32).floor() as usize;
        // Clamp index to last elemenet
        idx.min(num_steps - 1)
    }

    #[inline(always)]
    fn phase_within_step(&self, phase: f32) -> f32 {
        (phase * self.steps.len().min(self.num_steps - 1) as f32).fract()
    }
}

impl Node for StepSequencer {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let block_size = ctx.get_config().block_size;
        let phasor_in = inputs[0].expect("StepSequencer requires a phasor!");

        let (freq_out, rest) = outputs.split_at_mut(1);
        let (vel_out, rest) = rest.split_at_mut(1);
        let (gate_out, _) = rest.split_at_mut(1);

        let freq_out = &mut freq_out[0];
        let vel_out = &mut vel_out[0];
        let gate_out = &mut gate_out[0];

        for n in 0..block_size {
            let phase = phasor_in[n];
            let idx = self.step_index(phase).min(self.num_steps - 1);
            let step = &self.steps[idx];
            // local_phase is the interpolation between steps, so 0.5 is half the gap between the two steps
            let local_phase = self.phase_within_step(phase);

            freq_out[n] = step.freq;
            vel_out[n] = step.vel;
            gate_out[n] = if step.gate > 0.0 && local_phase < step.length {
                1.0
            } else {
                0.0
            };
        }
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        match msg {
            NodeMessage::SetParam(inner) => match (inner.param_name, inner.value) {
                ("num_steps", RtValue::U32(n)) => {
                    self.num_steps = (n as usize).max(self.steps.len())
                }
                _ => (),
            },
            NodeMessage::SetStep(payload) => {
                if let Some(step) = self.steps.get_mut(payload.index) {
                    if let Some(v) = payload.freq {
                        step.freq = v;
                    }
                    if let Some(v) = payload.vel {
                        step.vel = v;
                    }
                    if let Some(v) = payload.gate {
                        step.gate = v;
                    }
                    if let Some(v) = payload.length {
                        step.length = v.clamp(0.0, 1.0);
                    }
                }
            }
            _ => (),
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}
