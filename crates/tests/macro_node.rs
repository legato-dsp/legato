//! End-to-end test of the `include_node!` proc macro — the downstream story.
//!
//! Everything else in the suite verifies the emitter by comparing a *committed*
//! artifact. This drives the real path instead: a `.legato` file compiled by the
//! macro at build time, registered by name, and instantiated from a block-rate
//! graph — exactly what a downstream user writing a chorus or a reverb does.
//!
//! It also closes the loop on correctness. The same kernel is run through the
//! interpreter and compared sample for sample, so the macro path is held to the
//! same bit-exact standard as the checked-in artifacts. That is the payoff for
//! keeping the interpreter: the reference implementation stays live, and every
//! new layer gets checked against it rather than trusted.

use legato::{
    builder::{LegatoBuilder, ResourceBuilderView, Unconfigured},
    config::{BlockSize, Config},
    dsl::{ir::Object, lower::ast_to_graph, parse::legato_parser},
    kernel::lower_kernel,
    persample::PerSampleNode,
    ports::PortBuilder,
    resources::ResourceBuilder,
    spec::NodeDefinition,
};
use std::collections::HashMap;

// The unit under test: a DSL file becomes a Rust node at compile time.
legato_macros::include_node!("kernels/modtap4.legato", "modtap4");

/// Source of the same kernel, for the interpreted reference.
const MODTAP_SRC: &str = include_str!("../kernels/modtap4.legato");

/// The macro-generated node must be usable from a graph like any built-in:
/// registered by name, wired with `>>`, and driven as the sink.
#[test]
fn macro_generated_node_registers_and_renders() {
    let config = Config {
        sample_rate: 48_000,
        block_size: 512,
        channels: 2,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let (mut app, _frontend) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .register_node("audio", Modtap4::spec())
        .build_dsl(
            r#"
            audio {
                saw { freq: 110.0, chans: 1 },
                modtap4,
            }

            saw >> modtap4[0]

            { modtap4 }
        "#,
        );

    // Render enough blocks for the shortest tap (71 ms) to come back.
    let mut energy = [0.0f32; 2];
    for _ in 0..16 {
        let out = app.next_block(None);
        for channel in 0..2 {
            for &sample in *out.channels.get(channel).expect("stereo output") {
                assert!(
                    sample.is_finite(),
                    "generated node emitted a non-finite sample"
                );
                energy[channel] += sample * sample;
            }
        }
    }

    assert!(
        energy[0] > 1e-3 && energy[1] > 1e-3,
        "both channels should carry signal, got {energy:?}"
    );
}

/// The macro path must agree with the interpreter exactly, not approximately.
///
/// Both sides are driven at `tick` level so the comparison isolates the kernel
/// itself, with no block adapter or fan-in gains in between.
#[test]
fn macro_generated_node_matches_interpreter() {
    let sample_rate = 48_000;

    let config = Config::new(sample_rate, BlockSize::Block64, 1, 0);
    let mut resource_builder = ResourceBuilder::default();
    let mut external = HashMap::new();
    let mut delays = HashMap::new();
    let mut view = ResourceBuilderView {
        config: &config,
        resource_builder: &mut resource_builder,
        external_buffer_keys: &mut external,
        delay_keys: &mut delays,
        instance_alias: "modtap4",
    };

    // The macro salts identity seeds with the kernel name, so the interpreter
    // must use the same salt or any kernel with a `noise` node would diverge.
    let program = format!("{MODTAP_SRC}\n audio {{ sine }} {{ sine }}");
    let definition = ast_to_graph(legato_parser(&program).expect("kernel file should parse"))
        .macro_registry
        .get("modtap4")
        .expect("modtap4 should be in the registry")
        .clone();

    // No instance params: the macro resolves from the file's declared defaults,
    // so the reference has to do the same.
    let mut interpreted =
        lower_kernel(&definition, &Object::new(), &mut view).expect("should lower");
    let mut generated = Modtap4::new(&mut view).expect("generated node should build");

    let mut a = [0.0f32; 2];
    let mut b = [0.0f32; 2];

    for n in 0..48_000 {
        let x = if n == 0 { Some(1.0) } else { Some(0.0) };

        interpreted.tick(&[x], &mut a);
        generated.tick(&[x], &mut b);

        assert_eq!(
            a, b,
            "macro-generated node diverged from interpreter at {n}"
        );
    }
}

/// Declared params must be settable three ways, and all three must reach the
/// interior nodes rather than merely updating a field.
///
/// The check is behavioural, not a getter round-trip: a setter that recorded
/// the value but never forwarded the message would pass any assertion on
/// `feedback()` while changing nothing about the audio. So this drives an
/// impulse through and measures the tail, which only grows if the interior
/// `mult: fb` nodes actually saw the new gain.
#[test]
fn declared_params_reach_interior_nodes() {
    fn tail_energy(feedback: f32) -> f32 {
        let config = Config::new(48_000, BlockSize::Block64, 1, 0);
        let mut resource_builder = ResourceBuilder::default();
        let mut external = HashMap::new();
        let mut delays = HashMap::new();
        let mut view = ResourceBuilderView {
            config: &config,
            resource_builder: &mut resource_builder,
            external_buffer_keys: &mut external,
            delay_keys: &mut delays,
            instance_alias: "modtap4",
        };

        let mut node = Modtap4::new(&mut view).expect("should build");
        node.set_feedback(feedback);
        assert_eq!(node.feedback(), feedback);

        let mut out = [0.0f32; 2];
        let mut energy = 0.0f32;

        // Past the longest tap (241 ms), only recirculated signal remains.
        for n in 0..48_000 {
            let x = if n == 0 { Some(1.0) } else { Some(0.0) };
            node.tick(&[x], &mut out);
            if n > 24_000 {
                energy += out[0] * out[0] + out[1] * out[1];
            }
        }
        energy
    }

    let quiet = tail_energy(0.0);
    let loud = tail_energy(0.9);

    assert!(
        loud > quiet * 10.0,
        "feedback should recirculate: {quiet:e} at 0.0 vs {loud:e} at 0.9"
    );
}

/// The same param must be reachable by message, which is the path the frontend
/// and UI layer actually use — they send `NodeMessage`, they do not call
/// methods. An unknown name must be ignored, not panic: these arrive from user
/// input on the audio thread.
#[test]
fn params_route_through_node_messages() {
    use legato::msg::{NodeMessage, ParamPayload, RtValue};

    let config = Config::new(48_000, BlockSize::Block64, 1, 0);
    let mut resource_builder = ResourceBuilder::default();
    let mut external = HashMap::new();
    let mut delays = HashMap::new();
    let mut view = ResourceBuilderView {
        config: &config,
        resource_builder: &mut resource_builder,
        external_buffer_keys: &mut external,
        delay_keys: &mut delays,
        instance_alias: "modtap4",
    };

    let mut node = Modtap4::new(&mut view).expect("should build");

    node.handle_msg(NodeMessage::SetParam(ParamPayload {
        param_name: "feedback",
        value: RtValue::F32(0.42),
    }));
    assert_eq!(node.feedback(), 0.42);

    // Unknown params are dropped rather than taking down the audio thread.
    node.handle_msg(NodeMessage::SetParam(ParamPayload {
        param_name: "not_a_param",
        value: RtValue::F32(1.0),
    }));
    assert_eq!(node.feedback(), 0.42);
}

/// Params set on the instantiation in a graph must apply — the silent no-op
/// this work existed to remove. Two graphs differing only in `feedback` must
/// render differently.
#[test]
fn instantiation_params_apply_through_the_graph() {
    fn render(feedback: f32) -> f32 {
        let config = Config {
            sample_rate: 48_000,
            block_size: 512,
            channels: 2,
            rt_capacity: 0,
        };
        let ports = PortBuilder::default().audio_out(2).build();
        let graph = format!(
            r#"
            audio {{
                saw {{ freq: 110.0, chans: 1 }},
                modtap4 {{ feedback: {feedback} }},
            }}

            saw >> modtap4[0]

            {{ modtap4 }}
        "#
        );

        let (mut app, _frontend) = LegatoBuilder::<Unconfigured>::new(config, ports)
            .register_node("audio", Modtap4::spec())
            .build_dsl(&graph);

        let mut energy = 0.0f32;
        for _ in 0..40 {
            let out = app.next_block(None);
            for &sample in *out.channels.first().expect("output") {
                energy += sample * sample;
            }
        }
        energy
    }

    let low = render(0.0);
    let high = render(0.9);

    assert!(low.is_finite() && high.is_finite());
    assert!(
        (low - high).abs() / low.max(high) > 0.05,
        "instantiation params should change the render: {low:e} vs {high:e}"
    );
}

legato_macros::include_node!("kernels/noisy.legato", "noisy");

/// Two instances of the *same* compiled kernel must not produce identical
/// noise.
///
/// This is what polyphony depends on: a generated karplus voice excited by the
/// same stream on every note sounds like a click rather than a pluck — a bug
/// that has already been fixed once for the interpreted path. Generated code
/// gets it by deriving seeds from `rb.instance_alias` at construction rather
/// than baking a literal, so distinct graph aliases decorrelate exactly as
/// distinct interpreted instantiations do.
#[test]
fn separate_instances_of_a_generated_kernel_decorrelate() {
    fn render(alias: &str) -> Vec<f32> {
        let config = Config::new(48_000, BlockSize::Block64, 1, 0);
        let mut resource_builder = ResourceBuilder::default();
        let mut external = HashMap::new();
        let mut delays = HashMap::new();
        let mut view = ResourceBuilderView {
            config: &config,
            resource_builder: &mut resource_builder,
            external_buffer_keys: &mut external,
            delay_keys: &mut delays,
            instance_alias: alias,
        };

        let mut node = Noisy::new(&mut view).expect("should build");
        let mut out = [0.0f32];
        (0..512)
            .map(|_| {
                node.tick(&[None], &mut out);
                out[0]
            })
            .collect()
    }

    let voice0 = render("voice.0");
    let voice1 = render("voice.1");

    assert_ne!(voice0, voice1, "two instances share a noise stream");

    // Same alias must still be reproducible — decorrelation cannot come from
    // construction order or a global counter, or builds stop being repeatable.
    assert_eq!(
        voice0,
        render("voice.0"),
        "same alias must be deterministic"
    );

    // And they should be genuinely uncorrelated, not merely offset.
    let dot: f32 = voice0.iter().zip(&voice1).map(|(a, b)| a * b).sum();
    let norm: f32 = voice0.iter().map(|a| a * a).sum::<f32>().sqrt()
        * voice1.iter().map(|b| b * b).sum::<f32>().sqrt();
    assert!(
        (dot / norm).abs() < 0.2,
        "streams should be uncorrelated, got r = {}",
        dot / norm
    );
}

/// A noise-bearing kernel must match the interpreter for the same instance
/// alias.
///
/// This is the check that makes per-instance seeding trustworthy rather than
/// merely different: both backends call the same `identity_seed` with the same
/// alias, so switching a kernel from interpreted to compiled must not change
/// what it sounds like. Without it, decorrelation and equivalence could each
/// pass while disagreeing with one another.
#[test]
fn generated_noise_matches_interpreter_for_the_same_alias() {
    let config = Config::new(48_000, BlockSize::Block64, 1, 0);
    let mut resource_builder = ResourceBuilder::default();
    let mut external = HashMap::new();
    let mut delays = HashMap::new();
    let mut view = ResourceBuilderView {
        config: &config,
        resource_builder: &mut resource_builder,
        external_buffer_keys: &mut external,
        delay_keys: &mut delays,
        instance_alias: "voice.7",
    };

    let source = include_str!("../kernels/noisy.legato");
    let program = format!("{source}\n audio {{ sine }} {{ sine }}");
    let definition = ast_to_graph(legato_parser(&program).expect("should parse"))
        .macro_registry
        .get("noisy")
        .expect("noisy should be in the registry")
        .clone();

    let mut interpreted =
        lower_kernel(&definition, &Object::new(), &mut view).expect("should lower");
    let mut generated = Noisy::new(&mut view).expect("should build");

    let mut a = [0.0f32];
    let mut b = [0.0f32];

    for n in 0..4096 {
        interpreted.tick(&[None], &mut a);
        generated.tick(&[None], &mut b);
        assert_eq!(a[0], b[0], "noise seed disagreed between backends at {n}");
    }

    // Guard against both backends being silently silent.
    assert!(b[0] != 0.0, "kernel produced no noise");
}
