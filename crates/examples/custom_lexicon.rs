//! Live external input through the plate reverb authored as a `kernel` DSL
//! declaration ([`legato::kernel::PLATE_KERNEL`]).
//!
//! The Plate480 DSP originally lived in this example as a handwritten Rust
//! `PerSampleNode`. It now exists twice: as that Rust node
//! (`crates/src/nodes/audio/plate.rs`, kept for the equivalence test in
//! `tests/kernel_e2e.rs`) and as the ~66-node kernel DSL declaration used
//! here — the same figure-eight feedback tank, expressed entirely in
//! per-sample primitives (`allpass`, `tap`, `onepole`, `mult`, `add`, `sine`,
//! `map`).

use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    input::DeviceSelection,
    interface::{AudioInterface, InputSpec},
    kernel::PLATE_KERNEL,
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
        PLATE_KERNEL,
        r#"
        patches {
            plate: verb { predelay: 32.0, decay: 0.8, damping: 0.3, wet: 0.8, dry: 0.2 }
        }

        audio {
            external { interface_name: "one", chans: 1 },
            mono_fan_out { chans: 2 },
        }

        external >> mono_fan_out >> verb[0..2]

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

    let (producer, consumer) = rtrb::RingBuffer::new(4096 * 4); // 4 frames of headroom

    let (app, _) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .register_audio_input("one", consumer, 1, config.block_size)
        .build_dsl(&graph);

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("CoreAudio host not available");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    AudioInterface::builder(&host, config)
        .input(InputSpec {
            producer,
            chans: 1,
            device: DeviceSelection::Default,
        })
        .build(app)
        .expect("Failed to start audio")
        .run_forever();
}
