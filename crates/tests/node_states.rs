//! Executor node states: what reaches the output, and whether `process` runs,
//! as a node fades between Active, Muted, Bypassed and Suspended.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use legato::{
    FrontendError,
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    context::AudioContext,
    executor::Executor,
    graph::{Connection, ConnectionEntry},
    harness::build_placeholder_context,
    node::{Inputs, LegatoNode, Node},
    node_state::{NodeState, fade_samples},
    ports::{PortBuilder, Ports},
    runtime::NodeKey,
};
use proptest::prelude::*;

const SR: usize = 48_000;
const BLOCK: usize = 64;
const FADE: u32 = fade_samples(SR);

/// Outputs a constant and counts its `process` calls.
struct Const {
    val: f32,
    calls: Arc<AtomicUsize>,
    ports: Ports,
}

impl Node for Const {
    fn process(&mut self, _: &mut AudioContext, _: &Inputs, outputs: &mut [&mut [f32]]) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        for out in outputs {
            out.fill(self.val);
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

/// Doubles its input, so its processed output differs from a bypass.
struct Double {
    ports: Ports,
}

impl Node for Double {
    fn process(&mut self, _: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let input = inputs[0].unwrap();
        for (y, x) in outputs[0].iter_mut().zip(input) {
            *y = 2.0 * x;
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

struct Rig {
    executor: Executor,
    ctx: AudioContext,
    src: NodeKey,
    double: NodeKey,
    calls: Arc<AtomicUsize>,
}

/// `Const(val) >> Double`, with `sink` chosen by the caller.
fn rig(val: f32, block: usize, fade: u32, sink_is_double: bool) -> Rig {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut executor = Executor::default();
    let src = executor.graph.add_node(LegatoNode::new(
        "src".into(),
        "const".into(),
        Box::new(Const {
            val,
            calls: calls.clone(),
            ports: PortBuilder::default().audio_out(1).build(),
        }),
    ));
    let double = executor.graph.add_node(LegatoNode::new(
        "double".into(),
        "double".into(),
        Box::new(Double {
            ports: PortBuilder::default()
                .audio_in(1)
                .default_in()
                .audio_out(1)
                .build(),
        }),
    ));
    let entry = |node_key| ConnectionEntry {
        node_key,
        port_index: 0,
    };
    executor
        .graph
        .add_edge(Connection {
            source: entry(src),
            sink: entry(double),
        })
        .unwrap();
    executor
        .set_sink(if sink_is_double { double } else { src })
        .unwrap();
    executor.prepare(block, fade);

    let ctx = build_placeholder_context(Config {
        sample_rate: SR,
        block_size: block,
        channels: 1,
        rt_capacity: 0,
    });
    Rig {
        executor,
        ctx,
        src,
        double,
        calls,
    }
}

impl Rig {
    /// The sink's output over `blocks` blocks, concatenated.
    fn render(&mut self, blocks: usize) -> Vec<f32> {
        (0..blocks)
            .flat_map(|_| self.executor.process(&mut self.ctx).channels[0].to_vec())
            .collect()
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

/// Gain at each sample of a fade down from full: (len - 1 - n) / len, then 0.
fn fade_down(n: usize) -> f32 {
    FADE.saturating_sub(n as u32 + 1) as f32 / FADE as f32
}

/// Blocks needed to cover the fade, plus two settled blocks.
const BLOCKS: usize = (FADE as usize).div_ceil(BLOCK) + 2;

#[test]
fn muted_reaches_exact_zero_and_keeps_processing() {
    let mut r = rig(1.0, BLOCK, FADE, false);
    assert!(r.executor.set_state(r.src, NodeState::Muted));

    let out = r.render(BLOCKS);
    let expected: Vec<f32> = (0..out.len()).map(fade_down).collect();
    assert_eq!(out, expected);
    // Muted nodes keep running, e.g. so an LFO stays in phase.
    assert_eq!(r.calls(), BLOCKS);
}

#[test]
fn suspended_stops_processing_once_faded() {
    let mut r = rig(1.0, BLOCK, FADE, false);
    assert!(r.executor.set_state(r.src, NodeState::Suspended));

    let out = r.render(BLOCKS);
    let expected: Vec<f32> = (0..out.len()).map(fade_down).collect();
    assert_eq!(out, expected);
    // Only the blocks the fade touches call `process`.
    assert_eq!(r.calls(), (FADE as usize).div_ceil(BLOCK));
}

#[test]
fn bypassed_crossfades_to_the_dry_input() {
    let mut r = rig(0.25, BLOCK, FADE, true);
    assert!(r.executor.set_state(r.double, NodeState::Bypassed));

    // Processed output is 0.5, dry input 0.25; same arithmetic as the executor.
    let out = r.render(BLOCKS);
    let expected: Vec<f32> = (0..out.len())
        .map(|n| {
            let g = fade_down(n);
            0.5 * g + 0.25 * (1.0 - g)
        })
        .collect();
    assert_eq!(out, expected);
    assert_eq!(*out.last().unwrap(), 0.25);
}

#[test]
fn reactivating_a_suspended_node_fades_back_in() {
    let mut r = rig(1.0, BLOCK, FADE, false);
    r.executor.set_state(r.src, NodeState::Suspended);
    r.render(BLOCKS);

    r.executor.set_state(r.src, NodeState::Active);
    let out = r.render(BLOCKS);
    let expected: Vec<f32> = (0..out.len())
        .map(|n| (n as u32 + 1).min(FADE) as f32 / FADE as f32)
        .collect();
    assert_eq!(out, expected);
}

#[test]
fn every_channel_gets_the_same_fade() {
    let mut executor = Executor::default();
    let src = executor.graph.add_node(LegatoNode::new(
        "src".into(),
        "const".into(),
        Box::new(Const {
            val: 1.0,
            calls: Arc::default(),
            ports: PortBuilder::default().audio_out(2).build(),
        }),
    ));
    executor.set_sink(src).unwrap();
    executor.prepare(BLOCK, FADE);
    let mut ctx = build_placeholder_context(Config {
        sample_rate: SR,
        block_size: BLOCK,
        channels: 2,
        rt_capacity: 0,
    });
    executor.set_state(src, NodeState::Muted);

    let (mut left, mut right) = (vec![], vec![]);
    for _ in 0..BLOCKS {
        let out = executor.process(&mut ctx);
        left.extend_from_slice(out.channels[0]);
        right.extend_from_slice(out.channels[1]);
    }
    let expected: Vec<f32> = (0..left.len()).map(fade_down).collect();
    assert_eq!(left, expected);
    assert_eq!(right, expected);
}

#[test]
fn distortion_bypass_passes_only_the_stereo_signal() {
    // Inputs 0-1 are stereo audio, 2-3 are `drive` and `tone`.
    const VALS: [f32; 4] = [0.25, -0.5, 2.0, 0.125];

    /// Outputs `VALS[i]` on output `i`.
    struct Sources(Ports);
    impl Node for Sources {
        fn process(&mut self, _: &mut AudioContext, _: &Inputs, outputs: &mut [&mut [f32]]) {
            for (out, v) in outputs.iter_mut().zip(VALS) {
                out.fill(v);
            }
        }
        fn ports(&self) -> &Ports {
            &self.0
        }
    }

    /// `x * drive + tone` per channel, so a leaked modulation input is visible.
    struct Distortion(Ports);
    impl Node for Distortion {
        fn process(&mut self, _: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
            let (drive, tone) = (inputs[2].unwrap(), inputs[3].unwrap());
            for (c, out) in outputs.iter_mut().enumerate() {
                for (n, y) in out.iter_mut().enumerate() {
                    *y = inputs[c].unwrap()[n] * drive[n] + tone[n];
                }
            }
        }
        fn ports(&self) -> &Ports {
            &self.0
        }
    }

    let ports = |marked: bool| {
        let b = PortBuilder::default().audio_in(2);
        let b = if marked { b.default_in() } else { b };
        b.control_in_named(&["drive", "tone"]).audio_out(2).build()
    };

    // Without `default_in()` no input is marked, so there is no signal path.
    let mut forgot = Executor::default();
    let key = forgot.graph.add_node(LegatoNode::new(
        "dist".into(),
        "dist".into(),
        Box::new(Distortion(ports(false))),
    ));
    forgot.prepare(BLOCK, FADE);
    assert!(!forgot.can_bypass(key));

    let mut executor = Executor::default();
    let src = executor.graph.add_node(LegatoNode::new(
        "src".into(),
        "src".into(),
        Box::new(Sources(PortBuilder::default().audio_out(4).build())),
    ));
    let dist = executor.graph.add_node(LegatoNode::new(
        "dist".into(),
        "dist".into(),
        Box::new(Distortion(ports(true))),
    ));
    for i in 0..4 {
        executor
            .graph
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: src,
                    port_index: i,
                },
                sink: ConnectionEntry {
                    node_key: dist,
                    port_index: i,
                },
            })
            .unwrap();
    }
    executor.set_sink(dist).unwrap();
    executor.prepare(BLOCK, FADE);
    let mut ctx = build_placeholder_context(Config {
        sample_rate: SR,
        block_size: BLOCK,
        channels: 2,
        rt_capacity: 0,
    });
    assert!(executor.set_state(dist, NodeState::Bypassed));

    let (mut left, mut right) = (vec![], vec![]);
    for _ in 0..BLOCKS {
        let out = executor.process(&mut ctx);
        left.extend_from_slice(out.channels[0]);
        right.extend_from_slice(out.channels[1]);
    }
    // Crossfade from the distorted signal to the audio inputs alone.
    for (c, out) in [left, right].iter().enumerate() {
        let (x, wet) = (VALS[c], VALS[c] * VALS[2] + VALS[3]);
        let expected: Vec<f32> = (0..out.len())
            .map(|n| {
                let g = fade_down(n);
                wet * g + x * (1.0 - g)
            })
            .collect();
        assert_eq!(*out, expected, "channel {c}");
        assert_eq!(*out.last().unwrap(), x);
    }
}

#[test]
fn bypass_needs_paired_default_ports() {
    // `src` has no inputs to pass through.
    let mut r = rig(1.0, BLOCK, FADE, false);
    assert!(!r.executor.can_bypass(r.src));
    assert!(!r.executor.set_state(r.src, NodeState::Bypassed));
    assert!(r.executor.can_bypass(r.double));

    // One unmarked input and one output, like an oscillator's `freq`: the
    // counts match, but an unmarked input is never treated as signal.
    let mut executor = Executor::default();
    let osc = executor.graph.add_node(LegatoNode::new(
        "osc".into(),
        "osc".into(),
        Box::new(Double {
            ports: PortBuilder::default().audio_in(1).audio_out(1).build(),
        }),
    ));
    executor.prepare(BLOCK, FADE);
    assert!(!executor.can_bypass(osc));
}

#[test]
fn frontend_rejects_bypass_without_paired_ports() {
    let config = Config {
        sample_rate: SR,
        block_size: BLOCK,
        channels: 1,
        rt_capacity: 0,
    };
    let src = "control { constant { val: 1.0, chans: 1 } }\n\
               audio { svf { chans: 1 }, saw { chans: 1 } }\nconstant >> svf\n{ svf }";
    let (_app, mut frontend) = LegatoBuilder::<Unconfigured>::new(config)
        .build_dsl(src)
        .unwrap();

    let err = frontend
        .set_state("constant", NodeState::Bypassed)
        .unwrap_err();
    assert_eq!(
        err,
        FrontendError::CannotBypass {
            node: "constant".into(),
            inputs: 0,
            outputs: 1,
        }
    );
    assert_eq!(
        err.to_string(),
        "'constant' cannot be bypassed: bypass copies each input marked with \
         `PortBuilder::default_in()` to the matching default output, but it marks \
         0 inputs for 1 default outputs"
    );
    // `saw` has a `freq` input and one output, but marks no signal input.
    assert_eq!(
        frontend.set_state("saw", NodeState::Bypassed),
        Err(FrontendError::CannotBypass {
            node: "saw".into(),
            inputs: 0,
            outputs: 1,
        })
    );
    assert_eq!(frontend.set_state("svf", NodeState::Bypassed), Ok(()));
    assert_eq!(
        frontend.set_state("nope", NodeState::Muted),
        Err(FrontendError::NodeNotFound("nope".into()))
    );
}

fn any_state() -> impl Strategy<Value = NodeState> {
    prop_oneof![
        Just(NodeState::Active),
        Just(NodeState::Muted),
        Just(NodeState::Bypassed),
        Just(NodeState::Suspended),
    ]
}

proptest! {
    /// Any sequence of state changes, landing on any block boundary, moves the
    /// output by at most one ramp step per sample. The widest ramp is processed
    /// (1.0) to silence (0.0), so a step is at most 1.0 / fade.
    #[test]
    fn state_changes_never_jump(
        block in (1usize..=64).prop_map(|b| b * 2),
        fade in 1u32..=300,
        steps in prop::collection::vec((any_state(), 1usize..6), 1..12),
    ) {
        // Processed output 1.0, dry input 0.5.
        let mut r = rig(0.5, block, fade, true);
        let max_step = 1.0 / fade as f32 + 1e-6;

        let mut prev = 1.0;
        for (state, blocks) in steps {
            r.executor.set_state(r.double, state);
            for y in r.render(blocks) {
                prop_assert!((y - prev).abs() <= max_step, "{prev} -> {y}");
                prev = y;
            }
        }
    }
}
