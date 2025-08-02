use mini_graph::mini_graph::audio_graph::DynamicAudioGraph;
use mini_graph::mini_graph::write::write_data;
use mini_graph::nodes::audio::filters::{Svf, FilterType};
use mini_graph::nodes::audio::{osc::*};
use assert_no_alloc::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, BuildStreamError, FromSample, SampleRate, SizedSample, StreamConfig};
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
    let mut audio_graph_one = DynamicAudioGraph::<FRAME_SIZE, CHANNEL_COUNT>::with_capacity(32);

    let osc_id = audio_graph_one.add_node(Box::new(Oscillator::new(440.0, SAMPLE_RATE, 0.0, Wave::SinWave)));

    audio_graph_one.set_sink_index(osc_id);


    let mut audio_graph_two = DynamicAudioGraph::<FRAME_SIZE, CHANNEL_COUNT>::with_capacity(32);

    let input_graph = audio_graph_two.add_node(Box::new(audio_graph_one));

    let lfo = audio_graph_two.add_node(Box::new(Lfo::new(4.0 , 200.0, 4800.0, 0.0, SAMPLE_RATE as f32)));

    let filter = audio_graph_two.add_node(Box::new(Svf::new(SAMPLE_RATE as f32, FilterType::LowPass, 1200.0, 1.0, 0.4)));

    audio_graph_two.add_edge(input_graph, filter);

    audio_graph_two.add_edge(lfo, filter);

    audio_graph_two.set_sink_index(filter);

    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            assert_no_alloc( || write_data::<FRAME_SIZE, CHANNEL_COUNT, f32>(data, &mut audio_graph_two))
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