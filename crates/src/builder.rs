use std::{collections::{BTreeMap, HashMap}, marker::PhantomData, sync::{Arc, atomic::AtomicU64}};

use arc_swap::ArcSwapOption;

use crate::{
    LegatoApp, LegatoBackend, LegatoMsg, ValidationError, ast::{PortConnectionType, build_ast}, config::Config, graph::{Connection, ConnectionEntry}, node::{DynNode, LegatoNode}, nodes::audio::{delay::DelayLine, mixer::{MonoFanOut, TrackMixer}}, params::Params, parse::parse_legato_file, pipes::Pipe, ports::{PortRate, Ports}, registry::AudioRegistry, resources::{DelayLineKey, Resources, SampleKey}, runtime::{NodeKey, Runtime, RuntimeBackend, build_runtime}, sample::{AudioSampleBackend, AudioSampleHandle}, spec::NodeSpec
};

// Typestates for the builder
pub struct Unconfigured;
pub struct Configured;
pub struct ContainsNodes;
pub struct Connected;
pub struct ReadyToBuild;



// Different traits for varying levels
pub trait CanRegister {}
pub trait CanAddNode {}
pub trait CanConnect {}
pub trait CanApplyPipe {}
pub trait CanSetSink {}
pub trait CanBuild {}
pub trait CanBuildFromDSL {}

// Setting up "permissions" for different structs. May be too complicated but also easy to add more states with overlapping permissiosn

impl CanRegister for Unconfigured {}
impl CanRegister for Configured {}
impl CanRegister for ContainsNodes {}

impl CanAddNode for Configured {}
impl CanAddNode for ContainsNodes {}

impl CanApplyPipe for ContainsNodes {}

impl CanConnect for ContainsNodes {}
impl CanConnect for Connected {}

impl CanSetSink for ContainsNodes {}
impl CanSetSink for Connected {}

impl CanBuild for ReadyToBuild {}

impl CanBuildFromDSL for Configured {}

pub struct DslBuilding;

impl CanRegister for DslBuilding {}
impl CanAddNode for DslBuilding {}
impl CanConnect for DslBuilding {}
impl CanApplyPipe for DslBuilding {}
impl CanSetSink for DslBuilding {}

// Convenience struct for moving from one state to another
impl<S> LegatoBuilder<S> {
    #[inline]
    fn into_state<T>(self) -> LegatoBuilder<T> {
        LegatoBuilder {
            runtime: self.runtime,
            namespaces: self.namespaces,
            working_name_lookup: self.working_name_lookup,
            delay_name_to_key: self.delay_name_to_key,
            resources: self.resources,
            sample_backends: self.sample_backends,
            sample_name_to_key: self.sample_name_to_key,
            pipe_lookup: self.pipe_lookup,
            last_node_ref_added: self.last_node_ref_added,
            _state: PhantomData,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum NodeKeyStorage {
    Single(NodeKey),
    Multiple(Vec<NodeKey>),
}

pub struct LegatoBuilder<State> {
    runtime: Runtime,
    // String to registries of node spec (including factory fn) lookup
    namespaces: HashMap<String, AudioRegistry>,
    // Lookup from string to NodeKey
    working_name_lookup: HashMap<String, NodeKey>,
    // Lookup from string to Pipe Fn
    pipe_lookup: HashMap<String, Box<dyn Pipe>>,
    // Resources being built. These can be pased to node factories
    resources: Resources,
    // Name to key maps
    sample_name_to_key: HashMap<String, SampleKey>,
    delay_name_to_key: HashMap<String, DelayLineKey>,
    sample_backends: HashMap<String, AudioSampleBackend>,
    // When adding a node, this tracks and sets the node key for pipes
    last_node_ref_added: Option<NodeKeyStorage>,
    _state: PhantomData<State>,
}

impl<Unconfigured> LegatoBuilder<Unconfigured> {
    pub fn new(config: Config, ports: Ports) -> LegatoBuilder<Configured> {
        let mut namespaces = HashMap::new();
        let audio_registry = AudioRegistry::default();
        namespaces.insert("audio".into(), audio_registry);
        namespaces.insert("user".into(), AudioRegistry::new());
        
        let runtime = build_runtime(config, ports);

        LegatoBuilder::<Configured> {
            runtime: runtime,
            resources: Resources::default(),
            sample_name_to_key: HashMap::new(),
            delay_name_to_key: HashMap::new(),
            sample_backends: HashMap::new(),
            namespaces: namespaces,
            working_name_lookup: HashMap::new(),
            pipe_lookup: HashMap::new(),
            last_node_ref_added: None,
            _state: std::marker::PhantomData,
        }
    }
}

impl<S> LegatoBuilder<S> where S: CanRegister {
    /// Add a new registry. Think of registries like "DLC" or packs of nodes that users or developers can extend
    pub fn add_node_registry(&mut self, name: &'static str, registry: AudioRegistry) {
        self.namespaces.insert(name.into(), registry);
    }
    /// Register a node to the "user" namespace
    pub fn register_node(mut self, namespace: &'static str, spec: NodeSpec) {
        match self.namespaces.get_mut(namespace) {
            Some(ns) => ns.declare_node(spec),
            None => panic!("Cannot find namespace {}", namespace)
        }    
    }
    /// Register a custom pipe for transforming nodes
    pub fn register_pipe(mut self, name: &'static str, pipe: Box<dyn Pipe>) {
        self.pipe_lookup.insert(name.into(), pipe);  
    }
}


impl<S> LegatoBuilder<S> where S: CanAddNode,
{
    /// This pattern is used because we sometimes execute this in a non-owned context
    fn add_node_ref_self(
        &mut self,
        namespace: &String,
        node_kind: &String,
        alias: &String,
        params: &Params,
    ) {
        let ns = self.namespaces.get(namespace).expect(&format!("Could not find namespace {}", namespace));

        let mut resource_builder_view = ResourceBuilderView {
            config: &self.runtime.get_config(),
            resources: &mut self.resources,
            sample_keys: &mut self.sample_name_to_key,
            delay_keys: &mut self.delay_name_to_key,
            sample_backends: &mut self.sample_backends,
        };

        let node = ns
            .get_node(&mut resource_builder_view, node_kind, params)
            .expect(&format!("Could not find node {}", node_kind));

        let legato_node = LegatoNode::new(alias.into(), node_kind.into(), node);

        let key = self.runtime.add_node(legato_node);

        self.working_name_lookup
            .insert(alias.clone(), key.clone());

        // Set the last node_ref_added
        self.last_node_ref_added = Some(NodeKeyStorage::Single(key));
    }
    pub fn add_node(
        mut self,
        namespace: &String,
        node_kind: &String,
        alias: &String,
        params: &Params,
    ) -> LegatoBuilder<ContainsNodes> {
        self.add_node_ref_self(namespace, node_kind, alias, params);
        self.into_state()
    }

    /// Skip the ceremony with namespaces, specs, etc. and just add a LegatoNode. This still requires an alias for connections and debugging
    pub fn add_node_raw(mut self, node: LegatoNode, alias: &String) -> LegatoBuilder<ContainsNodes> {
        let key = self.runtime.add_node(node);
        self.last_node_ref_added = Some(NodeKeyStorage::Single(key));

        self.working_name_lookup
            .insert(alias.clone(), key.clone());

        self.into_state()
    }
}

impl<S> LegatoBuilder<S> where S: CanConnect
{
    /// This pattern is used because we sometimes execute this in a non-owned context
    fn connect_ref_self(&mut self, connection: AddConnectionProps) {
        let source_indicies: Vec<usize> = match connection.source_kind {
            PortConnectionType::Auto => {
                let ports = self.runtime.get_node_ports(&connection.source);
                let indicies = match connection.rate {
                    PortRate::Audio => ports.audio_out.iter().enumerate().map(|(i, _)| i).collect(),
                    PortRate::Control => ports
                        .control_out
                        .iter()
                        .enumerate()
                        .map(|(i, _)| i)
                        .collect(),
                };
                indicies
            }
            PortConnectionType::Indexed { port } => vec![port],
            PortConnectionType::Named { ref port } => {
                let ports = self.runtime.get_node_ports(&connection.source);
                let index = match connection.rate {
                    PortRate::Audio => ports.audio_out.iter().find(|x| x.name == port),
                    PortRate::Control => ports.control_out.iter().find(|x| x.name == port),
                }
                .expect(&format!("Could not find index for named port {}", port))
                .index;

                vec![index]
            }
            PortConnectionType::Slice { start, end } => {
                if end < start {
                    panic!("End slice cannot be less than start!");
                }

                (start..end).collect::<Vec<_>>()
            }
        };

        let sink_indicies: Vec<usize> = match connection.sink_kind {
            PortConnectionType::Auto => {
                let ports = self.runtime.get_node_ports(&connection.source);
                let indicies = match connection.rate {
                    PortRate::Audio => ports.audio_in.iter().enumerate().map(|(i, _)| i).collect(),
                    PortRate::Control => ports
                        .control_in
                        .iter()
                        .enumerate()
                        .map(|(i, _)| i)
                        .collect(),
                };
                indicies
            }
            PortConnectionType::Indexed { port } => vec![port],
            PortConnectionType::Named { ref port } => {
                let ports = self.runtime.get_node_ports(&connection.source);
                let index = match connection.rate {
                    PortRate::Audio => ports.audio_in.iter().find(|x| x.name == port),
                    PortRate::Control => ports.control_in.iter().find(|x| x.name == port),
                }
                .expect(&format!("Could not find index for named port {}", port))
                .index;

                vec![index]
            }
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
            (1, 1) => one_to_one(
                &mut self.runtime,
                connection,
                source_indicies[0],
                sink_indicies[0],
            ),
            (1, n) if n >= 1 => one_to_n(
                &mut self.runtime,
                connection,
                source_indicies[0],
                sink_indicies.as_slice(),
            ),
            (n, 1) if n >= 1 => n_to_one(
                &mut self.runtime,
                connection,
                source_indicies.as_slice(),
                sink_indicies[0],
            ),
            (n, m) if n == m => n_to_n(
                &mut self.runtime,
                connection,
                &source_indicies,
                &sink_indicies,
            ),
            (n, m) => unimplemented!("Cannot match request arity {}:{}", n, m),
        }
    }
    pub fn connect(mut self, connection: AddConnectionProps) -> LegatoBuilder<Connected> {
        self.connect_ref_self(connection);
        self.into_state()
    }
}

impl<S> LegatoBuilder<S> where S: CanSetSink
{
    pub fn set_sink(mut self, key: NodeKey) -> LegatoBuilder<ReadyToBuild> {
        self.runtime.set_sink_key(key).expect("Sink key not found");
        self.into_state()
    }
}

impl<S> LegatoBuilder<S> where S: CanApplyPipe
{
    pub fn pipe(&mut self, pipe_name: &'static str) {
        match self.last_node_ref_added {
            Some(_) => (),
            None => panic!("Cannot apply pipe to non-existent node!")
        }
    }
}

impl LegatoBuilder<ReadyToBuild> {
    pub fn build(self) -> (LegatoApp, LegatoBackend) {
        let mut runtime = self.runtime;
        runtime.set_resources(self.resources);

        // TODO: Perhaps a different crate here instead of leaking
        let queue = Box::leak(Box::new(heapless::spsc::Queue::<LegatoMsg, 512>::new()));

        let (producer, consumer) = queue.split();

        let app = LegatoApp::new(runtime, consumer);

        let rt_backend = RuntimeBackend::new(self.sample_backends);

        let backend = LegatoBackend::new(rt_backend, producer);

        (app, backend)
    }
}

impl<S> LegatoBuilder<S> where S: CanBuildFromDSL {
    pub fn build_dsl(self, content: &String) -> (LegatoApp, LegatoBackend) {
        let pairs = parse_legato_file(content).unwrap();

        let ast = build_ast(pairs).unwrap();

        let mut builder = self.into_state::<DslBuilding>();

        for scope in ast.declarations.iter() {
            for node in scope.declarations.iter() {
                builder.add_node_ref_self(
                    &scope.namespace,
                    &node.node_type,
                    &node.alias.clone().unwrap_or(node.node_type.clone()),
                    &Params(&node.params.clone().unwrap_or_else(|| BTreeMap::new()))
                );
            }
        }

        for connection in ast.connections.iter() {
            let source_key = builder
                .working_name_lookup
                .get(&connection.source_name)
                .expect(&format!(
                    "Could not find source key in connection {}",
                    &connection.source_name
                ));

            let sink_key = builder
                .working_name_lookup
                .get(&connection.sink_name)
                .expect(&format!(
                    "Could not find sink key in connection {}",
                    &connection.sink_name
                ));

            builder.connect_ref_self(AddConnectionProps {
                source: *source_key,
                sink: *sink_key,
                source_kind: connection.source_port.clone(),
                sink_kind: connection.sink_port.clone(),
                rate: PortRate::Audio, // TODO: Control as well
            });
        }

        let sink_key = builder
            .working_name_lookup
            .get(&ast.sink.name)
            .expect("Could not find sink!");

        builder.runtime
            .set_sink_key(*sink_key)
            .expect("Could not set sink!");

        let ready_to_build = builder.into_state::<ReadyToBuild>();

        ready_to_build.build()
    }
}


/// A small slice of the runtime exposed for nodes in their node factories.
///
/// This is useful to say reserve delay lines, or other shared logic.
pub struct ResourceBuilderView<'a> {
    pub config: &'a Config,
    pub resources: &'a mut Resources,
    pub sample_keys: &'a mut HashMap<String, SampleKey>,
    pub delay_keys: &'a mut HashMap<String, DelayLineKey>,
    pub sample_backends: &'a mut HashMap<String, AudioSampleBackend>,
}

impl<'a> ResourceBuilderView<'a> {
    pub fn add_delay_line(&mut self, name: &str, delay_line: DelayLine) -> DelayLineKey {
        let key = self.resources.add_delay_line(delay_line);
        self.delay_keys.insert(name.to_string(), key);

        key
    }

    pub fn get_delay_line_key(&self, name: &String) -> Result<DelayLineKey, ValidationError> {
        self.delay_keys.get(name).cloned().ok_or_else(|| {
            ValidationError::ResourceNotFound(format!("Could not find delay key {}", name))
        })
    }

    pub fn add_sampler(&mut self, name: &String) -> SampleKey {
        let sample_key = if let Some(&key) = self.sample_keys.get(name) {
            key
        } else {
            let data = ArcSwapOption::new(None);

            let handle = Arc::new(AudioSampleHandle {
                sample: data,
                sample_version: AtomicU64::new(0),
            });

            let backend = AudioSampleBackend::new(handle.clone());

            self.sample_backends.insert(name.clone(), backend);

            self.resources.add_sample_resource(handle)
        };
        sample_key
    }

    pub fn get_sampler_key(&self, name: &String) -> Result<SampleKey, ValidationError> {
        self.sample_keys.get(name).cloned().ok_or_else(|| {
            ValidationError::ResourceNotFound(format!("Could not find sample key {}", name))
        })
    }

    pub fn get_config(&self) -> &Config {
        &self.config
    }
}


pub struct AddConnectionProps {
    pub source: NodeKey,
    pub source_kind: PortConnectionType,
    pub sink: NodeKey,
    pub sink_kind: PortConnectionType,
    pub rate: PortRate, // Determines whether or not we look for control or audio matches
}

pub enum AddConnectionKind {
    Index(usize),
    Named(&'static str),
    Auto,
}


// Utility functions for handling connections 

fn one_to_one(
    runtime: &mut Runtime,
    props: AddConnectionProps,
    source_index: usize,
    sink_index: usize,
) {
    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: props.source,
                port_index: source_index,
                port_rate: props.rate,
            },
            sink: ConnectionEntry {
                node_key: props.sink,
                port_index: sink_index,
                port_rate: props.rate,
            },
        })
        .expect("Could not add edge");
}

fn one_to_n(
    runtime: &mut Runtime,
    props: AddConnectionProps,
    source_index: usize,
    sink_indicies: &[usize],
) {
    let n = sink_indicies.len();

    // Fanout mixer going from 1 -> n
    let mixer = runtime.add_node(
        LegatoNode::new(
            format!("MonoFanOut{:?}{:?}", props.source, props.sink), 
            "MonoFanOut".into(),
            Box::new(MonoFanOut::new(n))
        )
    );

    // Wire mono to mixer
    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: props.source,
                port_index: source_index,
                port_rate: props.rate,
            },
            sink: ConnectionEntry {
                node_key: mixer.clone(),
                port_index: 0,
                port_rate: props.rate,
            },
        })
        .expect("Could not add edge");

    // Wire fanout connection to each sink. We add this node in order to change the gain when fanning out
    for i in 0..n {
        runtime
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: mixer.clone(),
                    port_index: 0,
                    port_rate: props.rate,
                },
                sink: ConnectionEntry {
                    node_key: props.sink,
                    port_index: sink_indicies[i],
                    port_rate: props.rate,
                },
            })
            .expect("Could not add edge");
    }
}

fn n_to_one(
    runtime: &mut Runtime,
    props: AddConnectionProps,
    source_indicies: &[usize],
    sink_index: usize,
) {
    let n = source_indicies.len();

    // Make mixer with n mono tracks
    let mixer = runtime.add_node(
        LegatoNode::new(
            format!("TrackMixer{:?}{:?}", props.source, props.sink),
            "TrackMixer".into(),
            Box::new(TrackMixer::new(1, n, vec![1.0 / f32::sqrt(n as f32); n])),
        )
    );

    // Build connections into track mixer
    for i in 0..n {
        runtime
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: props.source,
                    port_index: source_indicies[i],
                    port_rate: props.rate,
                },
                sink: ConnectionEntry {
                    node_key: mixer.clone(),
                    port_index: i,
                    port_rate: props.rate,
                },
            })
            .expect("Could not add edge");
    }

    // Wire track mixer to sink index
    runtime
        .add_edge(Connection {
            source: ConnectionEntry {
                node_key: mixer.clone(),
                port_index: 0,
                port_rate: props.rate,
            },
            sink: ConnectionEntry {
                node_key: props.sink,
                port_index: sink_index,
                port_rate: props.rate,
            },
        })
        .expect("Could not add edge");
}

fn n_to_n(
    runtime: &mut Runtime,
    props: AddConnectionProps,
    source_indicies: &[usize],
    sink_indicies: &[usize],
) {
    assert!(source_indicies.len() == sink_indicies.len());
    source_indicies
        .iter()
        .zip(sink_indicies)
        .for_each(|(source, sink)| {
            runtime
                .add_edge(Connection {
                    source: ConnectionEntry {
                        node_key: props.source,
                        port_index: *source,
                        port_rate: props.rate,
                    },
                    sink: ConnectionEntry {
                        node_key: props.sink,
                        port_index: *sink,
                        port_rate: props.rate,
                    },
                })
                .expect("Could not add edge");
        });
}
