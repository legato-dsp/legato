use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    interface::AudioInterface,
    ports::PortBuilder,
    spec::NodeDefinition,
};

// An example with a code-gen node, typically a nice middle ground between a custom node, and the kernel feature

legato_macros::include_node!("kernels/modtap4.legato", "modtap4");

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A four-tap modulated feedback comb, mono in / stereo out.
///
/// This is using the [`include_node!`] macro, which is generally
/// faster than the interpreted nodes, albeit still not as fast as a
/// custom block based node.
fn main() {
    let graph = r#"
        audio {
            saw { freq: 110.0, chans: 1 },
            modtap4,
        }

        saw >> modtap4[0]

        { modtap4 }
    "#;

    let config = Config {
        sample_rate: env_or("LEGATO_SAMPLE_RATE", 44_100),
        block_size: env_or("LEGATO_BLOCK_SIZE", 256),
        channels: env_or("LEGATO_CHANNELS", 2),
        rt_capacity: env_or("LEGATO_RT_CAPACITY", 0),
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let (app, _frontend) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .register_node("audio", Modtap4::spec())
        .build_dsl(graph);

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("CoreAudio host not available");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    AudioInterface::builder(&host, config)
        .build(app)
        .expect("Failed to start audio")
        .run_forever();
}
