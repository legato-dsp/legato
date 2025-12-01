use cpal::{
    BufferSize, SampleRate, StreamConfig,
    traits::{DeviceTrait, HostTrait},
};
use legato_core_two::{
    nodes::ports::PortBuilder,
    runtime::{
        builder::{AddNode, get_runtime_builder},
        context::Config,
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

    let sampler = runtime_builder.add_node(AddNode::Sampler {
        chans: 2,
        sampler_name: String::from("amen"),
    });

    let (mut runtime, mut backend) = runtime_builder.get_owned();

    let _ = runtime.set_sink_key(sampler);

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
