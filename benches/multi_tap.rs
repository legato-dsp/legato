use criterion::{criterion_group, criterion_main, Criterion};
use mini_graph::mini_graph::audio_graph::{DynamicAudioGraph, AudioGraphApi, AddNodeProps};
use mini_graph::mini_graph::bang::Bang;
use mini_graph::mini_graph::write::write_data;
use mini_graph::mini_graph::buffer::Buffer;
use mini_graph::nodes::audio::osc::Wave;

const CHANNEL_COUNT: usize = 2;
const FRAME_SIZE: usize = 1024;
const SAMPLE_RATE: u32 = 48_000;

fn make_full_graph() -> DynamicAudioGraph<FRAME_SIZE, CHANNEL_COUNT> {
    let mut audio_graph = DynamicAudioGraph::<FRAME_SIZE, CHANNEL_COUNT>::with_capacity(32);

    let clock_one = audio_graph.add_audio_unit(AddNodeProps::Clock {
        sample_rate: SAMPLE_RATE,
        rate: std::time::Duration::from_secs_f32(0.5),
    });

    let clock_two = audio_graph.add_audio_unit(AddNodeProps::Clock {
        sample_rate: SAMPLE_RATE,
        rate: std::time::Duration::from_secs_f32(2.0 / 3.0),
    });

    let iterator_one = audio_graph.add_audio_unit(AddNodeProps::Iter {
        values: &[Bang::BangF32(440.0 / 2.0), Bang::BangF32(523.251 / 2.0), Bang::BangF32(783.991 / 2.0)],
    });

    let iterator_two = audio_graph.add_audio_unit(AddNodeProps::Iter {
        values: &[Bang::BangF32(440.0 * 2.0), Bang::BangF32(523.251 * 2.0), Bang::BangF32(783.991 * 2.0)],
    });

    let adsr_one = audio_graph.add_audio_unit(AddNodeProps::ADSR {
        sample_rate: SAMPLE_RATE,
    });

    let adsr_two = audio_graph.add_audio_unit(AddNodeProps::ADSR {
        sample_rate: SAMPLE_RATE,
    });

    let osc_one = audio_graph.add_audio_unit(AddNodeProps::Oscillator {
        freq: 440.0,
        sample_rate: SAMPLE_RATE,
        phase: 0.0,
        wave: Wave::SawWave,
    });

    let osc_two = audio_graph.add_audio_unit(AddNodeProps::Oscillator {
        freq: 440.0,
        sample_rate: SAMPLE_RATE,
        phase: 0.0,
        wave: Wave::SawWave,
    });

    audio_graph.add_edges(&[
        (clock_one, iterator_one),
        (iterator_one, osc_one),
        (osc_one, adsr_one),
        (clock_one, adsr_one),

        (clock_two, iterator_two),
        (iterator_two, osc_two),
        (osc_two, adsr_two),
        (clock_two, adsr_two),
    ]);

    let mixer = audio_graph.add_audio_unit(AddNodeProps::Mixer);
    audio_graph.add_edges(&[(adsr_one, mixer), (adsr_two, mixer)]);

    let delay_name = "delay_bus";
    let delay_capacity = (SAMPLE_RATE * 2) as usize;

    let write_one = audio_graph.add_audio_unit(AddNodeProps::DelayWrite {
        delay_line_name: delay_name,
        capacity: delay_capacity,
        name: delay_name,
    });

    let tap_one = audio_graph.add_audio_unit(AddNodeProps::DelayTap {
        name: delay_name,
        sample_offset: (2.0 / 3.0) * SAMPLE_RATE as f32,
        gain: 0.8,
    });

    let tap_two = audio_graph.add_audio_unit(AddNodeProps::DelayTap {
        name: delay_name,
        sample_offset: 1.0 * SAMPLE_RATE as f32,
        gain: 0.6,
    });

    let tap_three = audio_graph.add_audio_unit(AddNodeProps::DelayTap {
        name: delay_name,
        sample_offset: (3.0 / 2.0) * SAMPLE_RATE as f32,
        gain: 0.4,
    });

    let mixer_two = audio_graph.add_audio_unit(AddNodeProps::Mixer);
    audio_graph.add_edges(&[
        (mixer, mixer_two),
        (mixer, write_one),
        (tap_one, mixer_two),
        (tap_two, mixer_two),
        (tap_three, mixer_two),
    ]);

    audio_graph.set_sink_index(mixer_two);
    audio_graph
}

fn bench_complex_patch(c: &mut Criterion) {
    let mut graph = make_full_graph();
    let mut buffer = Buffer::<FRAME_SIZE>::default();

    c.bench_function("complex_audio_patch", |b| {
        b.iter(|| {
            write_data(&mut buffer, &mut graph);
        });
    });
}

criterion_group!(benches, bench_complex_patch);
criterion_main!(benches);
