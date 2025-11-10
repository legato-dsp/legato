use legato_core::{
    backend::out::render,
    engine::{
        builder::AddNode,
        runtime::{build_runtime, Runtime},
    },
    nodes::utils::port_utils::generate_audio_inputs,
};
use legato_core::{
    engine::{builder::RuntimeBuilder, port::Ports},
    nodes::utils::port_utils::generate_audio_outputs,
};
use std::{path::Path, time::Duration};

use typenum::{Prod, U0, U1, U2, U2048, U64};

fn main() {
    type BlockSize = U2048;
    type ControlSize = U64;
    type ChannelCount = U1;

    const SAMPLE_RATE: u32 = 44_100;
    const CAPACITY: usize = 16;
    const DECIMATION_FACTOR: f32 = 32.0;
    const CONTROL_RATE: f32 = SAMPLE_RATE as f32 / DECIMATION_FACTOR;

    let mut runtime: Runtime<BlockSize, ControlSize, ChannelCount, U0> = build_runtime(
        CAPACITY,
        SAMPLE_RATE as f32,
        CONTROL_RATE,
        Ports {
            audio_inputs: Some(generate_audio_inputs()),
            audio_outputs: Some(generate_audio_outputs()),
            control_inputs: None,
            control_outputs: None,
        },
    );

    type SRX2 = Prod<BlockSize, U2>;

    let mut oversampled_runtime: Runtime<SRX2, ControlSize, U1, U0> = build_runtime(
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

    let (a, _) = oversampled_runtime
        .add_node_api(AddNode::Sweep {
            range: (20.0, 32_000.0),
            duration: Duration::from_secs(5),
        })
        .expect("Could not add node");

    oversampled_runtime.set_sink_key(a).unwrap();

    let (b, _) = runtime
        .add_node_api(AddNode::Subgraph2XOversampled {
            runtime: Box::new(oversampled_runtime),
        })
        .unwrap();

    runtime.set_sink_key(b).unwrap();

    let path = Path::new("example.wav");

    render(runtime, path, SAMPLE_RATE, Duration::from_secs(5)).unwrap();
}
