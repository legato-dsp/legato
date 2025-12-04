use std::{path::Path, time::Duration};

use cpal::{
    BufferSize, SampleRate, StreamConfig,
    traits::{DeviceTrait, HostTrait},
};
use legato_core::{
    nodes::{audio::sweep::Sweep, ports::{PortBuilder, PortRate}},
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
        initial_graph_capacity: 4
    };

    #[cfg(target_os = "macos")]
    let config = Config {
        sample_rate: 44_100,
        audio_block_size: 1024,
        channels: 2,
        control_block_size: 1024 / 32,
        control_rate: 44_100 / 32,
        initial_graph_capacity: 4
    };

    let ports = PortBuilder::default().audio_out(2).build();


    // Create 2x oversampled config

    let mut os_config = config.clone();
    
    os_config.audio_block_size *= 2;
    os_config.sample_rate *= 2;

    // Make the 2x oversampled graph

    let mut sweep_runtime_builder = get_runtime_builder(os_config, ports.clone());

    let sweep_key = sweep_runtime_builder.add_node(AddNode::Sweep { range: (40.0, 42_000.0), duration: Duration::from_secs(5), chans: 2 });

    let (mut sweep_runtime, _) = sweep_runtime_builder.get_owned();

    let _ = sweep_runtime.set_sink_key(sweep_key);

    // Make the normal audio rate graph

    let mut runtime_builder = get_runtime_builder(config, ports);

    let sweep = runtime_builder.add_node(AddNode::Oversample2X { runtime: Box::new(sweep_runtime), chans: 2 });

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
