use mini_graph::{mini_graph::{audio_graph::{AddNodeProps, AudioGraphApi, DynamicAudioGraph}, write::write_data}, nodes::audio::sampler::build_audio_sampler};


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
    T: SizedSample + FromSample<f64> {

    let mut audio_graph = DynamicAudioGraph::<FRAME_SIZE, CHANNEL_COUNT>::with_capacity(4);

    let (sampler, backend) = build_audio_sampler(true);

    let _ = backend.load_file("./samples/amen.wav");

    let sink = audio_graph.add_audio_unit(AddNodeProps::Sampler { node: sampler });

    let comb_filter = audio_graph.add_audio_unit(AddNodeProps::CombFilter { delay_len: 120, feedback: 0.8 });

    audio_graph.add_edge(sink, comb_filter);

    audio_graph.set_sink_index(comb_filter);

    // Build CPAL output stream
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            assert_no_alloc(|| write_data::<FRAME_SIZE, CHANNEL_COUNT, f32>(data, &mut audio_graph))
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