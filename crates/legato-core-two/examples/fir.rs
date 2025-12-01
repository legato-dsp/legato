use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{BufferSize, SampleRate, StreamConfig};
use legato_core_two::nodes::ports::{PortBuilder, PortRate};
use legato_core_two::runtime::builder::{AddNode, get_runtime_builder};
use legato_core_two::runtime::context::Config;
use legato_core_two::runtime::graph::{Connection, ConnectionEntry};
use legato_core_two::runtime::out::start_runtime_audio_thread;

fn main() {
    #[cfg(target_os = "linux")]
    let config = Config {
        sample_rate: 48000,
        audio_block_size: 1024,
        channels: 2,
        control_block_size: 1024 / 32,
        control_rate: 48000 / 32,
    };

    #[cfg(target_os = "macos")]
    let config = Config {
        sample_rate: 44_100,
        audio_block_size: 1024,
        channels: 2,
        control_block_size: 1024 / 32,
        control_rate: 44_100 / 32,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let mut runtime_builder = get_runtime_builder(16, config, ports);

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

    let fir = runtime_builder.add_node(AddNode::Fir {
        coeffs: coeffs,
        chans: 2,
    });

    let sampler = runtime_builder.add_node(AddNode::Sampler {
        sampler_name: String::from("amen"),
        chans: 2,
    });

    let (mut runtime, mut backend) = runtime_builder.get_owned();

    let _ = backend.load_sample(
        &String::from("amen"),
        "../samples/amen.wav",
        2,
        config.sample_rate as u32,
    );

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
        channels: config.channels as u16,
        sample_rate: SampleRate(config.sample_rate as u32),
        buffer_size: BufferSize::Fixed(config.audio_block_size as u32),
    };

    start_runtime_audio_thread(&device, &config, runtime).expect("Runtime panic!");
}
