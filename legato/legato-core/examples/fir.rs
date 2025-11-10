use arc_swap::ArcSwapOption;
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{BufferSize, SampleRate, StreamConfig};
use legato_core::{
    backend::out::start_audio_thread,
    engine::{
        builder::AddNode,
        graph::{Connection, ConnectionEntry},
        port::{PortRate, Ports},
        runtime::{build_runtime, Runtime},
    },
    nodes::utils::port_utils::generate_audio_outputs,
};
use legato_core::{engine::builder::RuntimeBuilder, nodes::audio::sampler::AudioSampleBackend};
use std::{sync::Arc};

use typenum::{Unsigned, U0, U2, U2048, U64};

fn main() {
    type BlockSize = U2048;
    type ControlSize = U64;
    type ChannelCount = U2;

    const SAMPLE_RATE: u32 = 44_100;
    const CAPACITY: usize = 16;
    const DECIMATION_FACTOR: f32 = 32.0;
    const CONTROL_RATE: f32 = SAMPLE_RATE as f32 / DECIMATION_FACTOR;

    let mut runtime: Runtime<BlockSize, ControlSize, ChannelCount, U0> = build_runtime(
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

    let data = Arc::new(ArcSwapOption::new(None));
    let backend = AudioSampleBackend::new(data.clone());

    // Would suggest using Python + numpy + scipy. In the future there should be a tool for this here.
    // Here is a cool tool, the blog post is great as well: https://fiiir.com/
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

    let (fir, _) = runtime
        .add_node_api(AddNode::FirStereo { kernel: coeffs })
        .expect("Could not add FIR");

    let (sampler, _) = runtime
        .add_node_api(AddNode::SamplerStereo {
            props: data.clone(),
        })
        .expect("Could not add sampler");

    backend
        .load_file("./samples/amen.wav")
        .expect("Could not load amen sample!");

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: sampler,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: fir,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: sampler,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: fir,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime.set_sink_key(fir).expect("Bad sink key!");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    let device = host.default_output_device().unwrap();

    println!("{:?}", device.default_output_config());

    let config = StreamConfig {
        channels: U2::U16,
        sample_rate: SampleRate(SAMPLE_RATE),
        buffer_size: BufferSize::Fixed(BlockSize::U32),
    };

    start_audio_thread(&device, &config, runtime).expect("Runtime panic!");
}
