use std::path::Path;

use cpal::{SampleRate, StreamConfig, traits::HostTrait};
use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    out::start_application_audio_thread,
    ports::PortBuilder,
};

fn main() {
    let graph = String::from(
        r#"
        audio {
            sampler { sampler_name: "amen", chans: 2 },
            allpass { delay_length: 20, feedback: 0.5, chans: 2 },
            track_mixer { tracks: 2, chans_per_track: 2, gain: [0.5, 0.5] },
            sine: lfo { freq: 0.2, chans: 1 }
        }

        control {
            map { range: [-1.0, 1.0], new_range: [5.0, 50.0] }
        }

        lfo >> map

        sampler >> track_mixer[0..2]
        sampler >> allpass[0..2]
        allpass >> track_mixer[2..4]

        map >> allpass.delay_length

        { track_mixer }
    "#,
    );

    let config = Config {
        sample_rate: 48_000,
        block_size: 4096,
        channels: 2,
        initial_graph_capacity: 4,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let (app, mut frontend) = LegatoBuilder::<Unconfigured>::new(config, ports).build_dsl(&graph);

    let _ = frontend.load_sample(
        &String::from("amen"),
        Path::new("../samples/amen.wav"),
        2,
        config.sample_rate as u32,
    );

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    let device = host.default_output_device().unwrap();

    let stream_config = StreamConfig {
        channels: config.channels as u16,
        sample_rate: SampleRate(config.sample_rate as u32),
        buffer_size: cpal::BufferSize::Fixed(config.block_size as u32),
    };

    start_application_audio_thread(&device, stream_config, app).expect("Audio thread panic!")
}
