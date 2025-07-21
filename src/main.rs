use std::time::Duration;

use mini_graph::mini_graph::audio_graph::DynamicAudioGraph;
use mini_graph::mini_graph::write::write_data;
use mini_graph::nodes::bang::clock::Clock;
use mini_graph::nodes::audio::{adsr::ADSR, comb_filter::CombFilter, mixer:: Mixer, gain::Gain, hard_clipper::HardClipper, osc::*};
use assert_no_alloc::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, BuildStreamError, FromSample, SampleRate, SizedSample, StreamConfig};


#[cfg(debug_assertions)] // required when disable_release is set (default)
#[global_allocator]
static A: AllocDisabler = AllocDisabler;

const SAMPLE_RATE: u32 = 48_000;
const FRAME_SIZE: usize = 1024;
const CHANNEL_COUNT: usize = 2;

fn run<const N: usize, T>(device: &cpal::Device, config: &cpal::StreamConfig) -> Result<(), BuildStreamError>
where
    T: SizedSample + FromSample<f64>,
{
    let mut audio_graph = DynamicAudioGraph::<FRAME_SIZE, CHANNEL_COUNT>::with_capacity(32);

    let clock_id = audio_graph.add_node(Box::new(Clock::new(SAMPLE_RATE, Duration::from_secs_f32(1.0))));

    let clock_two = audio_graph.add_node(Box::new(Clock::new(SAMPLE_RATE, Duration::from_secs_f32(1.0 / 3.0))));

    let osc_id = audio_graph.add_node(Box::new(Oscillator::new(440.0, SAMPLE_RATE, 0.0, Wave::SinWave)));

    let osc_two = audio_graph.add_node(Box::new(Oscillator::new(880.0, SAMPLE_RATE, 0.0, Wave::SinWave)));

    let adsr_id = audio_graph.add_node(Box::new(ADSR::new(SAMPLE_RATE)));

    let adsr_two = audio_graph.add_node(Box::new(ADSR::new(SAMPLE_RATE)));

    audio_graph.add_edge(osc_two, adsr_two);

    audio_graph.add_edge(osc_id, adsr_id);

    audio_graph.add_edge(clock_id, adsr_id);

    audio_graph.add_edge(clock_two, adsr_two);

    let gain = audio_graph.add_node(Box::new(Gain::new(0.3)));

    let mixer = audio_graph.add_node(Box::new(Mixer {}));

    audio_graph.add_edge(adsr_id, mixer);

    audio_graph.add_edge(adsr_two, mixer);

    audio_graph.add_edge(mixer, gain);

    audio_graph.set_sink_index(gain);

    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            // assert_no_alloc( || write_data::<FRAME_SIZE, CHANNEL_COUNT, f32>(data, &mut audio_graph))
            write_data::<FRAME_SIZE, CHANNEL_COUNT, f32>(data, &mut audio_graph)
        },
        |err| eprintln!("An output stream error occured: {}", err),
        None,
    )?;

    stream.play().unwrap();

    std::thread::park();

    Ok(())
}


fn main(){
    
    let host = cpal::host_from_id(cpal::HostId::Jack)
    .expect("JACK host not available");

    let device = host.default_output_device().unwrap();

    let config = StreamConfig {
        channels: CHANNEL_COUNT as u16,
        sample_rate: SampleRate(SAMPLE_RATE),
        buffer_size: BufferSize::Fixed(FRAME_SIZE as u32),
    };

    run::<FRAME_SIZE, f32>(&device, &config.into()).unwrap();

    std::thread::park();
}