use crate::{
    context::AudioContext,
    midi::MidiMessageKind,
    node::{Inputs, Node, Outputs},
    ports::{PortBuilder, Ports},
};

#[derive(Clone)]
pub struct Voice {
    midi_channel: usize,
    ports: Ports,
    cur_freq: f32,
    cur_gate: f32,
    cur_vel: f32,
}

impl Voice {
    pub fn new(midi_channel: usize) -> Self {
        Self {
            midi_channel,
            ports: PortBuilder::default()
                .audio_out_named(&["gate", "freq", "velocity"])
                .build(),
            cur_freq: 0.0,
            cur_gate: 0.0,
            cur_vel: 0.0,
        }
    }
}

impl Node for Voice {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, outputs: &mut Outputs) {
        let block_start = ctx.get_instant();

        let cfg = ctx.get_config();
        let block_size = cfg.block_size;
        let fs = cfg.sample_rate as f32;

        let mut last_sample = 0;

        if let Some(store) = ctx.get_midi_store() {
            let res = store.get_channel(self.midi_channel);
            for item in res {
                if item.data == MidiMessageKind::Dummy {
                    continue;
                }

                let offset_duration = block_start.saturating_duration_since(item.instant);
                let idx = (offset_duration.as_secs_f32() * fs) as usize;

                dbg!(idx);

                let end_sample = idx.min(block_size);

                if end_sample > last_sample {
                    outputs[0][last_sample..end_sample].fill(self.cur_gate);
                    outputs[1][last_sample..end_sample].fill(self.cur_freq);
                    outputs[2][last_sample..end_sample].fill(self.cur_vel);
                }

                match item.data {
                    MidiMessageKind::NoteOn { note, velocity } => {
                        self.cur_freq = mtof(note);
                        self.cur_gate = 1.0;
                        self.cur_vel = velocity as f32 / 127.0;
                    }
                    // Keep velocity and frequency here, as there may be a synth with aftertouch logic
                    MidiMessageKind::NoteOff { .. } => {
                        self.cur_gate = 0.0;
                    }
                    _ => {}
                }

                last_sample = end_sample;
            }
            if last_sample < block_size {
                outputs[0][last_sample..block_size].fill(self.cur_gate);
                outputs[1][last_sample..block_size].fill(self.cur_freq);
                outputs[2][last_sample..block_size].fill(self.cur_vel);
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

#[inline]
fn mtof(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}
