//! Verifies the kernel emitter by treating its output the way a downstream
//! user will.
//!
//! The emitted module lives at `tests/generated/fm3.rs` and is generated with
//! the `"legato"` path prefix, so everything it touches must be reachable
//! through the crate's *public* API. That is the point of it being here rather
//! than inside `src/`: emitted code compiled in-crate with `crate::` paths
//! would happily reach private items and only fail once a real downstream user
//! tried it. It also keeps this fixture out of the published library, where it
//! would be dead weight.
//!
//! Three properties are checked, and they are deliberately independent:
//!
//! 1. **It compiles** — the module is part of this test binary, through the
//!    public API only.
//! 2. **It behaves** — run against the interpreter it was derived from, at
//!    exact equality.
//! 3. **It is current** — the emitter is re-run and byte-compared against the
//!    checked-in file, so an emitter change cannot leave a stale artifact
//!    passing tests against yesterday's output.
//!
//! # Formatting is rustfmt's job, not the emitter's
//!
//! Both sides of the snapshot comparison are piped through `rustfmt` first.
//! The alternative — having the emitter produce byte-exact final formatting —
//! loses to `cargo fmt` immediately: CI runs `cargo fmt --check`, rustfmt
//! reaches this file through the `#[path]` module below, and the reformatted
//! result then fails the snapshot on a clean tree. Normalizing both sides
//! instead means the emitter only has to emit *correct* code, the checked-in
//! artifact stays idiomatic and readable, and `cargo fmt` is a no-op on it.
//!
//! Once `legato-macros` exists, this file is what a `kernel_from_file!`
//! integration test replaces — same shape, real macro instead of a committed
//! artifact.

use legato::{
    builder::ResourceBuilderView,
    config::{BlockSize, Config},
    dsl::{
        ir::{IRMacro, Object, Value},
        lower::ast_to_graph,
        parse::legato_parser,
    },
    kernel::{
        EXAMPLE_MODTAP_KERNEL_PATCH, EXAMPLE_PLATE_KERNEL_PATCH, KernelGraph, ProbeOracle,
        lower_kernel,
    },
    kernel_codegen::{fm3_interpreter, fm3_plan},
    kernel_emit::emit_kernel,
    kernel_plan::{KernelPlan, resolve_plan},
    persample::PerSampleNode,
    resources::ResourceBuilder,
};
use std::collections::HashMap;

/// Parse a DSL program and pull out one kernel definition. The trailing patch
/// just gives the parser a complete program.
fn kernel_definition(src: &str, name: &str) -> IRMacro {
    let program = format!("{src} audio {{ sine }} {{ sine }}");
    let ast = legato_parser(&program).expect("kernel source should parse");
    ast_to_graph(ast)
        .macro_registry
        .get(name)
        .unwrap_or_else(|| panic!("kernel '{name}' missing from registry"))
        .clone()
}

/// Run a closure with a throwaway resource builder view.
///
/// `tap` allocates its delay lines through this, so anything with delay taps
/// needs one even outside a running graph.
fn with_resources<R>(sample_rate: u32, f: impl FnOnce(&mut ResourceBuilderView) -> R) -> R {
    let config = Config::new(sample_rate as usize, BlockSize::Block64, 1, 0);
    let mut resource_builder = ResourceBuilder::default();
    let mut external = HashMap::new();
    let mut delays = HashMap::new();
    let mut view = ResourceBuilderView {
        config: &config,
        resource_builder: &mut resource_builder,
        external_buffer_keys: &mut external,
        delay_keys: &mut delays,
    };
    f(&mut view)
}

/// Instantiation params for `modtap4`, taken from `examples/modtap.rs` so the
/// generated artifact matches what the example actually runs — and so the
/// `$depth`/`$rate`/`$feedback` template substitution is exercised with
/// overrides rather than defaults.
fn modtap_params() -> Object {
    let mut params = Object::new();
    params.insert("depth".into(), Value::F32(12.0));
    params.insert("rate".into(), Value::F32(0.05));
    params.insert("feedback".into(), Value::F32(0.6));
    params
}

/// The salt must match between plan and interpreter or identity seeds diverge.
const MODTAP_SALT: &str = "modtap4";

fn modtap_plan(sample_rate: u32) -> KernelPlan {
    let config = Config::new(sample_rate as usize, BlockSize::Block64, 1, 0);
    resolve_plan(
        &kernel_definition(EXAMPLE_MODTAP_KERNEL_PATCH, "modtap4"),
        &modtap_params(),
        MODTAP_SALT,
        &mut ProbeOracle::new(&config),
    )
    .expect("modtap4 should resolve")
}

fn modtap_interpreter(sample_rate: u32) -> KernelGraph {
    let def = kernel_definition(EXAMPLE_MODTAP_KERNEL_PATCH, "modtap4");
    with_resources(sample_rate, |rb| {
        lower_kernel(&def, &modtap_params(), MODTAP_SALT, rb).expect("modtap4 should lower")
    })
}

// Not a test target of its own: files in subdirectories of `tests/` are not
// compiled as separate integration-test crates, so this is just a module.
#[path = "generated/fm3.rs"]
mod generated_fm3;

#[path = "generated/modtap4.rs"]
mod generated_modtap4;

#[path = "generated/plate.rs"]
mod generated_plate;

/// Format Rust source the same way `cargo fmt` would, so the emitter never has
/// to reason about line breaks. Requires `rustfmt` on PATH — it ships with the
/// toolchain and is in the dev shell, so a failure here is a broken
/// environment rather than a broken test.
fn rustfmt(source: &str) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout", "--quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("rustfmt should be available on PATH");

    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(source.as_bytes())
        .expect("should write source to rustfmt");

    let output = child.wait_with_output().expect("rustfmt should run");
    assert!(
        output.status.success(),
        "rustfmt rejected the emitted source: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("rustfmt output should be utf8")
}

/// Instantiation params for `plate`, matching the settings the plate benchmark
/// and `plate_kernel_matches_rust_plate_envelope` use.
fn plate_params() -> Object {
    let mut params = Object::new();
    for (key, value) in [
        ("predelay", 10.0f32),
        ("decay", 0.5),
        ("damping", 0.3),
        ("bandwidth_a", 0.0005),
        ("wet", 1.0),
        ("dry", 0.0),
    ] {
        params.insert(key.into(), Value::F32(value));
    }
    params
}

const PLATE_SALT: &str = "plate";

fn plate_plan(sample_rate: u32) -> KernelPlan {
    let config = Config::new(sample_rate as usize, BlockSize::Block64, 1, 0);
    resolve_plan(
        &kernel_definition(EXAMPLE_PLATE_KERNEL_PATCH, "plate"),
        &plate_params(),
        PLATE_SALT,
        &mut ProbeOracle::new(&config),
    )
    .expect("plate should resolve")
}

fn plate_interpreter(sample_rate: u32) -> KernelGraph {
    let def = kernel_definition(EXAMPLE_PLATE_KERNEL_PATCH, "plate");
    with_resources(sample_rate, |rb| {
        lower_kernel(&def, &plate_params(), PLATE_SALT, rb).expect("plate should lower")
    })
}

/// Path of a checked-in emitter output.
fn generated_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/generated/{name}.rs"))
}

/// Every kernel with a checked-in artifact, paired with its emitter input.
fn generated_artifacts() -> Vec<(&'static str, KernelPlan)> {
    vec![
        ("fm3", fm3_plan(48_000)),
        ("modtap4", modtap_plan(48_000)),
        ("plate", plate_plan(48_000)),
    ]
}

/// Build the generated kernel. Unlike the hand-written reference it takes its
/// sample rate and delay-line allocations from a resource builder, which is
/// what lets the same emitter handle nodes like `tap`.
fn build_generated(sample_rate: u32) -> generated_fm3::Fm3 {
    with_resources(sample_rate, |rb| {
        generated_fm3::Fm3::new(rb).expect("generated fm3 should build")
    })
}

/// Emitted code must track the interpreter sample for sample, including
/// through the feedback path where any mismatch in the z⁻¹ or the summation
/// order would drift the oscillator phase.
///
/// Exact equality rather than a tolerance: summation-order drift is precisely
/// the bug class codegen introduces, and at `1e-4` it would pass silently
/// while slowly detuning a feedback loop.
#[test]
fn generated_fm3_matches_interpreter() {
    let mut interp = fm3_interpreter(48_000);
    let mut generated = build_generated(48_000);

    assert_eq!(PerSampleNode::ports(&generated).audio_in.len(), 1);
    assert_eq!(PerSampleNode::ports(&generated).audio_out.len(), 1);

    let mut rng: u32 = 0x9E37_79B9;
    let mut a = [0.0f32];
    let mut b = [0.0f32];
    let mut energy = 0.0f32;

    for n in 0..4096 {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let x = Some((rng as f32 / u32::MAX as f32) * 10.0 - 5.0);

        interp.tick(&[x], &mut a);
        generated.tick(&[x], &mut b);

        assert_eq!(
            a[0], b[0],
            "generated code diverged from the interpreter at sample {n}"
        );
        energy += b[0] * b[0];
    }

    assert!(energy > 1e-3, "generated fm3 produced no signal");
}

/// An unpatched exterior input must stay unpatched all the way through.
///
/// This is the one path the randomized test never takes: it always supplies
/// `Some(x)`, so the `patched` bookkeeping in a mixed interior/exterior port
/// sum is never exercised with `None`.
#[test]
fn generated_fm3_matches_interpreter_with_input_unpatched() {
    let mut interp = fm3_interpreter(48_000);
    let mut generated = build_generated(48_000);

    let mut a = [0.0f32];
    let mut b = [0.0f32];

    for n in 0..2048 {
        interp.tick(&[None], &mut a);
        generated.tick(&[None], &mut b);
        assert_eq!(a[0], b[0], "diverged on unpatched input at sample {n}");
    }
}

/// The checked-in file must be exactly what the emitter produces today.
/// Without this, an emitter change would leave the artifact stale while every
/// behavioral test above kept passing against the *old* generated code.
/// `modtap4` is the coverage kernel: 25 nodes exercising everything `fm3` does
/// not — `tap` delay lines allocated through the resource builder, 4-channel
/// `mult` nodes read by index (`depth[0]`, `fb[2]`), a 2-channel sink, four
/// independent feedback loops, and `$template` params overridden at
/// instantiation.
///
/// Driven with an impulse followed by silence so the delay lines fill and the
/// feedback loops recirculate; a mismatch anywhere in the z⁻¹ placement or the
/// cubic delay interpolation shows up as drift long before the run ends.
#[test]
fn generated_modtap_matches_interpreter() {
    let mut interp = modtap_interpreter(48_000);
    let mut generated = with_resources(48_000, |rb| {
        generated_modtap4::Modtap4::new(rb).expect("generated modtap4 should build")
    });

    assert_eq!(PerSampleNode::ports(&generated).audio_in.len(), 1);
    assert_eq!(PerSampleNode::ports(&generated).audio_out.len(), 2);

    let mut a = [0.0f32; 2];
    let mut b = [0.0f32; 2];
    let mut energy = 0.0f32;

    for n in 0..48_000 {
        let x = if n == 0 { Some(1.0) } else { Some(0.0) };

        interp.tick(&[x], &mut a);
        generated.tick(&[x], &mut b);

        assert_eq!(a, b, "generated modtap4 diverged at sample {n}");
        energy += b[0] * b[0] + b[1] * b[1];
    }

    assert!(energy > 1e-4, "generated modtap4 produced no wet signal");
}

/// Same kernel with the input left unpatched — exercises the `patched`
/// bookkeeping on the `sub` nodes, whose second port is fed only by the
/// exterior input.
#[test]
fn generated_modtap_matches_interpreter_with_input_unpatched() {
    let mut interp = modtap_interpreter(48_000);
    let mut generated = with_resources(48_000, |rb| {
        generated_modtap4::Modtap4::new(rb).expect("generated modtap4 should build")
    });

    let mut a = [0.0f32; 2];
    let mut b = [0.0f32; 2];

    for n in 0..4096 {
        interp.tick(&[None], &mut a);
        generated.tick(&[None], &mut b);
        assert_eq!(a, b, "diverged on unpatched input at sample {n}");
    }
}

/// The plate is the largest dogfooded kernel — 64 nodes, 2 in / 2 out, dense
/// with nested allpass feedback. It is also the one whose *interpreted* form is
/// already benchmarked against a hand-written Rust node, so it is the honest
/// real-world case for judging whether codegen closes that gap.
#[test]
fn generated_plate_matches_interpreter() {
    let mut interp = plate_interpreter(48_000);
    let mut generated = with_resources(48_000, |rb| {
        generated_plate::Plate::new(rb).expect("generated plate should build")
    });

    assert_eq!(PerSampleNode::ports(&generated).audio_in.len(), 2);
    assert_eq!(PerSampleNode::ports(&generated).audio_out.len(), 2);

    let mut a = [0.0f32; 2];
    let mut b = [0.0f32; 2];
    let mut energy = 0.0f32;

    // Impulse into both channels, then a long tail so the tank recirculates.
    for n in 0..48_000 {
        let x = if n == 0 { Some(1.0) } else { Some(0.0) };

        interp.tick(&[x, x], &mut a);
        generated.tick(&[x, x], &mut b);

        assert_eq!(a, b, "generated plate diverged at sample {n}");
        energy += b[0] * b[0] + b[1] * b[1];
    }

    assert!(energy > 1e-4, "generated plate produced no wet signal");
}

/// Every checked-in file must be exactly what the emitter produces today.
/// Without this, an emitter change would leave the artifacts stale while the
/// behavioral tests above kept passing against the *old* generated code.
#[test]
fn checked_in_artifacts_are_current_emitter_output() {
    for (name, plan) in generated_artifacts() {
        let expected = std::fs::read_to_string(generated_path(name))
            .unwrap_or_else(|_| panic!("checked-in {name} should exist"));
        let actual = rustfmt(&emit_kernel(&plan, "legato"));

        assert_eq!(
            actual, expected,
            "tests/generated/{name}.rs is stale. Regenerate with: \
             cargo test --test kernel_codegen regenerate -- --ignored"
        );
    }
}

/// Rewrites the checked-in artifacts from the current emitter. Ignored by
/// default so a normal test run never mutates the tree.
#[test]
#[ignore = "writes to tests/generated; run explicitly after changing the emitter"]
fn regenerate() {
    for (name, plan) in generated_artifacts() {
        let source = rustfmt(&emit_kernel(&plan, "legato"));
        std::fs::write(generated_path(name), source)
            .unwrap_or_else(|_| panic!("should write generated {name}"));
    }
}
