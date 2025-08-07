use mini_graph::mini_graph::audio_graph::{AddNodeProps, AudioGraphApi, DynamicAudioGraph};
use mini_graph::mini_graph::write::write_data;
use mini_graph::nodes::audio::filters::FilterType;
use mini_graph::nodes::audio::osc::Wave;

use assert_no_alloc::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, BuildStreamError, FromSample, SampleRate, SizedSample, StreamConfig};

#[cfg(debug_assertions)]
#[global_allocator]
static A: AllocDisabler = AllocDisabler;

const SAMPLE_RATE: u32 = 48_000;
const FRAME_SIZE: usize = 1024;
const CHANNEL_COUNT: usize = 2;

fn run<T>(device: &cpal::Device, config: &cpal::StreamConfig) -> Result<(), BuildStreamError>
where
    T: SizedSample + FromSample<f64>,
{
    let mut audio_graph = DynamicAudioGraph::<FRAME_SIZE, CHANNEL_COUNT>::with_capacity(4);

    let osc_id = audio_graph.add_audio_unit(AddNodeProps::Oscillator { freq: 440.0, sample_rate: SAMPLE_RATE, phase: 0.0, wave: Wave::SinWave });

    audio_graph.set_sink_index(osc_id);

    let mut audio_graph_two = DynamicAudioGraph::<FRAME_SIZE, CHANNEL_COUNT>::with_capacity(4);

    let lfo_id = audio_graph_two.add_audio_unit(AddNodeProps::Lfo { freq: 1.0, offset: 2400.0, amp: 1600.0 , phase: 0.0, sample_rate: SAMPLE_RATE as f32});

    let filter_id = audio_graph_two.add_audio_unit(AddNodeProps::Filter { sample_rate: SAMPLE_RATE as f32, filter_type: FilterType::LowPass, cutoff: 2400.0, gain: 0.6, q: 0.3 });

    audio_graph_two.add_edge(lfo_id, filter_id);

    let graph_id = audio_graph_two.add_audio_unit(AddNodeProps::Graph { graph: audio_graph });

    audio_graph_two.add_edge(graph_id, filter_id);

    audio_graph_two.set_sink_index(filter_id);

    // Build CPAL output stream
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            assert_no_alloc(|| write_data::<FRAME_SIZE, CHANNEL_COUNT, f32>(data, &mut audio_graph_two))
        },
        |err| eprintln!("An output stream error occurred: {}", err),
        None,
    )?;

    stream.play().unwrap();

    std::thread::park(); // Keep alive
    Ok(())
}

fn main() {
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");
    let device = host.default_output_device().expect("No output device available");

    let config = StreamConfig {
        channels: CHANNEL_COUNT as u16,
        sample_rate: SampleRate(SAMPLE_RATE),
        buffer_size: BufferSize::Fixed(FRAME_SIZE as u32),
    };

    run::<f32>(&device, &config.into()).unwrap();
}
