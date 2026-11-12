use legato::prelude::*;
use legato::{input::DeviceSelection, interface::InputSpec, kernel::EXAMPLE_PLATE_KERNEL_PATCH};

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Here is an example of spawning a kernel patch.
///
/// These are significantly slower than say a custom node.
/// I would suggest prototyping with the kernels, and if it becomes
/// slow, shifting to a custom node.
///
/// You can see the example patch below, as well as a custom node
/// implementation in nodes/plate.rs
fn main() {
    let graph = r#"
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
                    shape: 0.01,
                    feedback: 0.8
                },
                plate480 { mix: 0.8, decay: 0.8 },
                track_mixer: wet_dry { chans_per_track: 2, tracks: 2, gain: [0.5, 0.5] },
            }

            audio >> grain_delay >> plate480

            audio >> wet_dry[0..2]
            plate480 >> wet_dry[2..4]

            { wet_dry }
        }

        patches {
            smear: verb { }
        }

        audio {
            // only pull the first channel here
            external { interface_name: "one", chans: 2 },
            mono_fan_out { chans: 2 },
        }

        external[0] >> mono_fan_out >> verb

        { verb }
    "#;

    let config = Config {
        sample_rate: env_or("LEGATO_SAMPLE_RATE", 44_100),
        block_size: env_or("LEGATO_BLOCK_SIZE", 256),
        channels: env_or("LEGATO_CHANNELS", 2),
        rt_capacity: env_or("LEGATO_RT_CAPACITY", 0),
    };

    let (producer, consumer) = rtrb::RingBuffer::new(4096 * 4); // 4 frames of headroom

    let (app, _) = LegatoBuilder::<Unconfigured>::new(config)
        .register_audio_input("one", consumer, 2, config.block_size)
        .build_dsl(&graph)
        .expect("graph should build");

    #[cfg(target_os = "macos")]
    // let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("CoreAudio host not available");
    let host = cpal::default_host();

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    AudioInterface::builder(&host, config)
        .input(InputSpec {
            producer,
            chans: 2,
            device: DeviceSelection::Default,
        })
        .build(app)
        .expect("Failed to start audio")
        .run_forever();
}
