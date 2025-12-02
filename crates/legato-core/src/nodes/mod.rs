pub mod audio;
pub mod utils;

use std::ops::Mul;
use typenum::{Prod, U0, U2};

use crate::{
    engine::{
        graph::Connection,
        node::{BufferSize, Node},
        port::Ports,
        runtime::{Runtime, build_runtime},
    },
    nodes::utils::port_utils::generate_audio_outputs,
};

pub fn get_node_test_harness<AF, CF>(
    node: Box<dyn Node<AF, CF> + Send + 'static>,
) -> Runtime<AF, CF, U2, U0>
where
    AF: BufferSize + Mul<U2>,
    Prod<AF, U2>: BufferSize,
    CF: BufferSize,
{
    let mut graph = build_runtime::<AF, CF, U2, U0>(
        1,
        48_000.0,
        48_000.0 / 32.0,
        Ports {
            audio_inputs: None,
            audio_outputs: Some(generate_audio_outputs()),
            control_inputs: None,
            control_outputs: None,
        },
    );

    let id = graph.add_node(node);

    let _ = graph.set_sink_key(id);

    graph
}
