use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    interface::AudioInterface,
    ports::PortBuilder,
};

fn main() {
    let graph = String::from(
        r#"
        patch voice(
            attack = 200.0,
            decay = 800.0,
            sustain = 0.7,
            release = 1200.0
        ) {
            in freq gate

            audio {
                saw { chans: 1 },
                adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 1 },
            }

            freq >> saw
            gate >> adsr.gate
            saw >> adsr[1]

            { adsr }
        }

        patch generative_voice {
            patches {
                voice { },
            }

            audio {
                svf { chans: 1, cutoff: 3600.0, q: 0.4, type: "lowpass" },
            }

            modular {
                random { rate: 800.0, prob: 0.3, range: [48.0, 76.0] },
                quantize { notes: [0, 2, 3, 9, 10, 5] },
                trig_to_gate { gate_time: 400.0 }
            }

            random.stepped >> quantize

            // quantize only fires `trig` when the snapped note actually changes,
            // so a held random value sustains the current note instead of
            // retriggering the envelope.
            quantize.trig >> trig_to_gate >> voice.gate
            quantize.freq >> voice.freq

            voice >> svf

            { svf }
        }

        patches {
            generative_voice * 5 {}
        }

        audio {
            track_mixer { tracks: 5, chans_per_track: 1 },
            mono_fan_out { chans: 2 },
            plate480
        }

        generative_voice(*) >> track_mixer

        track_mixer >> mono_fan_out >> plate480

        { plate480 }
    "#,
    );

    let config = Config {
        sample_rate: 48_000,
        block_size: 4096,
        channels: 2,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let (app, _frontend) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .build_dsl(&graph)
        .expect("graph should build");

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    AudioInterface::builder(&host, config)
        .build(app)
        .expect("Failed to start audio")
        .run_forever();
}
