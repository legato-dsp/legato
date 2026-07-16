use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    interface::AudioInterface,
    kernel::{KARPLUS_KERNEL, PLATE_KERNEL},
    midi::{MidiPortKind, start_midi_thread},
    ports::PortBuilder,
};

fn main() {
    let graph = format!(
        "{}\n{}\n{}",
        KARPLUS_KERNEL,
        PLATE_KERNEL,
        r#"
        patches {
            // decay near 1 = long sustain; lower damping = brighter/longer ring.
            karplus: voice * 5 { damping: 0.4, decay: 0.996, pluck: 0.99 },
            plate {}
        }

        audio {
            // 5 mono strings summed to one bus, gently rolled off, then spread
            // to stereo. keep osc_mixer -> svf on port 0 only (>> svf would also
            // hit svf's cutoff/q mod ports).
            track_mixer: osc_mixer { tracks: 5, chans_per_track: 1, gain: [0.3, 0.3, 0.3, 0.3, 0.3] },
            svf { chans: 1, cutoff: 6000.0, q: 0.4, type: "lowpass" },
            mono_fan_out { chans: 2 },
        }

        midi {
            poly_voice { chan: 0, voices: 5 }
        }

        // poly_voice emits 3 chans per voice: [gate, freq, vel]. With 5 voices
        // that is 15 chans, so the strides run to 15 (0,3,6,9,12 / 1,4,7,10,13).
        poly_voice[0:15:3] >> voice(*).gate
        poly_voice[1:15:3] >> voice(*).freq
        voice(*) >> osc_mixer[0..5]

        osc_mixer >> mono_fan_out // no key tracking

        mono_fan_out >> plate

        { plate }
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
