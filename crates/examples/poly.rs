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
            attack = 200.0,
            decay = 200.0,
            sustain = 0.3,
            release = 200.0
        ) {
            in freq gate

            audio {
                sine: mod,
                sine: carrier,
                adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 1 },
                mult: freq_mult,
                mult: fm_gain { val: 1000.0 },
                add: fm_add
            }

            control {
                signal: ratio { name: "ratio", min: 1.0, max: 100.0, default: 1.5 }
            }

            freq >> freq_mult[0]

            ratio >> freq_mult[1]

            freq_mult >> mod.freq

            mod >> fm_gain[0]


            freq >> fm_add[0]
            fm_gain >> fm_add[1]

            fm_add >> carrier.freq

            gate >> adsr.gate

            carrier >> adsr[1]

            { adsr }
        }

        patches {
            voice * 5 { }
        }

        audio {
            track_mixer: osc_mixer { tracks: 5, chans_per_track: 1, gain: [0.1, 0.1, 0.1, 0.1, 0.1] },
            mono_fan_out { chans: 2 },

            delay_write: dw1 { delay_name: "d_one", delay_length: 2000.0, chans: 2 },
            delay_read: dr1 { delay_name: "d_one", chans: 2, delay_length: [ 938, 731 ] },
            delay_read: dr2 { delay_name: "d_one", chans: 2, delay_length: [ 459, 643 ] },

            track_mixer: master { tracks: 3, chans_per_track: 2, gain: [0.4, 0.5, 0.5] },
            
            track_mixer: feedback { tracks: 2, chans_per_track: 2, gain: [0.5, 0.5] }
        }

        midi { 
            poly_voice { chan: 0, voices: 5 }
        }

        poly_voice[0:13:3] >> voice(*).gate
        poly_voice[1:13:3] >> voice(*).freq
        voice(*) >> osc_mixer[0..5]

        osc_mixer >> mono_fan_out

        mono_fan_out >> master[0..2]
        mono_fan_out >> dw1[0..2]

        dr1[0..2] >> master[2..4]
        dr2[0..2] >> master[4..6]

        // feedback    
        dr1 >> feedback[0..2]
        dr2 >> feedback[2..4]

        feedback >> dw1

        { master }
    "#,
    );

    let config = Config {
        sample_rate: 48_000,
        block_size: 4096,
        channels: 2,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let (midi_rt_fe, _writer_fe) = start_midi_thread(
        256,
        "my_port",
        MidiPortKind::Index(0),
        MidiPortKind::Index(0),
        "my_port",
    )
    .unwrap();

    let (app, _frontend) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .set_midi_runtime(midi_rt_fe)
        .build_dsl(&graph);

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    AudioInterface::builder(&host, config)
        .build(app)
        .expect("Failed to start audio")
        .run_forever();
}
