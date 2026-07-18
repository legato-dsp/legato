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

const SR: f32 = 48_000.0;

/// Hann-windowed magnitude spectrum via a real FFT. Bin `i` holds the magnitude
/// at frequency `i * SR / samples.len()` Hz (so bin spacing is `SR / len`). The
/// Hann window keeps a strong harmonic from leaking into its neighbours.
fn spectrum(samples: &[f32]) -> Vec<f32> {
    use rustfft::{FftPlanner, num_complex::Complex};
    let n = samples.len();
    let mut buf: Vec<Complex<f32>> = samples
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            let hann = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
            Complex::new(x * hann, 0.0)
        })
        .collect();
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);
    buf[..n / 2].iter().map(|c| c.norm()).collect()
}

/// The spectrum bin nearest `hz` for a window of length `n`.
fn bin(hz: f32, n: usize) -> usize {
    (hz * n as f32 / SR).round() as usize
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

/// Dogfood check: the plate reverb authored in the kernel DSL
/// ([`legato::kernel::PLATE_KERNEL`], ~66 primitive nodes with three feedback
/// cycles) must sound like the handwritten Rust `plate480` node.
///
/// Sample-exact comparison is off the table by design: the DSL version's
/// implicit z⁻¹ placement, ±1-sample allpass skews, and fractional (vs
/// rounded) delay lengths shift individual samples. What must survive all of
/// that is the *energy envelope* — same input, same decay, same loudness —
/// so we drive both with a broadband saw and compare per-window RMS.
#[test]
fn plate_kernel_matches_rust_plate_envelope() {
    let rust_src = r#"
        audio {
            saw { chans: 1, freq: 55.0 },
            mono_fan_out { chans: 2 },
            plate480: verb { predelay: 10.0, decay: 0.5, damping: 0.3, bandwidth: 0.9995, mix: 1.0 }
        }

        saw >> mono_fan_out
        mono_fan_out >> verb[0..2]

        { verb }
    "#;

    let kernel_src = format!(
        "{}\n{}",
        legato::kernel::EXAMPLE_PLATE_KERNEL_PATCH,
        r#"
        patches {
            plate: verb { predelay: 10.0, decay: 0.5, damping: 0.3, bandwidth_a: 0.0005, wet: 1.0, dry: 0.0 }
        }

        audio {
            saw { chans: 1, freq: 55.0 },
            mono_fan_out { chans: 2 },
        }

        saw >> mono_fan_out
        mono_fan_out >> verb[0..2]

        { verb }
    "#
    );

    let mut rust_app = build(rust_src, 2);
    let mut kernel_app = build(&kernel_src, 2);

    // ~0.7s: enough for the tanks to fill and reach comparable steady energy.
    const WINDOW: usize = 2048;
    const WINDOWS: usize = 16;

    let render_rms = |app: &mut LegatoApp| -> Vec<f32> {
        let mut rms = Vec::with_capacity(WINDOWS);
        for _ in 0..WINDOWS {
            let mut energy = 0.0f64;
            for _ in 0..WINDOW / BLOCK {
                let view = app.next_block(None);
                for c in 0..2 {
                    for &x in view.channels[c] {
                        assert!(x.is_finite(), "plate output not finite");
                        energy += (x as f64) * (x as f64);
                    }
                }
            }
            rms.push((energy / (WINDOW * 2) as f64).sqrt() as f32);
        }
        rms
    };

    let rust_rms = render_rms(&mut rust_app);
    let kernel_rms = render_rms(&mut kernel_app);

    eprintln!("rust plate RMS envelope:   {rust_rms:.4?}");
    eprintln!("kernel plate RMS envelope: {kernel_rms:.4?}");

    // Skip the build-up windows; compare the settled envelope.
    for (w, (a, b)) in rust_rms.iter().zip(kernel_rms.iter()).enumerate().skip(4) {
        assert!(
            *a > 1e-4 && *b > 1e-4,
            "window {w}: a plate went silent (rust {a}, kernel {b})"
        );
        let rel = (a - b).abs() / a.max(*b);
        // Measured headroom: the two settle within ~0.2% of each other; 5%
        // leaves room for other SIMD lane widths without hiding regressions.
        assert!(
            rel < 0.05,
            "window {w}: RMS envelopes diverged: rust {a} vs kernel {b} (rel {rel:.3})"
        );
    }

    // Aggregate energy over the settled region must agree more tightly than
    // any individual window.
    let total = |rms: &[f32]| rms[4..].iter().map(|x| x * x).sum::<f32>().sqrt();
    let (ta, tb) = (total(&rust_rms), total(&kernel_rms));
    let rel = (ta - tb).abs() / ta.max(tb);
    assert!(
        rel < 0.02,
        "settled energy diverged: rust {ta} vs kernel {tb} (rel {rel:.3})"
    );
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

/// Basic Karplus–Strong ([`legato::kernel::KARPLUS_KERNEL`]): an internal
/// `noise` burst gated by `gate`, tuned by `freq` through the in-kernel `1/freq`
/// reciprocal. Held at a constant 220 Hz the ring must (1) be a real harmonic
/// tone — the loudest FFT bin is the fundamental, with a 2nd harmonic and a
/// quiet inter-harmonic null — and (2) persist, decaying but still ringing a
/// third of a second in (a click would be long gone).
#[test]
fn karplus_plucks_in_tune() {
    // gate and freq are both DC, synthesized as `sine { freq: 0, phase: 0.25 }`
    // == 1.0; the gate's rising edge at t0 fires one noise burst.
    let src = format!(
        "{}\n{}",
        legato::kernel::EXAMPLE_KARPLUS_KERNEL_PATCH,
        r#"
        patches {
            karplus: string { decay: 0.99, damping: 0.5, pluck: 0.995 }
        }

        audio {
            sine: g  { freq: 0.0, phase: 0.25 },
            sine: f0 { freq: 0.0, phase: 0.25 },
            mult: hz { val: 220.0 },
        }

        f0 >> hz[0]
        g  >> string[0]
        hz >> string[1]

        { string }
    "#,
    );

    let mut app = build(&src, 1);
    let mut sig: Vec<f32> = Vec::with_capacity(30_000);
    for _ in 0..(30_000 / BLOCK) {
        sig.extend_from_slice(app.next_block(None).channels[0]);
    }
    assert!(
        sig.iter().all(|x| x.is_finite() && x.abs() < 8.0),
        "karplus string blew up"
    );

    // (1) pitch + harmonics, in a window past the noisy attack.
    const N: usize = 8_192; // bin spacing SR/N ≈ 5.9 Hz
    let spec = spectrum(&sig[4_000..4_000 + N]);

    // The loudest bin (ignoring DC) is the fundamental — ~220 Hz. The string
    // tunes a hair flat: the loop's z⁻¹ + filter phase add ~1.5 samples to the
    // delay, so allow a bin of slack.
    let peak = (1..spec.len())
        .max_by(|&a, &b| spec[a].partial_cmp(&spec[b]).unwrap())
        .unwrap();
    let peak_hz = peak as f32 * SR / N as f32;
    assert!(
        (peak_hz - 220.0).abs() < 6.0,
        "loudest bin at {peak_hz:.1} Hz, expected ~220"
    );

    // A real harmonic comb: energy at f0 and 2·f0, but not at the inter-harmonic
    // null 1.5·f0 (330 Hz).
    let mag = |hz: f32| spec[bin(hz, N)];
    assert!(mag(220.0) > 4.0 * mag(330.0), "fundamental not dominant");
    assert!(mag(440.0) > 2.0 * mag(330.0), "2nd harmonic missing");

    // (2) persistence: the 220 Hz bin decays but is still ringing well after the
    // attack.
    let f0_mag = |start: usize| spectrum(&sig[start..start + N])[bin(220.0, N)];
    let early = f0_mag(4_000); //  ~0.08 s
    let late = f0_mag(20_000); //  ~0.42 s
    assert!(late < early, "220 Hz bin did not decay");
    assert!(
        late > 0.2 * early,
        "220 Hz bin died out — not a sustained pluck"
    );
}

/// End-to-end regression for the poly.rs "click + echoes" bug. A strided
/// multi-source (distinct value per port) fanned into `voice(*).freq` must land
/// one port per voice, so two spawned strings tune to *different*, non-octave
/// pitches (220 and 330). `pass` is a 4-channel `tap` (unity for DC) standing in
/// for `poly_voice`'s [gate, freq, gate, freq] layout. Before the spawn-pass fix
/// every voice received both frequencies summed, so 220 and 330 both collapsed.
#[test]
fn karplus_polyphony_routes_freq_per_voice() {
    let src = format!(
        "{}\n{}",
        legato::kernel::EXAMPLE_KARPLUS_KERNEL_PATCH,
        r#"
        patches { karplus: voice * 2 { decay: 0.99, damping: 0.5, pluck: 0.995 } }
        audio {
            sine: c { freq: 0.0, phase: 0.25 },
            mult: g220 { val: 220.0 },
            mult: g330 { val: 330.0 },
            tap: pass { chans: 4, delay_length: 1.0, capacity: 8000 },
            track_mixer: mix { tracks: 2, chans_per_track: 1, gain: [0.5, 0.5] },
        }

        c >> g220[0]
        c >> g330[0]

        c    >> pass[0]
        g220 >> pass[1]
        c    >> pass[2]
        g330 >> pass[3]

        pass[0:4:2] >> voice(*).gate
        pass[1:4:2] >> voice(*).freq
        voice(*) >> mix[0..2]
        { mix }
    "#,
    );

    let mut app = build(&src, 1);
    let mut sig: Vec<f32> = Vec::with_capacity(16_384);
    for _ in 0..(16_384 / BLOCK) {
        sig.extend_from_slice(app.next_block(None).channels[0]);
    }

    const N: usize = 8_192;
    let spec = spectrum(&sig[4_000..4_000 + N]);
    let mag = |hz: f32| spec[bin(hz, N)];
    // Both fundamentals present, each beating the 500 Hz null. 330 is not a
    // harmonic of 220, so its presence proves voice 1 was tuned to 330 — not
    // that both voices landed on 220.
    assert!(
        mag(220.0) > 4.0 * mag(500.0),
        "voice 0 lost 220 Hz (freq mis-routed)"
    );
    assert!(
        mag(330.0) > 4.0 * mag(500.0),
        "voice 1 lost 330 Hz (freq mis-routed)"
    );
}
