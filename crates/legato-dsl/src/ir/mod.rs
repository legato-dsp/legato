use std::ops::Mul;

use generic_array::ArrayLength;
use legato_core::engine::{
    builder::{AddNode, RuntimeBuilder, get_runtime_builder},
    graph::{Connection, ConnectionEntry, NodeKey},
    node::FrameSize,
    port::{PortRate, Ports},
    runtime::{Runtime, RuntimeBackend},
};
use std::collections::HashMap;
use typenum::{Prod, U0, U2};

use crate::{
    ast::{Ast, AstNodeConnection, PortConnectionType, Sink},
    ir::{params::Params, registry::LegatoRegistryContainer},
};

pub mod params;
pub mod registry;

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

pub struct IR<AF, CF>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
{
    add_node_instructions: HashMap<String, AddNode<AF, CF>>, // A hashmap of working names -> add node commands
    connections: Vec<AstNodeConnection>,
    sink: Sink, // TODO: Exports
}

impl<AF, CF> From<Ast> for IR<AF, CF>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
{
    fn from(ast: Ast) -> Self {
        let registry = LegatoRegistryContainer::new();

        let mut add_node_instructions = HashMap::new();

        for scope in ast.declarations {
            for node in scope.declarations {
                let params_ref = node.params.as_ref().map(|o| Params(o));

                let add_node = registry
                    .get(&scope.namespace, &node.node_type, params_ref.as_ref())
                    .unwrap();

                let working_name = node.alias.unwrap_or_else(|| node.node_type);

                if add_node_instructions.contains_key(&working_name) {
                    panic!(
                        "Node name {:?} is already in use. Please add an alias.",
                        working_name
                    );
                }

                add_node_instructions.insert(working_name, add_node);
            }
        }

        Self {
            add_node_instructions: add_node_instructions,
            connections: ast.connections,
            sink: ast.sink,
        }
    }
}

pub fn build_runtime_from_ir<AF, CF, C, Ci>(
    ir: IR<AF, CF>,
    initial_capacity: usize,
    sample_rate: u32,
    control_rate: usize,
    ports: Ports<C, C, Ci, U0>,
) -> (Runtime<AF, CF, C, Ci>, RuntimeBackend)
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
    Ci: ArrayLength,
{
    let mut runtime_builder: RuntimeBuilder<AF, CF, C, Ci> = get_runtime_builder(
        initial_capacity,
        sample_rate as f32,
        control_rate as f32,
        ports,
    );

    let mut node_working_name_to_key_map = HashMap::<String, NodeKey>::new();

    for (working_name, add_node) in ir.add_node_instructions.into_iter() {
        let key = runtime_builder.add_node(add_node);
        node_working_name_to_key_map.insert(working_name.clone(), key);
    }

    let mut connections = Vec::<Connection>::new();

    for connection in ir.connections {
        // TODO: Control logic
        // TODO: Messy enough that this needs some tests
        let source_key = node_working_name_to_key_map
            .get(&connection.source_name)
            .expect("Could not find source key in connection");
        let sink_key = node_working_name_to_key_map
            .get(&connection.sink_name)
            .expect("Could not find sink key in connection");

        let (_, source_audio_ports_out, _, _) = runtime_builder.get_port_info(&source_key);
        let (sink_audio_ports_in, _, _, _) = runtime_builder.get_port_info(&sink_key);

        let source_ports = source_audio_ports_out.unwrap();
        let sink_ports = sink_audio_ports_in.unwrap();

        // Assume audio for now

        let manual_port_source: Option<usize> = match connection.source_port {
            PortConnectionType::Auto => None,
            PortConnectionType::Named { ref port } => {
                let found = source_ports.iter().find(|x| x.meta.name == port);
                let index = found
                    .expect(&format!("Port {:?} not found", &port))
                    .meta
                    .index;
                Some(index)
            }
            PortConnectionType::Indexed { port } => Some(port),
        };

        let manual_port_sink: Option<usize> = match connection.sink_port {
            PortConnectionType::Auto => None,
            PortConnectionType::Named { ref port } => {
                let found = sink_ports.iter().find(|x| x.meta.name == port);
                let index = found
                    .expect(&format!("Port {:?} not found", &port))
                    .meta
                    .index;
                Some(index)
            }
            PortConnectionType::Indexed { port } => Some(port),
        };

        connections.push(Connection {
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
        })
    }

    let (mut runtime, backend) = runtime_builder.get_owned();

    for c in connections {
        runtime.add_edge(c).unwrap();
    }

    let sink_ref = node_working_name_to_key_map
        .get(&ir.sink.name)
        .expect("Could not find sink!");

    runtime
        .set_sink_key(*sink_ref)
        .expect("Could not set sink!");

    (runtime, backend)
}
