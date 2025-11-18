use legato_core::{
    out::render,
    engine::builder::{AddNode, RuntimeBuilder, get_runtime_builder},
    nodes::utils::port_utils::generate_audio_inputs,
};
use legato_core::{engine::port::Ports, nodes::utils::port_utils::generate_audio_outputs};
use std::{path::Path, time::Duration};

use typenum::{Prod, U0, U1, U2, U64, U2048};

fn main() {
    type BlockSize = U2048;
    type ControlSize = U64;
    type ChannelCount = U1;

    const SAMPLE_RATE: u32 = 44_100;
    const CAPACITY: usize = 16;
    const DECIMATION_FACTOR: f32 = 32.0;
    const CONTROL_RATE: f32 = SAMPLE_RATE as f32 / DECIMATION_FACTOR;

    let mut runtime_builder: RuntimeBuilder<BlockSize, ControlSize, ChannelCount, U0> =
        get_runtime_builder(
            CAPACITY,
            SAMPLE_RATE as f32,
            CONTROL_RATE,
            Ports {
                audio_inputs: None,
                audio_outputs: Some(generate_audio_outputs()),
                control_inputs: None,
                control_outputs: None,
            },
        );

    type SRX2 = Prod<BlockSize, U2>;

    let mut oversampled_runtime_builder: RuntimeBuilder<SRX2, ControlSize, U1, U0> =
        get_runtime_builder(
            CAPACITY,
            (SAMPLE_RATE * 2) as f32,
            CONTROL_RATE,
            Ports {
                audio_inputs: Some(generate_audio_inputs()),
                audio_outputs: Some(generate_audio_outputs()),
                control_inputs: None,
                control_outputs: None,
            },
        );

    let a = oversampled_runtime_builder.add_node(AddNode::Sweep {
        range: (20.0, 32_000.0),
        duration: Duration::from_secs(5),
    });

    let (mut oversampled_runtime, _) = oversampled_runtime_builder.get_owned();

    oversampled_runtime.set_sink_key(a).unwrap();

    let b = runtime_builder.add_node(AddNode::Subgraph2XOversampled {
        runtime: Box::new(oversampled_runtime),
    });

    let (mut runtime, _) = runtime_builder.get_owned();

    runtime.set_sink_key(b).unwrap();

    let path = Path::new("example.wav");

    render(runtime, path, SAMPLE_RATE, Duration::from_secs(5)).unwrap();
}
