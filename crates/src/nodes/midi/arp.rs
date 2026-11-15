use crate::{
    builder::{ResourceBuilderView, ValidationError},
    context::AudioContext,
    dsl::ir::DSLParams,
    midi::{MidiBuffer, MidiMessage, MidiMessageKind},
    midi_bus::NodeMidi,
    node::{DynNode, Inputs, Node},
    ports::{PortBuilder, Ports},
    rng::XorShift,
    spec::NodeDefinition,
};

/// Notes held at once; further note-ons are ignored until one is released.
pub const MAX_HELD: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpMode {
    Up,
    Down,
    UpDown,
    Random,
    /// The order the notes were pressed in.
    Played,
}

impl ArpMode {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "up" => ArpMode::Up,
            "down" => ArpMode::Down,
            "updown" => ArpMode::UpDown,
            "random" => ArpMode::Random,
            "played" => ArpMode::Played,
            _ => return None,
        })
    }
}

/// How long each arpeggiated note is held.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gate {
    /// A fraction of the step, so it follows the clock's tempo.
    StepFraction(f32),
    Samples(u32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Held {
    chan: u8,
    note: u8,
    velocity: u8,
}

#[derive(Clone, Copy, Debug)]
struct Sounding {
    held: Held,
    /// Samples left, for [`Gate::Samples`].
    remaining: u32,
}

/// Steps through the held notes on each step of a phasor.
///
/// The phasor is divided into `steps`, the same way the step sequencers read
/// it, so the arpeggio stays locked to every other clocked node.
pub struct Arp {
    mode: ArpMode,
    octaves: u8,
    steps: f32,
    gate: Gate,
    held: [Held; MAX_HELD],
    held_len: usize,
    step: usize,
    last_step_idx: Option<usize>,
    sounding: Option<Sounding>,
    rng: XorShift,
    ports: Ports,
}

impl Arp {
    pub fn new(mode: ArpMode, octaves: u8, steps: usize, gate: Gate, rng: XorShift) -> Self {
        Self {
            mode,
            octaves: octaves.max(1),
            steps: steps.max(1) as f32,
            gate,
            held: [Held::default(); MAX_HELD],
            held_len: 0,
            step: 0,
            last_step_idx: None,
            sounding: None,
            rng,
            ports: PortBuilder::default()
                .midi_in_named(&["notes"])
                .default_in()
                .host_fallback()
                .control_in_named(&["phasor"])
                .default_in()
                .required_in()
                .midi_out_named(&["out"])
                .default_out()
                .build(),
        }
    }

    fn apply(&mut self, msg: MidiMessage) {
        let held = &self.held[..self.held_len];
        let existing = |note| {
            held.iter()
                .position(|h| h.chan == msg.channel_idx && h.note == note)
        };
        match msg.data {
            MidiMessageKind::NoteOn { note, velocity } => match existing(note) {
                Some(i) => self.held[i].velocity = velocity,
                None if self.held_len < MAX_HELD => {
                    self.held[self.held_len] = Held {
                        chan: msg.channel_idx,
                        note,
                        velocity,
                    };
                    self.held_len += 1;
                }
                None => {}
            },
            MidiMessageKind::NoteOff { note, .. } => {
                if let Some(i) = existing(note) {
                    self.held.copy_within(i + 1..self.held_len, i);
                    self.held_len -= 1;
                }
            }
            _ => {}
        }
    }

    /// The note for this step, from the held notes spread over `octaves`.
    fn next_note(&mut self) -> Option<Held> {
        if self.held_len == 0 {
            return None;
        }
        let mut order = self.held;
        let order = &mut order[..self.held_len];
        if self.mode != ArpMode::Played {
            order.sort_unstable_by_key(|h| (h.note, h.chan));
        }

        let len = order.len() * self.octaves as usize;
        let position = match self.mode {
            ArpMode::Up | ArpMode::Played => self.step % len,
            ArpMode::Down => len - 1 - self.step % len,
            ArpMode::UpDown if len > 1 => {
                let i = self.step % (2 * len - 2);
                if i < len { i } else { 2 * len - 2 - i }
            }
            ArpMode::UpDown => 0,
            ArpMode::Random => self.rng.next_u32() as usize % len,
        };
        self.step = self.step.wrapping_add(1);

        let base = order[position % order.len()];
        let octave = (position / order.len()) as u8;
        let note = base.note.checked_add(12 * octave).filter(|&n| n < 128)?;
        Some(Held { note, ..base })
    }

    fn release(&mut self, out: &mut MidiBuffer, frame: u32) {
        if let Some(s) = self.sounding.take() {
            out.note_off(frame, s.held.chan, s.held.note);
        }
    }
}

impl Node for Arp {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, _: &mut [&mut [f32]]) {
        let phasor = inputs[0].expect("arp requires a phasor");
        let NodeMidi {
            inputs: midi_in,
            mut outputs,
        } = ctx.node_midi();
        let out = outputs.port(0);
        let mut notes = midi_in.port(0).iter().peekable();

        for (n, &phase) in phasor.iter().enumerate() {
            let frame = n as u32;
            while let Some(event) = notes.next_if(|e| e.frame == frame) {
                self.apply(event.msg);
            }

            let scaled = phase.clamp(0.0, 0.999_999) * self.steps;
            let step_idx = scaled as usize;

            if self.last_step_idx != Some(step_idx) {
                self.last_step_idx = Some(step_idx);
                self.release(out, frame);
                if let Some(held) = self.next_note() {
                    out.note_on(frame, held.chan, held.note, held.velocity);
                    let remaining = match self.gate {
                        Gate::Samples(samples) => samples.max(1),
                        Gate::StepFraction(_) => 0,
                    };
                    self.sounding = Some(Sounding { held, remaining });
                }
                continue;
            }

            let done = match (&mut self.sounding, self.gate) {
                (Some(s), Gate::Samples(_)) => {
                    s.remaining -= 1;
                    s.remaining == 0
                }
                (Some(_), Gate::StepFraction(fraction)) => scaled.fract() >= fraction,
                (None, _) => false,
            };
            if done {
                self.release(out, frame);
            }
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl NodeDefinition for Arp {
    const NAME: &'static str = "arp";
    const DESCRIPTION: &'static str = "Arpeggiates held MIDI notes, one per step of a phasor. \
         `gate` is a fraction of the step; `gate_ms` overrides it";
    const REQUIRED_PARAMS: &'static [&'static str] = &["steps"];
    const OPTIONAL_PARAMS: &'static [&'static str] =
        &["mode", "octaves", "gate", "gate_ms", "seed"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let invalid = |msg: String| ValidationError::InvalidParameter(format!("arp: {msg}"));

        let steps = p.require_usize("steps")?;
        if steps == 0 {
            return Err(invalid("steps must be at least 1".into()));
        }
        let mode = match p.get_str("mode")? {
            Some(name) => ArpMode::parse(&name).ok_or_else(|| {
                invalid(format!(
                    "unknown mode '{name}' (expected up, down, updown, random or played)"
                ))
            })?,
            None => ArpMode::Up,
        };
        let octaves = p.get_usize("octaves")?.unwrap_or(1).clamp(1, 10) as u8;
        let gate = match (p.get_duration_ms("gate_ms")?, p.get_f32("gate")?) {
            (Some(ms), _) => {
                Gate::Samples((ms.as_secs_f64() * rb.config.sample_rate as f64) as u32)
            }
            (None, fraction) => Gate::StepFraction(fraction.unwrap_or(0.5).clamp(0.0, 1.0)),
        };
        let rng = match p.get_u32("seed")? {
            Some(seed) => XorShift::with_seed(seed),
            None => XorShift::seeded(),
        };
        Ok(Box::new(Self::new(mode, octaves, steps, gate, rng)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{MidiHarness, TimedMidi, assert_well_formed, sounding_after};
    use proptest::prelude::*;

    const SR: usize = 48_000;
    /// One phasor cycle; a power of two so step edges land exactly.
    const CYCLE: u64 = 1024;
    const STEPS: usize = 4;
    const STEP: u64 = CYCLE / STEPS as u64;

    fn on(note: u8) -> MidiMessage {
        MidiMessage {
            data: MidiMessageKind::NoteOn { note, velocity: 90 },
            channel_idx: 0,
        }
    }

    fn off(note: u8) -> MidiMessage {
        MidiMessage {
            data: MidiMessageKind::NoteOff { note, velocity: 0 },
            channel_idx: 0,
        }
    }

    fn arp(mode: ArpMode, octaves: u8, gate: Gate) -> Box<Arp> {
        Box::new(Arp::new(mode, octaves, STEPS, gate, XorShift::with_seed(1)))
    }

    fn run(
        node: Box<Arp>,
        block_size: usize,
        keys: Vec<TimedMidi>,
        frames: usize,
    ) -> Vec<TimedMidi> {
        MidiHarness::new(SR, block_size)
            .midi_in(keys)
            .stream_in(0, |frame| (frame % CYCLE) as f32 / CYCLE as f32)
            .run(node, frames)
    }

    fn played(out: &[TimedMidi]) -> Vec<(u64, u8)> {
        out.iter()
            .filter_map(|(frame, m)| match m.data {
                MidiMessageKind::NoteOn { note, .. } => Some((*frame, note)),
                _ => None,
            })
            .collect()
    }

    fn chord(notes: &[u8]) -> Vec<TimedMidi> {
        notes.iter().map(|&n| (0, on(n))).collect()
    }

    fn sequence(mode: ArpMode, octaves: u8, keys: &[u8], steps: usize) -> Vec<u8> {
        let out = run(
            arp(mode, octaves, Gate::StepFraction(0.5)),
            64,
            chord(keys),
            steps * STEP as usize,
        );
        played(&out).into_iter().map(|(_, n)| n).collect()
    }

    #[test]
    fn up_plays_held_notes_in_pitch_order_on_each_step() {
        let out = run(
            arp(ArpMode::Up, 1, Gate::StepFraction(0.5)),
            64,
            chord(&[67, 60, 64]),
            5 * STEP as usize,
        );
        assert_eq!(
            played(&out),
            [
                (0, 60),
                (STEP, 64),
                (2 * STEP, 67),
                (3 * STEP, 60),
                (4 * STEP, 64)
            ]
        );
    }

    #[test]
    fn modes_order_the_notes() {
        let keys = [67, 60, 64];
        assert_eq!(sequence(ArpMode::Down, 1, &keys, 4), [67, 64, 60, 67]);
        assert_eq!(
            sequence(ArpMode::UpDown, 1, &keys, 6),
            [60, 64, 67, 64, 60, 64]
        );
        assert_eq!(sequence(ArpMode::Played, 1, &keys, 4), [67, 60, 64, 67]);
        assert_eq!(sequence(ArpMode::Up, 2, &[60, 64], 5), [60, 64, 72, 76, 60]);
    }

    #[test]
    fn random_only_plays_held_notes() {
        let notes = sequence(ArpMode::Random, 2, &[60, 64], 64);
        assert!(notes.iter().all(|n| [60, 64, 72, 76].contains(n)));
        assert!(notes.windows(2).any(|w| w[0] != w[1]));
    }

    #[test]
    fn step_fraction_gate_ends_notes_part_way_through_the_step() {
        let out = run(
            arp(ArpMode::Up, 1, Gate::StepFraction(0.25)),
            64,
            chord(&[60]),
            STEP as usize,
        );
        assert_eq!(out, [(0, on(60)), (STEP / 4, off(60))]);
    }

    #[test]
    fn sample_gate_ends_notes_after_that_many_samples() {
        let out = run(
            arp(ArpMode::Up, 1, Gate::Samples(100)),
            64,
            chord(&[60]),
            STEP as usize,
        );
        assert_eq!(out, [(0, on(60)), (100, off(60))]);
    }

    #[test]
    fn a_full_gate_ends_each_note_as_the_next_starts() {
        let out = run(
            arp(ArpMode::Up, 1, Gate::StepFraction(1.0)),
            64,
            chord(&[60]),
            2 * STEP as usize,
        );
        assert_eq!(out, [(0, on(60)), (STEP, off(60)), (STEP, on(60))]);
    }

    #[test]
    fn releasing_every_key_stops_after_the_current_note() {
        let keys = vec![(0, on(60)), (STEP + 10, off(60))];
        let out = run(
            arp(ArpMode::Up, 1, Gate::StepFraction(1.0)),
            64,
            keys,
            4 * STEP as usize,
        );
        assert_eq!(
            out,
            [
                (0, on(60)),
                (STEP, off(60)),
                (STEP, on(60)),
                (2 * STEP, off(60))
            ]
        );
    }

    /// Presses and releases of a few keys at any time.
    fn any_keys() -> impl Strategy<Value = Vec<TimedMidi>> {
        prop::collection::vec((0u64..4 * CYCLE, 48u8..72, any::<bool>()), 0..60).prop_map(
            |mut v| {
                v.sort_by_key(|e| e.0);
                v.into_iter()
                    .map(|(frame, note, press)| (frame, if press { on(note) } else { off(note) }))
                    .collect()
            },
        )
    }

    fn any_mode() -> impl Strategy<Value = ArpMode> {
        prop::sample::select(vec![
            ArpMode::Up,
            ArpMode::Down,
            ArpMode::UpDown,
            ArpMode::Random,
            ArpMode::Played,
        ])
    }

    fn any_gate() -> impl Strategy<Value = Gate> {
        prop_oneof![
            (0.0f32..=1.0).prop_map(Gate::StepFraction),
            (1u32..600).prop_map(Gate::Samples),
        ]
    }

    proptest! {
        #[test]
        fn output_ignores_block_size(
            keys in any_keys(), mode in any_mode(), gate in any_gate(), octaves in 1u8..4
        ) {
            let frames = 5 * CYCLE as usize;
            let small = run(Box::new(Arp::new(mode, octaves, STEPS, gate, XorShift::with_seed(9))), 16, keys.clone(), frames);
            let large = run(Box::new(Arp::new(mode, octaves, STEPS, gate, XorShift::with_seed(9))), 512, keys, frames);
            prop_assert_eq!(small, large);
        }

        #[test]
        fn notes_start_on_steps_and_are_well_formed(
            keys in any_keys(), mode in any_mode(), gate in any_gate(), octaves in 1u8..4
        ) {
            let out = run(Box::new(Arp::new(mode, octaves, STEPS, gate, XorShift::with_seed(9))), 64, keys, 5 * CYCLE as usize);
            assert_well_formed(&out);
            for (frame, _) in played(&out) {
                prop_assert_eq!(frame % STEP, 0);
            }
            prop_assert!(sounding_after(&out).len() <= 1);
        }
    }
}
