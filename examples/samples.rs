use std::{cell::UnsafeCell, sync::Arc};

use arc_swap::ArcSwapOption;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device,
};
use cpal::{BufferSize, BuildStreamError, SampleRate, StreamConfig};
use legato::{
    backend::write_data_cpal,
    engine::{
        builder::Nodes,
        graph::{Connection, ConnectionEntry},
        port::{PortRate, Stereo},
        runtime::{build_runtime, Runtime},
    },
    nodes::audio::delay::DelayLine,
};
use legato::{engine::builder::RuntimeBuilder, nodes::audio::sampler::AudioSampleBackend};

use assert_no_alloc::*;

#[cfg(debug_assertions)]
#[global_allocator]
static A: AllocDisabler = AllocDisabler;

// TODO: We configure this somewhere?

const SAMPLE_RATE: u32 = 44_000;
const BLOCK_SIZE: usize = 2048;

const DECIMATION_FACTOR: f32 = 32.0;

// 32 seems nice, we likely get a size that could have some vectorization wins?
const CONTROL_RATE: f32 = SAMPLE_RATE as f32 / DECIMATION_FACTOR;
const CONTROL_FRAME_SIZE: usize = BLOCK_SIZE / DECIMATION_FACTOR as usize;

const CAPACITY: usize = 16;
const CHANNEL_COUNT: usize = 2;

fn run<const AF: usize, const CF: usize, const C: usize>(
    device: &Device,
    config: &StreamConfig,
    mut runtime: Runtime<AF, CF, C>,
) -> Result<(), BuildStreamError> {
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
    let mut runtime: Runtime<BLOCK_SIZE, CONTROL_FRAME_SIZE, CHANNEL_COUNT> =
        build_runtime(CAPACITY, SAMPLE_RATE as f32, CONTROL_RATE);

    let data = Arc::new(ArcSwapOption::new(None));
    let backend = AudioSampleBackend::new(data.clone());

    let sampler = runtime
        .add_node_api(Nodes::SamplerStereo {
            props: data.clone(),
        })
        .expect("Could not add sampler");

    let _ = backend.load_file("./samples/amen.wav");

    let delay_line = Arc::new(UnsafeCell::new(DelayLine::<BLOCK_SIZE, Stereo>::new(
        48_000 * 1,
    )));

    let delay_write = runtime
        .add_node_api(Nodes::DelayWriteStereo {
            props: delay_line.clone(),
        })
        .unwrap();

    let delay_read = runtime
        .add_node_api(Nodes::DelayReadStereo {
            props: delay_line.clone(),
        })
        .unwrap();

    let mixer = runtime.add_node_api(Nodes::TwoTrackStereoMixer).unwrap();

    let delay_gain = runtime
        .add_node_api(Nodes::MultStereo { props: 0.8 })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: sampler,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: delay_write,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: sampler,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: delay_write,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: sampler,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: mixer,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: sampler,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: mixer,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: delay_read,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: delay_gain,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: delay_read,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: delay_gain,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: delay_gain,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: mixer,
                port_index: 2,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: delay_gain,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: mixer,
                port_index: 3,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime.set_sink_key(mixer).expect("Bad sink key!");

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
