use crate::ir::*;
use indexmap::IndexMap;
use std::collections::HashMap;

const MAXIMUM_DEPTH: u8 = 16;

// ---------------------------------------------------------------------------
// GraphPass trait
// ---------------------------------------------------------------------------

/// A single, named transformation of an [`IRGraph`].
pub trait GraphPass {
    fn name(&self) -> &'static str;
    fn run(&self, graph: IRGraph) -> IRGraph;
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// An ordered sequence of [`GraphPass`]es applied to an [`IRGraph`].
pub struct Pipeline {
    passes: Vec<Box<dyn GraphPass>>,
}

impl Pipeline {
    pub fn new() -> Self {
        Self { passes: vec![] }
    }

    /// Append a pass to the end of the pipeline.
    pub fn add_pass<P: GraphPass + 'static>(mut self, pass: P) -> Self {
        self.passes.push(Box::new(pass));
        self
    }

    /// Translate `ast` to a literal [`IRGraph`] (see [`ast_to_graph`]), then
    /// run all passes in order.
    pub fn run_from_ast(self, ast: Ast) -> IRGraph {
        let initial = ast_to_graph(ast);
        self.run(initial)
    }

    /// Run all passes on an already-constructed graph.
    pub fn run(self, graph: IRGraph) -> IRGraph {
        self.passes.into_iter().fold(graph, |g, pass| pass.run(g))
    }
}

impl Default for Pipeline {
    /// The default pipeline. This will eventually handle sample rates, spawning nodes N times, etc.
    fn default() -> Self {
        Self::new().add_pass(MacroExpansionPass::default())
    }
}

fn convert_macro(
    name: &str,
    ast_map: &mut HashMap<String, AstMacro>,
    converted: &mut HashMap<String, IRMacro>,
) {
    // Check to avoid redundant macro conversion
    if converted.contains_key(name) {
        return;
    }

    let ast_macro = ast_map
        .remove(name)
        .unwrap_or_else(|| panic!("Macro '{}' not found", name));

    // Resolve dependencies first
    for scope in &ast_macro.declarations {
        for decl in &scope.declarations {
            if ast_map.contains_key(&decl.node_type) {
                convert_macro(&decl.node_type, ast_map, converted);
            }
        }
    }

    // Now build the body IRGraph for this macro
    let mut body = IRGraph::new();
    let mut local_alias_to_id: HashMap<String, NodeId> = HashMap::new();

    for scope in &ast_macro.declarations {
        for decl in &scope.declarations {
            let alias = decl.alias.clone().unwrap_or_else(|| decl.node_type.clone());

            // Classify as MacroRef if it names a known macro
            let kind = if converted.contains_key(&decl.node_type) {
                IRNodeKind::MacroRef
            } else {
                IRNodeKind::Leaf
            };

            let id = body.add_node(
                kind,
                scope.namespace.clone(),
                decl.node_type.clone(),
                alias.clone(),
                decl.params.clone().unwrap_or_default(),
                decl.pipes.clone(),
                decl.count,
            );
            local_alias_to_id.insert(alias, id);
        }
    }

    for conn in &ast_macro.connections {
        // This is handled below
        if ast_macro.virtual_ports_in.contains(&conn.source.node) {
            continue;
        }
        let src = local_alias_to_id[&conn.source.node];
        let snk = local_alias_to_id[&conn.sink.node];
        body.connect(src, conn.source.port.clone(), snk, conn.sink.port.clone());
    }

    let virtual_input_map = ast_macro
        .connections
        .iter()
        .filter(|c| ast_macro.virtual_ports_in.contains(&c.source.node))
        .map(|c| {
            let target_id = local_alias_to_id[&c.sink.node];
            (c.source.node.clone(), (target_id, c.sink.port.clone()))
        })
        .collect();

    let sink_id = local_alias_to_id[&ast_macro.sink];
    body.sink = Some(sink_id);

    converted.insert(
        name.to_string(),
        IRMacro {
            name: name.to_string(),
            default_params: ast_macro.default_params,
            virtual_input_map,
            body,
            sink: sink_id,
        },
    );
}

/// We have a bit of an easier Ast shape that the actual IR, since patches
/// are not-quite recursive. Here, we convert macros to a type that can recurse,
/// this makes various graph transformations easier.
pub fn ast_to_graph(ast: Ast) -> IRGraph {
    let mut graph = IRGraph::new();

    let mut macro_ast_map: HashMap<String, AstMacro> = ast
        .macros
        .into_iter()
        .map(|m| (m.name.clone(), m))
        .collect();

    let mut converted: HashMap<String, IRMacro> = HashMap::new();

    // Process each macro, recursing into dependencies first
    let names: Vec<String> = macro_ast_map.keys().cloned().collect();
    for name in names {
        convert_macro(&name, &mut macro_ast_map, &mut converted);
    }

    graph.macro_registry = converted;

    // Add one node per declaration; classify each as Leaf or MacroRef.
    let mut alias_to_id: HashMap<String, NodeId> = HashMap::new();

    for scope in &ast.declarations {
        for decl in &scope.declarations {
            let alias = decl.alias.clone().unwrap_or_else(|| decl.node_type.clone());

            let kind = if graph.macro_registry.contains_key(&decl.node_type) {
                IRNodeKind::MacroRef
            } else {
                IRNodeKind::Leaf
            };

            let id = graph.add_node(
                kind,
                scope.namespace.clone(),
                decl.node_type.clone(),
                alias.clone(),
                decl.params.clone().unwrap_or_default(),
                decl.pipes.clone(),
                decl.count,
            );
            alias_to_id.insert(alias, id);
        }
    }

    // Preserve connections verbatim.  Virtual ports are not resolved here;
    // they pass through as `Port::Named` and are handled by MacroExpansionPass.
    for conn in &ast.connections {
        let src = *alias_to_id.get(&conn.source.node).unwrap_or_else(|| {
            panic!("ast_to_graph: source node '{}' not found", conn.source.node)
        });
        let snk = *alias_to_id
            .get(&conn.sink.node)
            .unwrap_or_else(|| panic!("ast_to_graph: sink node '{}' not found", conn.sink.node));
        graph.connect(src, conn.source.port.clone(), snk, conn.sink.port.clone());
    }

    graph.sink = alias_to_id.get(&ast.sink).copied();
    graph.source = ast
        .source
        .as_ref()
        .and_then(|s| alias_to_id.get(s).copied());

    graph
}

/// This pass expands all [`IRMacros`] into the interior nodes,
/// wires the new interior connections, then handles connections
/// in and out to the macro instance.
#[derive(Default)]
pub struct MacroExpansionPass;

impl GraphPass for MacroExpansionPass {
    fn name(&self) -> &'static str {
        "MacroExpansionPass"
    }
    /// Expand macros while they still exist.
    fn run(&self, mut graph: IRGraph) -> IRGraph {
        let mut depth = 0u8;
        while graph.has_unresolved_macros() {
            assert!(
                depth < MAXIMUM_DEPTH,
                "MacroExpansionPass exceeded maximum depth — possible cycle in macro definitions"
            );
            let macro_ids: Vec<NodeId> = graph.macro_nodes().map(|n| n.id).collect();
            for id in macro_ids {
                self.expand_node(&mut graph, id);
            }
            depth += 1;
        }
        graph
    }
}

impl MacroExpansionPass {
    fn expand_node(&self, graph: &mut IRGraph, node_id: NodeId) {
        let node = graph.get_node(node_id).unwrap().clone();

        let ir_macro = graph
            .macro_registry
            .get(&node.node_type)
            .cloned()
            .unwrap_or_else(|| panic!("Macro '{}' not found in registry", node.node_type));

        // defaults first then override
        let mut resolved_params = ir_macro.default_params.clone().unwrap_or_default();
        for (k, v) in &node.params {
            resolved_params.insert(k.clone(), v.clone());
        }

        // Hold onto edges before mutation
        let incoming: Vec<IREdge> = graph.incoming_edges(node_id).cloned().collect();
        let outgoing: Vec<IREdge> = graph.outgoing_edges(node_id).cloned().collect();
        graph.remove_node(node_id);

        // Clone the body into the top-level graph.
        let id_map = self.clone_body_into(graph, &ir_macro, &node.alias, &resolved_params);

        // Remap the sink through the clone map — this is the macro's output
        let new_sink = id_map[&ir_macro.sink];

        // Remap virtual_input_map through the same clone map.
        let remapped_virtual: IndexMap<String, (NodeId, Port)> = ir_macro
            .virtual_input_map
            .iter()
            .map(|(name, (id, port))| (name.clone(), (id_map[id], port.clone())))
            .collect();

        // Rewire incoming edges through virtual ports
        for edge in incoming {
            let (target_id, target_port) = match &edge.sink_port {
                Port::Named(name) => remapped_virtual
                    .get(name)
                    .map_or((new_sink, edge.sink_port.clone()), |(id, port)| {
                        (*id, port.clone())
                    }),
                Port::Index(i) => remapped_virtual
                    .get_index(*i)
                    .map_or((new_sink, edge.sink_port.clone()), |(_, (id, port))| {
                        (*id, port.clone())
                    }),
                Port::None => (new_sink, Port::None),
                Port::Slice(..) | Port::Stride { .. } => panic!(
                    "Slice/Stride not supported on virtual ports (macro '{}')",
                    node.node_type
                ),
            };
            graph.connect(edge.source, edge.source_port, target_id, target_port);
        }

        // Rewire outgoing edges
        for edge in outgoing {
            graph.connect(new_sink, edge.source_port, edge.sink, edge.sink_port);
        }

        // Update sink and source
        if graph.sink == Some(node_id) {
            graph.sink = Some(new_sink);
        }
        if graph.source == Some(node_id) {
            graph.source = Some(new_sink);
        }
    }

    /// Clone an IRMacro's body into [`IRGraph`], prefixing all aliases.
    fn clone_body_into(
        &self,
        graph: &mut IRGraph,
        ir_macro: &IRMacro,
        instance_alias: &str,
        resolved_params: &Object,
    ) -> HashMap<NodeId, NodeId> {
        let mut id_map: HashMap<NodeId, NodeId> = HashMap::new();

        // A this point, everything should be a leaf!
        for node in ir_macro.body.nodes() {
            let fqn = format!("{}.{}", instance_alias, node.alias);

            let mut params = node.params.clone();
            Self::substitute_templates(&mut params, resolved_params);

            let new_id = graph.add_node(
                node.kind.clone(),
                node.namespace.clone(),
                node.node_type.clone(),
                fqn,
                params,
                node.pipes.clone(),
                node.count,
            );
            id_map.insert(node.id, new_id);
        }

        // Clone edges
        for edge in ir_macro.body.edges() {
            graph.connect(
                id_map[&edge.source],
                edge.source_port.clone(),
                id_map[&edge.sink],
                edge.sink_port.clone(),
            );
        }

        id_map
    }

    fn substitute_templates(params: &mut Object, lookup: &Object) {
        for val in params.values_mut() {
            if let Value::Template(tpl) = val {
                let key = tpl.trim_start_matches('$');
                if let Some(replacement) = lookup.get(key) {
                    *val = replacement.clone();
                }
            }
        }
    }
}

#[macro_export]
macro_rules! object {
    () => { ::std::collections::BTreeMap::new() };
    ( $($key:expr => template $val:expr),* $(,)? ) => {{
        let mut _map = ::std::collections::BTreeMap::new();
        $( _map.insert($key.to_string(), $crate::ir::Value::Template($val.to_string())); )*
        _map
    }};
    ( $($key:expr => $value:expr),* $(,)? ) => {{
        let mut _map = ::std::collections::BTreeMap::new();
        $( _map.insert($key.to_string(), $crate::ir::Value::from($value)); )*
        _map
    }};
}

#[cfg(test)]
mod tests {
    use indexmap::IndexSet;

    use super::*;

    fn make_voice_macro() -> AstMacro {
        AstMacro {
            name: "voice".into(),
            default_params: Some(object! {
                "freq"    => 440.0f32,
                "attack"  => 100.0f32,
                "release" => 500.0f32
            }),
            virtual_ports_in: {
                let mut s = IndexSet::new();
                s.insert("gate".into());
                s.insert("freq_in".into());
                s
            },
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![
                    NodeDeclaration {
                        node_type: "sine".into(),
                        alias: Some("osc".into()),
                        params: Some(object! { "freq" => Value::Template("$freq".into()) }),
                        pipes: vec![],
                        count: 1,
                    },
                    NodeDeclaration {
                        node_type: "adsr".into(),
                        alias: Some("env".into()),
                        params: Some(object! {
                            "attack"  => Value::Template("$attack".into()),
                            "release" => Value::Template("$release".into())
                        }),
                        pipes: vec![],
                        count: 1,
                    },
                ],
            }],
            connections: vec![
                Connection {
                    source: Endpoint {
                        node: "freq_in".into(),
                        port: Port::None,
                    },
                    sink: Endpoint {
                        node: "osc".into(),
                        port: Port::Named("freq".into()),
                    },
                },
                Connection {
                    source: Endpoint {
                        node: "gate".into(),
                        port: Port::None,
                    },
                    sink: Endpoint {
                        node: "env".into(),
                        port: Port::Named("gate".into()),
                    },
                },
                Connection {
                    source: Endpoint {
                        node: "osc".into(),
                        port: Port::None,
                    },
                    sink: Endpoint {
                        node: "env".into(),
                        port: Port::Index(1),
                    },
                },
            ],
            sink: "env".into(),
        }
    }

    // -----------------------------------------------------------------------
    // ast_to_graph tests: verify the literal (pre-expansion) graph
    // -----------------------------------------------------------------------

    #[test]
    fn test_ast_to_graph_produces_macro_ref_nodes() {
        let ast = Ast {
            macros: vec![make_voice_macro()],
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![
                    NodeDeclaration {
                        node_type: "voice".into(),
                        alias: Some("v1".into()),
                        params: None,
                        pipes: vec![],
                        count: 1,
                    },
                    NodeDeclaration {
                        node_type: "track_mixer".into(),
                        alias: Some("mixer".into()),
                        params: None,
                        pipes: vec![],
                        count: 1,
                    },
                ],
            }],
            connections: vec![Connection {
                source: Endpoint {
                    node: "v1".into(),
                    port: Port::None,
                },
                sink: Endpoint {
                    node: "mixer".into(),
                    port: Port::None,
                },
            }],
            sink: "mixer".into(),
            ..Default::default()
        };

        let graph = ast_to_graph(ast);

        // Literal graph: one MacroRef + one Leaf, not yet expanded.
        assert_eq!(graph.node_count(), 2);
        assert!(graph.has_unresolved_macros());

        let v1 = graph.find_node_by_alias("v1").expect("v1 missing");
        assert_eq!(v1.kind, IRNodeKind::MacroRef);
        assert_eq!(v1.node_type, "voice");

        let mixer = graph.find_node_by_alias("mixer").expect("mixer missing");
        assert_eq!(mixer.kind, IRNodeKind::Leaf);

        // Connection is present verbatim — virtual ports not resolved yet.
        assert_eq!(graph.edge_count(), 1);
        let edges = graph.find_edges_between("v1", "mixer");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source_port, Port::None);
    }

    #[test]
    fn test_ast_to_graph_preserves_raw_params() {
        // Template values should be left unsubstituted in the literal graph.
        let ast = Ast {
            macros: vec![make_voice_macro()],
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "voice".into(),
                    alias: Some("v1".into()),
                    params: Some(object! { "freq" => 420.0f32 }),
                    pipes: vec![],
                    count: 1,
                }],
            }],
            sink: "v1".into(),
            ..Default::default()
        };

        let graph = ast_to_graph(ast);
        let v1 = graph.find_node_by_alias("v1").unwrap();
        // The call-site param should be on the MacroRef node as written.
        assert_eq!(v1.params.get("freq"), Some(&Value::F32(420.0)));
    }

    // MacroExpansionPass tests

    fn expand(ast: Ast) -> IRGraph {
        Pipeline::default().run_from_ast(ast)
    }

    #[test]
    fn test_expansion_replaces_macro_ref_with_leaves() {
        let ast = Ast {
            macros: vec![make_voice_macro()],
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "voice".into(),
                    alias: Some("v1".into()),
                    params: Some(object! { "freq" => 420.0f32 }),
                    pipes: vec![],
                    count: 1,
                }],
            }],
            sink: "v1".into(),
            ..Default::default()
        };

        let graph = expand(ast);

        assert!(!graph.has_unresolved_macros());
        assert_eq!(graph.node_count(), 2);

        let osc = graph.find_node_by_alias("v1.osc").expect("v1.osc missing");
        assert_eq!(osc.params.get("freq"), Some(&Value::F32(420.0)));

        // Only the interior osc→env edge; virtual-port connections produce no edge.
        assert_eq!(graph.edge_count(), 1);
        let edges = graph.find_edges_between("v1.osc", "v1.env");
        assert_eq!(edges[0].sink_port, Port::Index(1));
    }

    #[test]
    fn test_external_connection_through_virtual_port() {
        let ast = Ast {
            macros: vec![make_voice_macro()],
            declarations: vec![
                DeclarationScope {
                    namespace: "audio".into(),
                    declarations: vec![NodeDeclaration {
                        node_type: "voice".into(),
                        alias: Some("v1".into()),
                        params: None,
                        pipes: vec![],
                        count: 1,
                    }],
                },
                DeclarationScope {
                    namespace: "midi".into(),
                    declarations: vec![NodeDeclaration {
                        node_type: "poly_voice".into(),
                        alias: Some("poly".into()),
                        params: None,
                        pipes: vec![],
                        count: 1,
                    }],
                },
            ],
            connections: vec![
                Connection {
                    source: Endpoint {
                        node: "poly".into(),
                        port: Port::Named("freq".into()),
                    },
                    sink: Endpoint {
                        node: "v1".into(),
                        port: Port::Named("freq_in".into()),
                    },
                },
                Connection {
                    source: Endpoint {
                        node: "poly".into(),
                        port: Port::Named("gate".into()),
                    },
                    sink: Endpoint {
                        node: "v1".into(),
                        port: Port::Named("gate".into()),
                    },
                },
            ],
            sink: "v1".into(),
            ..Default::default()
        };

        let graph = expand(ast);

        // 1 interior + 2 external
        assert_eq!(graph.edge_count(), 3);

        let osc = graph.find_node_by_alias("v1.osc").unwrap();
        let env = graph.find_node_by_alias("v1.env").unwrap();
        let poly_edges = graph.find_edges_from("poly");

        let freq_edge = poly_edges
            .iter()
            .find(|e| e.source_port == Port::Named("freq".into()))
            .expect("poly.freq edge missing");
        assert_eq!(freq_edge.sink, osc.id);
        assert_eq!(freq_edge.sink_port, Port::Named("freq".into()));

        let gate_edge = poly_edges
            .iter()
            .find(|e| e.source_port == Port::Named("gate".into()))
            .expect("poly.gate edge missing");
        assert_eq!(gate_edge.sink, env.id);
        assert_eq!(gate_edge.sink_port, Port::Named("gate".into()));
    }

    #[test]
    fn test_multiple_instances_get_distinct_fqns() {
        let ast = Ast {
            macros: vec![make_voice_macro()],
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![
                    NodeDeclaration {
                        node_type: "voice".into(),
                        alias: Some("v1".into()),
                        params: Some(object! { "freq" => 440.0f32 }),
                        pipes: vec![],
                        count: 1,
                    },
                    NodeDeclaration {
                        node_type: "voice".into(),
                        alias: Some("v2".into()),
                        params: Some(object! { "freq" => 880.0f32 }),
                        pipes: vec![],
                        count: 1,
                    },
                ],
            }],
            ..Default::default()
        };

        let graph = expand(ast);
        assert_eq!(graph.node_count(), 4);

        for alias in ["v1.osc", "v1.env", "v2.osc", "v2.env"] {
            assert!(
                graph.find_node_by_alias(alias).is_some(),
                "missing {}",
                alias
            );
        }

        let v2_osc = graph.find_node_by_alias("v2.osc").unwrap();
        assert_eq!(v2_osc.params.get("freq"), Some(&Value::F32(880.0)));
    }

    #[test]
    fn test_passthrough_via_sink() {
        let ast = Ast {
            macros: vec![make_voice_macro()],
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![
                    NodeDeclaration {
                        node_type: "voice".into(),
                        alias: Some("v1".into()),
                        params: None,
                        pipes: vec![],
                        count: 1,
                    },
                    NodeDeclaration {
                        node_type: "track_mixer".into(),
                        alias: Some("mixer".into()),
                        params: None,
                        pipes: vec![],
                        count: 1,
                    },
                ],
            }],
            connections: vec![Connection {
                source: Endpoint {
                    node: "v1".into(),
                    port: Port::None,
                },
                sink: Endpoint {
                    node: "mixer".into(),
                    port: Port::None,
                },
            }],
            sink: "mixer".into(),
            ..Default::default()
        };

        let graph = expand(ast);
        let edges_to_mixer = graph.find_edges_to("mixer");
        assert_eq!(edges_to_mixer.len(), 1);

        let env = graph.find_node_by_alias("v1.env").unwrap();
        assert_eq!(
            edges_to_mixer[0].source, env.id,
            "passthrough should originate from v1.env (the voice sink leaf)"
        );
    }

    #[test]
    fn test_nested_macro_virtual_ports_and_connections() {
        let fm_osc = AstMacro {
            name: "fm_osc".into(),
            default_params: Some(object! { "freq" => 440.0f32, "mod_freq" => 880.0f32 }),
            virtual_ports_in: {
                let mut s = IndexSet::new();
                s.insert("freq_in".into());
                s
            },
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![
                    NodeDeclaration {
                        node_type: "sine".into(),
                        alias: Some("modulator".into()),
                        params: Some(object! { "freq" => Value::Template("$mod_freq".into()) }),
                        pipes: vec![],
                        count: 1,
                    },
                    NodeDeclaration {
                        node_type: "sine".into(),
                        alias: Some("carrier".into()),
                        params: Some(object! { "freq" => Value::Template("$freq".into()) }),
                        pipes: vec![],
                        count: 1,
                    },
                ],
            }],
            connections: vec![
                Connection {
                    source: Endpoint {
                        node: "freq_in".into(),
                        port: Port::None,
                    },
                    sink: Endpoint {
                        node: "carrier".into(),
                        port: Port::Named("freq".into()),
                    },
                },
                Connection {
                    source: Endpoint {
                        node: "modulator".into(),
                        port: Port::None,
                    },
                    sink: Endpoint {
                        node: "carrier".into(),
                        port: Port::Index(0),
                    },
                },
            ],
            sink: "carrier".into(),
        };

        let voice = AstMacro {
            name: "voice".into(),
            default_params: Some(object! { "freq" => 440.0f32, "attack" => 100.0f32 }),
            virtual_ports_in: {
                let mut s = IndexSet::new();
                s.insert("gate".into());
                s.insert("voice_freq".into());
                s
            },
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![
                    NodeDeclaration {
                        node_type: "fm_osc".into(),
                        alias: Some("osc_inst".into()),
                        params: Some(object! { "freq" => Value::Template("$freq".into()) }),
                        pipes: vec![],
                        count: 1,
                    },
                    NodeDeclaration {
                        node_type: "adsr".into(),
                        alias: Some("env".into()),
                        params: Some(object! { "attack" => Value::Template("$attack".into()) }),
                        pipes: vec![],
                        count: 1,
                    },
                ],
            }],
            connections: vec![
                Connection {
                    source: Endpoint {
                        node: "voice_freq".into(),
                        port: Port::None,
                    },
                    sink: Endpoint {
                        node: "osc_inst".into(),
                        port: Port::Named("freq_in".into()),
                    },
                },
                Connection {
                    source: Endpoint {
                        node: "gate".into(),
                        port: Port::None,
                    },
                    sink: Endpoint {
                        node: "env".into(),
                        port: Port::Named("gate".into()),
                    },
                },
                Connection {
                    source: Endpoint {
                        node: "osc_inst".into(),
                        port: Port::None,
                    },
                    sink: Endpoint {
                        node: "env".into(),
                        port: Port::Index(1),
                    },
                },
            ],
            sink: "env".into(),
        };

        let ast = Ast {
            macros: vec![fm_osc, voice],
            declarations: vec![
                DeclarationScope {
                    namespace: "audio".into(),
                    declarations: vec![NodeDeclaration {
                        node_type: "voice".into(),
                        alias: Some("lead".into()),
                        params: Some(object! { "freq" => 880.0f32, "attack" => 200.0f32 }),
                        pipes: vec![],
                        count: 1,
                    }],
                },
                DeclarationScope {
                    namespace: "midi".into(),
                    declarations: vec![NodeDeclaration {
                        node_type: "poly_voice".into(),
                        alias: Some("poly".into()),
                        params: None,
                        pipes: vec![],
                        count: 1,
                    }],
                },
            ],
            connections: vec![
                Connection {
                    source: Endpoint {
                        node: "poly".into(),
                        port: Port::Named("freq".into()),
                    },
                    sink: Endpoint {
                        node: "lead".into(),
                        port: Port::Named("voice_freq".into()),
                    },
                },
                Connection {
                    source: Endpoint {
                        node: "poly".into(),
                        port: Port::Named("gate".into()),
                    },
                    sink: Endpoint {
                        node: "lead".into(),
                        port: Port::Named("gate".into()),
                    },
                },
            ],
            sink: "lead".into(),
            ..Default::default()
        };

        let graph = expand(ast);

        for alias in [
            "lead.osc_inst.modulator",
            "lead.osc_inst.carrier",
            "lead.env",
        ] {
            assert!(
                graph.find_node_by_alias(alias).is_some(),
                "missing {}",
                alias
            );
        }

        let carrier = graph.find_node_by_alias("lead.osc_inst.carrier").unwrap();
        assert_eq!(carrier.params.get("freq"), Some(&Value::F32(880.0)));

        let env = graph.find_node_by_alias("lead.env").unwrap();
        assert_eq!(env.params.get("attack"), Some(&Value::F32(200.0)));

        // fm_osc interior: modulator → carrier[0]
        let mod_to_carrier =
            graph.find_edges_between("lead.osc_inst.modulator", "lead.osc_inst.carrier");
        assert_eq!(mod_to_carrier.len(), 1);
        assert_eq!(mod_to_carrier[0].sink_port, Port::Index(0));

        // voice interior: osc_inst (carrier) → env[1]
        let osc_to_env = graph.find_edges_between("lead.osc_inst.carrier", "lead.env");
        assert_eq!(osc_to_env.len(), 1);
        assert_eq!(osc_to_env[0].sink_port, Port::Index(1));

        // External connections resolved through two levels of virtual ports.
        let poly_edges = graph.find_edges_from("poly");

        let freq_edge = poly_edges
            .iter()
            .find(|e| e.source_port == Port::Named("freq".into()))
            .expect("poly.freq edge missing");
        assert_eq!(
            freq_edge.sink, carrier.id,
            "poly.freq should route through voice_freq → freq_in → carrier.freq"
        );
        assert_eq!(freq_edge.sink_port, Port::Named("freq".into()));

        let gate_edge = poly_edges
            .iter()
            .find(|e| e.source_port == Port::Named("gate".into()))
            .expect("poly.gate edge missing");
        assert_eq!(
            gate_edge.sink, env.id,
            "poly.gate should route through gate → env.gate"
        );
        assert_eq!(gate_edge.sink_port, Port::Named("gate".into()));

        // 1 fm_osc interior + 1 voice interior + 2 external = 4
        assert_eq!(graph.edge_count(), 4);
    }

    #[test]
    fn test_topological_sort_respects_edges() {
        let ast = Ast {
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![
                    NodeDeclaration {
                        node_type: "osc".into(),
                        alias: Some("src".into()),
                        params: None,
                        pipes: vec![],
                        count: 1,
                    },
                    NodeDeclaration {
                        node_type: "filter".into(),
                        alias: Some("mid".into()),
                        params: None,
                        pipes: vec![],
                        count: 1,
                    },
                    NodeDeclaration {
                        node_type: "output".into(),
                        alias: Some("snk".into()),
                        params: None,
                        pipes: vec![],
                        count: 1,
                    },
                ],
            }],
            connections: vec![
                Connection {
                    source: Endpoint {
                        node: "src".into(),
                        port: Port::None,
                    },
                    sink: Endpoint {
                        node: "mid".into(),
                        port: Port::None,
                    },
                },
                Connection {
                    source: Endpoint {
                        node: "mid".into(),
                        port: Port::None,
                    },
                    sink: Endpoint {
                        node: "snk".into(),
                        port: Port::None,
                    },
                },
            ],
            sink: "snk".into(),
            ..Default::default()
        };

        // No macros -> default pipeline is a no-op expansion, graph is unchanged.
        let graph = Pipeline::default().run_from_ast(ast);
        let order = graph.topological_sort();

        let pos = |alias: &str| {
            let id = graph.resolve_alias(alias).unwrap();
            order.iter().position(|&x| x == id).unwrap()
        };
        assert!(pos("src") < pos("mid"));
        assert!(pos("mid") < pos("snk"));
    }

    #[test]
    fn test_insert_between_splits_edge() {
        let ast = Ast {
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![
                    NodeDeclaration {
                        node_type: "osc".into(),
                        alias: Some("src".into()),
                        params: None,
                        pipes: vec![],
                        count: 1,
                    },
                    NodeDeclaration {
                        node_type: "output".into(),
                        alias: Some("snk".into()),
                        params: None,
                        pipes: vec![],
                        count: 1,
                    },
                ],
            }],
            connections: vec![Connection {
                source: Endpoint {
                    node: "src".into(),
                    port: Port::None,
                },
                sink: Endpoint {
                    node: "snk".into(),
                    port: Port::None,
                },
            }],
            sink: "snk".into(),
            ..Default::default()
        };

        let mut graph = Pipeline::default().run_from_ast(ast);
        assert_eq!(graph.edge_count(), 1);

        graph.insert_between(0, "audio", "meter", "meter_0", Default::default());

        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2);
        assert_eq!(graph.find_edges_between("src", "meter_0").len(), 1);
        assert_eq!(graph.find_edges_between("meter_0", "snk").len(), 1);

        let src_id = graph.resolve_alias("src").unwrap();
        let snk_id = graph.resolve_alias("snk").unwrap();
        assert_eq!(
            graph
                .outgoing_edges(src_id)
                .filter(|e| e.sink == snk_id)
                .count(),
            0,
            "original direct edge should be removed"
        );
    }
}
