use legato_core::{
    backend::out::render,
    engine::{
        builder::AddNode,
        runtime::{build_runtime, Runtime},
    },
};
use legato_core::{
    engine::{builder::RuntimeBuilder, port::Ports},
    nodes::utils::port_utils::generate_audio_outputs,
};
use std::{path::Path, time::Duration};

use typenum::{U0, U1, U2048, U64};

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
            audio_inputs: None,
            audio_outputs: Some(generate_audio_outputs()),
            control_inputs: None,
            control_outputs: None,
        },
    );

    let (a, _) = runtime
        .add_node_api(AddNode::Sweep {
            range: (20.0, 26_000.0),
            duration: Duration::from_secs(5),
        })
        .expect("Could not add node");

    runtime.set_sink_key(a).expect("Bad sink key!");

    let path = Path::new("example.wav");

    render(runtime, path, SAMPLE_RATE, Duration::from_secs(5)).unwrap();
}
