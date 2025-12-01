use std::time::Duration;

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use legato_core_two::{nodes::{audio::sine::Sine, ports::{PortBuilder, PortRate}}, runtime::{builder::{AddNode, RuntimeBuilder, get_runtime_builder}, context::Config, graph::{Connection, ConnectionEntry}}, utils::bench_harness::get_node_test_harness};

fn bench_stereo_sine(c: &mut Criterion){
    let mut graph = get_node_test_harness(Box::new(Sine::new(440.0, 2)));

    c.bench_function("Sine node legato two", |b| {
        b.iter(|| {
            let out = graph.next_block(None);
            black_box(out);
        })
    });
}

fn bench_stereo_delay(c: &mut Criterion){
    let config = Config {
        audio_block_size: 4096,
        control_block_size: 4096 / 32,
        channels: 2,
        sample_rate: 44_100,
        control_rate: 44_100 / 32
    };

    let mut runtime_builder: RuntimeBuilder =
        get_runtime_builder(
            4,
            config,
            PortBuilder::default()
                .audio_in(2)
                .audio_out(2)
                .build()
    );

    let a = runtime_builder.add_node(AddNode::DelayWrite { chans: 2, delay_name: 'a'.into(), delay_length: Duration::from_secs_f32(1.0) });

    let b = runtime_builder.add_node(AddNode::DelayRead { chans: 2, delay_name: 'a'.into(), delay_length: vec![Duration::from_millis(120), Duration::from_millis(240)]});

    let (mut runtime, _) = runtime_builder.get_owned();

    let _ = runtime.add_edge(Connection {
        source: ConnectionEntry {
            node_key: a,
            port_index: 0,
            port_rate: PortRate::Audio
        },
        sink: ConnectionEntry { node_key: b, port_index: 0, port_rate: PortRate::Audio }
    });

    let _ = runtime.add_edge(Connection {
        source: ConnectionEntry {
            node_key: a,
            port_index: 1,
            port_rate: PortRate::Audio
        },
        sink: ConnectionEntry { node_key: b, port_index: 1, port_rate: PortRate::Audio }
    });

    let _ = runtime.set_sink_key(b);

    c.bench_function("Basic stereo delay", |b| {
        let ai: &[Box<[f32]>] = &[vec![0.0; config.audio_block_size].into(), vec![0.0; config.audio_block_size].into()];
        let ci = &[];
        b.iter(|| {
            let out = runtime.next_block(Some(&(ai, ci)));
            black_box(out);
        });
    });
}




criterion_group!(benches, bench_stereo_sine, bench_stereo_delay);
criterion_main!(benches);


