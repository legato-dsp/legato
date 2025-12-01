use std::time::Duration;

use cpal::{
    BufferSize, SampleRate, StreamConfig,
    traits::{DeviceTrait, HostTrait},
};
use legato_core_two::{
    nodes::ports::{PortBuilder, PortRate},
    runtime::{
        builder::{AddNode, get_runtime_builder},
        context::Config,
        graph::{Connection, ConnectionEntry},
        out::start_runtime_audio_thread,
    },
};

fn main() {
    let config = Config {
        sample_rate: 48000,
        audio_block_size: 1024,
        channels: 2,
        control_block_size: 1024 / 32,
        control_rate: 48000 / 32,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let mut runtime_builder = get_runtime_builder(16, config, ports);

    let sampler = runtime_builder.add_node(AddNode::Sampler {
        chans: 2,
        sampler_name: String::from("amen"),
    });

    let delay_write = runtime_builder.add_node(AddNode::DelayWrite {
        delay_name: String::from("amen"),
        chans: 2,
        delay_length: Duration::from_secs_f32(3.0),
    });

    let delay_read = runtime_builder.add_node(AddNode::DelayRead {
        delay_name: String::from("amen"),
        chans: 2,
        delay_length: vec![Duration::from_millis(17), Duration::from_millis(23)],
    });

    let delay_gain = runtime_builder.add_node(AddNode::Gain { val: 0.4, chans: 2 });

    let mixer = runtime_builder.add_node(AddNode::TrackMixer {
        chans_per_track: 2,
        tracks: 2,
        gain: vec![0.6, 0.6], // TODO: Log values as well
    });

    let (mut runtime, mut backend) = runtime_builder.get_owned();

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

    let _ = runtime.set_sink_key(mixer);

    let output = std::process::Command::new("ls")
        .arg("../")
        .output()
        .expect("Failed to run ls");

    println!("stdout:\n{}", String::from_utf8_lossy(&output.stdout));
    println!("stderr:\n{}", String::from_utf8_lossy(&output.stderr));

    backend
        .load_sample(
            &String::from("amen"),
            "../samples/amen.wav",
            config.channels,
            config.sample_rate as u32,
        )
        .expect("Could not load sample");

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
