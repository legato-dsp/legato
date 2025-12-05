use std::{collections::HashMap};
use legato_core::{nodes::{audio::mixer::TrackMixer, ports::{PortBuilder, PortRate}}, runtime::{builder::{AddNode, get_runtime_builder}, context::Config, graph::{Connection, ConnectionEntry, NodeKey}, runtime::{Runtime, RuntimeBackend}}};
use crate::{ast::{Ast, PortConnectionType}, ir::{params::Params, registry::AudioRegistry}};

pub mod params;
pub mod registry;
#[macro_use]
pub mod node_spec;

/// ValidationError covers logical issues
/// when lowering from the AST to the IR.
///
/// These might be bad parameters,
/// bad values, nodes that don't exist, etc.
#[derive(Clone, PartialEq, Debug)]
pub enum ValidationError {
    NodeNotFound(String),
    NamespaceNotFound(String),
    InvalidParameter(String),
    MissingRequiredParameters(String),
    MissingRequiredParameter(String),
}

// TODO: A proper IR for users that don't want to use the DSL. 
pub fn build_runtime_from_ast(ast: Ast, config: Config) -> (Runtime, RuntimeBackend) {
    let default_registry = AudioRegistry::default();

    let mut registries: HashMap<String, AudioRegistry> = HashMap::new();
    registries.insert("audio".into(), default_registry);

    // TODO: Explicit source ports?

    let ports = PortBuilder::default()
        .audio_out(config.channels)
        .build();

    let mut builder = get_runtime_builder(config, ports);

    let mut add_node_instructions: HashMap<String, AddNode> = HashMap::new();

    for scope in ast.declarations.iter() {
        for node in scope.declarations.iter() {
            let params_ref = node.params.as_ref().map(|o| Params(o));

            let add_node = registries
                .get(&scope.namespace)
                .expect(&format!("Could not find namespace {}", scope.namespace))
                .get_node(&node.node_type, params_ref.as_ref())
                .expect(&format!("Unable to find {} node from registry", node.node_type));

            let working_name = node.alias.clone().unwrap_or_else(|| node.clone().node_type);

            if add_node_instructions.contains_key(&working_name) {
                panic!(
                    "Node name {:?} is already in use. Please add an alias.",
                    working_name
                );
            }

            add_node_instructions.insert(working_name, add_node);
        }
    }

    let mut working_name_to_key = HashMap::<String, NodeKey>::new();

    for (w_name, instr) in add_node_instructions {
        let key = builder.add_node(instr);
        working_name_to_key.insert(w_name, key);
    }

    let (mut runtime, backend) = builder.get_owned();

    // Now, we start adding explicit connections, as well as nodes for mixing with port automapping

    for connection in ast.connections.iter() {
        dbg!(connection);

        let source_key = working_name_to_key
            .get(&connection.source_name)
            .expect(&format!("Could not find source key in connection {}", &connection.source_name));
        let sink_key = working_name_to_key
            .get(&connection.sink_name)
            .expect(&format!("Could not find sink key in connection {}", &connection.sink_name));

        let source_ports = runtime.get_node_ports(&source_key).clone();
        let sink_ports = runtime.get_node_ports(&sink_key).clone();

        let source_arity = source_ports.audio_out.len();
        let sink_arity = sink_ports.audio_in.len();

        if source_arity == 0 || sink_arity == 0 {
            // TODO: Maybe we let "phantom connections"
            panic!("Port arity found with 0 in graph connection!");
        }

        // Node mapping logic. This needs to be cleaned up, I am sure there is some way to generalize this

        match (&connection.source_port, &connection.sink_port) {
            // Auto map, we always map to the sink's arity
            (PortConnectionType::Auto, PortConnectionType::Auto) => {

                if source_arity == sink_arity {
                    for i in 0..source_ports.audio_out.len(){
                        // TODO: Control rate!
                        runtime.add_edge(Connection { 
                            source: ConnectionEntry { 
                                node_key: source_key.clone(), 
                                port_index: i, 
                                port_rate: PortRate::Audio }, 
                            sink: ConnectionEntry { 
                                node_key: sink_key.clone(), 
                                port_index: i, 
                                port_rate: PortRate::Audio 
                            } 
                        }).expect("Could not add edge");
                    }
                }
                else {
                    match (source_arity, sink_arity) {
                        (1, 2) => {
                            let mono_to_stereo = runtime.add_node(Box::new(TrackMixer::new(
                                1, 
                                1, 
                                vec![0.71],
                            )));

                            runtime.add_edge(Connection { 
                                source: ConnectionEntry { 
                                    node_key: source_key.clone(), 
                                    port_index: 0, 
                                    port_rate: PortRate::Audio }, 
                                sink: ConnectionEntry { 
                                    node_key: mono_to_stereo.clone(), 
                                    port_index: 0, 
                                    port_rate: PortRate::Audio 
                                } 
                            }).expect("Could not add edge");

                            for i in 0..2 {
                                runtime.add_edge(
                                    Connection { 
                                        source: ConnectionEntry { 
                                            node_key: mono_to_stereo.clone(), 
                                            port_index:i, 
                                            port_rate: PortRate::Audio }, 
                                        sink: ConnectionEntry { 
                                            node_key: sink_key.clone(), 
                                            port_index: i, 
                                            port_rate: PortRate::Audio 
                                        } 
                                    }
                                ).expect("Could not add edge");
                            }
                        },
                        (2, 1) => {

                            let stereo_to_mono = runtime.add_node(Box::new(TrackMixer::new(
                                1, 
                                2, 
                                vec![0.71, 0.71],
                            )));

                            for i in 0..2 {
                                runtime.add_edge(Connection { 
                                    source: ConnectionEntry { 
                                        node_key: source_key.clone(), 
                                        port_index: i, 
                                        port_rate: PortRate::Audio }, 
                                    sink: ConnectionEntry { 
                                        node_key: stereo_to_mono.clone(), 
                                        port_index: i, 
                                        port_rate: PortRate::Audio 
                                    } 
                                }).expect("Could not add edge");
                            }

                            runtime.add_edge(Connection { 
                                source: ConnectionEntry { 
                                    node_key: stereo_to_mono.clone(), 
                                    port_index:0, 
                                    port_rate: PortRate::Audio }, 
                                sink: ConnectionEntry { 
                                    node_key: sink_key.clone(), 
                                    port_index: 0, 
                                    port_rate: PortRate::Audio 
                                } 
                            }).expect("Could not add edge");
                        },
                        _ => panic!("Auto mapping not supported for arities: {} {}, found on nodes {} and {}.", source_arity, sink_arity, connection.source_name, connection.sink_name)
                    }
                }
                

            },
            (PortConnectionType::Auto, _) => {
                // In this case, we have an automap >> explicit

                // If we automap[1] >> explicit, just do a normal connection
                if source_arity == 1  {
                    let manual_port_sink: usize = match connection.sink_port {
                        PortConnectionType::Named { ref port } => {
                            let found = sink_ports.audio_in.iter().find(|x| x.name == port);
                            let index = found
                                .expect(&format!("Port {:?} not found", &port))
                                .index;
                            index
                        }
                        PortConnectionType::Indexed { port } => port,
                        PortConnectionType::Auto => unreachable!(),
                    };

                    runtime.add_edge(Connection { 
                        source: ConnectionEntry { 
                            node_key: source_key.clone(), 
                            port_index:0, 
                            port_rate: PortRate::Audio }, 
                        sink: ConnectionEntry { 
                            node_key: sink_key.clone(), 
                            port_index: manual_port_sink, 
                            port_rate: PortRate::Audio 
                        } 
                    }).expect("Could not add edge");
                }

                // In this case, a user wants to automap say a port of size N to a mono signal

                else {
                    let n_to_mono = runtime.add_node(Box::new(TrackMixer::new(
                        1, 
                        source_arity, 
                        vec![0.71, 0.71],
                    )));

                    for n in 0..source_arity {
                        runtime.add_edge(Connection { 
                            source: ConnectionEntry { 
                                node_key: source_key.clone(), 
                                port_index:0, 
                                port_rate: PortRate::Audio }, 
                            sink: ConnectionEntry { 
                                node_key: n_to_mono, 
                                port_index: n, 
                                port_rate: PortRate::Audio 
                        } 
                        }).expect("Could not add edge");
                    }

                    runtime.add_edge(Connection { 
                            source: ConnectionEntry { 
                                node_key: n_to_mono.clone(), 
                                port_index: 0, 
                                port_rate: PortRate::Audio }, 
                            sink: ConnectionEntry { 
                                node_key: sink_key.clone(), 
                                port_index: 0, 
                                port_rate: PortRate::Audio 
                        } 
                    }).expect("Could not add edge");
                }
            },
            // Map explicit to auto. m => n
            (_, PortConnectionType::Auto) => {
                // If both are len one, just a normal connection
                if source_arity == sink_arity {
                    runtime.add_edge(Connection { 
                            source: ConnectionEntry { 
                                node_key: source_key.clone(), 
                                port_index: 0, 
                                port_rate: PortRate::Audio }, 
                            sink: ConnectionEntry { 
                                node_key: sink_key.clone(), 
                                port_index: 0, 
                                port_rate: PortRate::Audio 
                        } 
                    }).expect("Could not add edge");
                }
                else {
                    // We now need to insert a number of normalized connections for each sink. This may be a bit more costly and perhaps we just add gain to connections in the future
                    let mono_to_n = runtime.add_node(Box::new(TrackMixer::new(
                                1, 
                                1, 
                                vec![1.0 / f32::sqrt(sink_arity as f32)],
                        )));

                    runtime.add_edge(Connection { 
                        source: ConnectionEntry { 
                            node_key: source_key.clone(), 
                            port_index: 0, 
                            port_rate: PortRate::Audio }, 
                        sink: ConnectionEntry { 
                            node_key: mono_to_n.clone(), 
                            port_index: 0, 
                            port_rate: PortRate::Audio } 
                    }).expect("Could not add edge");

                    for i in 0..sink_arity {
                        runtime.add_edge(
                            Connection { 
                            source: ConnectionEntry { 
                                node_key: mono_to_n.clone(), 
                                port_index: 0, 
                                port_rate: PortRate::Audio }, 
                            sink: ConnectionEntry { 
                                node_key: sink_key.clone(), 
                                port_index: i, 
                                port_rate: PortRate::Audio 
                            }
                        }).expect("Could not add edge");
                    }
                }

            }
            // Now, we only have manual to manual mapping
            _ => {
                let manual_port_source: Option<usize> = match connection.source_port {
                    PortConnectionType::Auto => None,
                    PortConnectionType::Named { ref port } => {
                        let found = source_ports.audio_out.iter().find(|x| x.name == port);
                        let index = found
                            .expect(&format!("Port {:?} not found", &port)).index;
                        Some(index)
                    }
                    PortConnectionType::Indexed { port } => Some(port),
                };

                let manual_port_sink: Option<usize> = match connection.sink_port {
                    PortConnectionType::Auto => None,
                    PortConnectionType::Named { ref port } => {
                        let found = sink_ports.audio_in.iter().find(|x| x.name == port);
                        let index = found
                            .expect(&format!("Port {:?} not found", &port))
                            .index;
                        Some(index)
                    }
                    PortConnectionType::Indexed { port } => Some(port),
                };

                let _ = runtime.add_edge(Connection {
                    source: ConnectionEntry {
                        node_key: *source_key,
                        port_index: manual_port_source.unwrap(),
                        port_rate: PortRate::Audio,
                    },
                    sink: ConnectionEntry {
                        node_key: *sink_key,
                        port_index: manual_port_sink.unwrap(),
                        port_rate: PortRate::Audio,
                    },
                });
            }

        }
    }   

    let sink_ref = working_name_to_key
            .get(&ast.sink.name)
            .expect("Could not find sink!");

    runtime
        .set_sink_key(*sink_ref)
        .expect("Could not set sink!");


    (runtime, backend)
}