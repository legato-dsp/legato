//! Kernel codegen — proof-of-concept.
//!
//! The `kernel` DSL currently lowers to a [`KernelGraph`](crate::kernel::KernelGraph):
//! a data-oriented *interpreter* that walks flat wiring tables every sample.
//! That costs us enum dispatch (`KernelNode`) plus gather/accumulate
//! indirection through `src_pool`/`values` on the hot path.
//!
//! The end goal is a second backend that consumes the *same* resolved plan the
//! interpreter does and emits straight-line Rust: one struct field per
//! primitive, SSA locals instead of a `values` table, literal additions instead
//! of the per-port accumulate loop, and no enum dispatch. This module is
//! **step 1 of that plan**: a hand-written example of exactly what the generator
//! should emit, so we can (a) measure the performance ceiling and (b) lock in an
//! equivalence oracle before building the generator itself.
//!
//! The worked example is a 3-operator FM voice with a self-feedback operator:
//!
//! ```text
//! kernel fm3() {
//!     in fm_in
//!     audio {
//!         sine: dc  { freq: 0.0, phase: 0.25 },   // constant 1.0 (sin @ quarter turn)
//!         sine: op1 { freq: 110.0 },              // carrier -> output
//!         sine: op2 { freq: 220.0 },              // modulator
//!         sine: op3 { freq: 330.0 },              // modulator w/ self-feedback
//!         mult: b1  { val: 110.0 },               // base-frequency DC rails
//!         mult: b2  { val: 220.0 },
//!         mult: b3  { val: 330.0 },
//!         mult: i2  { val: 300.0 },               // FM index op2 -> op1
//!         mult: i3  { val: 300.0 },               // FM index op3 -> op2
//!         mult: fb  { val: 50.0 }                 // feedback depth on op3
//!     }
//!     dc >> b1[0]  dc >> b2[0]  dc >> b3[0]
//!     b3 >> op3[0]  op3 >> fb[0]  fb >> op3[0]    // fb -> op3 is the cycle => z^-1
//!     op3 >> i3[0]  b2 >> op2[0]  i3 >> op2[0]
//!     op2 >> i2[0]  b1 >> op1[0]  i2 >> op1[0]  fm_in >> op1[0]
//!     { op1 }
//! }
//! ```
//!
//! A sine's `freq` port *replaces* its param when patched, and multiple sources
//! into one port *sum* — so an operator's instantaneous frequency is built as
//! `base_rail + index*modulator (+ external)` by fanning those sources into the
//! single `freq` port. The `dc` node is the idiomatic constant source
//! (`sin(0.25 turns) == 1.0`, phase never advances at 0 Hz) scaled by the `mult`
//! rails to get DC base frequencies.
//!
//! [`Fm3`] below is the literal transcription. The two rules the real generator
//! encodes are visible in `tick`:
//!   1. **z^-1 on back edges.** `op3`'s frequency reads `fb` from the *previous*
//!      sample (`self.z_fb`), because `fb -> op3` closes the only cycle; every
//!      other read is same-sample. This mirrors the interpreter's persistent
//!      `values` table exactly.
//!   2. **Summation order.** Sources are added in the interpreter's `src_pool`
//!      order (interior edges in declaration order, external inputs last), so the
//!      floating-point result is bit-for-bit identical.

use crate::{
    nodes::audio::{
        ops::{ApplyOp, ApplyOpKind, mult_node_factory},
        sine::{Quality, Sine},
    },
    persample::PerSampleNode,
    ports::{PortBuilder, Ports},
};

/// The `fm3` kernel, hand-lowered to straight-line Rust — the shape the codegen
/// backend should emit. One field per stateful primitive; `z_fb` is the single
/// feedback slot (the `fb -> op3` back edge's z^-1).
#[derive(Clone)]
pub struct Fm3 {
    dc: Sine,
    op1: Sine,
    op2: Sine,
    op3: Sine,
    b1: ApplyOp,
    b2: ApplyOp,
    b3: ApplyOp,
    i2: ApplyOp,
    i3: ApplyOp,
    fb: ApplyOp,
    /// Previous-sample output of `fb`, read by `op3`'s frequency (the z^-1).
    z_fb: f32,
    ports: Ports,
}

impl Fm3 {
    pub fn new(sr: f32) -> Self {
        // `mult { val }` with the default single channel, matching the kernel's
        // `build_kernel_node` "mult" arm.
        let mult = |val: f32| mult_node_factory(val, 1, ApplyOpKind::Mult);

        Self {
            // `sine { freq, phase }` -> Sine::from_params (default quality High).
            dc: Sine::with_quality(0.0, sr, Quality::High).with_start_phase(0.25),
            op1: Sine::with_quality(110.0, sr, Quality::High),
            op2: Sine::with_quality(220.0, sr, Quality::High),
            op3: Sine::with_quality(330.0, sr, Quality::High),
            b1: mult(110.0),
            b2: mult(220.0),
            b3: mult(330.0),
            i2: mult(300.0),
            i3: mult(300.0),
            fb: mult(50.0),
            z_fb: 0.0,
            ports: PortBuilder::default()
                .audio_in_named(&["fm_in"])
                .audio_out(1)
                .build(),
        }
    }
}

/// The `fm3` kernel as DSL text — the interpreter form of [`Fm3`]. Public so the
/// equivalence test and the benchmark share one source of truth.
///
/// A sine's `freq` port replaces its param when patched, and multiple sources
/// into one port sum, so each operator's instantaneous frequency is built as
/// `base_rail + index*modulator (+ external)` fanned into the single `freq`
/// port. `dc` is the constant source (`sin(0.25 turns) == 1.0`, 0 Hz) scaled by
/// the `mult` rails into DC base frequencies. `fb -> op3` closes the only cycle,
/// so the engine puts its implicit z^-1 there.
pub const FM3_KERNEL_SRC: &str = r#"
    kernel fm3() {
        in fm_in

        audio {
            sine: dc  { freq: 0.0, phase: 0.25 },
            sine: op1 { freq: 110.0 },
            sine: op2 { freq: 220.0 },
            sine: op3 { freq: 330.0 },
            mult: b1  { val: 110.0 },
            mult: b2  { val: 220.0 },
            mult: b3  { val: 330.0 },
            mult: i2  { val: 300.0 },
            mult: i3  { val: 300.0 },
            mult: fb  { val: 50.0 }
        }

        dc >> b1[0]
        dc >> b2[0]
        dc >> b3[0]

        b3 >> op3[0]
        op3 >> fb[0]
        fb >> op3[0]

        op3 >> i3[0]
        b2 >> op2[0]
        i3 >> op2[0]

        op2 >> i2[0]
        b1 >> op1[0]
        i2 >> op1[0]
        fm_in >> op1[0]

        { op1 }
    }
"#;

/// Lower [`FM3_KERNEL_SRC`] to an interpreted `KernelGraph` at `sample_rate` —
/// the baseline [`Fm3`] is measured and checked against. Driven at the
/// `PerSampleNode::tick` level so both forms are compared apples-to-apples,
/// without the block adapter or fan-in gains in between.
pub fn fm3_interpreter(sample_rate: u32) -> crate::kernel::KernelGraph {
    use crate::{
        builder::ResourceBuilderView, config::Config, dsl::ir::Object, kernel::lower_kernel,
        resources::ResourceBuilder,
    };
    use std::collections::HashMap;

    let def = fm3_definition();

    let config = Config::new(
        sample_rate as usize,
        crate::config::BlockSize::Block64,
        1,
        0,
    );
    let mut resource_builder = ResourceBuilder::default();
    let mut external = HashMap::new();
    let mut delays = HashMap::new();
    let mut view = ResourceBuilderView {
        config: &config,
        resource_builder: &mut resource_builder,
        external_buffer_keys: &mut external,
        delay_keys: &mut delays,
        instance_alias: "fm3",
    };
    lower_kernel(&def, &Object::new(), &mut view).expect("fm3 kernel should lower")
}

/// Parse [`FM3_KERNEL_SRC`] and hand back the `fm3` kernel definition.
fn fm3_definition() -> crate::dsl::ir::IRMacro {
    use crate::dsl::{lower::ast_to_graph, parse::legato_parser};

    // The kernel is a declaration; the trailing patch just gives the parser a
    // complete program to chew on.
    let src = format!("{FM3_KERNEL_SRC} audio {{ sine }} {{ sine }}");
    let ast = legato_parser(&src).expect("fm3 source should parse");
    ast_to_graph(ast)
        .macro_registry
        .get("fm3")
        .expect("fm3 kernel missing from registry")
        .clone()
}

/// Resolve [`FM3_KERNEL_SRC`] into a plan — the emitter's input, and the same
/// plan [`fm3_interpreter`] is built from.
///
/// The `"fm3"` salt must match what `fm3_interpreter` passes, or the two would
/// derive different identity seeds and any kernel with a `noise` node would
/// stop comparing equal.
pub fn fm3_plan(sample_rate: u32) -> crate::kernel_plan::KernelPlan {
    use crate::{
        config::{BlockSize, Config},
        dsl::ir::Object,
        kernel::ProbeOracle,
        kernel_plan::resolve_plan,
    };

    let config = Config::new(sample_rate as usize, BlockSize::Block64, 1, 0);
    resolve_plan(
        &fm3_definition(),
        &Object::new(),
        "fm3",
        &mut ProbeOracle::new(&config),
    )
    .expect("fm3 kernel should resolve")
}

impl PerSampleNode for Fm3 {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        // Exterior input 0 == `fm_in`.
        let fm_in = in_frame[0];

        // Scratch for the reused primitive ticks. Each primitive owns its state
        // (phase, etc.); we only route values between them.
        let mut o = [0.0f32];

        // dc: constant ~1.0 (0 Hz sine started at a quarter turn). Its `freq`
        // port has no sources, so it falls back to the 0.0 param -> None here.
        self.dc.tick(&[None], &mut o);
        let dc = o[0];

        // Base-frequency DC rails: dc * {110, 220, 330}. `mult` reads the signal
        // at port 0 and its `val` at port 1 (unpatched -> internal 110/220/330).
        self.b1.tick(&[Some(dc), None], &mut o);
        let b1 = o[0];
        self.b2.tick(&[Some(dc), None], &mut o);
        let b2 = o[0];
        self.b3.tick(&[Some(dc), None], &mut o);
        let b3 = o[0];

        // op3: self-feedback operator. freq = b3 (this sample) + fb (previous
        // sample). Sources in src_pool order are [b3, fb]; fb is the back edge.
        self.op3.tick(&[Some(b3 + self.z_fb)], &mut o);
        let op3 = o[0];
        // Feedback gain for the *next* sample: fb = op3 * 50.
        self.fb.tick(&[Some(op3), None], &mut o);
        let fb = o[0];

        // op2: modulated by op3. freq = b2 + (op3 * 300). Sources [b2, i3].
        self.i3.tick(&[Some(op3), None], &mut o);
        let i3 = o[0];
        self.op2.tick(&[Some(b2 + i3)], &mut o);
        let op2 = o[0];

        // op1 (carrier): freq = b1 + (op2 * 300) + fm_in. Interior sources
        // [b1, i2] come first, the external `fm_in` is summed last (and only
        // when patched), exactly as the interpreter accumulates them.
        self.i2.tick(&[Some(op2), None], &mut o);
        let i2 = o[0];
        let mut carrier_freq = b1 + i2;
        if let Some(v) = fm_in {
            carrier_freq += v;
        }
        self.op1.tick(&[Some(carrier_freq)], &mut o);
        let op1 = o[0];

        // Sink: { op1 }.
        out_frame[0] = op1;

        // Commit the one-sample feedback delay.
        self.z_fb = fb;
    }
}
