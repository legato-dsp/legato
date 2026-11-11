use legato::prelude::*;

/// A five voice sawtooth example
fn main() {
    let graph = String::from(
        r#"
        patch voice(
            attack = 200.0,
            decay = 200.0,
            sustain = 0.2,
            release = 120.0
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

        patch smear(){
            in audio

            audio {
                grain_delay {
                    chans: 2,
                    diffusion: 1.0,
                    grain_size: 400,
                    spray: 6.0,
                    damping: 0.8,
                    scan: 1.0,
                    delay_length: 1000,
                    shape: 0.5,
                    feedback: 0.8
                },
                plate480 { mix: 0.8, decay: 0.8 },
                track_mixer: wet_dry { chans_per_track: 2, tracks: 2 },
            }

            audio >> grain_delay >> plate480

            audio >> wet_dry[0..2]
            plate480 >> wet_dry[2..4]

            { wet_dry }
        }

        patches {
            voice * 5 { },
            smear { }
        }

        audio {
            svf { chans: 1, cutoff: 3700.0, q: 0.4, type: "lowpass" },
            track_mixer: osc_mixer { tracks: 5, chans_per_track: 1, gain: [0.1, 0.1, 0.1, 0.1, 0.1] },
            mono_fan_out { chans: 2 },
        }

        midi {
            poly_voice { chan: 0, voices: 5 }
        }

        poly_voice[0:13:3] >> voice(*).gate
        poly_voice[1:13:3] >> voice(*).freq
        voice(*) >> osc_mixer[0..5]

        osc_mixer >> svf

        svf >> mono_fan_out >> smear

        { smear }
    "#,
    );

    let config = Config {
        sample_rate: 48_000,
        block_size: 4096,
        channels: 2,
        rt_capacity: 0,
    };

    let midi_rt_fe = start_midi_thread(
        256,
        "my_port",
        MidiPortKind::Index(0),
        MidiPortKind::Index(0),
        "my_port",
    )
    .unwrap();

    let (app, _frontend) = LegatoBuilder::<Unconfigured>::new(config)
        .set_midi_runtime(midi_rt_fe)
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
