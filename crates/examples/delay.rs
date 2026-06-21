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
    /*
        This patch is a resonant filterbank. A patch like this may make
        more sense being implemented as a custom node.
    */

    // Make an lfo within a certain range, linear mapping [-1,1] to the requested range
    patch lfo(
        freq = 1.0,
        new_range = [-1.0, 1.0]
    )
    {
        audio {
            sine { freq: $freq }
        }

        control {
            map { range: [-1.0, 1.0], new_range: $new_range }
        }

        sine >> map

        { map }
    }

    patch filter(
        freq = 3400.0,
        lfo_freq = 1.0,
        q_range = [2.0, 2.0],
        gain = 1.0,
        chans = 2,
    ){
        in audio_in

        audio {
            svf { cutoff: $freq, chans: $chans, type: "bandpass" },
            lfo { freq: $lfo_freq, new_range: $q_range },
            gain { val: $gain, chans: $chans  }
        }

        audio_in >> svf[0..2]
        lfo >> svf.q

        svf >> gain[0..2]

        { gain }
    }

    audio {
        sampler { sampler_name: "amen" },
        // Setup all of the filterbanks
        filter: fb1 { freq: 80.0 },
        filter: fb2 { freq: 164.0 },
        filter: fb3 { freq: 335.0 },
        filter: fb4 { freq: 685.0, q_range: [1.0, 8.0], lfo_freq: 1.5 },
        filter: fb5 { freq: 1402.0 },
        filter: fb6 { freq: 2868.0, q_range: [2.0, 10.0], lfo_freq: 0.05  },
        filter: fb7 { freq: 5868.0, q_range: [1.0, 8.0], lfo_freq: 1.0 },
        filter: fb8 { freq: 12000.0 },
        // Mix the filters down to one stereo track
        track_mixer { chans_per_track: 2, tracks: 8 }
    }

    // Wire the filter banks
    sampler >> fb1
    sampler >> fb2
    sampler >> fb3
    sampler >> fb4
    sampler >> fb5
    sampler >> fb6
    sampler >> fb7
    sampler >> fb8

    fb1 >> track_mixer[0..2]
    fb2 >> track_mixer[2..4]
    fb3 >> track_mixer[4..6]
    fb4 >> track_mixer[6..8]
    fb5 >> track_mixer[8..10]
    fb6 >> track_mixer[10..12]
    fb7 >> track_mixer[12..14]
    fb8 >> track_mixer[14..16]

    { track_mixer }           
    "#,
    );

    let config = Config {
        sample_rate: 44_100,
        block_size: 4096,
        channels: 2,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let (app, mut frontend) = LegatoBuilder::<Unconfigured>::new(config, ports).build_dsl(&graph);

    dbg!(&app);

    let _ = frontend.load_sample(
        &String::from("amen"),
        Path::new("../samples/example_two.wav"),
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
