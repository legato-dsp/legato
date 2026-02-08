use std::path::Path;

use cpal::{SampleRate, StreamConfig, traits::HostTrait};
use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::{BlockSize, Config},
    out::start_application_audio_thread,
    ports::PortBuilder,
};

fn main() {
    let graph = String::from(
        r#"
        audio {
            // Carrier and mod waves
            sine: carrier { freq: 440.0, chans: 1 },
            sine: mod { freq: 550.0, chans: 1 },
            
            // The FM ratio, just 1.5 for now
            mult: fm_freq { val: 1.5 },

            // The FM gain
            mult: fm_gain { val: 1000.0, chans: 1 },

            // One output chan, another control chan
            add: fm_add,

            mono_fan_out: master { chans: 2 },
        }

        control {
            // The carrier frequency
            signal: freq { name: "freq", min: 40.0, max: 22000.0, default: 440.0 },
        }

        freq >> fm_freq

        fm_freq >> mod.freq

        mod >> fm_gain[0]

        fm_gain >> fm_add[1]

        freq >> fm_add[0]

        fm_add >> carrier.freq

        carrier >> master

        { master }
    "#,
    );

    let config = Config::new(48_000, BlockSize::Block1024, 2, 6);

    let ports = PortBuilder::default().audio_out(2).build();

    let (app, _frontend) = LegatoBuilder::<Unconfigured>::new(config, ports).build_dsl(&graph);

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
