//! MIDI flowing between nodes: each node reads merged copies of its sources and
//! writes its own outputs, which start empty every block.

use std::sync::{Arc, Mutex};

use legato::{
    builder::{Configured, LegatoBuilder, Unconfigured},
    midi_bus::NodeMidi,
    node_state::NodeState,
    prelude::*,
};

const BLOCK: usize = 64;

fn config() -> Config {
    Config {
        sample_rate: 48_000,
        block_size: BLOCK,
        channels: 2,
        rt_capacity: 0,
    }
}

fn note_on(note: u8) -> MidiMessage {
    MidiMessage {
        channel_idx: 0,
        data: MidiMessageKind::NoteOn {
            note,
            velocity: 100,
        },
    }
}

/// Writes `notes` at `frames` in its first block only.
struct Emit {
    script: Vec<(u32, u8)>,
    fired: bool,
    ports: Ports,
}

impl Node for Emit {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, _: &mut [&mut [f32]]) {
        if std::mem::replace(&mut self.fired, true) {
            return;
        }
        let NodeMidi { mut outputs, .. } = ctx.node_midi();
        let out = outputs.port(0);
        for &(frame, note) in &self.script {
            out.note_on(frame, 0, note, 100);
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

/// Forwards every note a semitone up.
struct Transpose {
    ports: Ports,
}

impl Node for Transpose {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, _: &mut [&mut [f32]]) {
        let NodeMidi {
            inputs,
            mut outputs,
        } = ctx.node_midi();
        let out = outputs.port(0);
        for event in inputs.port(0) {
            let mut event = *event;
            if let MidiMessageKind::NoteOn { note, .. } | MidiMessageKind::NoteOff { note, .. } =
                &mut event.msg.data
            {
                *note += 1;
            }
            out.push(event);
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Note {
    On(u8),
    Off(u8),
}
use Note::{Off, On};

/// (block, frame, note) for every note a probe received.
type Log = Arc<Mutex<Vec<(usize, u32, Note)>>>;

struct Probe {
    log: Log,
    block: usize,
    ports: Ports,
}

impl Node for Probe {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, _: &mut [&mut [f32]]) {
        let NodeMidi { inputs, .. } = ctx.node_midi();
        for event in inputs.port(0) {
            let note = match event.msg.data {
                MidiMessageKind::NoteOn { note, .. } => On(note),
                MidiMessageKind::NoteOff { note, .. } => Off(note),
                _ => continue,
            };
            self.log
                .lock()
                .unwrap()
                .push((self.block, event.frame, note));
        }
        self.block += 1;
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

struct Rig {
    builder: LegatoBuilder<Configured>,
    logs: Arc<Mutex<Vec<Log>>>,
}

/// Registers `emit { frames, notes }`, `transpose`, `probe { host }`, `required` and `audio_in`.
fn rig() -> Rig {
    let logs: Arc<Mutex<Vec<Log>>> = Arc::default();
    let probe_logs = logs.clone();

    let emit = NodeSpec::from_fn("emit", &["frames", "notes"], &[], |_, p| {
        let frames = p.require_array_f32("frames")?;
        let notes = p.require_array_f32("notes")?;
        Ok(Box::new(Emit {
            script: frames
                .into_iter()
                .zip(notes)
                .map(|(f, n)| (f as u32, n as u8))
                .collect(),
            fired: false,
            ports: PortBuilder::default()
                .midi_out_named(&["out"])
                .default_out()
                .build(),
        }))
    });
    let transpose = NodeSpec::from_fn("transpose", &[], &[], |_, _| {
        Ok(Box::new(Transpose {
            ports: PortBuilder::default()
                .midi_in_named(&["in"])
                .default_in()
                .midi_out_named(&["out"])
                .default_out()
                .build(),
        }))
    });
    let probe = NodeSpec::from_fn("probe", &[], &["host"], move |_, p| {
        let log = Log::default();
        probe_logs.lock().unwrap().push(log.clone());
        let ports = PortBuilder::default().midi_in_named(&["in"]);
        let ports = match p.get_bool("host")? {
            Some(true) => ports.host_fallback(),
            _ => ports,
        };
        Ok(Box::new(Probe {
            log,
            block: 0,
            ports: ports.default_in().audio_out(1).default_out().build(),
        }))
    });
    let required = NodeSpec::from_fn("required", &[], &[], |_, _| {
        Ok(Box::new(Transpose {
            ports: PortBuilder::default()
                .midi_in_named(&["in"])
                .required_in()
                .midi_out_named(&["notes"])
                .audio_out(1)
                .default_out()
                .build(),
        }))
    });
    let audio_in = NodeSpec::from_fn("audio_in", &[], &[], |_, _| {
        Ok(Box::new(Transpose {
            ports: PortBuilder::default()
                .audio_in(1)
                .default_in()
                .audio_out(1)
                .default_out()
                .build(),
        }))
    });

    let builder = LegatoBuilder::<Unconfigured>::new(config())
        .register_node("midi", emit)
        .register_node("midi", transpose)
        .register_node("midi", probe)
        .register_node("midi", required)
        .register_node("audio", audio_in);

    Rig { builder, logs }
}

fn run(src: &str, blocks: usize) -> Vec<Vec<(usize, u32, Note)>> {
    let Rig { builder, logs } = rig();
    let (mut app, _) = builder.build_dsl(src).expect("graph should build");
    for _ in 0..blocks {
        app.next_block();
    }
    let logs = logs.lock().unwrap();
    logs.iter().map(|l| l.lock().unwrap().clone()).collect()
}

fn build_err(src: &str) -> String {
    let Rig { builder, .. } = rig();
    match builder.build_dsl(src) {
        Ok(_) => panic!("graph should not build"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn a_chain_passes_events_through_each_node() {
    let logs = run(
        r#"
        midi {
            emit { frames: [3, 40], notes: [60, 64] },
            transpose: t1,
            transpose: t2,
            probe,
        }
        emit >> t1 >> t2 >> probe
        { probe }
        "#,
        1,
    );
    assert_eq!(logs, [vec![(0, 3, On(62)), (0, 40, On(66))]]);
}

#[test]
fn fan_in_merges_by_frame_and_keeps_connection_order_on_ties() {
    let logs = run(
        r#"
        midi {
            emit: a { frames: [5, 10], notes: [60, 61] },
            emit: b { frames: [0, 5], notes: [70, 71] },
            probe,
        }
        a >> probe
        b >> probe
        { probe }
        "#,
        1,
    );
    assert_eq!(
        logs,
        [vec![
            (0, 0, On(70)),
            (0, 5, On(60)),
            (0, 5, On(71)),
            (0, 10, On(61))
        ]]
    );
}

#[test]
fn fan_out_gives_every_consumer_its_own_copy() {
    let logs = run(
        r#"
        midi {
            emit { frames: [7], notes: [60] },
            transpose,
            probe: p1,
            probe: p2,
        }
        emit >> transpose
        emit >> p1
        transpose >> p2
        { p1 }
        "#,
        1,
    );
    assert_eq!(logs, [vec![(0, 7, On(60))], vec![(0, 7, On(61))]]);
}

#[test]
fn outputs_start_empty_every_block() {
    let logs = run(
        r#"
        midi { emit { frames: [1], notes: [60] }, transpose, probe }
        emit >> transpose >> probe
        { probe }
        "#,
        3,
    );
    assert_eq!(logs, [vec![(0, 1, On(61))]]);
}

#[test]
fn unpatched_input_is_empty_unless_it_falls_back_to_the_host() {
    let Rig { builder, logs } = rig();
    let (mut app, _) = builder
        .build_dsl(
            r#"
            midi { probe: plain, probe: host { host: true } }
            { plain }
            "#,
        )
        .unwrap();

    app.write_midi(12, note_on(60)).unwrap();
    app.next_block();

    let logs = logs.lock().unwrap();
    assert_eq!(*logs[0].lock().unwrap(), []);
    assert_eq!(*logs[1].lock().unwrap(), [(0, 12, On(60))]);
}

#[test]
fn a_connected_host_fallback_input_ignores_the_host() {
    let Rig { builder, logs } = rig();
    let (mut app, _) = builder
        .build_dsl(
            r#"
            midi { emit { frames: [2], notes: [50] }, probe { host: true } }
            emit >> probe
            { probe }
            "#,
        )
        .unwrap();

    app.write_midi(12, note_on(60)).unwrap();
    app.next_block();

    assert_eq!(*logs.lock().unwrap()[0].lock().unwrap(), [(0, 2, On(50))]);
}

#[test]
fn muted_nodes_write_no_midi() {
    let logs = run(
        r#"
        midi { emit { frames: [1], notes: [60] }, muted transpose, probe }
        emit >> transpose >> probe
        { probe }
        "#,
        1,
    );
    assert_eq!(logs, [vec![]]);
}

#[test]
fn midi_cannot_connect_to_a_stream_input() {
    let err = build_err(
        r#"
        midi { emit { frames: [1], notes: [60] } }
        audio { audio_in }
        emit >> audio_in.in
        { audio_in }
        "#,
    );
    assert!(err.contains("cannot connect a Midi output"), "{err}");
}

#[test]
fn bare_midi_into_a_node_without_midi_inputs_is_an_error() {
    let err = build_err(
        r#"
        midi { emit { frames: [1], notes: [60] } }
        audio { audio_in }
        emit >> audio_in
        { audio_in }
        "#,
    );
    assert!(err.contains("has no MIDI input ports"), "{err}");
}

#[test]
fn a_required_midi_input_must_be_connected() {
    let err = build_err("midi { required }\n{ required }");
    assert!(err.contains("requires its 'in' input"), "{err}");
}

#[test]
fn voice_reads_host_midi_on_its_channel_when_unpatched() {
    let (mut app, _) = LegatoBuilder::<Unconfigured>::new(config())
        .build_dsl("midi { voice { chan: 1 } }\n{ voice }")
        .unwrap();

    app.write_midi(
        10,
        MidiMessage {
            channel_idx: 0,
            ..note_on(40)
        },
    )
    .unwrap();
    app.write_midi(
        20,
        MidiMessage {
            channel_idx: 1,
            ..note_on(69)
        },
    )
    .unwrap();
    let out = app.next_block();

    let gate = out.channels[0];
    let freq = out.channels[1];
    assert!(gate[..20].iter().all(|&g| g == 0.0));
    assert!(gate[20..].iter().all(|&g| g == 1.0));
    assert_eq!(freq[BLOCK - 1], 440.0);
}

#[test]
fn voice_reads_a_patched_midi_input() {
    let Rig { builder, .. } = rig();
    let (mut app, _) = builder
        .build_dsl(
            r#"
            midi { emit { frames: [8], notes: [69] }, voice { chan: 0 } }
            emit >> voice
            { voice }
            "#,
        )
        .unwrap();

    let gate = app.next_block().channels[0];
    assert_eq!(gate[7], 0.0);
    assert_eq!(gate[8], 1.0);
}

#[test]
fn bypassed_nodes_pass_midi_through() {
    let logs = run(
        r#"
        midi { emit { frames: [4], notes: [60] }, bypassed transpose, probe }
        emit >> transpose >> probe
        { probe }
        "#,
        1,
    );
    assert_eq!(logs, [vec![(0, 4, On(60))]]);
}

#[test]
fn a_midi_node_without_a_marked_input_cannot_start_bypassed() {
    let err = build_err(
        r#"
        midi { bypassed emit { frames: [1], notes: [60] }, probe }
        emit >> probe
        { probe }
        "#,
    );
    assert!(
        err.contains("marks 0 inputs for 1 default outputs"),
        "{err}"
    );
}

/// Runs `transpose` one block, switches it to `state`, then runs `blocks` more.
fn switch_after_a_note(state: NodeState, blocks: usize) -> Vec<(usize, u32, Note)> {
    let Rig { builder, logs } = rig();
    let (mut app, mut frontend) = builder
        .build_dsl(
            r#"
            midi { emit { frames: [3], notes: [60] }, transpose, probe }
            emit >> transpose >> probe
            { probe }
            "#,
        )
        .unwrap();

    app.next_block();
    frontend.set_state("transpose", state).unwrap();
    for _ in 0..blocks {
        app.next_block();
    }
    logs.lock().unwrap()[0].lock().unwrap().clone()
}

#[test]
fn bypassing_releases_notes_the_node_left_sounding() {
    let log = switch_after_a_note(NodeState::Bypassed, 10);
    let release = log
        .iter()
        .position(|e| e.2 == Off(61))
        .expect("61 released");

    assert_eq!(log[0], (0, 3, On(61)));
    assert_eq!(log[release].1, 0);
    assert_eq!(log.len(), 2);
}

#[test]
fn muting_and_suspending_release_notes_too() {
    for state in [NodeState::Muted, NodeState::Suspended] {
        let log = switch_after_a_note(state, 10);
        assert_eq!(log.len(), 2, "{state:?}");
        assert_eq!(log[1].1, 0);
        assert_eq!(log[1].2, Off(61), "{state:?}");
    }
}

#[test]
fn a_bypassed_node_forgets_notes_the_executor_already_released() {
    let Rig { builder, logs } = rig();
    let (mut app, mut frontend) = builder
        .build_dsl(
            r#"
            midi { chance { p: 1.0 }, probe }
            chance >> probe
            { probe }
            "#,
        )
        .unwrap();

    app.write_midi(0, note_on(60)).unwrap();
    app.next_block();
    frontend.set_state("chance", NodeState::Bypassed).unwrap();
    for _ in 0..10 {
        app.next_block();
    }
    frontend.set_state("chance", NodeState::Active).unwrap();
    for _ in 0..10 {
        app.next_block();
    }

    let off = MidiMessage {
        channel_idx: 0,
        data: MidiMessageKind::NoteOff {
            note: 60,
            velocity: 0,
        },
    };
    app.write_midi(0, off).unwrap();
    app.next_block();

    let log = logs.lock().unwrap()[0].lock().unwrap().clone();
    let offs = log.iter().filter(|e| e.2 == Off(60)).count();
    assert_eq!(offs, 1, "{log:?}");
}

#[test]
fn the_arp_demo_plays_a_held_chord() {
    let source = include_str!("../demos/arp.legato");
    let render = |chord: &[u8]| {
        let (mut app, _) = LegatoBuilder::<Unconfigured>::new(config())
            .build_dsl(source)
            .unwrap();
        for &note in chord {
            app.write_midi(0, note_on(note)).unwrap();
        }
        let mut peak = 0.0f32;
        for _ in 0..(48_000 / BLOCK) {
            let out = app.next_block();
            peak = out.channels[0].iter().fold(peak, |p, y| p.max(y.abs()));
        }
        peak
    };
    assert_eq!(render(&[]), 0.0);
    assert!(render(&[60, 64, 67]) > 0.01);
}
