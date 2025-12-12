use std::path::Path;

use cpal::{SampleRate, StreamConfig, traits::HostTrait};
use legato::{
    builder::LegatoBuilder, config::Config, out::start_application_audio_thread, pipes::{Pipe, PipeResult}, ports::PortBuilder
};

// Example registering a custom pipe, using aliasing, and the spread operator for indexing

struct Logger {}

impl Pipe for Logger {
    fn pipe(&self, inputs: PipeResult, _props: Option<legato::ast::Value>) -> legato::pipes::PipeResult {
        dbg!(&inputs);
        inputs
    }
}

fn main() {
    let graph = String::from(
        r#"
        audio {
            sampler { sampler_name: "amen" } | logger(),
            delay_write: dw1 { delay_name: "d_one", chans: 2 },
            delay_read: dr1 { delay_name: "d_one", chans: 2, delay_length: [ 200, 240 ] },
            delay_read: dr2 { delay_name: "d_one", chans: 2, delay_length: [ 310, 330 ] },
            track_mixer { tracks: 3, chans_per_track: 2, gain: [1.0, 0.2, 0.2] }
        }

        sampler[0..2] >> track_mixer[0..2]
        sampler[0..2] >> dw1[0..2]
        dr1[0..2] >> track_mixer[2..4]
        dr2[0] >> track_mixer[4..6]

        { track_mixer }
    "#,
    );

    let config = Config {
        sample_rate: 48_000,
        control_rate: 48_000 / 32,
        audio_block_size: 1024,
        control_block_size: 1024 / 32,
        channels: 2,
        initial_graph_capacity: 4,
    };

    
    let mut builder = LegatoBuilder::new(config, PortBuilder::default().audio_out(2).build());
    builder.register_pipe(&"logger".into(), Box::new(Logger {}));
            
    let (app, mut backend) = builder.build_from_str(&graph);

    let _ = backend.load_sample(
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
        buffer_size: cpal::BufferSize::Fixed(config.audio_block_size as u32),
    };

    start_application_audio_thread(&device, stream_config, app).expect("Audio thread panic!")
}
