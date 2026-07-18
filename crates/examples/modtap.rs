use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    interface::AudioInterface,
    kernel::EXAMPLE_MODTAP_KERNEL_PATCH,
    ports::PortBuilder,
};

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// An 8-tap modulated delay kernel.
///
/// Kernels are generally better for prototyping, and
/// I would suggest graduating to a custom Rust node
/// when you start reaching your performance budget,
/// or are looking for a production deployment
fn main() {
    let graph = format!(
        "{}\n{}",
        EXAMPLE_MODTAP_KERNEL_PATCH,
        r#"
        patches {
            modtap4 { depth: 12.0, rate: 0.05, feedback: 0.6 },
        }

        audio {
            saw { freq: 110.0, chans: 1 },
        }

        saw >> modtap4[0]

        { modtap4 }
    "#,
    );

    let config = Config {
        sample_rate: env_or("LEGATO_SAMPLE_RATE", 44_100),
        block_size: env_or("LEGATO_BLOCK_SIZE", 256),
        channels: env_or("LEGATO_CHANNELS", 2),
        rt_capacity: env_or("LEGATO_RT_CAPACITY", 0),
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let (app, _frontend) = LegatoBuilder::<Unconfigured>::new(config, ports).build_dsl(&graph);

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("CoreAudio host not available");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    AudioInterface::builder(&host, config)
        .build(app)
        .expect("Failed to start audio")
        .run_forever();
}
