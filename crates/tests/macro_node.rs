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
        lower_kernel(&definition, &Object::new(), "modtap4", &mut view).expect("should lower");
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
