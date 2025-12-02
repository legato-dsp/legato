use std::{path::Path, time::Duration};

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
        out::{render, start_runtime_audio_thread},
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

    // let sweep = runtime_builder.add_node(AddNode::Sweep { range: (0.0, 48_000.0), duration: Duration::from_secs(5), chans: 2 });

    let sweep = runtime_builder.add_node(AddNode::Sweep { range: (40.0, 48_000.0), duration: Duration::from_secs(5), chans: 2 });


    let (mut runtime, _) = runtime_builder.get_owned();

    let _ = runtime.set_sink_key(sweep);

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    let device = host.default_output_device().unwrap();

    println!("{:?}", device.default_output_config());

    let path = Path::new("./out.wav");

    render(runtime, path, config.sample_rate as u32, Duration::from_secs(5)).unwrap();
}
