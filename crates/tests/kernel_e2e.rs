//! End-to-end tests for `kernel` DSL declarations: parse → pipeline → builder
//! → runtime audio, alongside the block-rate graph features (spawning, port
//! selection, patches) that must keep working around kernels.

use legato::{
    LegatoApp,
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    ports::PortBuilder,
};

const BLOCK: usize = 256;
const BLOCKS: usize = 8;

fn build(src: &str, out_chans: usize) -> LegatoApp {
    let config = Config {
        sample_rate: 48_000,
        block_size: BLOCK,
        channels: out_chans,
        rt_capacity: 0,
    };
    let ports = PortBuilder::default().audio_out(out_chans).build();
    let (app, _frontend) = LegatoBuilder::<Unconfigured>::new(config, ports).build_dsl(src);
    app
}

/// Collect `BLOCKS` blocks of the sink's first `chans` channels.
fn render(app: &mut LegatoApp, chans: usize) -> Vec<Vec<f32>> {
    let mut out = vec![Vec::with_capacity(BLOCK * BLOCKS); chans];
    for _ in 0..BLOCKS {
        let view = app.next_block(None);
        for (c, chan) in out.iter_mut().enumerate() {
            chan.extend_from_slice(view.channels[c]);
        }
    }
    out
}

/// A cycle-free kernel must sound the same as the identical chain written as
/// block-rate nodes (within the osc SIMD-vs-scalar tolerance).
#[test]
fn kernel_chain_matches_block_chain() {
    let block_src = r#"
        audio {
            saw { chans: 1, freq: 110.0 },
            onepole { cutoff: 1000.0, chans: 1 },
            mult { val: 0.5 }
        }

        saw >> onepole[0]
        onepole >> mult[0]

        { mult }
    "#;

    let kernel_src = r#"
        kernel voice() {
            audio {
                saw { chans: 1, freq: 110.0 },
                onepole { cutoff: 1000.0, chans: 1 },
                mult { val: 0.5 }
            }

            saw >> onepole[0]
            onepole >> mult[0]

            { mult }
        }

        patches {
            voice: v {}
        }

        { v }
    "#;

    let mut block_app = build(block_src, 1);
    let mut kernel_app = build(kernel_src, 1);

    let block_out = render(&mut block_app, 1);
    let kernel_out = render(&mut kernel_app, 1);

    let mut nonzero = false;
    for (i, (a, b)) in block_out[0].iter().zip(kernel_out[0].iter()).enumerate() {
        assert!(
            (a - b).abs() <= 1e-2,
            "kernel chain diverged from block chain at sample {i}: {a} vs {b}"
        );
        nonzero |= a.abs() > 1e-3;
    }
    assert!(nonzero, "test signal was silent");
}

/// Kernel default params substitute into the interior and are overridable at
/// the instantiation site — checked bit-exactly against a block-rate mult.
#[test]
fn kernel_params_apply_at_instantiation() {
    let block_src = r#"
        audio {
            sine { freq: 220.0 },
            mult { val: 0.25 }
        }

        sine >> mult[0]

        { mult }
    "#;

    let kernel_src = r#"
        kernel scaled(amount = 0.5) {
            in audio_in

            audio {
                mult { val: $amount }
            }

            audio_in >> mult[0]

            { mult }
        }

        patches {
            scaled: s { amount: 0.25 }
        }

        audio {
            sine { freq: 220.0 }
        }

        sine >> s.audio_in

        { s }
    "#;

    let mut block_app = build(block_src, 1);
    let mut kernel_app = build(kernel_src, 1);

    let block_out = render(&mut block_app, 1);
    let kernel_out = render(&mut kernel_app, 1);

    // The sine is block-rate in both graphs and multiplication is exact, so
    // the outputs must agree exactly.
    assert_eq!(block_out[0], kernel_out[0]);
    assert!(block_out[0].iter().any(|x| x.abs() > 1e-3));
}

/// `kernel_inst * N` spawning plus port-selection wiring into a mixer — the
/// surrounding block-rate features must treat a kernel like any other node.
#[test]
fn kernel_multi_spawn_and_port_selection_build() {
    let src = r#"
        kernel voice(freq = 110.0) {
            audio {
                saw { chans: 1, freq: $freq },
                mult { val: 0.2 }
            }

            saw >> mult[0]

            { mult }
        }

        patches {
            voice * 3 {}
        }

        audio {
            track_mixer { tracks: 3, chans_per_track: 1 }
        }

        voice(*) >> track_mixer[0..3]

        { track_mixer }
    "#;

    let mut app = build(src, 1);
    let out = render(&mut app, 1);

    assert!(
        out[0].iter().all(|x| x.is_finite()),
        "spawned kernels produced non-finite output"
    );
    assert!(
        out[0].iter().any(|x| x.abs() > 1e-3),
        "spawned kernels were silent"
    );
}

/// A feedback comb (tap + mult + add loop) — impossible at block rate — must
/// build from DSL source and produce bounded, nonzero audio.
#[test]
fn feedback_comb_kernel_runs() {
    let src = r#"
        kernel comb(fb = 0.6) {
            in audio_in

            audio {
                add { val: 0.0 },
                tap { delay_length: 5, chans: 1 },
                mult { val: $fb }
            }

            audio_in >> add[0]
            add >> tap
            tap >> mult[0]
            mult >> add[1]

            { add }
        }

        patches {
            comb: c {}
        }

        audio {
            sine { freq: 330.0 }
        }

        sine >> c.audio_in

        { c }
    "#;

    let mut app = build(src, 1);
    let out = render(&mut app, 1);

    assert!(
        out[0].iter().all(|x| x.is_finite() && x.abs() < 10.0),
        "comb feedback blew up"
    );
    assert!(out[0].iter().any(|x| x.abs() > 1e-3), "comb was silent");
}

/// A kernel instantiated *inside* a patch: the KernelRef must survive macro
/// expansion and lower to a per-sample node with a fully-qualified alias.
#[test]
fn kernel_inside_patch_expands() {
    let src = r#"
        kernel scaled(amount = 0.5) {
            in audio_in

            audio {
                mult { val: $amount }
            }

            audio_in >> mult[0]

            { mult }
        }

        patch wrapper(level = 0.25) {
            in w_in

            audio {
                scaled { amount: $level },
                add { val: 0.0 }
            }

            w_in >> add[0]
            add >> scaled.audio_in

            { scaled }
        }

        patches {
            wrapper: w {}
        }

        audio {
            sine { freq: 220.0 }
        }

        sine >> w.w_in

        { w }
    "#;

    let mut app = build(src, 1);
    let out = render(&mut app, 1);

    assert!(out[0].iter().all(|x| x.is_finite()));
    assert!(
        out[0].iter().any(|x| x.abs() > 1e-3),
        "kernel inside patch was silent"
    );
    // The 0.25 level actually applied.
    assert!(
        out[0].iter().all(|x| x.abs() <= 0.26),
        "kernel param inside patch was not substituted"
    );
}
