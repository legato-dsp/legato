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
}

impl Voice {
    pub fn new(midi_channel: usize) -> Self {
        Self {
            midi_channel,
            ports: PortBuilder::default()
                .audio_out_named(&["gate", "freq", "velocity"])
                .build(),
        }
    }
}

impl Node for Voice {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, outputs: &mut Outputs) {
        if let Some(store) = ctx.get_midi_store() {
            let res = store.get_channel(self.midi_channel);
            for item in res {
                match item {
                    MidiMessageKind::Dummy => {}
                    _ => {
                        dbg!(item);
                    }
                };
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
