use crate::{
    builder::{ResourceBuilderView, ValidationError},
    context::AudioContext,
    dsl::ir::DSLParams,
    midi::MidiMessageKind,
    midi_bus::NodeMidi,
    msg::{NodeMessage, RtValue},
    node::{DynNode, Inputs, Node},
    ports::{PortBuilder, Ports},
    rng::XorShift,
    spec::NodeDefinition,
};

/// Passes each note-on with probability `p`, and only the note-offs of notes it passed.
pub struct Chance {
    p: f32,
    /// One bit per (channel, note) passed and not yet ended.
    passed: [u128; 16],
    rng: XorShift,
    ports: Ports,
}

impl Chance {
    pub fn new(p: f32, rng: XorShift) -> Self {
        Self {
            p: p.clamp(0.0, 1.0),
            passed: [0; 16],
            rng,
            ports: PortBuilder::default()
                .midi_in_named(&["in"])
                .default_in()
                .host_fallback()
                .control_in_named(&["p"])
                .midi_out_named(&["out"])
                .default_out()
                .build(),
        }
    }
}

impl Node for Chance {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, _: &mut [&mut [f32]]) {
        let p_in = inputs[0];
        let NodeMidi {
            inputs: midi_in,
            mut outputs,
        } = ctx.node_midi();
        let out = outputs.port(0);

        for &event in midi_in.port(0) {
            let passed = &mut self.passed[event.msg.channel_idx as usize & 0x0F];
            match event.msg.data {
                MidiMessageKind::NoteOn { note, .. } => {
                    let p = p_in.map_or(self.p, |p| p[event.frame as usize]);
                    if self.rng.unipolar() < p {
                        *passed |= 1 << note;
                        out.push(event);
                    }
                }
                MidiMessageKind::NoteOff { note, .. } => {
                    if *passed & (1 << note) != 0 {
                        *passed &= !(1 << note);
                        out.push(event);
                    }
                }
                _ => out.push(event),
            }
        }
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        if let NodeMessage::SetParam(param) = msg
            && let ("p", RtValue::F32(p)) = (param.param_name, param.value)
        {
            self.p = p.clamp(0.0, 1.0);
        }
    }

    fn runtime_params(&self) -> Option<Vec<&str>> {
        Some(vec!["p"])
    }

    fn reset(&mut self, _ctx: &mut AudioContext) {
        self.passed = [0; 16];
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl NodeDefinition for Chance {
    const NAME: &'static str = "chance";
    const DESCRIPTION: &'static str =
        "Passes each MIDI note with probability `p`; other messages always pass";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["p", "seed"];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let chance = p.get_f32("p")?.unwrap_or(0.5);
        let rng = match p.get_u32("seed")? {
            Some(seed) => XorShift::with_seed(seed),
            None => XorShift::seeded(),
        };
        Ok(Box::new(Self::new(chance, rng)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        harness::{MidiHarness, TimedMidi, assert_well_formed},
        midi::MidiMessage,
    };
    use proptest::prelude::*;

    const SR: usize = 48_000;

    fn msg(data: MidiMessageKind) -> MidiMessage {
        MidiMessage {
            data,
            channel_idx: 0,
        }
    }

    fn on(note: u8) -> MidiMessage {
        msg(MidiMessageKind::NoteOn { note, velocity: 90 })
    }

    fn off(note: u8) -> MidiMessage {
        msg(MidiMessageKind::NoteOff { note, velocity: 0 })
    }

    fn run(p: f32, seed: u32, block_size: usize, events: Vec<TimedMidi>) -> Vec<TimedMidi> {
        let frames = events.last().map_or(1, |e| e.0 as usize + 1);
        MidiHarness::new(SR, block_size)
            .midi_in(events)
            .run(Box::new(Chance::new(p, XorShift::with_seed(seed))), frames)
    }

    /// One note at a time, so the input itself is well formed.
    fn notes(count: u64) -> Vec<TimedMidi> {
        (0..count)
            .flat_map(|i| {
                [
                    (i * 10, on(60 + (i % 12) as u8)),
                    (i * 10 + 5, off(60 + (i % 12) as u8)),
                ]
            })
            .collect()
    }

    #[test]
    fn certain_passes_everything() {
        let mut events = notes(20);
        events.push((
            300,
            msg(MidiMessageKind::Control {
                control_number: 1,
                value: 64,
            }),
        ));
        assert_eq!(run(1.0, 7, 64, events.clone()), events);
    }

    #[test]
    fn never_passes_only_non_note_messages() {
        let cc = (
            12,
            msg(MidiMessageKind::Control {
                control_number: 1,
                value: 64,
            }),
        );
        let mut events = notes(20);
        events.push(cc);
        events.sort_by_key(|e| e.0);
        assert_eq!(run(0.0, 7, 64, events), [cc]);
    }

    #[test]
    fn passes_about_p_of_the_notes() {
        let passed = run(0.3, 11, 256, notes(10_000))
            .iter()
            .filter(|(_, m)| matches!(m.data, MidiMessageKind::NoteOn { .. }))
            .count();
        let rate = passed as f32 / 10_000.0;
        assert!((rate - 0.3).abs() < 0.02, "{rate}");
    }

    #[test]
    fn the_p_input_overrides_the_param() {
        let out = MidiHarness::new(SR, 64)
            .midi_in(notes(40))
            .stream_in(0, |frame| if frame < 200 { 0.0 } else { 1.0 })
            .run(Box::new(Chance::new(0.5, XorShift::with_seed(3))), 400);
        assert!(out.iter().all(|(frame, _)| *frame >= 200));
        assert_eq!(out, notes(40)[40..]);
    }

    /// Note-ons and offs in any order, including repeats and stray offs.
    fn any_notes() -> impl Strategy<Value = Vec<TimedMidi>> {
        prop::collection::vec((0u64..2_000, 60u8..64, any::<bool>()), 0..200).prop_map(|mut v| {
            v.sort_by_key(|e| e.0);
            v.into_iter()
                .map(|(frame, note, is_on)| (frame, if is_on { on(note) } else { off(note) }))
                .collect()
        })
    }

    proptest! {
        #[test]
        fn output_is_a_subsequence_and_ignores_block_size(
            events in any_notes(), p in 0.0f32..=1.0, seed in any::<u32>()
        ) {
            let out = run(p, seed, 16, events.clone());
            prop_assert_eq!(&out, &run(p, seed, 512, events.clone()));

            let mut rest = events.iter();
            for e in &out {
                prop_assert!(rest.any(|x| x == e), "{:?} is not from the input", e);
            }
        }

        #[test]
        fn well_formed_input_gives_well_formed_output(
            count in 0u64..200, p in 0.0f32..=1.0, seed in any::<u32>()
        ) {
            assert_well_formed(&run(p, seed, 64, notes(count)));
        }

        #[test]
        fn every_note_off_ends_a_passed_note(events in any_notes(), p in 0.0f32..=1.0, seed in any::<u32>()) {
            let mut sounding = std::collections::HashSet::new();
            for (_, m) in run(p, seed, 64, events) {
                match m.data {
                    MidiMessageKind::NoteOn { note, .. } => { sounding.insert(note); }
                    MidiMessageKind::NoteOff { note, .. } => prop_assert!(sounding.remove(&note)),
                    _ => {}
                }
            }
        }
    }
}
