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

    let carrier = runtime_builder.add_node(AddNode::Sine {
        freq: 440.0,
        chans: 2,
    });

    // TODO: Apply gain

    let modulator = runtime_builder.add_node(AddNode::Sine {
        freq: 440.0 * (5.0 / 4.0),
        chans: 1,
    });

    let (mut runtime, _) = runtime_builder.get_owned();

    let _ = runtime.add_edge(Connection {
        source: ConnectionEntry {
            node_key: modulator,
            port_index: 0,
            port_rate: PortRate::Audio,
        },
        sink: ConnectionEntry {
            node_key: carrier,
            port_index: 0,
            port_rate: PortRate::Audio,
        },
    });

    let _ = runtime.set_sink_key(carrier);

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
