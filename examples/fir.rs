use std::{sync::Arc};

use arc_swap::ArcSwapOption;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device,
};
use cpal::{BufferSize, BuildStreamError, SampleRate, StreamConfig};
use generic_array::ArrayLength;
use legato::{
    backend::write_data_cpal,
    engine::{
        builder::{Nodes},
        graph::{Connection, ConnectionEntry},
        port::PortRate,
        runtime::{build_runtime, Runtime},
    },
};
use legato::{engine::builder::RuntimeBuilder, nodes::audio::sampler::AudioSampleBackend};

use assert_no_alloc::*;
use typenum::U2;

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

    let data = Arc::new(ArcSwapOption::new(None));
    let backend = AudioSampleBackend::new(data.clone());

    // Would suggest using Python + numpy + scipy. In the future there should be a tool for this here.
    // Here is a cool tool, the blog post is great as well: https://fiiir.com/
    let coeffs: Vec<f32> = vec![
        0.0,
        -0.000005844052064368,
        -0.000023820134943225,
        -0.000054014799697821,
        -0.000095582479747764,
        -0.000146488652240137,
        -0.000203195486496576,
        -0.000260312240141828,
        -0.000310242125590523,
        -0.000342864778695147,
        -0.000345297969930517,
        -0.000301783157890913,
        -0.000193736507668975,
        0.0,
        0.000302683516790705,
        0.00073896146790474,
        0.0013339353699943,
        0.00211202142599331,
        0.003095649626814402,
        0.004303859403838475,
        0.005750863568281213,
        0.007444662160847579,
        0.009385792950955006,
        0.011566304999335716,
        0.013969035618708839,
        0.01656725931600208,
        0.019324760360190832,
        0.02219635935780855,
        0.025128899832021673,
        0.028062674748091803,
        0.030933246829661972,
        0.03367359204427679,
        0.03621647442207788,
        0.038496943856761,
        0.04045483789687657,
        0.042037164583643,
        0.04320024652348417,
        0.04391151654101618,
        0.044150871927461845,
        0.04391151654101618,
        0.04320024652348417,
        0.042037164583643004,
        0.04045483789687657,
        0.038496943856761,
        0.03621647442207789,
        0.03367359204427679,
        0.03093324682966199,
        0.028062674748091803,
        0.02512889983202168,
        0.022196359357808556,
        0.019324760360190832,
        0.01656725931600209,
        0.013969035618708835,
        0.011566304999335721,
        0.00938579295095501,
        0.007444662160847576,
        0.005750863568281216,
        0.004303859403838477,
        0.003095649626814402,
        0.002112021425993311,
        0.001333935369994301,
        0.000738961467904741,
        0.000302683516790706,
        0.0,
        -0.000193736507668975,
        -0.000301783157890913,
        -0.000345297969930516,
        -0.000342864778695147,
        -0.000310242125590522,
        -0.000260312240141827,
        -0.000203195486496576,
        -0.000146488652240137,
        -0.000095582479747764,
        -0.000054014799697822,
        -0.000023820134943225,
        -0.000005844052064368,
        0.0,
    ];

    let (fir, _) = runtime
        .add_node_api(Nodes::FirStereo { kernel: coeffs })
        .expect("Could not add FIR");

    let (sampler, _) = runtime
        .add_node_api(Nodes::SamplerStereo {
            props: data.clone(),
        })
        .expect("Could not add sampler");

    let _ = backend
        .load_file("./samples/amen.wav")
        .expect("Could not load amen sample!");

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: sampler,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: fir,
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
                node_key: fir,
                port_index: 1,
                port_rate: PortRate::Audio,
            },
        })
        .unwrap();

    runtime.set_sink_key(fir).expect("Bad sink key!");

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
