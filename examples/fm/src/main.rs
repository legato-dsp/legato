use cpal::{SampleRate, StreamConfig, traits::HostTrait};
use legato::{
    core::runtime::{context::Config, out::start_runtime_audio_thread}, dsl::build_application
};

fn main() {
    let graph = String::from(
        r#"
        audio {
            sine: mod { freq: 550.0, chans: 1 },
            sine: carrier { freq: 440.0, chans: 2 },
            mult: fm_gain { val: 1000.0, chans: 1 }
        }

        mod >> fm_gain >> carrier[0]

        { carrier }
    "#,
    );

    let config = Config {
            sample_rate: 48_000,
            control_rate: 48_000 / 32,
            audio_block_size: 1024,
            control_block_size: 1024 / 32,
            channels: 2,
            initial_graph_capacity: 4
        };

    let (runtime, _) = build_application(&graph, config).expect("Could not build application");

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    let device = host.default_output_device().unwrap();

    let stream_config = StreamConfig {
        channels: config.channels as u16,
        sample_rate: SampleRate(config.sample_rate as u32),
        buffer_size: cpal::BufferSize::Fixed(config.audio_block_size as u32),
    };

    start_runtime_audio_thread(&device, stream_config, runtime).expect("Audio thread panic!")
}
