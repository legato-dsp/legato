use std::{collections::HashMap, sync::Arc};

use arc_swap::ArcSwapOption;

use crate::{nodes::{Node, audio::{sampler::Sampler, sine::Sine}, ports::Ports}, runtime::{context::Config, graph::NodeKey, resources::{DelayLineKey, SampleKey, audio_sample::AudioSampleBackend}, runtime::{Runtime, RuntimeBackend, build_runtime}}};

pub enum AddNode
{
    // Osc
    Sine {
        freq: f32,
        chans: usize
    },
    Sampler {
        sampler_name: String,
        chans: usize
    }
    // // Fan mono to stereo
    // Stereo,
    // // Sampler utils
    // SamplerMono {
    //     sampler_name: String,
    // },
    // SamplerStereo {
    //     sampler_name: String,
    // },
    // // Delays
    // DelayWriteMono {
    //     delay_name: String,
    //     delay_length: Duration,
    // },
    // DelayWriteStereo {
    //     delay_name: String,
    //     delay_length: Duration,
    // },
    // DelayReadMono {
    //     delay_name: String,
    //     offsets: Vec<Duration>,
    // },
    // DelayReadStereo {
    //     delay_name: String,
    //     offsets: Vec<Duration>,
    // },
    // // Filter
    // FirMono {
    //     coeffs: Vec<f32>,
    // },
    // FirStereo {
    //     coeffs: Vec<f32>,
    // },
    // // Ops
    // AddMono {
    //     props: f32,
    // },
    // AddStereo {
    //     props: f32,
    // },
    // MultMono {
    //     props: f32,
    // },
    // MultStereo {
    //     props: f32,
    // },
    // // Mixers
    // StereoMixer,           // U2 -> U2
    // StereoToMono,          // U2 -> U1
    // TwoTrackStereoMixer,   // U4 -> U2
    // FourTrackStereoMixer,  // U8 -> U2
    // EightTrackStereoMixer, // U16 -> U2
    // FourToMonoMixer,       // U8  -> U1
    // TwoTrackMonoMixer,     // U4 -> U1
    // // SvfMono,
    // // SvfStereo
    // // Subgraph
    // Subgraph {
    //     runtime: Box<dyn RuntimeErased<AF, CF> + Send + 'static>,
    // },
    // Subgraph2XOversampled {
    //     runtime: Box<dyn RuntimeErased<Prod<AF, U2>, CF> + Send + 'static>,
    // },
    // // Utils
    // Sweep {
    //     range: (f32, f32),
    //     duration: Duration,
    // },
    // // User defined nodes
    // UserDefined {
    //     node: Box<dyn Node<AF, CF> + Send + 'static>,
    // },
    // UserDefinedFactory {
    //     factory: Box<dyn Fn() -> Box<dyn Node<AF, CF> + Send>>,
    // },
}

pub struct RuntimeBuilder
{
    runtime: Runtime,
    delay_resource_lookup: HashMap<String, DelayLineKey>,
    sample_key_lookup: HashMap<String, SampleKey>,
    sample_backend_lookup: HashMap<String, AudioSampleBackend>,
}

impl RuntimeBuilder {
    pub fn new(runtime: Runtime) -> Self {
        Self {
            runtime,
            delay_resource_lookup: HashMap::default(),
            sample_key_lookup: HashMap::default(),
            sample_backend_lookup: HashMap::default(),
        }
    }
    fn get_runtime_mut(&mut self) -> &mut Runtime {
        &mut self.runtime
    }

    // Get owned runtime value. In practice, you won't use this struct anymore after this
    pub fn get_owned(self) -> (Runtime, RuntimeBackend) {
        (self.runtime,RuntimeBackend::new(self.sample_backend_lookup))
    }

    fn get_sample_rate(&self) -> usize {
        self.runtime.get_config().sample_rate
    }

    pub fn get_port_info(&self, node_key: &NodeKey) -> &Ports {
        self.runtime.get_node_ports(&node_key)
    }

    // Add nodes to runtime
    pub fn add_node(&mut self, node_to_add: AddNode) -> NodeKey {

        let node: Box<dyn Node + Send> = match node_to_add {
            AddNode::Sine { freq, chans } => Box::new(Sine::new(freq, chans)),
            AddNode::Sampler { sampler_name, chans } => {
                let sample_key = if let Some(&key) = self.sample_key_lookup.get(&sampler_name) {
                    key
                } else {
                    let ctx = self.runtime.get_context_mut();

                    let data = Arc::new(ArcSwapOption::new(None));
                    let backend = AudioSampleBackend::new(data.clone());

                    self.sample_backend_lookup.insert(sampler_name, backend);

                    ctx.add_sample_resource(data)
                };

                Box::new(Sampler::new(sample_key, chans))
            } 
        };
        self.runtime.add_node(node)
    }
}

pub fn get_runtime_builder(initial_capacity: usize, config: Config, ports: Ports)-> RuntimeBuilder {
    config.validate();
    let runtime = build_runtime(initial_capacity, config, ports);
    RuntimeBuilder::new(runtime)
}
