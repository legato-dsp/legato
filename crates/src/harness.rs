use crate::{
    config::Config,
    node::Node,
    ports::PortBuilder,
    runtime::{Runtime, build_runtime},
};

pub fn get_node_test_harness(node: Box<dyn Node + Send + 'static>) -> Runtime {
    let config = Config {
        sample_rate: 48_000,
        audio_block_size: 4096,
        control_rate: 48_000 / 32,
        control_block_size: 4096 / 32,
        channels: 2,
        initial_graph_capacity: 1,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let mut graph = build_runtime(config, ports);

    let id = graph.add_node(node, "test node".into(), "test".into());

    let _ = graph.set_sink_key(id);

    graph
}
