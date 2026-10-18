use std::path::Path;

use legato::kernel::EXAMPLE_PLATE_KERNEL_PATCH;
use legato::prelude::*;

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
        patch sampler_voice(
            attack = 5.0,
            decay = 300.0,
            sustain = 0.8,
            release = 700.0
        ) {
            in freq gate

            audio {
                sampler { sampler_name: "main", chans: 2 },
                adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 2 },
            }

            freq >> sampler.pitch
            gate >> sampler.trig

            gate >> adsr.gate
            sampler >> adsr[1..3]

            { adsr }
        }

        patches {
            sampler_voice: lead { },
        }

        midi {
            voice { chan: 0 }
        }

        patches {
            plate: verb { predelay: 32.0, decay: 0.4, damping: 0.3, wet: 0.8, dry: 0.2 }
        }

        voice.gate >> lead.gate
        voice.freq >> lead.freq

        lead >> verb

        { verb }
    "#,
    );

    let config = Config {
        sample_rate: env_or("LEGATO_SAMPLE_RATE", 48_000),
        block_size: env_or("LEGATO_BLOCK_SIZE", 256),
        channels: env_or("LEGATO_CHANNELS", 2),
        rt_capacity: env_or("LEGATO_RT_CAPACITY", 0),
    };

    let midi_rt_fe = start_midi_thread(
        256,
        "my_port",
        MidiPortKind::Index(0),
        MidiPortKind::Index(0),
        "my_port",
    )
    .unwrap();

    let (app, mut frontend) = LegatoBuilder::new(config)
        .set_midi_runtime(midi_rt_fe)
        .build_dsl(&graph)
        .expect("graph should build");

    frontend
        .load_sample(
            &String::from("main"),
            Path::new("../samples/string.wav"),
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
