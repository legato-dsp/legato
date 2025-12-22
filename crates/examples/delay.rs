use std::path::Path;

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
            sampler { sampler_name: "amen" } | logger(),
            delay_write: dw1 { delay_name: "d_one", chans: 2 },
            delay_read: dr1 { delay_name: "d_one", chans: 2, delay_length: [ 200, 240 ] },
            delay_read: dr2 { delay_name: "d_one", chans: 1, delay_length: [ 231, 257 ] },
            track_mixer { tracks: 3, chans_per_track: 2, gain: [1.0, 0.3, 0.2] },
            svf { chans: 2, cutoff: 2400.0, q: 0.2, type: "lowpass" },
        }

        control {
            signal { name: "cutoff", min: 120.0, max: 24000.0, default: 800.0 }
        }

        sampler[0..2] >> track_mixer[0..2]
        sampler[0..2] >> dw1[0..2]
        dr1[0..2] >> track_mixer[2..4]
        dr2[0] >> track_mixer[4..6]

        signal >> svf.cutoff

        track_mixer[0..2] >> svf[0..2]

        { svf }
    "#,
    );

    let config = Config {
        sample_rate: 44_100,
        block_size: 4096,
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

    start_application_audio_thread(&device, stream_config, app).expect("Audio thread panic!")
}
