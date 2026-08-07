//! Property tests to ensure that various block sizes render the same

use legato::{
    LegatoApp,
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    ports::PortBuilder,
};
use proptest::prelude::*;
use std::ops::Range;

const SR: usize = 48_000;
/// Total samples rendered per case, chosen to divide by every block size tested.
const SAMPLES: usize = 3072;

const FREQ: Range<f32> = 40.0..4000.0;
const CUTOFF: Range<f32> = 100.0..8000.0;
const Q: Range<f32> = 0.5..4.0;
const GAIN: Range<f32> = 0.1..1.0;

/// Block sizes the executor supports; every one divides SAMPLES.
const BLOCK_SIZES: [usize; 4] = [64, 128, 256, 512];

/// This patch is bit-exact across block sizes today; any drift is a regression worth failing on.
const TOLERANCE: f32 = 0.0;

#[derive(Debug, Clone)]
struct PatchSpec {
    freq: f32,
    cutoff: f32,
    q: f32,
    gain: f32,
}

impl PatchSpec {
    fn source(&self) -> String {
        let Self {
            freq,
            cutoff,
            q,
            gain,
        } = self;

        format!(
            r#"
            audio {{
                sine {{ freq: {freq:?}, chans: 1 }},
                svf {{ cutoff: {cutoff:?}, q: {q:?}, chans: 1 }},
                onepole {{ cutoff: {cutoff:?}, chans: 1 }},
                gain {{ val: {gain:?}, chans: 1 }},
            }}

            sine >> svf
            svf >> onepole
            onepole >> gain

            {{ gain }}
        "#
        )
    }
}

fn patch_spec() -> impl Strategy<Value = PatchSpec> {
    (FREQ, CUTOFF, Q, GAIN).prop_map(|(freq, cutoff, q, gain)| PatchSpec {
        freq,
        cutoff,
        q,
        gain,
    })
}

fn render(src: &str, block_size: usize) -> Vec<f32> {
    let config = Config {
        sample_rate: SR,
        block_size,
        channels: 1,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_out(1).build();
    let (mut app, _frontend): (LegatoApp, _) =
        LegatoBuilder::<Unconfigured>::new(config, ports).build_dsl(src);

    let mut out = Vec::with_capacity(SAMPLES);
    while out.len() < SAMPLES {
        out.extend_from_slice(app.next_block().channels[0]);
    }
    out.truncate(SAMPLES);
    out
}

proptest! {
    /// P3: the graph rate is an implementation detail, so the rendered signal
    /// must not depend on the block size it was rendered at.
    #[test]
    fn output_is_block_size_invariant(
        spec in patch_spec(),
        a in 0..BLOCK_SIZES.len(),
        b in 0..BLOCK_SIZES.len(),
    ) {
        prop_assume!(a != b);

        let src = spec.source();
        let (block_a, block_b) = (BLOCK_SIZES[a], BLOCK_SIZES[b]);

        let rendered_a = render(&src, block_a);
        let rendered_b = render(&src, block_b);

        prop_assert_eq!(rendered_a.len(), SAMPLES);
        prop_assert_eq!(rendered_b.len(), SAMPLES);
        prop_assert!(rendered_a.iter().any(|x| *x != 0.0), "patch rendered silence");

        for (i, (x, y)) in rendered_a.iter().zip(rendered_b.iter()).enumerate() {
            prop_assert!(
                (x - y).abs() <= TOLERANCE * x.abs().max(1.0),
                "sample {} diverged: {} at block {}, {} at block {}",
                i, x, block_a, y, block_b
            );
        }
    }
}
