use legato::{backend::write_data_cpal, engine::{graph::Connection, runtime::{build_runtime, Nodes, Runtime, RuntimeBuilder}}};
use cpal::{traits::{DeviceTrait, HostTrait, StreamTrait}, Device};
use cpal::{BufferSize, BuildStreamError, SampleRate, StreamConfig};

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_SIZE: usize = 1024;
const CAPACITY: usize = 16;
const CHANNEL_COUNT: usize = 2;

fn run<const N: usize, const C: usize>(device: &Device, config: &StreamConfig, mut runtime: Runtime<N, C>) -> Result<(), BuildStreamError> {
    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            write_data_cpal::<N, C, f32>(data, &mut runtime)
        },
        |err| eprintln!("An output stream error occurred: {}", err),
        None,
    )?;

    stream.play().unwrap();

    std::thread::park();

    Ok(())
}


fn main(){
    let mut runtime: Runtime::<BLOCK_SIZE, CHANNEL_COUNT> = build_runtime(CAPACITY, SAMPLE_RATE);

    let a = runtime.add_node_api(Nodes::Osc);

    let b = runtime.add_node_api(Nodes::Stereo);

    let _ = runtime.add_edge(Connection { source_key: a, sink_key: b, source_port_index: 0, sink_port_index: 0 });

    runtime.set_sink_key(b).expect("Bad sink key!");

    let host = cpal::host_from_id(cpal::HostId::Jack)
        .expect("JACK host not available");

    let device = host.default_output_device().unwrap();

    let config = StreamConfig {
        channels: CHANNEL_COUNT as u16,
        sample_rate: SampleRate(SAMPLE_RATE),
        buffer_size: BufferSize::Fixed(BLOCK_SIZE as u32),
    };

    run(&device, &config, runtime).expect("Runtime panic!");
}