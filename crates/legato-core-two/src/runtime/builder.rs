use std::{collections::HashMap, sync::Arc, time::Duration};

use arc_swap::ArcSwapOption;

use crate::{
    nodes::{
        Node,
        audio::{
            delay::{DelayLine, DelayRead, DelayWrite},
            mixer::TrackMixer,
            ops::{ApplyOpKind, mult_node_factory},
            sampler::Sampler,
            sine::Sine,
        },
        ports::Ports,
    },
    runtime::{
        context::Config,
        graph::NodeKey,
        resources::{DelayLineKey, SampleKey, audio_sample::AudioSampleBackend},
        runtime::{Runtime, RuntimeBackend, build_runtime},
    },
};

pub enum AddNode {
    // Osc
    Sine {
        freq: f32,
        chans: usize,
    },
    // Sampler
    Sampler {
        sampler_name: String,
        chans: usize,
    },
    // Apply Op
    Add {
        val: f32,
        chans: usize,
    },
    Subtract {
        val: f32,
        chans: usize,
    },
    Mult {
        val: f32,
        chans: usize,
    },
    Div {
        val: f32,
        chans: usize,
    },
    Gain {
        val: f32,
        chans: usize,
    },
    // Delay Lines
    DelayWrite {
        delay_name: String,
        delay_length: Duration,
        chans: usize,
    },
    DelayRead {
        delay_name: String,
        delay_length: Vec<Duration>,
        chans: usize,
    },
    // Mixers
    TrackMixer {
        chans_per_track: usize,
        tracks: usize,
        gain: Vec<f32>,
    },
}

pub struct RuntimeBuilder {
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
        (
            self.runtime,
            RuntimeBackend::new(self.sample_backend_lookup),
        )
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
            AddNode::Sampler {
                sampler_name,
                chans,
            } => {
                let sample_key = if let Some(&key) = self.sample_key_lookup.get(&sampler_name) {
                    key
                } else {
                    let ctx = self.runtime.get_context_mut();

                    let data = Arc::new(ArcSwapOption::new(None));
                    let backend = AudioSampleBackend::new(data.clone());

                    self.sample_backend_lookup.insert(sampler_name, backend);

                    ctx.get_resources_mut().add_sample_resource(data)
                };

                Box::new(Sampler::new(sample_key, chans))
            }
            AddNode::Add { val, chans } => {
                Box::new(mult_node_factory(val, chans, ApplyOpKind::Add))
            }
            AddNode::Subtract { val, chans } => {
                Box::new(mult_node_factory(val, chans, ApplyOpKind::Subtract))
            }
            AddNode::Mult { val, chans } => {
                Box::new(mult_node_factory(val, chans, ApplyOpKind::Mult))
            }
            AddNode::Div { val, chans } => {
                Box::new(mult_node_factory(val, chans, ApplyOpKind::Div))
            }
            AddNode::Gain { val, chans } => {
                Box::new(mult_node_factory(val, chans, ApplyOpKind::Gain))
            }
            AddNode::DelayWrite {
                delay_name,
                delay_length,
                chans,
            } => {
                let sr = self.get_sample_rate() as f32;
                let capacity = sr * delay_length.as_secs_f32();
                let delay_line = DelayLine::new(capacity as usize, chans);

                let ctx = self.get_runtime_mut().get_context_mut();
                let delay_key = ctx.get_resources_mut().add_delay_line(delay_line);

                self.delay_resource_lookup.insert(delay_name, delay_key);

                Box::new(DelayWrite::new(delay_key, chans))
            }
            AddNode::DelayRead {
                delay_name,
                delay_length,
                chans,
            } => {
                let delay_line_key = self
                    .delay_resource_lookup
                    .get(&delay_name)
                    .expect("Delay read instantiated before line initialized");
                Box::new(DelayRead::new(chans, delay_line_key.clone(), delay_length))
            }
            AddNode::TrackMixer {
                chans_per_track,
                tracks,
                gain,
            } => Box::new(TrackMixer::new(chans_per_track, tracks, gain)),
        };
        self.runtime.add_node(node)
    }
}

pub fn get_runtime_builder(
    initial_capacity: usize,
    config: Config,
    ports: Ports,
) -> RuntimeBuilder {
    config.validate();
    let runtime = build_runtime(initial_capacity, config, ports);
    RuntimeBuilder::new(runtime)
}
