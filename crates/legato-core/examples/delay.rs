use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{BufferSize, SampleRate, StreamConfig};
use legato_core::engine::builder::{RuntimeBuilder, get_runtime_builder};
use legato_core::{
    engine::{
        builder::AddNode,
        graph::{Connection, ConnectionEntry},
        port::{PortRate, Ports},
    },
    nodes::utils::port_utils::generate_audio_outputs,
    out::start_runtime_audio_thread,
};
use std::time::Duration;
use typenum::{U0, U2, U128, U4096, Unsigned};

fn main() {
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

    let sampler = runtime_builder.add_node(AddNode::SamplerStereo {
        sampler_name: String::from("amen"),
    });

    let delay_write = runtime_builder.add_node(AddNode::DelayWriteStereo {
        delay_name: String::from("amen"),
        delay_length: Duration::from_secs_f32(3.0),
    });

    let delay_read = runtime_builder.add_node(AddNode::DelayReadStereo {
        delay_name: String::from("amen"),
        offsets: vec![Duration::from_millis(12), Duration::from_millis(32)],
    });

    let mixer = runtime_builder.add_node(AddNode::TwoTrackStereoMixer);

    let delay_gain = runtime_builder.add_node(AddNode::MultStereo { props: 0.6 });

    let (mut runtime, mut backend) = runtime_builder.get_owned();

    backend.load_sample(
        &String::from("amen"),
        "./samples/amen.wav",
        2,
        SAMPLE_RATE as u32,
    );

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: sampler,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: delay_write,
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
                node_key: delay_write,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: sampler,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: mixer,
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
                node_key: mixer,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: delay_read,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: delay_gain,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: delay_read,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: delay_gain,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: delay_gain,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: mixer,
                port_index: 2,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: delay_gain,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: mixer,
                port_index: 3,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: delay_gain,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: delay_write,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: delay_gain,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: delay_write,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime.set_sink_key(mixer).expect("Bad sink key!");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    let device = host.default_output_device().unwrap();

    println!("{:?}", device.default_output_config());

    let config = StreamConfig {
        channels: U2::U16,
        sample_rate: SampleRate(SAMPLE_RATE),
        buffer_size: BufferSize::Fixed(BlockSize::to_u32()),
    };

    start_runtime_audio_thread(&device, &config, runtime).expect("Runtime panic!");
}
