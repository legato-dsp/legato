use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device,
};
use cpal::{BufferSize, BuildStreamError, SampleRate, StreamConfig};
use generic_array::ArrayLength;
use legato::engine::{builder::RuntimeBuilder, graph::ConnectionEntry, port::PortRate};
use legato::{
    backend::write_data_cpal,
    engine::{
        builder::Nodes,
        graph::Connection,
        runtime::{build_runtime, Runtime},
    },
};

use assert_no_alloc::*;
use typenum::U2;

#[cfg(debug_assertions)]
#[global_allocator]
static A: AllocDisabler = AllocDisabler;

// TODO: We configure this somewhere?

const SAMPLE_RATE: u32 = 44_100;
const BLOCK_SIZE: usize = 2048;

const DECIMATION_FACTOR: f32 = 32.0;

// 32 seems nice, we likely get a size that could have some vectorization wins?
const CONTROL_RATE: f32 = SAMPLE_RATE as f32 / DECIMATION_FACTOR;
const CONTROL_FRAME_SIZE: usize = BLOCK_SIZE / DECIMATION_FACTOR as usize;

const CAPACITY: usize = 16;
const CHANNEL_COUNT: usize = 2;

fn run<const AF: usize, const CF: usize, C>(
    device: &Device,
    config: &StreamConfig,
    mut runtime: Runtime<AF, CF, C>,
) -> Result<(), BuildStreamError> where C: ArrayLength + Send {
    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            assert_no_alloc(|| write_data_cpal::<AF, CF, C, f32>(data, &mut runtime))
        },
        |err| eprintln!("An output stream error occurred: {}", err),
        None,
    )?;

    stream.play().unwrap();

    std::thread::park();

    Ok(())
}

fn main() {
    let mut runtime: Runtime<BLOCK_SIZE, CONTROL_FRAME_SIZE, U2> =
        build_runtime(CAPACITY, SAMPLE_RATE as f32, CONTROL_RATE);

    let (a, _) = runtime
        .add_node_api(Nodes::OscMono)
        .expect("Could not add node");

    let (b, _) = runtime
        .add_node_api(Nodes::Stereo)
        .expect("Could not add node");

    let _ = runtime.add_edge(Connection {
        source: ConnectionEntry {
            node_key: a,
            port_index: 0,
            port_rate: PortRate::Audio,
        },
        sink: ConnectionEntry {
            node_key: b,
            port_index: 0,
            port_rate: PortRate::Audio,
        },
    });

    runtime.set_sink_key(b).expect("Bad sink key!");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    let device = host.default_output_device().unwrap();

    let config = StreamConfig {
        channels: CHANNEL_COUNT as u16,
        sample_rate: SampleRate(SAMPLE_RATE as u32),
        buffer_size: BufferSize::Fixed(BLOCK_SIZE as u32),
    };

    run(&device, &config, runtime).expect("Runtime panic!");
}
