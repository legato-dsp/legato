use std::{path::Path, thread::sleep, time::Duration};

use cpal::{SampleRate, StreamConfig, traits::HostTrait};
use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    out::start_application_audio_thread,
    pipes::Pipe,
    ports::PortBuilder,
};

// Example registering a custom pipe, using aliasing, and the spread operator for indexing

struct Logger;

impl Pipe for Logger {
    fn pipe(&self, view: &mut legato::builder::SelectionView, _: Option<legato::ast::Value>) {
        println!("In a pipe!!");
        dbg!(view);
    }
}

fn main() {
    let graph = String::from(
        r#"
        audio {
            sine { freq: 440.0, chans: 2 }
        }

        // control {
        //     signal { name: "pitch", default: 440.0, min: 0.0, max: 24000.0 }
        // }

        // signal >> sine

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
        .register_pipe("logger", Box::new(Logger {}))
        .build_dsl(&graph);

    dbg!(&app);

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

    start_application_audio_thread(&device, stream_config, app).expect("Audio thread panic!");

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(5));
        frontend.set_param("pitch", 880.0).unwrap();
    });
}
