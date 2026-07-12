use std::path::Path;

use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    interface::AudioInterface,
    midi::{MidiPortKind, start_midi_thread},
    ports::PortBuilder,
};

fn main() {
    let graph = String::from(
        r#"
        patch voice(
            attack = 50.0,
            decay = 30.0,
            sustain = 0.3,
            release = 50.0
        ) {
            in freq gate

            audio {
                grain { sampler_name: "main", chans: 2 },
                adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 2 },
            }

            freq >> grain.freq
            gate >> grain.trig

            gate >> adsr.gate
            grain >> adsr[1..3]

            { adsr }
        }

        patches {
            voice * 3 { },
        }
        
        audio {
            track_mixer { tracks: 3, chans_per_track: 2 },
        }

        midi {
            poly_voice { chan: 0, voices: 3 }
        }

        poly_voice[0:10:3] >> voice(*).gate
        poly_voice[1:10:3] >> voice(*).freq

        voice(*)[0] >> track_mixer[0:6:2]
        voice(*)[1] >> track_mixer[1:6:2]

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

    let midi_rt_fe = start_midi_thread(
        256,
        "my_port",
        MidiPortKind::Index(0),
        MidiPortKind::Index(0),
        "my_port",
    )
    .unwrap();

    let (app, mut frontend) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .set_midi_runtime(midi_rt_fe)
        .build_dsl(&graph);

    dbg!(&app);

    let _ = frontend.load_sample(
        &String::from("main"),
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
