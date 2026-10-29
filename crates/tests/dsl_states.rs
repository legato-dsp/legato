//! Initial node states from the DSL's `muted` / `bypassed` / `suspended` prefixes.

use legato::{
    LegatoApp, LegatoFrontend,
    builder::{LegatoBuilder, Unconfigured, ValidationError},
    config::Config,
    node_state::NodeState::{self, *},
};

const CONFIG: Config = Config {
    sample_rate: 48_000,
    block_size: 64,
    channels: 1,
    rt_capacity: 0,
};

fn build(src: &str) -> Result<(LegatoApp, LegatoFrontend), ValidationError> {
    LegatoBuilder::<Unconfigured>::new(CONFIG).build_dsl(src)
}

/// The state of each named node.
fn states(src: &str, names: &[&str]) -> Vec<(String, NodeState)> {
    let (app, frontend) = build(src).unwrap();
    let registry = frontend.clone_registry();
    names
        .iter()
        .map(|&name| {
            let key = registry
                .get(name)
                .unwrap_or_else(|| panic!("no node '{name}'"));
            (name.to_string(), app.node_state(*key).unwrap())
        })
        .collect()
}

fn expect(pairs: &[(&str, NodeState)]) -> Vec<(String, NodeState)> {
    pairs.iter().map(|&(n, s)| (n.to_string(), s)).collect()
}

#[test]
fn muted_node_is_silent_from_the_first_block() {
    let render = |src: &str| {
        let (mut app, _) = build(src).unwrap();
        app.next_block().channels[0].to_vec()
    };
    // No fade at startup, so the very first block is exactly silent.
    assert_eq!(
        render("audio { muted saw { chans: 1 } }\n{ saw }"),
        vec![0.0; 64]
    );
    assert!(
        render("audio { saw { chans: 1 } }\n{ saw }")
            .iter()
            .any(|&y| y != 0.0)
    );
}

const VOICE: &str = "
    patch voice() {
        audio {
            sine: osc { freq: 220.0 },
            bypassed mult: amp { val: 0.5 }
        }
        osc >> amp[0]
        { amp }
    }
";

#[test]
fn patch_instance_state_applies_to_its_nodes_unless_overridden() {
    let src = format!(
        "{VOICE}
        patches {{ suspended voice * 2 {{}}, voice: lone {{}} }}
        {{ lone }}"
    );
    assert_eq!(
        states(
            &src,
            &[
                "voice.0.osc",
                "voice.0.amp",
                "voice.1.osc",
                "voice.1.amp",
                "lone.osc",
                "lone.amp"
            ]
        ),
        expect(&[
            ("voice.0.osc", Suspended),
            // The keyword in the body wins over the instance's.
            ("voice.0.amp", Bypassed),
            ("voice.1.osc", Suspended),
            ("voice.1.amp", Bypassed),
            ("lone.osc", Active),
            ("lone.amp", Bypassed),
        ])
    );
}

#[test]
fn nested_instances_inherit_the_outer_state() {
    let src = format!(
        "{VOICE}
        patch outer() {{
            audio {{ voice: v {{}}, mult: gain {{ val: 1.0 }} }}
            v >> gain[0]
            {{ gain }}
        }}
        patches {{ muted outer: o {{}} }}
        {{ o }}"
    );
    assert_eq!(
        states(&src, &["o.v.osc", "o.v.amp", "o.gain"]),
        expect(&[("o.v.osc", Muted), ("o.v.amp", Bypassed), ("o.gain", Muted)])
    );
}

#[test]
fn spawned_copies_share_the_state() {
    let src = "audio { suspended saw * 3 { chans: 1 }, track_mixer { tracks: 3, chans_per_track: 1, gain: [1.0, 1.0, 1.0] } }\n\
               saw(*) >> track_mixer[0..3]\n{ track_mixer }";
    assert_eq!(
        states(src, &["saw.0", "saw.1", "saw.2", "track_mixer"]),
        expect(&[
            ("saw.0", Suspended),
            ("saw.1", Suspended),
            ("saw.2", Suspended),
            ("track_mixer", Active),
        ])
    );
}

#[test]
fn bypassed_without_paired_ports_is_a_build_error() {
    let err = build("audio { bypassed saw { chans: 1 } }\n{ saw }")
        .err()
        .unwrap();
    assert_eq!(
        err.to_string(),
        "cannot bypass: 'saw' is declared `bypassed`, but bypass copies each input marked \
         with `PortBuilder::default_in()` to the matching default output, and it marks \
         0 inputs for 1 default outputs"
    );
}

const KERNEL: &str = "
    kernel tone() {
        audio {
            saw { chans: 1, freq: 110.0 },
            STATE mult { val: 0.5 }
        }
        saw >> mult[0]
        { mult }
    }
";

#[test]
fn kernels_take_a_state_only_as_a_whole() {
    // On the instance: the kernel is one node.
    let src = format!(
        "{}\npatches {{ suspended tone: t {{}} }}\n{{ t }}",
        KERNEL.replace("STATE ", "")
    );
    assert_eq!(states(&src, &["t"]), expect(&[("t", Suspended)]));

    // Inside the body there is no node for the executor to hold a state for.
    let src = format!(
        "{}\npatches {{ tone: t {{}} }}\n{{ t }}",
        KERNEL.replace("STATE", "muted")
    );
    assert!(matches!(
        build(&src),
        Err(ValidationError::UnsupportedInKernel(_))
    ));
}

#[test]
fn effects_that_mark_their_signal_inputs_can_start_bypassed() {
    let src = "audio { saw { chans: 1 }, mono_fan_out { chans: 2 }, bypassed plate480 { mix: 0.4 } }\n\
               saw >> mono_fan_out >> plate480\n{ plate480 }";
    assert_eq!(
        states(src, &["plate480"]),
        expect(&[("plate480", Bypassed)])
    );
}
