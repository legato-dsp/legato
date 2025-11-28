use criterion::{Criterion, black_box, criterion_group, criterion_main};
use legato_core_two::{nodes::audio::sine::Sine, utils::bench_harness::get_node_test_harness};

fn bench_sine(c: &mut Criterion){
    let mut graph = get_node_test_harness(Box::new(Sine::new(440.0, 2)));

    c.bench_function("Sine node legato two", |b| {
        b.iter(|| {
            let out = graph.next_block(None);
            black_box(out);
        })
    });
}


criterion_group!(benches, bench_sine);
criterion_main!(benches);


