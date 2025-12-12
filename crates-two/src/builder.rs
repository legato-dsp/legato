
use std::{collections::HashMap, sync::{Arc, atomic::AtomicU64}};

use arc_swap::ArcSwapOption;

use crate::{ValidationError, config::Config, graph::{Connection, ConnectionEntry}, nodes::audio::{delay::DelayLine, mixer::{MonoFanOut, TrackMixer}}, params::Params, ports::{PortRate, Ports}, registry::AudioRegistry, resources::{DelayLineKey, Resources, SampleKey}, runtime::{NodeKey, Runtime, RuntimeBackend, build_runtime}, sample::{AudioSampleBackend, AudioSampleHandle}, spec::NodeSpec };



pub struct AddConnectionProps {
    source: NodeKey,
    source_kind: AddConnectionKind,
    sink: NodeKey,
    sink_kind: AddConnectionKind,
    rate: PortRate // Determines whether or not we look for control or audio matches
}


pub enum AddConnectionKind {
    Index(usize),
    Named(&'static str),
    Auto
}

/// A small slice of the runtime exposed for nodes in their node factories.
/// 
/// This is useful to say reserve delay lines, or other shared logic.
pub struct ResourceBuilderView<'a> {
    pub config: &'a Config,
    pub resources: &'a mut Resources,
    pub sample_keys: &'a mut HashMap<String, SampleKey>,
    pub delay_keys:  &'a mut HashMap<String, DelayLineKey>,
    pub sample_backends: &'a mut HashMap<String, AudioSampleBackend>
}

impl<'a> ResourceBuilderView<'a> {
    pub fn add_delay_line(&mut self, name: &String, delay_line: DelayLine) -> DelayLineKey {
        let key = self.resources.add_delay_line(delay_line);
        self.delay_keys.insert(name.clone(), key);

        key
    }

    pub fn get_delay_line_key(&self, name: &String) -> Result<DelayLineKey, ValidationError> {
        self.delay_keys.get(name).cloned().ok_or_else(|| ValidationError::ResourceNotFound(format!("Could not find delay key {}", name)))
    }

    pub fn add_sampler(&mut self, name: &String) -> SampleKey {
        let sample_key = if let Some(&key) = self.sample_keys.get(name) {
            key
        } else {
            let data = ArcSwapOption::new(None);
            
            let handle = Arc::new(AudioSampleHandle {
                sample: data,
                sample_version: AtomicU64::new(0)   
            });

            let backend = AudioSampleBackend::new(handle.clone());
                
            self.sample_backends.insert(name.clone(), backend);

            self.resources.add_sample_resource(handle)
        };
        sample_key
    }

    pub fn get_sampler_key(&self, name: &String) -> Result<SampleKey, ValidationError> {
        self.sample_keys.get(name).cloned().ok_or_else(|| ValidationError::ResourceNotFound(format!("Could not find sample key {}", name)))
    }

    pub fn get_config(&self) -> &Config {
        &self.config
    }
}





/// The legato application builder.
pub struct LegatoBuilder {
    // Namespaces are collections of registries, e.g a namespace "reverb" might contain a custom reverb alg.
    namespaces: HashMap<String, AudioRegistry>,
    // Nodes can have a default/working name or alias. This map keeps track of that and maps to the actual node key.
    working_name_to_key: HashMap<String, NodeKey>,
    // The actual runtime being built
    runtime: Runtime,
    // Resources being built. These can be pased to node factories
    resources: Resources,
    // Name to key maps
    sample_name_to_key: HashMap<String, SampleKey>,
    delay_name_to_key:  HashMap<String, DelayLineKey>,
    sample_backends: HashMap<String, AudioSampleBackend>
}

impl LegatoBuilder {
    pub fn new(config: Config, ports: Ports) -> Self {
        let mut namespaces = HashMap::new();
        let audio_registry = AudioRegistry::default();
        namespaces.insert("audio".into(), audio_registry);
        namespaces.insert("user".into(), AudioRegistry::new());

        let runtime = build_runtime(config, ports);

        Self {
            namespaces,
            working_name_to_key: HashMap::new(),
            runtime,
            resources: Resources::default(),
            sample_name_to_key: HashMap::new(),
            delay_name_to_key: HashMap::new(),
            sample_backends: HashMap::new()
        }

    }

    pub fn add_node(&mut self, namespace: &String, name: &String, alias: Option<&String>, params: Option<&Params>) -> Result<NodeKey, ValidationError> {
        let ns = self.namespaces.get(namespace).ok_or_else(|| ValidationError::NamespaceNotFound(format!("Could not find namespace {}", namespace)))?;
        
        let mut resource_builder_view = ResourceBuilderView {
            config: &self.runtime.get_config(),
            resources: &mut self.resources,
            sample_keys: &mut self.sample_name_to_key,
            delay_keys: &mut self.delay_name_to_key,
            sample_backends: &mut self.sample_backends
        };

        let node = ns.get_node(&mut resource_builder_view, name, params).map_err(|_| ValidationError::NodeNotFound(format!("Could not find node {}", name)))?;

        let working_name = alias.map_or(name.clone(), |inner| inner.clone());

        let key = self.runtime.add_node(node, working_name.clone(), name.clone());

        self.working_name_to_key.insert(working_name, key.clone());

        Ok(key)
    }

    pub fn add_connection(&mut self, connection: AddConnectionProps){
        let source_indicies: Vec<usize> = match connection.source_kind  {
            AddConnectionKind::Auto => {
                let ports = self.runtime.get_node_ports(&connection.source);
                let indicies = match connection.rate {
                    PortRate::Audio =>  ports.audio_out.iter().enumerate().map(|(i, _)| i).collect(),
                    PortRate::Control =>  ports.control_out.iter().enumerate().map(|(i, _)| i).collect()
                };
                indicies
            }
            AddConnectionKind::Index(i) => vec![i],
            AddConnectionKind::Named(name) => {
                let ports = self.runtime.get_node_ports(&connection.source);
                let index = match connection.rate {
                    PortRate::Audio => ports.audio_out.iter().find(|x| x.name == name),
                    PortRate::Control => ports.control_out.iter().find(|x| x.name == name)
                }.expect(&format!("Could not find index for named port {}", name)).index;
                
                vec![index]
            },
        }; 

        let sink_indicies: Vec<usize> = match connection.sink_kind  {
            AddConnectionKind::Auto => {
                let ports = self.runtime.get_node_ports(&connection.source);
                let indicies = match connection.rate {
                    PortRate::Audio =>  ports.audio_in.iter().enumerate().map(|(i, _)| i).collect(),
                    PortRate::Control =>  ports.control_in.iter().enumerate().map(|(i, _)| i).collect()
                };
                indicies
            }
            AddConnectionKind::Index(i) => vec![i],
            AddConnectionKind::Named(name) => {
                let ports = self.runtime.get_node_ports(&connection.source);
                let index = match connection.rate {
                    PortRate::Audio => ports.audio_in.iter().find(|x| x.name == name),
                    PortRate::Control => ports.control_in.iter().find(|x| x.name == name)
                }.expect(&format!("Could not find index for named port {}", name)).index;
                
                vec![index]
            },
        }; 

        let source_arity = source_indicies.len();
        let sink_arity = sink_indicies.len();

        match (source_arity, sink_arity) {
            (1, 1) => one_to_one(&mut self.runtime, connection, source_indicies[0], sink_indicies[0]),
            (1, n) if n >= 1 => one_to_n(&mut self.runtime, connection, source_indicies[0], sink_indicies.as_slice()),
            (n, 1) if n >= 1 => n_to_one(&mut self.runtime, connection, source_indicies.as_slice(), sink_indicies[0]),
            (n, m) if n == m => n_to_n(&mut self.runtime, connection, &source_indicies, &sink_indicies),
            (n, m) => unimplemented!("Cannot match request arity {}:{}", n, m),
        }
    }

    pub fn add_registry(&mut self, name: &String, registry: AudioRegistry) {
        self.namespaces.insert(name.clone(), registry);
    }

    pub fn register_node(&mut self, namespace: &String, spec: NodeSpec) -> Result<(), ValidationError>{
        if let Some(ns) = self.namespaces.get_mut(namespace){
            ns.declare_node(spec);
            return Ok(());
        }
        Err(ValidationError::NamespaceNotFound(format!("Could not find namespace {}", namespace)))
    }

    pub fn build(self) -> (Runtime, RuntimeBackend) {
        (
            self.runtime,
            RuntimeBackend::new(self.sample_backends)
        )
    }
}

// A number of utility functions to handle explicit port arity

fn one_to_one(runtime: &mut Runtime, props: AddConnectionProps, source_index: usize, sink_index: usize) {
    runtime.add_edge(Connection { 
        source: ConnectionEntry { node_key: props.source, port_index: source_index, port_rate: props.rate }, 
        sink: ConnectionEntry { node_key: props.sink, port_index: sink_index, port_rate: props.rate } }).expect("Could not add edge");
}

fn one_to_n(runtime: &mut Runtime, props: AddConnectionProps, source_index: usize, sink_indicies: &[usize]){
    let n = sink_indicies.len();

    // Fanout mixer going from 1 -> n
    let mixer = runtime.add_node(Box::new(MonoFanOut::new(n)), format!("MonoFanOut{:?}{:?}", props.source, props.sink), "MonoFanOut".into());
    
    // Wire mono to mixer
    runtime.add_edge(Connection { 
        source: ConnectionEntry { 
            node_key: props.source, 
            port_index: source_index, 
            port_rate: props.rate }, 
        sink: ConnectionEntry { 
            node_key: mixer.clone(), 
            port_index: 0, 
            port_rate: props.rate } 
    }).expect("Could not add edge");    

    // Wire fanout connection to each sink. We add this node in order to change the gain when fanning out
    for i in 0..n{
        runtime.add_edge(Connection {
            source: ConnectionEntry {
                node_key: mixer.clone(),
                port_index: 0,
                port_rate: props.rate
            },
            sink: ConnectionEntry {
                node_key: props.sink,
                port_index: sink_indicies[i],
                port_rate: props.rate
            }
        }).expect("Could not add edge");
    }
}

fn n_to_one(runtime: &mut Runtime, props: AddConnectionProps, source_indicies: &[usize], sink_index: usize){
    let n = source_indicies.len();
    
    // Make mixer with n mono tracks
    let mixer = runtime.add_node(
        Box::new(TrackMixer::new(1, n, vec![1.0 / f32::sqrt(n as f32); n])), 
        format!("TrackMixer{:?}{:?}", props.source, props.sink), 
        "TrackMixer".into());

    // Build connections into track mixer
    for i in 0..n {
        runtime.add_edge(Connection {
            source: ConnectionEntry {
                node_key: props.source,
                port_index: source_indicies[i],
                port_rate: props.rate
            },
            sink: ConnectionEntry {
                node_key: mixer.clone(),
                port_index: i,
                port_rate: props.rate
            }
        }).expect("Could not add edge");
    }

    // Wire track mixer to sink index
    runtime.add_edge(Connection {
        source: ConnectionEntry {
            node_key: mixer.clone(),
            port_index: 0,
            port_rate: props.rate
        },
        sink: ConnectionEntry {
            node_key: props.sink,
            port_index: sink_index,
            port_rate: props.rate
        }
    }).expect("Could not add edge");
}

fn n_to_n(runtime: &mut Runtime, props: AddConnectionProps, source_indicies: &[usize], sink_indicies: &[usize]){
    assert!(source_indicies.len() == sink_indicies.len());
    source_indicies.iter().zip(sink_indicies).for_each(|(source, sink)| {
        runtime.add_edge(
            Connection { 
                source: ConnectionEntry {
                    node_key: props.source,
                    port_index: *source,
                    port_rate: props.rate
                },
                sink: ConnectionEntry {
                    node_key: props.sink,
                    port_index: *sink,
                    port_rate: props.rate
                }}
        ).expect("Could not add edge");
    });
}
