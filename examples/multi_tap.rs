use std::time::Duration;

use mini_graph::mini_graph::audio_graph::DynamicAudioGraph;
use mini_graph::mini_graph::bang::Bang;
use mini_graph::mini_graph::write::write_data;
use mini_graph::nodes::audio::delay::{DelayLine, DelayTap, DelayWrite};
use mini_graph::nodes::audio::gain::Gain;
use mini_graph::nodes::audio::hard_clipper::HardClipper;
use mini_graph::nodes::audio::mixer::Mixer;
use mini_graph::nodes::audio::filters::{Svf, FilterType};
use mini_graph::nodes::control::clock::Clock;
use mini_graph::nodes::audio::{adsr::ADSR, osc::*};
use assert_no_alloc::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, BuildStreamError, FromSample, SampleRate, SizedSample, StreamConfig};
use mini_graph::nodes::control::iter::BangIter;
use mini_graph::nodes::control::lfo::Lfo;


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

    let clock_one = audio_graph.add_node(Box::new(Clock::<FRAME_SIZE, CHANNEL_COUNT>::new(SAMPLE_RATE, Duration::from_secs_f32(0.5))));

    let clock_two = audio_graph.add_node(Box::new(Clock::<FRAME_SIZE, CHANNEL_COUNT>::new(SAMPLE_RATE, Duration::from_secs_f32(2.0 / 3.0))));

    let iterator_one = audio_graph.add_node(Box::new(BangIter::<FRAME_SIZE, CHANNEL_COUNT>::new(&[Bang::BangF32(440.0 / 2.0), Bang::BangF32(523.251 / 2.0), Bang::BangF32(783.991 / 2.0)])));

    let iterator_two = audio_graph.add_node(Box::new(BangIter::<FRAME_SIZE, CHANNEL_COUNT>::new(&[Bang::BangF32(440.0 * 2.0), Bang::BangF32(523.251 * 2.0), Bang::BangF32(783.991 * 2.0)])));

    let adsr_one = audio_graph.add_node(Box::new(ADSR::new(SAMPLE_RATE)));

    let adsr_two = audio_graph.add_node(Box::new(ADSR::new(SAMPLE_RATE)));

    let osc_one = audio_graph.add_node(Box::new(Oscillator::new(440.0, SAMPLE_RATE, 0.0, Wave::SawWave)));

    let osc_two = audio_graph.add_node(Box::new(Oscillator::new(440.0, SAMPLE_RATE, 0.0, Wave::SawWave)));

    audio_graph.add_edges(&[
        (clock_one, iterator_one), 
        (iterator_one, osc_one),
        (osc_one, adsr_one),
        (clock_one, adsr_one), 

        (clock_two, iterator_two),
        (iterator_two, osc_two),
        (osc_two, adsr_two),
        (clock_two, adsr_two)
        ]
    );

    let mixer = audio_graph.add_node(Box::new(Mixer {}));

    audio_graph.add_edges(&[(adsr_one, mixer), (adsr_two, mixer)]);

    let delay_line = DelayLine::new((SAMPLE_RATE * 2) as usize);

    let write_one = audio_graph.add_node(Box::new(DelayWrite::new(delay_line.clone())));

    let tap_one = audio_graph.add_node(Box::new(DelayTap::new(delay_line.clone(), (2.0 / 3.0) * SAMPLE_RATE as f32, 0.8)));

    let tap_two = audio_graph.add_node(Box::new(DelayTap::new(delay_line.clone(), 1.0 * SAMPLE_RATE as f32, 0.6)));

    let tap_three = audio_graph.add_node(Box::new(DelayTap::new(delay_line.clone(), (3.0 / 2.0) * SAMPLE_RATE as f32, 0.4)));

    let mixer_two = audio_graph.add_node(Box::new(Mixer {}));

    audio_graph.add_edges(&[(mixer, mixer_two),(mixer, write_one),(tap_one, mixer_two), (tap_two, mixer_two), (tap_three, mixer_two)]);

    audio_graph.set_sink_index(mixer_two);

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