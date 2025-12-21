use std::{path::Path, time::Duration};

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
            sine { freq: 440.0, chans: 2 }
        }

        control {
            signal { name: "pitch", default: 440.0, min: 0.0, max: 24000.0 }
        }

        signal >> sine

        { sine }
    "#,
    );

    let config = Config {
        sample_rate: 48_000,
        block_size: 1024,
        channels: 2,
        initial_graph_capacity: 4,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let (app, mut frontend) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .build_dsl(&graph);

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

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(5));
        dbg!("other thread!");
        frontend.set_param("pitch", 880.0).unwrap();
    });

    start_application_audio_thread(&device, stream_config, app).expect("Audio thread panic!");
}
