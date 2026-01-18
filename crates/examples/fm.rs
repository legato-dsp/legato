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
            sine: carrier { freq: 440.0, chans: 2 },
            sine: mod { freq: 550.0, chans: 2 }
        }

        control {
            map { range: [-1.0, 1.0], new_range: [432.0, 448.0] }
        }

        lfo >> map >> sine[0]

        { sine }
    "#,
    );

    let config = Config {
        sample_rate: 44_100,
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

    dbg!(&app);

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

    // std::thread::spawn(move || {
    //     std::thread::sleep(Duration::from_secs(5));
    //     frontend.set_param("pitch", 880.0).unwrap();
    // });

    start_application_audio_thread(&device, stream_config, app).expect("Audio thread panic!");
}
