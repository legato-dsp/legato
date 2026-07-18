use std::path::Path;

use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    interface::AudioInterface,
    kernel::EXAMPLE_PLATE_KERNEL_PATCH,
    midi::{MidiPortKind, start_midi_thread},
    ports::PortBuilder,
};

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let graph = format!(
        "{}\n{}",
        EXAMPLE_PLATE_KERNEL_PATCH,
        r#"
        patch voice(
            attack = 1200.0,
            decay = 300.0,
            sustain = 0.8,
            release = 700.0
        ) {
            in freq gate

            audio {
                sine: lfo { freq: 0.1 },
                grain { sampler_name: "main", chans: 2, size: 70, shape: 0.5, scan: 0.05 },
                adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 2 },
            }

            control { 
                map { range: [-1.0, 1.0], new_range: [100, 300] }
            }

            lfo >> map
            map >> grain.size

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

        patches {
            plate: verb { predelay: 32.0, decay: 0.4, damping: 0.3, wet: 0.8, dry: 0.2 }
        }

        poly_voice[0:10:3] >> voice(*).gate
        poly_voice[1:10:3] >> voice(*).freq

        voice(*)[0] >> track_mixer[0:6:2]
        voice(*)[1] >> track_mixer[1:6:2]

        track_mixer >> verb

        { verb }
    "#,
    );

    let config = Config {
        sample_rate: env_or("LEGATO_SAMPLE_RATE", 44_100),
        block_size: env_or("LEGATO_BLOCK_SIZE", 256),
        channels: env_or("LEGATO_CHANNELS", 2),
        rt_capacity: env_or("LEGATO_RT_CAPACITY", 0),
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

    frontend
        .load_sample(
            &String::from("main"),
            Path::new("../samples/guitar.wav"),
            2,
            config.sample_rate as u32,
        )
        .expect("Could not load sample!");

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    AudioInterface::builder(&host, config)
        .build(app)
        .expect("Failed to start audio")
        .run_forever();
}
