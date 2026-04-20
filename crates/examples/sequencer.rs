use cpal::{SampleRate, StreamConfig, traits::HostTrait};
use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
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
        sample_rate: 48_000,
        block_size: 256,
        channels: 2,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let (app, _) = LegatoBuilder::<Unconfigured>::new(config, ports).build_dsl(&graph);

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    let device = host.default_output_device().unwrap();

    let stream_config = StreamConfig {
        channels: config.channels as u16,
        sample_rate: SampleRate(config.sample_rate as u32),
        buffer_size: cpal::BufferSize::Fixed(config.block_size as u32),
    };

    // std::thread::spawn(move || {
    //     std::thread::sleep(Duration::from_secs(5));
    //     frontend.set_param("pitch", 880.0).unwrap();
    // });

    start_application_audio_thread(&device, stream_config, app).expect("Audio thread panic!");
}
