use std::{collections::HashMap, ops::Mul, sync::Arc, time::Duration};

use arc_swap::ArcSwapOption;
use generic_array::ArrayLength;

use crate::{
    engine::{
        graph::NodeKey,
        node::{FrameSize, Node},
        port::{GetPorts, Ports},
        resources::{DelayLineKey, SampleKey, audio_sample::AudioSampleBackend},
        runtime::{Runtime, RuntimeBackend, RuntimeErased, build_runtime},
    },
    nodes::audio::{
        audio_ops::{ApplyOpMono, ApplyOpStereo},
        delay::{DelayLine, DelayReadMono, DelayReadStereo, DelayWriteMono, DelayWriteStereo},
        filters::fir::{FirMono, FirStereo},
        mixer::*,
        sampler::{SamplerMono, SamplerStereo},
        sine::{SineMono, SineStereo},
        stereo::Stereo,
        subgraph::Oversample2X,
        sweep::Sweep,
    },
};

use typenum::{Prod, U0, U1, U2};

pub enum AddNode<AF, CF>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
{
    // Osc
    SineMono {
        freq: f32,
    },
    SineStereo {
        freq: f32,
    },
    // Fan mono to stereo
    Stereo,
    // Sampler utils
    SamplerMono {
        sampler_name: String,
    },
    SamplerStereo {
        sampler_name: String,
    },
    // Delays
    DelayWriteMono {
        delay_name: String,
        delay_length: Duration,
    },
    DelayWriteStereo {
        delay_name: String,
        delay_length: Duration,
    },
    DelayReadMono {
        delay_name: String,
        offsets: Vec<Duration>,
    },
    DelayReadStereo {
        delay_name: String,
        offsets: Vec<Duration>,
    },
    // Filter
    FirMono {
        coeffs: Vec<f32>,
    },
    FirStereo {
        coeffs: Vec<f32>,
    },
    // Ops
    AddMono {
        props: f32,
    },
    AddStereo {
        props: f32,
    },
    MultMono {
        props: f32,
    },
    MultStereo {
        props: f32,
    },
    // Mixers
    StereoMixer,           // U2 -> U2
    StereoToMono,          // U2 -> U1
    TwoTrackStereoMixer,   // U4 -> U2
    FourTrackStereoMixer,  // U8 -> U2
    EightTrackStereoMixer, // U16 -> U2
    FourToMonoMixer,       // U8  -> U1
    TwoTrackMonoMixer,     // U4 -> U1
    // SvfMono,
    // SvfStereo
    // Subgraph
    Subgraph {
        runtime: Box<dyn RuntimeErased<AF, CF> + Send + 'static>,
    },
    Subgraph2XOversampled {
        runtime: Box<dyn RuntimeErased<Prod<AF, U2>, CF> + Send + 'static>,
    },
    // Utils
    Sweep {
        range: (f32, f32),
        duration: Duration,
    },
    // User defined nodes
    UserDefined {
        node: Box<dyn Node<AF, CF> + Send + 'static>,
    },
    UserDefinedFactory {
        factory: Box<dyn Fn() -> Box<dyn Node<AF, CF> + Send>>,
    },
}

pub struct RuntimeBuilder<AF, CF, C, Ci>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
    Ci: ArrayLength,
{
    runtime: Runtime<AF, CF, C, Ci>,
    delay_resource_lookup: HashMap<String, DelayLineKey>,
    sample_key_lookup: HashMap<String, SampleKey>,
    sample_backend_lookup: HashMap<String, AudioSampleBackend>,
}

impl<AF, CF, C, Ci> RuntimeBuilder<AF, CF, C, Ci>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
    Ci: ArrayLength,
{
    pub fn new(runtime: Runtime<AF, CF, C, Ci>) -> Self {
        Self {
            runtime,
            delay_resource_lookup: HashMap::default(),
            sample_key_lookup: HashMap::default(),
            sample_backend_lookup: HashMap::default(),
        }
    }
    fn get_runtime_mut(&mut self) -> &mut Runtime<AF, CF, C, Ci> {
        &mut self.runtime
    }

    // Get owned runtime value. In practice, you won't use this struct anymore after this
    pub fn get_owned(self) -> (Runtime<AF, CF, C, Ci>, RuntimeBackend) {
        (self.runtime,RuntimeBackend::new(self.sample_backend_lookup))
    }

    fn get_sample_rate(&self) -> f32 {
        self.runtime.get_sample_rate()
    }

    pub fn get_port_info(&self, node_key: &NodeKey) -> GetPorts {
        self.runtime.get_node_ports(&node_key)
    }

    // Add nodes to runtime
    pub fn add_node(&mut self, node_to_add: AddNode<AF, CF>) -> NodeKey {
        let node: Box<dyn Node<AF, CF> + Send + 'static> = match node_to_add {
            // Ops
            AddNode::AddMono { props } => Box::new(ApplyOpMono::new(|a, b| a + b, props)),
            AddNode::AddStereo { props } => Box::new(ApplyOpStereo::new(|a, b| a + b, props)),
            AddNode::MultMono { props } => Box::new(ApplyOpMono::new(|a, b| a * b, props)),
            AddNode::MultStereo { props } => Box::new(ApplyOpStereo::new(|a, b| a * b, props)),
            // Mono to stereo
            AddNode::Stereo => Box::new(Stereo::default()),
            // Mixers
            AddNode::StereoMixer => Box::new(StereoMixer::default()),
            AddNode::StereoToMono => Box::new(StereoToMonoMixer::default()),
            AddNode::FourToMonoMixer => Box::new(FourToMonoMixer::default()),
            AddNode::TwoTrackStereoMixer => Box::new(TwoTrackStereoMixer::default()),
            AddNode::FourTrackStereoMixer => Box::new(FourTrackStereoMixer::default()),
            AddNode::EightTrackStereoMixer => Box::new(EightTrackStereoMixer::default()),
            AddNode::TwoTrackMonoMixer => Box::new(TwoTrackMonoMixer::default()),
            // Filters
            AddNode::FirMono { coeffs } => Box::new(FirMono::new(coeffs)),
            AddNode::FirStereo { coeffs } => Box::new(FirStereo::new(coeffs)),
            // Osc
            AddNode::SineMono { freq } => Box::new(SineMono::new(freq, 0.0)),
            AddNode::SineStereo { freq } => Box::new(SineStereo::new(freq, 0.0)),
            // Samplers
            AddNode::SamplerMono {
                sampler_name: sample_name,
            } => {
                let sample_key = if let Some(&key) = self.sample_key_lookup.get(&sample_name) {
                    key
                } else {
                    let ctx = self.runtime.get_context_mut();

                    let data = Arc::new(ArcSwapOption::new(None));
                    let backend = AudioSampleBackend::new(data.clone());

                    self.sample_backend_lookup.insert(sample_name, backend);

                    ctx.add_sample_resource(data)
                };

                Box::new(SamplerMono::new(sample_key))
            }
            AddNode::SamplerStereo {
                sampler_name: sample_name,
            } => {
                let sample_key = if let Some(&key) = self.sample_key_lookup.get(&sample_name) {
                    key
                } else {
                    let ctx = self.runtime.get_context_mut();

                    let data = Arc::new(ArcSwapOption::new(None));
                    let backend = AudioSampleBackend::new(data.clone());

                    self.sample_backend_lookup.insert(sample_name, backend);

                    ctx.add_sample_resource(data)
                };

                Box::new(SamplerStereo::new(sample_key))
            }
            // Delay Line
            AddNode::DelayWriteMono {
                delay_name,
                delay_length,
            } => {
                let sr = self.get_sample_rate();
                let capacity = sr * delay_length.as_secs_f32();
                let delay_line = Box::new(DelayLine::<AF, U1>::new(capacity as usize));

                let ctx = self.get_runtime_mut().get_context_mut();
                let delay_key = ctx.add_delay_line(delay_line);

                self.delay_resource_lookup.insert(delay_name, delay_key);

                Box::new(DelayWriteMono::new(delay_key))
            }
            AddNode::DelayWriteStereo {
                delay_name,
                delay_length,
            } => {
                let sr = self.get_sample_rate();
                let capacity = sr * delay_length.as_secs_f32();
                let delay_line = Box::new(DelayLine::<AF, U2>::new(capacity as usize));

                let ctx = self.get_runtime_mut().get_context_mut();
                let delay_key = ctx.add_delay_line(delay_line);

                self.delay_resource_lookup.insert(delay_name, delay_key);

                Box::new(DelayWriteStereo::new(delay_key))
            }
            AddNode::DelayReadMono {
                delay_name,
                offsets,
            } => {
                let delay_key = self
                    .delay_resource_lookup
                    .get(&delay_name)
                    .expect("Delay read instantiated before line initialized");
                Box::new(DelayReadMono::new(delay_key.clone(), offsets))
            }
            AddNode::DelayReadStereo {
                delay_name,
                offsets,
            } => {
                let delay_key = self
                    .delay_resource_lookup
                    .get(&delay_name)
                    .expect("Delay read instantiated before line initialized");
                Box::new(DelayReadStereo::new(delay_key.clone(), offsets))
            }
            // Utils
            AddNode::Sweep { range, duration } => Box::new(Sweep::new(range, duration)),
            // Oversampler
            AddNode::Subgraph { runtime } => runtime,
            AddNode::Subgraph2XOversampled { runtime } => {
                Box::new(Oversample2X::<AF, CF, C>::new(runtime))
            }
            // Custom
            AddNode::UserDefined { node } => node,
            AddNode::UserDefinedFactory { factory } => factory(),
        };
        self.runtime.add_node(node)
    }
}

pub fn get_runtime_builder<AF, CF, C, Ci>(
    initial_capacity: usize,
    sample_rate: f32,
    control_rate: f32,
    ports: Ports<C, C, Ci, U0>,
) -> RuntimeBuilder<AF, CF, C, Ci>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
    Ci: ArrayLength,
{
    let runtime = build_runtime(initial_capacity, sample_rate, control_rate, ports);
    RuntimeBuilder::new(runtime)
}
