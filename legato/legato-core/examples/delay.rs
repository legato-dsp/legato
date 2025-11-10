use arc_swap::ArcSwapOption;
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{BufferSize, SampleRate, StreamConfig};
use legato_core::{
    backend::out::start_audio_thread,
    engine::{
        builder::{AddNodeResponse, AddNode},
        graph::{Connection, ConnectionEntry},
        port::{PortRate, Ports},
        runtime::{build_runtime, Runtime},
    },
    nodes::utils::port_utils::generate_audio_outputs,
};
use legato_core::{engine::builder::RuntimeBuilder, nodes::audio::sampler::AudioSampleBackend};
use std::{sync::Arc, time::Duration};

use typenum::{Unsigned, U0, U2, U2048, U64};

fn main() {
    type BlockSize = U2048;
    type ControlSize = U64;
    type ChannelCount = U2;

    const SAMPLE_RATE: u32 = 44_100;
    const CAPACITY: usize = 16;
    const DECIMATION_FACTOR: f32 = 32.0;
    const CONTROL_RATE: f32 = SAMPLE_RATE as f32 / DECIMATION_FACTOR;

    let mut runtime: Runtime<BlockSize, ControlSize, ChannelCount, U0> = build_runtime(
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

    let data = Arc::new(ArcSwapOption::new(None));
    let backend = AudioSampleBackend::new(data.clone());

    let (sampler, _) = runtime
        .add_node_api(AddNode::SamplerStereo {
            props: data.clone(),
        })
        .expect("Could not add sampler");

    backend
        .load_file("./samples/amen.wav")
        .expect("Could not load amen sample!");

    let (delay_write, delay_write_key_res) = runtime
        .add_node_api(AddNode::DelayWriteStereo {
            props: Duration::from_secs(1),
        })
        .unwrap();

    let res = delay_write_key_res.unwrap();

    let AddNodeResponse::DelayWrite(delay_key) = res;

    let (delay_read, _) = runtime
        .add_node_api(AddNode::DelayReadStereo {
            key: delay_key,
            offsets: [Duration::from_millis(12), Duration::from_millis(32)],
        })
        .unwrap();

    let (mixer, _) = runtime.add_node_api(AddNode::TwoTrackStereoMixer).unwrap();

    let (delay_gain, _) = runtime
        .add_node_api(AddNode::MultStereo { props: 0.6 })
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

    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: delay_gain,
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
                node_key: delay_gain,
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

    runtime.set_sink_key(mixer).expect("Bad sink key!");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    let device = host.default_output_device().unwrap();

    println!("{:?}", device.default_output_config());

    let config = StreamConfig {
        channels: U2::U16,
        sample_rate: SampleRate(SAMPLE_RATE),
        buffer_size: BufferSize::Fixed(BlockSize::U32),
    };

    start_audio_thread(&device, &config, runtime).expect("Runtime panic!");
}
