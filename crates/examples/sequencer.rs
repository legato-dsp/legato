use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    interface::AudioInterface,
    out::start_application_audio_thread,
    ports::PortBuilder,
};

fn main() {
    let graph = String::from(
        r#"
        audio {
            sine { freq: 440.0, chans: 1},
            adsr { attack: 30.0, decay: 40.0, sustain: 0.0, release: 30.0, chans: 1 },
            mono_fan_out { chans: 2 }
        }

        control {
            clock { bpm: 120, division: 4, steps: 64 },
            sequencer { num_steps: 64 }
        }

        clock >> sequencer

        sequencer.gate >> adsr.gate
        sine >> adsr[1]

        sequencer.freq >> sine

        adsr >> mono_fan_out

        { mono_fan_out }
    "#,
    );

    let config = Config {
        sample_rate: 44_000,
        block_size: 256,
        channels: 2,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let (app, _) = LegatoBuilder::<Unconfigured>::new(config, ports).build_dsl(&graph);

    let interface = AudioInterface::default_with_config(&config);

    start_application_audio_thread(interface, app).expect("Audio thread panic!");
}
