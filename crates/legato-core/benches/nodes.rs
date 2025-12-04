use std::time::Duration;

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use legato_core::{
    nodes::{
        audio::{fir::FirFilter, sine::Sine},
        ports::{PortBuilder, PortRate},
    },
    runtime::{
        builder::{AddNode, RuntimeBuilder, get_runtime_builder},
        context::Config,
        graph::{Connection, ConnectionEntry},
    },
    utils::bench_harness::get_node_test_harness,
};

fn bench_stereo_sine(c: &mut Criterion) {
    let mut graph = get_node_test_harness(Box::new(Sine::new(440.0, 2)));

    c.bench_function("Sine node legato two", |b| {
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

    let mut graph = get_node_test_harness(Box::new(FirFilter::new(coeffs, 2)));

    c.bench_function("fir node", |b| {
        b.iter(|| {
            let out = graph.next_block(None);
            black_box(out);
        })
    });
}

fn bench_stereo_delay(c: &mut Criterion) {
    let config = Config {
        audio_block_size: 4096,
        control_block_size: 4096 / 32,
        channels: 2,
        sample_rate: 44_100,
        control_rate: 44_100 / 32,
        initial_graph_capacity: 4
    };

    let mut runtime_builder: RuntimeBuilder = get_runtime_builder(
        config,
        PortBuilder::default().audio_in(2).audio_out(2).build(),
    );

    let a = runtime_builder.add_node(AddNode::DelayWrite {
        chans: 2,
        delay_name: 'a'.into(),
        delay_length: Duration::from_secs_f32(1.0),
    });

    let b = runtime_builder.add_node(AddNode::DelayRead {
        chans: 2,
        delay_name: 'a'.into(),
        delay_length: vec![Duration::from_millis(120), Duration::from_millis(240)],
    });

    let (mut runtime, _) = runtime_builder.get_owned();

    let _ = runtime.add_edge(Connection {
        source: ConnectionEntry {
            node_key: a,
            port_index: 0,
            port_rate: PortRate::Audio,
        },
        sink: ConnectionEntry {
            node_key: b,
            port_index: 0,
            port_rate: PortRate::Audio,
        },
    });

    let _ = runtime.add_edge(Connection {
        source: ConnectionEntry {
            node_key: a,
            port_index: 1,
            port_rate: PortRate::Audio,
        },
        sink: ConnectionEntry {
            node_key: b,
            port_index: 1,
            port_rate: PortRate::Audio,
        },
    });

    let _ = runtime.set_sink_key(b);

    c.bench_function("Basic stereo delay", |b| {
        let ai: &[Box<[f32]>] = &[
            vec![0.0; config.audio_block_size].into(),
            vec![0.0; config.audio_block_size].into(),
        ];
        let ci = &[];
        b.iter(|| {
            let out = runtime.next_block(Some(&(ai, ci)));
            black_box(out);
        });
    });
}

criterion_group!(benches, bench_stereo_sine, bench_fir, bench_stereo_delay);
criterion_main!(benches);
