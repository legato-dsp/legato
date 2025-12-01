use std::{array, time::Duration};

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use generic_array::{GenericArray, sequence::GenericSequence};
use legato_core::{engine::{buffer::{Buffer, Frame}, builder::{AddNode, RuntimeBuilder, get_runtime_builder}, graph::{self, Connection, ConnectionEntry}, port::{PortRate, Ports}}, nodes::{audio::{filters::fir::FirStereo, sine::SineStereo}, get_node_test_harness, utils::port_utils::generate_audio_outputs}};
use typenum::{U0, U2, U128, U4096};

fn bench_sine_legato_one(c: &mut Criterion){
    let mut graph = get_node_test_harness::<U4096, U128>(Box::new(SineStereo::new(440.0, 0.0)));

    c.bench_function("Sine node legato one", |b| {
        b.iter(|| {
            let out = graph.next_block(None);
            black_box(out);
        })
    });
}

fn bench_fir(c: &mut Criterion){
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

    let mut graph = get_node_test_harness::<U4096, U128>(Box::new(FirStereo::new(coeffs)));

    c.bench_function("fir node", |b| {
        b.iter(|| {
            let out = graph.next_block(None);
            black_box(out);
        })
    });
}




fn bench_stereo_delay(c: &mut Criterion){
    type BlockSize = U4096;
    type ControlSize = U128;
    type ChannelCount = U2;

    const SAMPLE_RATE: u32 = 44_100;
    const CAPACITY: usize = 16;
    const DECIMATION_FACTOR: f32 = 32.0;
    const CONTROL_RATE: f32 = SAMPLE_RATE as f32 / DECIMATION_FACTOR;

    let mut runtime_builder: RuntimeBuilder<BlockSize, ControlSize, ChannelCount, U0> =
        get_runtime_builder(
            CAPACITY,
            SAMPLE_RATE as f32,
            CONTROL_RATE,
            Ports {
                audio_inputs: None,
                audio_outputs: Some(generate_audio_outputs()),
                control_inputs: None,
                control_outputs: None,
            },
    );

    let a = runtime_builder.add_node(AddNode::DelayWriteStereo { delay_name: 'a'.into(), delay_length: Duration::from_secs_f32(1.0) });

    let b = runtime_builder.add_node(AddNode::DelayReadStereo {delay_name: 'a'.into(), offsets: vec![Duration::from_millis(120), Duration::from_millis(240)]});

    let (mut runtime, _) = runtime_builder.get_owned();

    let _ = runtime.add_edge(Connection {
        source: ConnectionEntry {
            node_key: a,
            port_index: 0,
            port_rate: PortRate::Audio
        },
        sink: ConnectionEntry { node_key: b, port_index: 0, port_rate: PortRate::Audio }
    });

    let _ = runtime.add_edge(Connection {
        source: ConnectionEntry {
            node_key: a,
            port_index: 1,
            port_rate: PortRate::Audio
        },
        sink: ConnectionEntry { node_key: b, port_index: 1, port_rate: PortRate::Audio }
    });

    let _ = runtime.set_sink_key(b);

    c.bench_function("Basic stereo delay", |b| {
        let ai = [Buffer::<BlockSize>::silent(),Buffer::<BlockSize>::silent()];
        let ci = [];
        b.iter(|| {
            let out = runtime.next_block(Some((&ai, &ci)));
            black_box(out);
        });
    });
}



criterion_group!(benches, bench_sine_legato_one, bench_fir, bench_stereo_delay);
criterion_main!(benches);


