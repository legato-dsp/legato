//! Equivalence between the block-rate `Node::process` path and the per-sample
//! `PerSampleNode::tick` path.
//!
//! Every node that implements both traits must produce (near-)identical output
//! when driven sample-by-sample through the `PerSample` adapter as when
//! processing whole blocks. Purely scalar nodes are expected to match exactly;
//! nodes with SIMD block paths (sine, saw) accumulate tiny floating-point
//! differences from chunked phase accumulation, so they get a small tolerance.

use legato::{
    config::{BlockSize, Config},
    harness::build_placeholder_context,
    node::Node,
    nodes::audio::{
        allpass::Allpass,
        onepole::OnePole,
        ops::{ApplyOpKind, mult_node_factory},
        saw::Saw,
        sine::Sine,
        svf::{FilterType, Svf},
        tap::DelayTap,
    },
    persample::{PerSample, PerSampleNode},
};

const SR: usize = 48_000;
const BLOCK: usize = 256;
const BLOCKS: usize = 8;
const TOTAL: usize = BLOCK * BLOCKS;

/// Tiny deterministic LCG so the suite needs no RNG dependency.
struct Lcg(u64);

impl Lcg {
    /// Uniform-ish in [-1, 1).
    fn next_f32(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32 / (1u64 << 31) as f32) - 1.0
    }
}

fn noise(seed: u64) -> Vec<f32> {
    let mut lcg = Lcg(seed);
    (0..TOTAL).map(|_| lcg.next_f32()).collect()
}

/// A slow ramp from `lo` to `hi` over the whole test signal.
fn ramp(lo: f32, hi: f32) -> Vec<f32> {
    (0..TOTAL)
        .map(|i| lo + (hi - lo) * (i as f32 / TOTAL as f32))
        .collect()
}

/// Drive one clone of `node` block-wise and another through the `PerSample`
/// adapter, asserting the outputs agree within `tol` on every port and sample.
fn assert_tick_equivalence<T>(node: T, inputs: &[Option<Vec<f32>>], tol: f32, name: &str)
where
    T: Node + PerSampleNode + Clone + Send + 'static,
{
    let n_out = Node::ports(&node).audio_out.len();

    let mut block_node = node.clone();
    let mut per_sample = PerSample::new(node);

    let mut ctx = build_placeholder_context(Config::new(SR, BlockSize::Block256, 2, 0));

    let mut block_out = vec![vec![0.0f32; TOTAL]; n_out];
    let mut tick_out = vec![vec![0.0f32; TOTAL]; n_out];

    for blk in 0..BLOCKS {
        let range = blk * BLOCK..(blk + 1) * BLOCK;

        let ins: Vec<Option<&[f32]>> = inputs
            .iter()
            .map(|o| o.as_ref().map(|v| &v[range.clone()]))
            .collect();

        {
            let mut outs: Vec<&mut [f32]> = block_out
                .iter_mut()
                .map(|v| &mut v[range.clone()])
                .collect();
            block_node.process(&mut ctx, &ins, &mut outs);
        }
        {
            let mut outs: Vec<&mut [f32]> =
                tick_out.iter_mut().map(|v| &mut v[range.clone()]).collect();
            per_sample.process(&mut ctx, &ins, &mut outs);
        }
    }

    for (p, (block_chan, tick_chan)) in block_out.iter().zip(tick_out.iter()).enumerate() {
        for (i, (a, b)) in block_chan.iter().zip(tick_chan.iter()).enumerate() {
            assert!(
                (a - b).abs() <= tol,
                "{name}: port {p}, sample {i}: block={a} vs tick={b} (tol {tol})"
            );
        }
    }
}

// ── Oscillators ─────────────────────────────────────────────────────────────

#[test]
fn sine_internal_freq_matches() {
    assert_tick_equivalence(Sine::new(440.0, SR as f32), &[None], 1e-3, "sine internal");
}

#[test]
fn sine_external_fm_matches() {
    assert_tick_equivalence(
        Sine::new(440.0, SR as f32),
        &[Some(ramp(100.0, 2000.0))],
        1e-3,
        "sine fm",
    );
}

#[test]
fn saw_internal_freq_matches() {
    // The PolyBLEP correction has slope ~2/dt near the wrap, so ulp-level
    // phase drift between chunked SIMD and scalar accumulation is amplified
    // by a factor of a few hundred right at the discontinuity samples.
    assert_tick_equivalence(
        Saw::new(220.0, 2, SR as f32),
        &[None],
        1e-2,
        "saw internal",
    );
}

#[test]
fn saw_external_freq_matches() {
    assert_tick_equivalence(
        Saw::new(220.0, 2, SR as f32),
        &[Some(ramp(50.0, 1000.0))],
        1e-2,
        "saw fm",
    );
}

// ── Filters ─────────────────────────────────────────────────────────────────

#[test]
fn onepole_matches() {
    assert_tick_equivalence(
        OnePole::new(2000.0, 2, SR),
        &[Some(noise(1)), Some(noise(2)), None],
        0.0,
        "onepole",
    );
}

#[test]
fn svf_static_matches() {
    assert_tick_equivalence(
        Svf::new(SR as f32, FilterType::LowPass, 3400.0, 1.0, 0.6, 2),
        &[Some(noise(3)), Some(noise(4)), None, None],
        0.0,
        "svf static",
    );
}

#[test]
fn svf_modulated_matches() {
    assert_tick_equivalence(
        Svf::new(SR as f32, FilterType::BandPass, 3400.0, 1.0, 0.6, 2),
        &[
            Some(noise(5)),
            Some(noise(6)),
            Some(ramp(200.0, 8000.0)),
            Some(ramp(0.3, 1.2)),
        ],
        0.0,
        "svf modulated",
    );
}

// ── Delays ──────────────────────────────────────────────────────────────────

#[test]
fn allpass_static_matches() {
    assert_tick_equivalence(
        Allpass::new(2, 0.5, 480.0, 4096, SR as f32),
        &[Some(noise(7)), Some(noise(8)), None, None],
        0.0,
        "allpass static",
    );
}

#[test]
fn allpass_modulated_matches() {
    assert_tick_equivalence(
        Allpass::new(2, 0.5, 480.0, 4096, SR as f32),
        &[
            Some(noise(9)),
            Some(noise(10)),
            Some(ramp(5.0, 20.0)),
            Some(ramp(0.3, 0.6)),
        ],
        0.0,
        "allpass modulated",
    );
}

#[test]
fn tap_static_matches() {
    assert_tick_equivalence(
        DelayTap::new(2, 480.0, 4096, SR as f32),
        &[Some(noise(11)), Some(noise(12)), None],
        0.0,
        "tap static",
    );
}

#[test]
fn tap_modulated_matches() {
    assert_tick_equivalence(
        DelayTap::new(2, 480.0, 4096, SR as f32),
        &[
            Some(noise(13)),
            Some(noise(14)),
            Some(ramp(2.0, 30.0)),
        ],
        0.0,
        "tap modulated",
    );
}

// ── Ops ─────────────────────────────────────────────────────────────────────

#[test]
fn ops_match() {
    // (kind, name, with modulated val port)
    let cases = [
        (ApplyOpKind::Add, "add"),
        (ApplyOpKind::Mult, "mult"),
        (ApplyOpKind::Gain, "gain"),
    ];

    for (kind, name) in cases {
        let make = || match kind {
            ApplyOpKind::Add => mult_node_factory(0.25, 1, ApplyOpKind::Add),
            ApplyOpKind::Mult => mult_node_factory(0.25, 1, ApplyOpKind::Mult),
            ApplyOpKind::Gain => mult_node_factory(0.25, 1, ApplyOpKind::Gain),
            _ => unreachable!(),
        };

        assert_tick_equivalence(
            make(),
            &[Some(noise(15)), None],
            0.0,
            &format!("{name} internal val"),
        );
        assert_tick_equivalence(
            make(),
            &[Some(noise(16)), Some(ramp(0.0, 2.0))],
            0.0,
            &format!("{name} modulated val"),
        );
    }
}
