use criterion::{Criterion, black_box, criterion_group, criterion_main};

// The emitter's checked-in output lives under `tests/` so it is verified
// through the public API and stays out of the published library. Pulled in
// here by path so the benchmark can measure the code the generator actually
// produces, not just the hand-written shape it targets.
#[path = "../tests/generated/fm3.rs"]
mod generated_fm3;

#[path = "../tests/generated/modtap4.rs"]
mod generated_modtap4;

#[path = "../tests/generated/plate.rs"]
mod generated_plate;
use legato::{
    builder::LegatoBuilder,
    config::Config,
    harness::get_node_test_harness_stereo_4096,
    kernel::EXAMPLE_PLATE_KERNEL_PATCH,
    kernel_codegen::{Fm3, fm3_interpreter},
    nodes::audio::{
        fir::FirFilter,
        saw::Saw,
        sine::Sine,
        svf::{FilterType, Svf},
    },
    persample::PerSampleNode,
    ports::PortBuilder,
    runtime::MAX_INPUTS,
};

fn bench_stereo_sine(c: &mut Criterion) {
    let mut graph = get_node_test_harness_stereo_4096(Box::new(Sine::new(440.0, 48_000.0)));

    c.bench_function("Sine", |b| {
        b.iter(|| {
            let out = graph.next_block(None);
            black_box(out);
        })
    });
}

fn bench_stereo_saw(c: &mut Criterion) {
    let mut graph = get_node_test_harness_stereo_4096(Box::new(Saw::new(440.0, 2, 48_000.0)));

    c.bench_function("Saw", |b| {
        b.iter(|| {
            let out = graph.next_block(None);
            black_box(out);
        })
    });
}

fn bench_fir(c: &mut Criterion) {
    let coeffs: Vec<f32> = vec![
        0.0,
        -0.000_005_844_052,
        -0.000_023_820_136,
        -0.000_054_014_8,
        -0.000_095_582_48,
        -0.000_146_488_66,
        -0.000_203_195_5,
        -0.000_260_312_23,
        -0.000_310_242_12,
        -0.000_342_864_78,
        -0.000_345_297_97,
        -0.000_301_783_15,
        -0.000_193_736_5,
        0.0,
        0.000_302_683_5,
        0.000_738_961_45,
        0.001_333_935_4,
        0.002_112_021_4,
        0.003_095_649_6,
        0.004_303_859_5,
        0.005_750_863_3,
        0.007_444_662,
        0.009_385_793,
        0.011_566_305,
        0.013_969_036,
        0.016_567_26,
        0.019_324_76,
        0.022_196_36,
        0.025_128_9,
        0.028_062_675,
        0.030_933_246,
        0.033_673_592,
        0.036_216_475,
        0.038_496_945,
        0.040_454_84,
        0.042_037_163,
        0.043_200_247,
        0.043_911_517,
        0.044_150_87,
        0.043_911_517,
        0.043_200_247,
        0.042_037_163,
        0.040_454_84,
        0.038_496_945,
        0.036_216_475,
        0.033_673_592,
        0.030_933_246,
        0.028_062_675,
        0.025_128_9,
        0.022_196_36,
        0.019_324_76,
        0.016_567_26,
        0.013_969_036,
        0.011_566_305,
        0.009_385_793,
        0.007_444_662,
        0.005_750_863_3,
        0.004_303_859_5,
        0.003_095_649_6,
        0.002_112_021_4,
        0.001_333_935_4,
        0.000_738_961_45,
        0.000_302_683_5,
        0.0,
        -0.000_193_736_5,
        -0.000_301_783_15,
        -0.000_345_297_97,
        -0.000_342_864_78,
        -0.000_310_242_12,
        -0.000_260_312_23,
        -0.000_203_195_5,
        -0.000_146_488_66,
        -0.000_095_582_48,
        -0.000_054_014_8,
        -0.000_023_820_136,
        -0.000_005_844_052,
        0.0,
    ];

    let mut graph = get_node_test_harness_stereo_4096(Box::new(FirFilter::new(coeffs, 2)));

    let ai: &[Box<[f32]>] = &[vec![1.0; 4096].into(), vec![1.0; 4096].into()];

    let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];

    for (i, x) in ai.iter().enumerate() {
        inputs[i] = Some(&x)
    }

    c.bench_function("fir", |b| {
        b.iter(|| {
            let out = graph.next_block(Some(black_box(&inputs)));
            black_box(out);
        })
    });
}

fn bench_stereo_delay(c: &mut Criterion) {
    let config = Config {
        block_size: 4096,
        channels: 2,
        sample_rate: 44_100,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_in(2).audio_out(2).build();

    let (mut app, _) = LegatoBuilder::new(config, ports).build_dsl(&String::from(
        r#"
            { delay_write }

            audio {
                delay_write { delay_name: "a", chans: 2, delay_length: 1000 },
                delay_read { delay_name: "a", chans: 2, delay_length: 120 }
            }

            { delay_read }
        "#,
    ));

    c.bench_function("Basic stereo delay", |b| {
        let ai: &[Box<[f32]>] = &[
            vec![0.0; config.block_size].into(),
            vec![0.0; config.block_size].into(),
        ];

        let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];

        for (i, x) in ai.iter().enumerate() {
            inputs[i] = Some(&x)
        }

        b.iter(|| {
            let out = app.next_block(Some(black_box(&inputs)));
            black_box(out);
        });
    });
}

fn bench_delay_quality(c: &mut Criterion) {
    let config = Config {
        block_size: 4096,
        channels: 2,
        sample_rate: 44_100,
        rt_capacity: 0,
    };

    let build = |quality: &str| {
        let ports = PortBuilder::default().audio_in(2).audio_out(2).build();
        let graph = format!(
            r#"
                audio {{
                    delay_write {{ delay_name: "a", chans: 2, delay_length: 1000 }},
                    delay_read {{ delay_name: "a", chans: 2, delay_length: 120, quality: "{quality}" }}
                }}

                {{ delay_read }}
            "#
        );
        let (app, _) = LegatoBuilder::new(config, ports).build_dsl(&graph);
        app
    };

    let ai: &[Box<[f32]>] = &[
        vec![0.0; config.block_size].into(),
        vec![0.0; config.block_size].into(),
    ];

    let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];
    for (i, x) in ai.iter().enumerate() {
        inputs[i] = Some(&x)
    }

    let mut group = c.benchmark_group("Delay interpolation quality");

    let mut linear = build("linear");
    group.bench_function("linear", |b| {
        b.iter(|| {
            let out = linear.next_block(Some(black_box(&inputs)));
            black_box(out);
        });
    });

    let mut cubic = build("cubic");
    group.bench_function("cubic", |b| {
        b.iter(|| {
            let out = cubic.next_block(Some(black_box(&inputs)));
            black_box(out);
        });
    });

    group.finish();
}

// Removing pipe idea now for oversampling

// fn bench_oversampler(c: &mut Criterion) {
//     let config = Config {
//         block_size: 4096,
//         channels: 2,
//         sample_rate: 44_100,
//         rt_capacity: 0,
//     };

//     let ports = PortBuilder::default().audio_in(2).audio_out(2).build();

//     let (mut app, _) = LegatoBuilder::new(config, ports).build_dsl(&String::from(
//         r#"
//             audio {
//                 sweep { range: [40.0, 48000.0], duration: 5000.0, chans: 2 } | oversample2X()
//             }

//             { sweep }
//         "#,
//     ));

//     c.bench_function("Basic oversampler", |b| {
//         let ai: &[Box<[f32]>] = &[
//             vec![0.0; config.block_size].into(),
//             vec![0.0; config.block_size].into(),
//         ];

//         let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];

//         for (i, x) in ai.iter().enumerate() {
//             inputs[i] = Some(&x)
//         }

//         b.iter(|| {
//             let out = app.next_block(black_box(Some(&inputs)));
//             black_box(out);
//         });
//     });
// }

fn bench_kitchen_sink(c: &mut Criterion) {
    let config = Config {
        block_size: 4096,
        channels: 2,
        sample_rate: 44_100,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_in(2).audio_out(2).build();

    let (mut app, _) = LegatoBuilder::new(config, ports).build_dsl(&String::from(
       r#"
        patch voice(
            freq_m = 440.0,
            freq_c = 660.0,
            attack = 200.0,
            decay = 200.0,
            sustain = 0.3,
            release = 200.0
        ) {
            audio {
                sine: mod { freq: $freq_m },
                sine: carrier { freq: $freq_c },
                mult: freq_mult,
                mult: fm_gain { val: 1000.0 },
                add: fm_add,
            }

            control {
                signal: ratio { name: "ratio", min: 1.0, max: 100.0, default: 1.5 },
                signal: freq { name: "freq", min: 10.0, max: 10000.0, default: $freq_c }
            }

            freq >> freq_mult[0]
            ratio >> freq_mult[1]

            freq_mult >> mod.freq

            mod >> fm_gain[0]

            freq >> fm_add[0]
            fm_gain >> fm_add[1]

            fm_add >> carrier.freq

            { carrier }
        }

        patches {
            voice * 5 {}
        }

        audio {
            track_mixer: osc_mixer { tracks: 5, chans_per_track: 1, gain: [0.1, 0.1, 0.1, 0.1, 0.1] },
            mono_fan_out { chans: 2 },

            delay_write: dw1 { delay_name: "d_one", delay_length: 2000.0, chans: 2 },
            delay_read: dr1 { delay_name: "d_one", chans: 2, delay_length: 938 },
            delay_read: dr2 { delay_name: "d_one", chans: 2, delay_length: 459 },

            track_mixer: master { tracks: 3, chans_per_track: 2, gain: [0.4, 0.5, 0.5] },
            
            track_mixer: feedback { tracks: 2, chans_per_track: 2, gain: [0.5, 0.5] }
        }

        voice(*) >> osc_mixer[0..5]

        osc_mixer >> mono_fan_out

        mono_fan_out >> master[0..2]
        mono_fan_out >> dw1[0..2]

        dr1[0..2] >> master[2..4]
        dr2[0..2] >> master[4..6]

        // feedback    
        dr1 >> feedback[0..2]
        dr2 >> feedback[2..4]

        feedback >> dw1

        { master }
    "#,
    ));

    c.bench_function("Kitchen Sink", |b| {
        let ai: &[Box<[f32]>] = &[
            vec![0.0; config.block_size].into(),
            vec![0.0; config.block_size].into(),
        ];

        let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];

        for (i, x) in ai.iter().enumerate() {
            inputs[i] = Some(&x)
        }

        b.iter(|| {
            let out = app.next_block(black_box(Some(&inputs)));
            black_box(out);
        });
    });
}

/// The same plate reverb two ways: the handwritten Rust `PerSampleNode`
/// (`plate480`) versus the ~66-node kernel DSL declaration (`PLATE_KERNEL`).
/// This is the "when should I graduate to a custom node?" number — the DSL
/// kernel pays per-node dispatch + wiring gathers every sample, the Rust node
/// is one tick with everything inlined.
fn bench_plate_rust_vs_kernel(c: &mut Criterion) {
    let config = Config {
        block_size: 4096,
        channels: 2,
        sample_rate: 48_000,
        rt_capacity: 0,
    };

    let build = |graph: &str| {
        let ports = PortBuilder::default().audio_out(2).build();
        let (app, _) = LegatoBuilder::new(config, ports).build_dsl(graph);
        app
    };

    let rust_graph = r#"
        audio {
            saw { chans: 1, freq: 55.0 },
            mono_fan_out { chans: 2 },
            plate480: verb { predelay: 10.0, decay: 0.5, damping: 0.3, bandwidth: 0.9995, mix: 1.0 }
        }

        saw >> mono_fan_out
        mono_fan_out >> verb[0..2]

        { verb }
    "#;

    let kernel_graph = format!(
        "{}\n{}",
        EXAMPLE_PLATE_KERNEL_PATCH,
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

    let mut group = c.benchmark_group("Plate reverb");

    let mut rust_app = build(rust_graph);
    group.bench_function("rust node (plate480)", |b| {
        b.iter(|| {
            let out = rust_app.next_block(None);
            black_box(out);
        })
    });

    let mut kernel_app = build(&kernel_graph);
    group.bench_function("kernel DSL (PLATE_KERNEL)", |b| {
        b.iter(|| {
            let out = kernel_app.next_block(None);
            black_box(out);
        })
    });

    group.finish();
}

fn bench_svf(c: &mut Criterion) {
    let mut graph = get_node_test_harness_stereo_4096(Box::new(Svf::new(
        48_000.0,
        FilterType::LowPass,
        5400.0,
        0.8,
        0.6,
        2,
    )));

    let ai: &[Box<[f32]>] = &[vec![0.0; 4096].into(), vec![0.0; 4096].into()];

    let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];

    for (i, x) in ai.iter().enumerate() {
        inputs[i] = Some(&x)
    }

    c.bench_function("SVF", |b| {
        b.iter(|| {
            let out = graph.next_block(black_box(Some(&inputs)));
            black_box(out);
        })
    });
}

/// Measures the ceiling for the kernel codegen backend: the same 10-node FM
/// voice as a hand-written straight-line struct ([`Fm3`]) versus the
/// interpreted [`KernelGraph`] it is verified bit-identical to.
///
/// Driven at [`PerSampleNode::tick`] rather than through the block graph, so
/// the number isolates what codegen actually removes — enum dispatch, the
/// `port_sources`/`src_pool` gather, the `Option` scratch frame, and the value
/// table — with no block adapter or fan-in gains in between.
///
/// This is the number that decides whether emitting `tick` calls is enough, or
/// whether inlining the primitives' DSP math is worth its duplication cost.
fn bench_fm3_codegen_vs_interpreter(c: &mut Criterion) {
    const BLOCK: usize = 4096;
    const SR: u32 = 48_000;

    let mut group = c.benchmark_group("FM3 kernel (per-sample tick)");
    group.throughput(criterion::Throughput::Elements(BLOCK as u64));

    let mut interp = fm3_interpreter(SR);
    group.bench_function("interpreted (KernelGraph)", |b| {
        b.iter(|| {
            let mut out = [0.0f32];
            for _ in 0..BLOCK {
                interp.tick(black_box(&[None]), &mut out);
                black_box(out[0]);
            }
        })
    });

    let mut hand = Fm3::new(SR as f32);
    group.bench_function("hand-written (target shape)", |b| {
        b.iter(|| {
            let mut out = [0.0f32];
            for _ in 0..BLOCK {
                hand.tick(black_box(&[None]), &mut out);
                black_box(out[0]);
            }
        })
    });

    // The emitter's actual output. Benched next to the hand-written form so any
    // cost the generator adds over the target shape — the `0.0` accumulator
    // primes, the `o[..n]` reslicing — shows up as a gap rather than hiding.
    let config = Config {
        block_size: 64,
        channels: 1,
        sample_rate: SR as usize,
        rt_capacity: 0,
    };
    let mut resource_builder = legato::resources::ResourceBuilder::default();
    let mut external = std::collections::HashMap::new();
    let mut delays = std::collections::HashMap::new();
    let mut generated = {
        let mut view = legato::builder::ResourceBuilderView {
            config: &config,
            resource_builder: &mut resource_builder,
            external_buffer_keys: &mut external,
            delay_keys: &mut delays,
        };
        generated_fm3::Fm3::new(&mut view).expect("generated fm3 should build")
    };
    group.bench_function("generated (emitter output)", |b| {
        b.iter(|| {
            let mut out = [0.0f32];
            for _ in 0..BLOCK {
                generated.tick(black_box(&[None]), &mut out);
                black_box(out[0]);
            }
        })
    });

    group.finish();
}

/// The delay-heavy counterpart to the FM3 measurement.
///
/// `modtap4` is 25 nodes with four modulated `tap` delay lines, 4-channel
/// `mult` nodes and four feedback loops — a very different work mix from FM3's
/// oscillators, and much closer in shape to the plate. Worth measuring
/// separately: codegen removes a fixed per-node overhead, so the speedup
/// depends on how much real DSP work each node does, and a cubic-interpolated
/// delay read does considerably more than a sine.
fn bench_modtap_codegen_vs_interpreter(c: &mut Criterion) {
    use legato::{
        builder::ResourceBuilderView,
        dsl::{
            ir::{Object, Value},
            lower::ast_to_graph,
            parse::legato_parser,
        },
        kernel::{EXAMPLE_MODTAP_KERNEL_PATCH, lower_kernel},
        resources::ResourceBuilder,
    };

    const BLOCK: usize = 4096;
    const SR: u32 = 48_000;

    let mut params = Object::new();
    params.insert("depth".into(), Value::F32(12.0));
    params.insert("rate".into(), Value::F32(0.05));
    params.insert("feedback".into(), Value::F32(0.6));

    let program = format!("{EXAMPLE_MODTAP_KERNEL_PATCH} audio {{ sine }} {{ sine }}");
    let def = ast_to_graph(legato_parser(&program).expect("should parse"))
        .macro_registry
        .get("modtap4")
        .expect("modtap4 in registry")
        .clone();

    let config = Config {
        block_size: 64,
        channels: 1,
        sample_rate: SR as usize,
        rt_capacity: 0,
    };

    let mut group = c.benchmark_group("modtap4 kernel (per-sample tick)");
    group.throughput(criterion::Throughput::Elements(BLOCK as u64));

    let mut rb1 = ResourceBuilder::default();
    let (mut e1, mut d1) = (
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
    );
    let mut interp = {
        let mut view = ResourceBuilderView {
            config: &config,
            resource_builder: &mut rb1,
            external_buffer_keys: &mut e1,
            delay_keys: &mut d1,
        };
        lower_kernel(&def, &params, "modtap4", &mut view).expect("should lower")
    };
    group.bench_function("interpreted (KernelGraph)", |b| {
        b.iter(|| {
            let mut out = [0.0f32; 2];
            for _ in 0..BLOCK {
                interp.tick(black_box(&[Some(0.01)]), &mut out);
                black_box(out);
            }
        })
    });

    let mut rb2 = ResourceBuilder::default();
    let (mut e2, mut d2) = (
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
    );
    let mut generated = {
        let mut view = ResourceBuilderView {
            config: &config,
            resource_builder: &mut rb2,
            external_buffer_keys: &mut e2,
            delay_keys: &mut d2,
        };
        generated_modtap4::Modtap4::new(&mut view).expect("should build")
    };
    group.bench_function("generated (emitter output)", |b| {
        b.iter(|| {
            let mut out = [0.0f32; 2];
            for _ in 0..BLOCK {
                generated.tick(black_box(&[Some(0.01)]), &mut out);
                black_box(out);
            }
        })
    });

    group.finish();
}

/// The decision-grade measurement: a 64-node plate reverb as an interpreted
/// kernel, as generated code, and as the hand-written Rust `plate480` node.
///
/// The first two are measured at `tick` level; `plate480` is included as the
/// standing reference for what "graduating to Rust" buys, taken from the
/// existing `Plate reverb` group. This is the shape of kernel people actually
/// ship, so it is the honest input to a keep-or-scrap call.
fn bench_plate_codegen_vs_interpreter(c: &mut Criterion) {
    use legato::{
        builder::ResourceBuilderView,
        dsl::{
            ir::{Object, Value},
            lower::ast_to_graph,
            parse::legato_parser,
        },
        kernel::{EXAMPLE_PLATE_KERNEL_PATCH, lower_kernel},
        resources::ResourceBuilder,
    };

    const BLOCK: usize = 4096;
    const SR: u32 = 48_000;

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

    let program = format!("{EXAMPLE_PLATE_KERNEL_PATCH} audio {{ sine }} {{ sine }}");
    let def = ast_to_graph(legato_parser(&program).expect("should parse"))
        .macro_registry
        .get("plate")
        .expect("plate in registry")
        .clone();

    let config = Config {
        block_size: 64,
        channels: 2,
        sample_rate: SR as usize,
        rt_capacity: 0,
    };

    let mut group = c.benchmark_group("plate kernel (per-sample tick)");
    group.throughput(criterion::Throughput::Elements(BLOCK as u64));

    let mut rb1 = ResourceBuilder::default();
    let (mut e1, mut d1) = (
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
    );
    let mut interp = {
        let mut view = ResourceBuilderView {
            config: &config,
            resource_builder: &mut rb1,
            external_buffer_keys: &mut e1,
            delay_keys: &mut d1,
        };
        lower_kernel(&def, &params, "plate", &mut view).expect("should lower")
    };
    group.bench_function("interpreted (KernelGraph)", |b| {
        b.iter(|| {
            let mut out = [0.0f32; 2];
            for _ in 0..BLOCK {
                interp.tick(black_box(&[Some(0.01), Some(0.01)]), &mut out);
                black_box(out);
            }
        })
    });

    let mut rb2 = ResourceBuilder::default();
    let (mut e2, mut d2) = (
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
    );
    let mut generated = {
        let mut view = ResourceBuilderView {
            config: &config,
            resource_builder: &mut rb2,
            external_buffer_keys: &mut e2,
            delay_keys: &mut d2,
        };
        generated_plate::Plate::new(&mut view).expect("should build")
    };
    group.bench_function("generated (emitter output)", |b| {
        b.iter(|| {
            let mut out = [0.0f32; 2];
            for _ in 0..BLOCK {
                generated.tick(black_box(&[Some(0.01), Some(0.01)]), &mut out);
                black_box(out);
            }
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_fm3_codegen_vs_interpreter,
    bench_plate_codegen_vs_interpreter,
    bench_modtap_codegen_vs_interpreter,
    bench_stereo_sine,
    bench_stereo_saw,
    bench_fir,
    bench_stereo_delay,
    bench_delay_quality,
    bench_svf,
    bench_kitchen_sink,
    bench_plate_rust_vs_kernel
);
criterion_main!(benches);
