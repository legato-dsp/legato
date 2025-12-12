
use std::{collections::HashMap, sync::{Arc, atomic::AtomicU64}};
use arc_swap::ArcSwapOption;
use crate::{LegatoApp, LegatoBackend, LegatoMsg, ValidationError, ast::{ExpandedNode, PortConnectionType, build_ast}, config::Config, graph::{Connection, ConnectionEntry}, node::Node, nodes::audio::{delay::DelayLine, mixer::{MonoFanOut, TrackMixer}}, params::Params, parse::parse_legato_file, pipes::PipeRegistry, ports::{PortRate, Ports}, registry::AudioRegistry, resources::{DelayLineKey, Resources, SampleKey}, runtime::{self, NodeKey, Runtime, RuntimeBackend, build_runtime}, sample::{AudioSampleBackend, AudioSampleHandle}, spec::NodeSpec };



pub struct AddConnectionProps {
    source: NodeKey,
    source_kind: PortConnectionType,
    sink: NodeKey,
    sink_kind: PortConnectionType,
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
    // A registry of pipe functions
    pipes: PipeRegistry,
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
            pipes: PipeRegistry::default(),
            runtime,
            resources: Resources::default(),
            sample_name_to_key: HashMap::new(),
            delay_name_to_key: HashMap::new(),
            sample_backends: HashMap::new()
        }

    }

    // Add a node using the params object
    pub fn add_node(&mut self, namespace: &String, name: &String, alias: &String, params: &Params) -> Result<NodeKey, ValidationError> {
        let ns = self.namespaces.get(namespace).ok_or_else(|| ValidationError::NamespaceNotFound(format!("Could not find namespace {}", namespace)))?;
        
        let mut resource_builder_view = ResourceBuilderView {
            config: &self.runtime.get_config(),
            resources: &mut self.resources,
            sample_keys: &mut self.sample_name_to_key,
            delay_keys: &mut self.delay_name_to_key,
            sample_backends: &mut self.sample_backends
        };

        let node = ns.get_node(&mut resource_builder_view, name, params).map_err(|_| ValidationError::NodeNotFound(format!("Could not find node {}", name)))?;

        let key = self.runtime.add_node(node, alias.clone(), name.clone());

        self.working_name_to_key.insert(alias.to_string(), key.clone());

        Ok(key)
    }

    /// Ignore the params and DSL ceremony and insert a node directly
    pub fn add_node_raw(&mut self, node: Box<dyn Node + Send>, name: &String, alias: Option<&String>, kind: &String) -> NodeKey {
        let working_name = alias.map_or(name.clone(), |inner| inner.clone());

        let key = self.runtime.add_node(node, working_name.clone(), kind.clone());

        self.working_name_to_key.insert(working_name, key.clone());

        key
    }

    /// Add a connection by specifying the node and connection type
    pub fn add_connection(&mut self, connection: AddConnectionProps){
        let source_indicies: Vec<usize> = match connection.source_kind  {
            PortConnectionType::Auto => {
                let ports = self.runtime.get_node_ports(&connection.source);
                let indicies = match connection.rate {
                    PortRate::Audio =>  ports.audio_out.iter().enumerate().map(|(i, _)| i).collect(),
                    PortRate::Control =>  ports.control_out.iter().enumerate().map(|(i, _)| i).collect()
                };
                indicies
            }
            PortConnectionType::Indexed {port} => vec![port],
            PortConnectionType::Named {ref port} => {
                let ports = self.runtime.get_node_ports(&connection.source);
                let index = match connection.rate {
                    PortRate::Audio => ports.audio_out.iter().find(|x| x.name == port),
                    PortRate::Control => ports.control_out.iter().find(|x| x.name == port)
                }.expect(&format!("Could not find index for named port {}", port)).index;
                
                vec![index]
            },
            PortConnectionType::Slice { start, end } => {
                if end < start {
                    panic!("End slice cannot be less than start!");
                }

                (start..end).collect::<Vec<_>>()
            }
        }; 

        let sink_indicies: Vec<usize> = match connection.sink_kind  {
            PortConnectionType::Auto => {
                let ports = self.runtime.get_node_ports(&connection.source);
                let indicies = match connection.rate {
                    PortRate::Audio =>  ports.audio_in.iter().enumerate().map(|(i, _)| i).collect(),
                    PortRate::Control =>  ports.control_in.iter().enumerate().map(|(i, _)| i).collect()
                };
                indicies
            }
            PortConnectionType::Indexed { port } => vec![port],
            PortConnectionType::Named { ref port } => {
                let ports = self.runtime.get_node_ports(&connection.source);
                let index = match connection.rate {
                    PortRate::Audio => ports.audio_in.iter().find(|x| x.name == port),
                    PortRate::Control => ports.control_in.iter().find(|x| x.name == port)
                }.expect(&format!("Could not find index for named port {}", port)).index;
                
                vec![index]
            },
            PortConnectionType::Slice { start, end } => {
                if end < start {
                    panic!("End slice cannot be less than start!");
                }

                (start..end).collect::<Vec<_>>()
            }
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

    pub fn build_from_str(mut self, file_contents: &String) -> (LegatoApp, LegatoBackend) {
        let pairs = parse_legato_file(file_contents).unwrap();
        let ast = build_ast(pairs, &self.pipes).unwrap();

        for scope in ast.declarations.iter(){
            for node in scope.declarations.iter(){
                match node {
                    ExpandedNode::Node(inner) => {
                        self.add_node(&scope.namespace, &inner.node_type, &inner.alias, &Params(&inner.params)).unwrap();
                    },
                    ExpandedNode::Multiple(inner) => {
                        for item in inner {
                            self.add_node(&scope.namespace, &item.node_type, &item.alias, &Params(&item.params)).unwrap();
                        }
                    }
                }
            }
        }

        for connection in ast.connections.iter() {
            let source_key = self.working_name_to_key
                .get(&connection.source_name)
                .expect(&format!("Could not find source key in connection {}", &connection.source_name));
            
            let sink_key = self.working_name_to_key
                .get(&connection.sink_name)
                .expect(&format!("Could not find sink key in connection {}", &connection.sink_name));
                
            self.add_connection(AddConnectionProps {
                source: *source_key,
                sink: *sink_key,
                source_kind: connection.source_port.clone(),
                sink_kind: connection.sink_port.clone(),
                rate: PortRate::Audio // TODO: Control as well
            });
        }

        let sink_key = self.working_name_to_key
            .get(&ast.sink.name)
            .expect("Could not find sink!");

        self.runtime.set_sink_key(*sink_key)
            .expect("Could not set sink!");

        self.build()
    }

    pub fn build(mut self) -> (LegatoApp, LegatoBackend) {
        // I am okay with leaking this onto the heap here, maybe some Rustaceans can give a second opinion
        // I am using heapless::spsc for it's better realtime performance, not because I explicitly want to keep it on the stack
        let queue: &'static mut heapless::spsc::Queue<LegatoMsg, 128> = Box::leak(Box::new(heapless::spsc::Queue::<LegatoMsg, 128>::new()));
                
        let (tx, rx) = queue.split();

        let runtime_backend = RuntimeBackend::new(self.sample_backends);

        // important because we only pass a small window of resources, instead of the whole runtime, to node's factory functions
        self.runtime.set_resources(self.resources);

        let app = LegatoApp::new(self.runtime, rx);
        let backend = LegatoBackend::new(runtime_backend, tx);

        (
            app,
            backend
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
