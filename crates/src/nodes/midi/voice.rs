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

                let offset_duration = block_start - item.instant;

                let idx = (offset_duration.as_secs_f32() * fs) as usize;

                let end_sample = idx.min(block_size);

                // Update state from past to now
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
                    // TODO: Pitch bend? Aftertouch logic
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

#[inline(always)]
fn mtof(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}

#[derive(Default, Clone, PartialEq)]
enum VoiceStateKind {
    #[default]
    Idle,
    Active,
}

#[derive(Default, Clone, PartialEq)]
struct VoiceState {
    kind: VoiceStateKind,
    note: u8,
    velocity: u8,
    // TODO: Pitch Shift
}

#[derive(Default, Clone, PartialEq)]
struct VoiceAllocator {
    voices: Box<[VoiceState]>,
}

impl VoiceAllocator {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            voices: vec![VoiceState::default(); capacity].into(),
        }
    }

    fn steal_voice(&mut self) -> Option<(usize, &mut VoiceState)> {
        self.voices
            .iter_mut()
            .enumerate()
            .fold(None, |candidate, (i, voice)| {
                // If we just have an available voice take it
                if voice.kind == VoiceStateKind::Idle {
                    return Some((i, voice));
                }
                match candidate {
                    Some(v) => Some(if voice.velocity < v.1.velocity {
                        (i, voice)
                    } else {
                        v
                    }),
                    None => Some((i, voice)),
                }
            })
    }

    fn on_note_off(&mut self, note: u8, velocity: u8) -> Option<usize> {
        if let Some((i, inner)) = self
            .voices
            .iter_mut()
            .enumerate()
            .find(|(_, x)| x.note == note)
        {
            inner.kind = VoiceStateKind::Idle;
            inner.note = note;
            inner.velocity = velocity;

            return Some(i);
        }

        None
    }

    fn on_note_on(&mut self, note: u8, velocity: u8) -> Option<usize> {
        let voice = self.steal_voice();
        if let Some((i, inner)) = voice {
            inner.note = note;
            inner.velocity = velocity;
            inner.kind = VoiceStateKind::Active;

            return Some(i);
        }

        None
    }
}

const PER_VOICE_CHANS: usize = 3; // Current amount of chans per voice

#[derive(Default, Clone)]
struct NodePortCached {
    gate: f32,
    freq: f32,
    vel: f32,
}

#[derive(Clone)]
pub struct PolyVoice {
    voice_allocator: VoiceAllocator,
    port_caches: Vec<NodePortCached>,
    last_sample_buffers: Box<[usize]>,
    midi_channel: usize,
    ports: Ports,
}

impl PolyVoice {
    pub fn new(voices: usize, midi_channel: usize) -> Self {
        Self {
            voice_allocator: VoiceAllocator::with_capacity(voices),
            port_caches: vec![NodePortCached::default(); voices],
            last_sample_buffers: vec![0_usize; voices].into(),
            midi_channel,
            ports: PortBuilder::default()
                .audio_out(voices * PER_VOICE_CHANS)
                .build(),
        }
    }
}

impl Node for PolyVoice {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, outputs: &mut Outputs) {
        let block_start = ctx.get_instant();

        let cfg = ctx.get_config();
        let block_size = cfg.block_size;
        let fs = cfg.sample_rate as f32;

        // Reset last sample buffer. This buffer helps create the slices.
        for idx in self.last_sample_buffers.iter_mut() {
            *idx = 0;
        }

        if let Some(store) = ctx.get_midi_store() {
            let res = store.get_channel(self.midi_channel);

            for item in res {
                if item.data == MidiMessageKind::Dummy {
                    continue;
                }

                // Here, we use the voice allocator to figure out which voice we are going to write to.
                // You can think of voices in the same way that tracks are used in the mixer.
                // If we have 3 midi channels here, and 3 voice, we end up with 9 total channels.
                let chan_option = match item.data {
                    MidiMessageKind::NoteOn { note, velocity } => {
                        self.voice_allocator.on_note_on(note, velocity)
                    }
                    MidiMessageKind::NoteOff { note, velocity } => {
                        self.voice_allocator.on_note_off(note, velocity)
                    }
                    _ => None,
                };

                if let Some(chan_idx) = chan_option {
                    let offset_duration = block_start - item.instant;

                    let idx = (offset_duration.as_secs_f32() * fs) as usize;

                    let end_sample = idx.min(block_size);

                    let last_sample = &mut self.last_sample_buffers[chan_idx];

                    let state = &mut self.port_caches[chan_idx];

                    let start = chan_idx * PER_VOICE_CHANS;

                    // Update state from past to now
                    if end_sample > *last_sample {
                        outputs[start][*last_sample..end_sample].fill(state.gate);
                        outputs[start + 1][*last_sample..end_sample].fill(state.freq);
                        outputs[start + 2][*last_sample..end_sample].fill(state.vel);
                    }

                    match item.data {
                        MidiMessageKind::NoteOn { note, velocity } => {
                            state.freq = mtof(note);
                            state.gate = 1.0;
                            state.vel = velocity as f32 / 127.0;
                        }
                        // TODO: Keep velocity and frequency here, as there may be a synth with aftertouch logic
                        MidiMessageKind::NoteOff { .. } => {
                            state.gate = 0.0;
                        }
                        // TODO: Pitch bend? Aftertouch logic
                        _ => {}
                    }

                    *last_sample = end_sample;
                }
            }

            // Finish the slices to the end of the buffer with the current state

            for (i, state) in self.port_caches.iter().enumerate() {
                let last_sample = self.last_sample_buffers[i];

                let start = i * PER_VOICE_CHANS;

                if last_sample < block_size {
                    outputs[start][last_sample..block_size].fill(state.gate);
                    outputs[start + 1][last_sample..block_size].fill(state.freq);
                    outputs[start + 2][last_sample..block_size].fill(state.vel);
                }
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}
