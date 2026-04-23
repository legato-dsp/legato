use std::sync::Arc;

use slotmap::SlotMap;

use crate::{
    config::Config,
    context::AudioContext,
    node::{DynNode, LegatoNode},
    ports::{PortBuilder, Ports},
    resources::{Resources, arena::RuntimeArena, params::ParamStore},
    runtime::Runtime,
};

pub fn build_placeholder_context(config: Config) -> AudioContext {
    let (_, dummy_sample_cons) = rtrb::RingBuffer::new(64);
    let (dummy_garbage_prod, _) = rtrb::RingBuffer::new(64);

    AudioContext::new(
        config,
        Resources::new(
            RuntimeArena::default(),
            ParamStore::new(Arc::new([])),
            dummy_sample_cons,
            dummy_garbage_prod,
            SlotMap::default(),
            SlotMap::default(),
            SlotMap::default(),
        ),
    )
}

fn build_placeholder_runtime(config: Config, ports: Ports) -> Runtime {
    let temporary_context = build_placeholder_context(config);

    Runtime::new(temporary_context, ports)
}

pub fn get_node_test_harness_stereo_4096(node: Box<dyn DynNode>) -> Runtime {
    let config = Config {
        sample_rate: 48_000,
        block_size: 4096,
        channels: 2,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let mut runtime = build_placeholder_runtime(config, ports);

    let id = runtime.add_node(LegatoNode::new("test node".into(), "test".into(), node));

    let _ = runtime.set_sink_key(id);

    runtime.prepare();

    runtime
}

pub fn get_node_test_harness_stereo(
    node: Box<dyn DynNode>,
    sr: usize,
    block_size: usize,
) -> Runtime {
    let config = Config {
        sample_rate: sr,
        block_size,
        channels: 2,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let mut runtime = build_placeholder_runtime(config, ports);

    let id = runtime.add_node(LegatoNode::new("test node".into(), "test".into(), node));

    let _ = runtime.set_sink_key(id);

    runtime.prepare();

    runtime
}
