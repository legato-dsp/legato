use std::time::Duration;

use crate::{
    context::AudioContext,
    midi::{MidiMessage, MidiMessageKind},
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
pub struct MidiSequencer {
    midi_chan: u8,
    last_idx: usize,
    held_note: Option<u8>,
    note_off_sent: bool,
    steps: Box<[SequencerStep]>,
    num_steps: usize, // Essentially, we take the first 0..num_steps, so we can preallocate the max step size
    ports: Ports,
}

impl MidiSequencer {
    pub fn new(midi_chan: u8, num_steps: usize) -> Self {
        let ports = PortBuilder::default()
            .audio_in_named(&["phasor"])
            .audio_out_named(&["freq", "vel", "gate"])
            .build();

        Self {
            midi_chan,
            last_idx: 0,
            held_note: None,
            note_off_sent: false,
            steps: vec![SequencerStep::default(); MAXIMUM_SIZE].into(),
            num_steps,
            ports,
        }
    }

    #[inline(always)]
    fn step_index(&self, phase: f32) -> usize {
        let num_steps = self.num_steps;
        // map range of phasor (0,1) to (0,num_steps)
        let idx = (phase.min(0.999_999) * num_steps as f32).floor() as usize;
        // Clamp index to last elemenet
        idx.min(num_steps - 1)
    }
}

impl Node for MidiSequencer {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, _outputs: &mut [&mut [f32]]) {
        let phasor_in = inputs[0].expect("MidiSequencer requires a phasor!");

        let cfg = ctx.get_config();

        let sr = cfg.sample_rate;
        let block_size = cfg.block_size;

        let block_start = ctx.get_instant();
        for n in 0..block_size {
            let phase = phasor_in[n];
            let idx = self.step_index(phase);
            let local_phase = (phase * self.num_steps as f32).fract();

            let when = block_start + Duration::from_secs_f32(n as f32 / sr as f32);

            // Step edge: send NoteOff for previous note, NoteOn for new step
            if idx != self.last_idx {
                self.note_off_sent = false;

                if let Some(prev_note) = self.held_note.take() {
                    let _ = ctx.send_to_system_midi(
                        MidiMessage {
                            data: MidiMessageKind::NoteOff {
                                note: prev_note,
                                velocity: 0,
                            },
                            instant: when,
                            channel_idx: self.midi_chan,
                        },
                        when,
                    );
                }

                let step = &self.steps[idx];
                if step.gate > 0.0 {
                    let note = ftom(step.freq);
                    let _ = ctx.send_to_system_midi(
                        MidiMessage {
                            data: MidiMessageKind::NoteOn {
                                note,
                                velocity: (step.vel * 127.0) as u8,
                            },
                            instant: when,
                            channel_idx: self.midi_chan,
                        },
                        when,
                    );
                    self.held_note = Some(note);
                }

                self.last_idx = idx;
            }

            // Within-step NoteOff based on length
            if !self.note_off_sent {
                let step = &self.steps[idx];
                if local_phase >= step.length {
                    if let Some(note) = self.held_note.take() {
                        let _ = ctx.send_to_system_midi(
                            MidiMessage {
                                data: MidiMessageKind::NoteOff { note, velocity: 0 },
                                instant: when,
                                channel_idx: self.midi_chan,
                            },
                            when,
                        );
                    }
                    self.note_off_sent = true;
                }
            }
        }
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        match msg {
            NodeMessage::SetParam(inner) => {
                if let ("num_steps", RtValue::U32(n)) = (inner.param_name, inner.value) {
                    self.num_steps = (n as usize).min(MAXIMUM_SIZE)
                }
            }
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

/// I am not dealing with pitch bend for the time being
fn ftom(freq: f32) -> u8 {
    (69.0 + 12.0 * (freq / 440.0).log2())
        .round()
        .clamp(0.0, 127.0) as u8
}

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl NodeDefinition for MidiSequencer {
    const NAME: &'static str = "midi_sequencer";
    const DESCRIPTION: &'static str =
        "Midi step sequencer sending note information to the selected midi channel";
    const REQUIRED_PARAMS: &'static [&'static str] = &["num_steps", "midi_chan"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let midi_chan = p
            .get_usize("midi_chan")
            .expect("Must pass midi_chan to MidiSequencer!");

        let num_steps = p
            .get_usize("num_steps")
            .expect("Must pass num_steps to sequencer");
        Ok(Box::new(Self::new(
            midi_chan
                .try_into()
                .expect("Could not cast midi channel to u8!"),
            num_steps,
        )))
    }
}
