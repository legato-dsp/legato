use std::time::Duration;

use mini_graph::mini_graph::audio_graph::DynamicAudioGraph;
use mini_graph::mini_graph::bang::Bang;
use mini_graph::mini_graph::write::write_data;
use mini_graph::nodes::audio::gain::Gain;
use mini_graph::nodes::audio::mixer::Mixer;
use mini_graph::nodes::audio::moog::MoogFilter;
use mini_graph::nodes::bang::clock::Clock;
use mini_graph::nodes::audio::{adsr::ADSR, osc::*};
use assert_no_alloc::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, BuildStreamError, FromSample, SampleRate, SizedSample, StreamConfig};
use mini_graph::nodes::bang::iter::BangIter;


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

    let gain = audio_graph.add_node(Box::new(Gain::new(0.3)));

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

    let filter = audio_graph.add_node(Box::new(
        MoogFilter::<FRAME_SIZE, CHANNEL_COUNT>::new(SAMPLE_RATE)
    ));

    audio_graph.add_edges(&[(adsr_one, mixer), (adsr_two, mixer), (mixer, filter), (filter, gain)]);

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