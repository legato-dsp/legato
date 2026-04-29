use std::path::Path;

use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    interface::AudioInterface,
    ports::PortBuilder,
};

fn main() {
    let graph = String::from(
        r#"
        audio {
            sampler { sampler_name: "amen" },
            delay_write: dw1 { delay_name: "d_one", chans: 2 },
            delay_read: dr1 { delay_name: "d_one", chans: 2, delay_length: [ 200, 240 ] },
            delay_read: dr2 { delay_name: "d_one", chans: 1, delay_length: [ 231, 257 ] },
            track_mixer { tracks: 3, chans_per_track: 2, gain: [1.0, 0.3, 0.2] },
            svf { chans: 2, cutoff: 2400.0, q: 0.2, type: "lowpass" },
        }

        control {
            signal { name: "cutoff", min: 120.0, max: 24000.0, default: 3200.0 }
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
        sample_rate: 48_000,
        block_size: 1024,
        channels: 2,
        rt_capacity: 0,
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

    AudioInterface::builder(&host, config)
        .build(app)
        .expect("Failed to start audio")
        .run_forever();
}
