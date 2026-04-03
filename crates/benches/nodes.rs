use criterion::{Criterion, black_box, criterion_group, criterion_main};
use legato::{
    builder::LegatoBuilder,
    config::Config,
    harness::get_node_test_harness_stereo_4096,
    nodes::audio::{
        fir::FirFilter,
        sine::Sine,
        svf::{FilterType, Svf},
    },
    ports::PortBuilder,
    runtime::MAX_INPUTS,
};

fn bench_stereo_sine(c: &mut Criterion) {
    let mut graph = get_node_test_harness_stereo_4096(Box::new(Sine::new(440.0, 2)));

    c.bench_function("Sine", |b| {
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
        initial_graph_capacity: 4,
    };

    let ports = PortBuilder::default().audio_in(2).audio_out(2).build();

    let (mut app, _) = LegatoBuilder::new(config, ports).build_dsl(&String::from(
        r#"
            { delay_write }

            audio {
                delay_write { delay_name: "a", chans: 2, delay_length: 1000 },
                delay_read { delay_name: "a", chans: 2, delay_length: [120, 240] }
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

fn bench_oversampler(c: &mut Criterion) {
    let config = Config {
        block_size: 256,
        channels: 2,
        sample_rate: 44_100,
        initial_graph_capacity: 4,
    };

    let ports = PortBuilder::default().audio_in(2).audio_out(2).build();

    let (mut app, _) = LegatoBuilder::new(config, ports).build_dsl(&String::from(
        r#"
            audio {
                sweep { range: [40.0, 48000.0], duration: 5000.0, chans: 2 } | oversample2X()
            }
        
            { sweep }
        "#,
    ));

    c.bench_function("Basic oversampler", |b| {
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

fn bench_kitchen_sink(c: &mut Criterion) {
    let config = Config {
        block_size: 4096,
        channels: 2,
        sample_rate: 44_100,
        initial_graph_capacity: 4,
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
            delay_read: dr1 { delay_name: "d_one", chans: 2, delay_length: [ 938, 731 ] },
            delay_read: dr2 { delay_name: "d_one", chans: 2, delay_length: [ 459, 643 ] },

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

    c.bench_function("Basic oversampler", |b| {
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

criterion_group!(
    benches,
    bench_stereo_sine,
    bench_fir,
    bench_stereo_delay,
    bench_svf,
    bench_oversampler,
    bench_kitchen_sink
);
criterion_main!(benches);
