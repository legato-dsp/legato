use crate::{
    context::AudioContext,
    midi::MidiMessageKind,
    midi_bus::NodeMidi,
    node::{Inputs, Node},
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
                .midi_in_named(&["midi"])
                .default_in()
                .host_fallback()
                .control_out_named(&["gate", "freq", "vel", "trig"])
                .build(),
            cur_freq: 0.0,
            cur_gate: 0.0,
            cur_vel: 0.0,
        }
    }
}

impl Node for Voice {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, outputs: &mut [&mut [f32]]) {
        let block_size = ctx.get_config().block_size;

        let mut last_sample = 0;
        outputs[3].fill(0.0);

        let NodeMidi { inputs, .. } = ctx.node_midi();
        let channel = inputs
            .port(0)
            .iter()
            .filter(|e| e.msg.channel_idx as usize == self.midi_channel);

        for event in channel {
            let end_sample = (event.frame as usize).min(block_size);

            // Update state from past to now
            if end_sample > last_sample {
                outputs[0][last_sample..end_sample].fill(self.cur_gate);
                outputs[1][last_sample..end_sample].fill(self.cur_freq);
                outputs[2][last_sample..end_sample].fill(self.cur_vel);
            }

            match event.msg.data {
                MidiMessageKind::NoteOn { note, velocity } => {
                    self.cur_freq = mtof(note);
                    self.cur_gate = 1.0;
                    self.cur_vel = velocity as f32 / 127.0;
                    if let Some(trig) = outputs[3].get_mut(end_sample) {
                        *trig = 1.0;
                    }
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
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

/// A middle C constant, used for reference when repitching
pub const MIDDLE_C: f32 = 261.625_58;

/// MIDI note number -> frequency in Hz.
///
/// This is the whole pitch convention of the framework in one line: a *pitch*
/// is a MIDI note number, 69 is A4, and A4 is 440 Hz. Note 0 is C-1, so
/// `note % 12 == 0` is always a C — which is what lets the quantizer treat
/// bit 0 of its scale mask as C. See [`crate::nodes::modular::quantize`].
#[inline(always)]
pub fn mtof(note: u8) -> f32 {
    mtof_f32(note as f32)
}

/// The continuous form of [`mtof`]: fractional and out-of-MIDI-range notes are
/// meaningful, so pitch stays a real-valued signal all the way to the
/// oscillator. `mtof_f32(69.5)` is the quarter-tone above A4, and negative
/// notes descend below C-1 rather than clamping at zero the way `as u8` would.
#[inline(always)]
pub fn mtof_f32(note: f32) -> f32 {
    440.0 * 2.0_f32.powf((note - 69.0) / 12.0)
}

/// Frequency in Hz -> MIDI note number, the inverse of [`mtof_f32`].
///
/// Non-positive frequencies have no pitch; they come back as `f32::NEG_INFINITY`
/// rather than a `NaN` that would poison a signal path.
#[inline(always)]
pub fn ftom(freq: f32) -> f32 {
    if freq <= 0.0 {
        return f32::NEG_INFINITY;
    }
    69.0 + 12.0 * (freq / 440.0).log2()
}

#[derive(Default, Clone, PartialEq, Debug)]
enum VoiceStateKind {
    #[default]
    Idle,
    Active,
}

#[derive(Default, Clone, PartialEq, Debug)]
struct VoiceState {
    kind: VoiceStateKind,
    note: u8,
    velocity: u8,
    last_used: u64,
}

#[derive(Default, Clone, PartialEq)]
struct VoiceAllocator {
    voices: Box<[VoiceState]>,
    counter: u64,
}

impl VoiceAllocator {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            voices: vec![VoiceState::default(); capacity].into(),
            counter: 0,
        }
    }

    fn steal_voice(&mut self) -> (usize, &mut VoiceState) {
        self.counter += 1;

        // 1. Try to find an Idle voice first
        let available_idx = self
            .voices
            .iter()
            .enumerate()
            .find(|(_, x)| x.kind == VoiceStateKind::Idle)
            .map(|(i, _)| i);

        let target_idx = match available_idx {
            Some(idx) => idx,
            None => {
                // 2. All voices active: find the one with the smallest counter (Oldest)
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, x)| x.last_used)
                    .map(|(i, _)| i)
                    .unwrap() // Safe because voices is non-empty
            }
        };

        let state = &mut self.voices[target_idx];
        state.last_used = self.counter;
        (target_idx, state)
    }

    pub fn on_note_on(&mut self, note: u8, velocity: u8) -> Option<usize> {
        // If the note is already playing, re-use that voice and update "last_used"
        if let Some((i, voice)) = self
            .voices
            .iter_mut()
            .enumerate()
            .find(|(_, x)| x.note == note && x.kind == VoiceStateKind::Active)
        {
            self.counter += 1;
            voice.velocity = velocity;
            voice.last_used = self.counter;
            return Some(i);
        }

        // Otherwise, steal
        let (i, state) = self.steal_voice();
        state.note = note;
        state.velocity = velocity;
        state.kind = VoiceStateKind::Active;
        Some(i)
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
}

const PER_VOICE_CHANS: usize = 4;
const TRIG: usize = 3;

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
    last_index_buffers: Box<[usize]>,
    midi_channel: usize,
    ports: Ports,
}

impl PolyVoice {
    pub fn new(voices: usize, midi_channel: usize) -> Self {
        Self {
            voice_allocator: VoiceAllocator::with_capacity(voices),
            port_caches: vec![NodePortCached::default(); voices],
            last_index_buffers: vec![0_usize; voices].into(),
            midi_channel,
            ports: PortBuilder::default()
                .midi_in_named(&["midi"])
                .default_in()
                .host_fallback()
                .control_out_group(&["gate", "freq", "vel", "trig"], voices)
                .build(),
        }
    }
}

impl Node for PolyVoice {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, outputs: &mut [&mut [f32]]) {
        let block_size = ctx.get_config().block_size;

        // Reset last sample buffer. This buffer helps create the slices.
        for idx in self.last_index_buffers.iter_mut() {
            *idx = 0;
        }
        for voice in outputs.chunks_exact_mut(PER_VOICE_CHANS) {
            voice[TRIG].fill(0.0);
        }

        let NodeMidi { inputs, .. } = ctx.node_midi();
        let channel = inputs
            .port(0)
            .iter()
            .filter(|e| e.msg.channel_idx as usize == self.midi_channel);

        for event in channel {
            // Here, we use the voice allocator to figure out which voice we are going to write to.
            // You can think of voices in the same way that tracks are used in the mixer.
            // If we have 3 midi channels here, and 3 voice, we end up with 9 total channels.
            let chan_option = match event.msg.data {
                MidiMessageKind::NoteOn { note, velocity } => {
                    self.voice_allocator.on_note_on(note, velocity)
                }
                MidiMessageKind::NoteOff { note, velocity } => {
                    self.voice_allocator.on_note_off(note, velocity)
                }
                _ => None,
            };

            if let Some(chan_idx) = chan_option {
                let idx = (event.frame as usize).min(block_size);

                let start = chan_idx * PER_VOICE_CHANS;

                let last_index = &mut self.last_index_buffers[chan_idx];

                let state = &mut self.port_caches[chan_idx];

                // Update state from past to now
                if idx > *last_index {
                    outputs[start][*last_index..idx].fill(state.gate);
                    outputs[start + 1][*last_index..idx].fill(state.freq);
                    outputs[start + 2][*last_index..idx].fill(state.vel);
                }

                match event.msg.data {
                    MidiMessageKind::NoteOn { note, velocity } => {
                        state.freq = mtof(note);
                        state.gate = 1.0;
                        state.vel = velocity as f32 / 127.0;
                        if let Some(trig) = outputs[start + TRIG].get_mut(idx) {
                            *trig = 1.0;
                        }
                    }
                    // TODO: Keep velocity and frequency here, as there may be a synth with aftertouch logic
                    MidiMessageKind::NoteOff { note: _, velocity } => {
                        state.gate = 0.0;
                        state.vel = velocity as f32 / 127.0;
                    }
                    // TODO: Pitch bend? Aftertouch logic
                    _ => {}
                }

                *last_index = idx;
            }
        }

        // Finish the slices to the end of the buffer with the current state

        for (i, state) in self.port_caches.iter().enumerate() {
            let last_sample = self.last_index_buffers[i];

            let start = i * PER_VOICE_CHANS;

            if last_sample < block_size {
                outputs[start][last_sample..block_size].fill(state.gate);
                outputs[start + 1][last_sample..block_size].fill(state.freq);
                outputs[start + 2][last_sample..block_size].fill(state.vel);
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl NodeDefinition for Voice {
    const NAME: &'static str = "voice";
    const DESCRIPTION: &'static str = "Decodes MIDI note events on a channel to gate, frequency, velocity, and a one-sample trig on each note-on";
    const REQUIRED_PARAMS: &'static [&'static str] = &["chan"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let channel = p.require_usize("chan")?;
        assert!(channel <= 15);
        Ok(Box::new(Self::new(channel)))
    }
}

impl NodeDefinition for PolyVoice {
    const NAME: &'static str = "poly_voice";
    const DESCRIPTION: &'static str = "Polyphonic MIDI voice decoder outputting per-voice gate, freq, vel, and trig groups; `poly_voice >> voice(*)` wires them to a patch's matching `in` ports";
    const REQUIRED_PARAMS: &'static [&'static str] = &["voices", "chan"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let channel = p.require_usize("chan")?;
        let voices = p.require_usize("voices")?;
        assert!(channel <= 15);
        assert!(
            voices < 10,
            "Currently, a maximum of 32 tracks is supported."
        );
        Ok(Box::new(Self::new(voices, channel)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        harness::{MidiHarness, TimedMidi},
        midi::MidiMessage,
    };

    fn on(frame: u64, note: u8) -> TimedMidi {
        let data = MidiMessageKind::NoteOn {
            note,
            velocity: 100,
        };
        (
            frame,
            MidiMessage {
                data,
                channel_idx: 0,
            },
        )
    }

    fn off(frame: u64, note: u8) -> TimedMidi {
        let data = MidiMessageKind::NoteOff { note, velocity: 0 };
        (
            frame,
            MidiMessage {
                data,
                channel_idx: 0,
            },
        )
    }

    fn pulses(chan: &[f32]) -> Vec<usize> {
        (0..chan.len()).filter(|&n| chan[n] == 1.0).collect()
    }

    #[test]
    fn voice_trig_pulses_once_per_note_on_across_blocks() {
        let out = MidiHarness::new(48_000, 8)
            .midi_in([on(3, 60), off(10, 60), on(10, 62), on(20, 64)])
            .run_streams(Box::new(Voice::new(0)), 24);
        assert_eq!(pulses(&out[3]), vec![3, 10, 20]);
        assert!(out[0][3..].iter().all(|&g| g == 1.0), "gate never drops");
    }

    #[test]
    fn poly_voice_trig_marks_a_same_frame_retrigger_the_gate_hides() {
        let out = MidiHarness::new(48_000, 16)
            .midi_in([on(2, 60), off(9, 60), on(9, 60)])
            .run_streams(Box::new(PolyVoice::new(2, 0)), 16);
        assert_eq!(pulses(&out[TRIG]), vec![2, 9]);
        assert!(out[0][2..].iter().all(|&g| g == 1.0));
        assert!(pulses(&out[PER_VOICE_CHANS + TRIG]).is_empty());
    }

    #[test]
    fn poly_voice_trig_marks_a_stolen_voice() {
        let out = MidiHarness::new(48_000, 16)
            .midi_in([on(0, 60), on(4, 64)])
            .run_streams(Box::new(PolyVoice::new(1, 0)), 16);
        assert_eq!(pulses(&out[TRIG]), vec![0, 4]);
        assert_eq!(out[1][4], mtof(64));
    }
}
