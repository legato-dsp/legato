use std::{path::Path, time::Duration};

use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    out::render,
    ports::PortBuilder,
};

// Example with a poc oversample pipe

fn main() {
    let graph = String::from(
        r#"
        audio {
            sweep { freq: [40.0, 48000.0], duration: 5000.0, chans: 2 } | oversample2X()
        }

        { sweep }
    "#,
    );

    let config = Config {
        sample_rate: 48_000,
        block_size: 1024,
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

    let path = Path::new("example.wav");

    render(app, path, Duration::from_secs(5)).expect("Audio thread panic!")
}
