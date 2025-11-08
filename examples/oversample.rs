use std::{path::Path, time::Duration};

use cpal::{
    traits::{DeviceTrait, StreamTrait},
    Device,
};
use cpal::{BuildStreamError, StreamConfig};
use generic_array::ArrayLength;
use legato::{
    backend::write_data_cpal,
    engine::{
        builder::Nodes,
        runtime::{build_runtime, Runtime},
    },
    nodes::utils::render::render,
};
use legato::{
    engine::{builder::RuntimeBuilder, port::Ports},
    nodes::utils::port_utils::generate_audio_outputs,
};

use assert_no_alloc::*;
use typenum::{U0, U1, U2};

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

fn run<const AF: usize, const CF: usize, C, Ci>(
    device: &Device,
    config: &StreamConfig,
    mut runtime: Runtime<AF, CF, C, Ci>,
) -> Result<(), BuildStreamError>
where
    C: ArrayLength + Send,
    Ci: ArrayLength + Send,
{
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            // assert_no_alloc(|| write_data_cpal::<AF, CF, C, f32>(data, &mut runtime))
            write_data_cpal(data, &mut runtime);
        },
        |err| eprintln!("An output stream error occurred: {}", err),
        None,
    )?;

    stream.play().unwrap();

    std::thread::park();

    Ok(())
}

fn main() {
    let mut runtime: Runtime<BLOCK_SIZE, CONTROL_FRAME_SIZE, U1, U0> = build_runtime(
        CAPACITY,
        SAMPLE_RATE as f32,
        CONTROL_RATE,
        Ports {
            audio_inputs: None,
            audio_outputs: Some(generate_audio_outputs()),
            control_inputs: None,
            control_outputs: None,
        },
    );

    const DOUBLE_SIZE: usize = BLOCK_SIZE * 2;

    let mut oversampled_runtime: Runtime<DOUBLE_SIZE, CONTROL_FRAME_SIZE, U1, U0> = build_runtime(
        CAPACITY,
        (SAMPLE_RATE * 2) as f32,
        CONTROL_RATE,
        Ports {
            audio_inputs: None,
            audio_outputs: Some(generate_audio_outputs()),
            control_inputs: None,
            control_outputs: None,
        },
    );

    let (a, _) = oversampled_runtime
        .add_node_api(Nodes::Sweep {
            range: (20.0, 48_000.0),
            duration: Duration::from_secs(10),
        })
        .expect("Could not add node");

    let (b, _) = runtime.set_sink_key(a).expect("Bad sink key!");

    let path = Path::new("example.wav");

    render(runtime, path, SAMPLE_RATE, Duration::from_secs(5)).unwrap();
}
