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
    /// The standard pipeline: literal translation → macro expansion.
    /// Further passes (sample-rate conversion, port expansion, etc.) are
    /// added on top of this baseline.
    fn default() -> Self {
        Self::new().add_pass(MacroExpansionPass::default())
    }
}

// ---------------------------------------------------------------------------
// ast_to_graph  —  step 0: pure literal translation
// ---------------------------------------------------------------------------

/// Translate an [`Ast`] into a literal [`IRGraph`] **without any expansion**.
///
/// This is always the first step in any pipeline.  The graph produced here
/// mirrors the DSL source text exactly:
///
/// - Macro definitions are stored in [`IRGraph::macro_registry`].
/// - Macro instantiations become [`IRNodeKind::MacroRef`] nodes.
/// - Connections are preserved verbatim (virtual ports are *not* resolved yet).
/// - Parameters are stored as-is (templates are *not* substituted yet).
///
/// Inspecting the graph at this stage is the easiest way to understand the
/// high-level topology the developer wrote.
pub fn ast_to_graph(ast: Ast) -> IRGraph {
    let mut graph = IRGraph::new();

    // Register all macro definitions first so we can classify node types.
    for m in ast.macros {
        graph.macro_registry.insert(m.name.clone(), m);
    }

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

// ---------------------------------------------------------------------------
// MacroExpansionPass
// ---------------------------------------------------------------------------

/// Expands all [`IRNodeKind::MacroRef`] nodes into their constituent leaf
/// nodes, resolves virtual ports to concrete edges, and applies template
/// parameter substitution.
///
/// After this pass, [`IRGraph::has_unresolved_macros`] will return `false`
/// and every alias will be a fully-qualified name.
///
/// The macro registry on the graph is **not** cleared after expansion so
/// that later passes or the builder can still consult it if needed.
#[derive(Default)]
pub struct MacroExpansionPass;

impl GraphPass for MacroExpansionPass {
    fn name(&self) -> &'static str {
        "MacroExpansionPass"
    }

    fn run(&self, mut graph: IRGraph) -> IRGraph {
        // Collect all MacroRef nodes that were present *before* we start
        // expanding.  Expansion adds only Leaf nodes, so one pass is enough
        // to resolve all top-level macro instantiations; recursive calls
        // inside `expand_macro_into_graph` handle nested macros.
        let macro_ids: Vec<NodeId> = graph.macro_nodes().map(|n| n.id).collect();

        for macro_id in macro_ids {
            self.expand_node(&mut graph, macro_id, 0);
        }

        debug_assert!(
            !graph.has_unresolved_macros(),
            "MacroExpansionPass finished but unresolved MacroRef nodes remain"
        );

        graph
    }
}

impl MacroExpansionPass {
    /// Expand a single `MacroRef` node in-place:
    ///
    /// 1. Recursively expand the macro's definition into the graph.
    /// 2. Collect all edges incident to the macro node.
    /// 3. Remove the macro node (and its incident edges).
    /// 4. Rewire the collected edges through the expansion result.
    fn expand_node(&self, graph: &mut IRGraph, node_id: NodeId, depth: u8) {
        let node = graph
            .get_node(node_id)
            .expect("expand_node: NodeId not in graph")
            .clone();

        debug_assert_eq!(
            node.kind,
            IRNodeKind::MacroRef,
            "expand_node called on a Leaf node"
        );

        let macro_def = graph
            .macro_registry
            .get(&node.node_type)
            .cloned()
            .unwrap_or_else(|| panic!("Macro '{}' not found in registry", node.node_type));

        // Snapshot incident edges before we mutate the graph.
        let incoming: Vec<IREdge> = graph.incoming_edges(node_id).cloned().collect();
        let outgoing: Vec<IREdge> = graph.outgoing_edges(node_id).cloned().collect();

        // Remove the macro node — this also drops its incident edges.
        graph.remove_node(node_id);

        // Recursively expand the macro definition into `graph`.
        let ExpansionResult {
            source_id,
            virtual_inputs,
        } = self.expand_macro_into_graph(
            graph,
            &macro_def,
            &node.alias, // becomes the FQN prefix for children
            "",
            &node.params,
            &node.namespace,
            depth,
        );

        // Rewire incoming edges.
        //
        // An incoming edge to a macro node either:
        //   (a) targets a named/indexed virtual port → route to the concrete
        //       leaf that the virtual port maps to, or
        //   (b) uses Port::None → wire to the macro's sink leaf.
        for edge in incoming {
            let (new_sink_id, new_sink_port) = match &edge.sink_port {
                Port::Named(name) => virtual_inputs
                    .get(name)
                    .map_or((source_id, edge.sink_port.clone()), |(id, port)| {
                        (*id, port.clone())
                    }),
                Port::Index(i) => virtual_inputs
                    .get_index(*i)
                    .map_or((source_id, edge.sink_port.clone()), |(_, (id, port))| {
                        (*id, port.clone())
                    }),
                Port::None => (source_id, Port::None),
                Port::Slice(_, _) => panic!(
                    "Port::Slice is not supported on virtual ports (macro '{}')",
                    node.node_type
                ),
            };
            graph.connect(edge.source, edge.source_port, new_sink_id, new_sink_port);
        }

        // Rewire outgoing edges: they all originate from the macro's sink leaf.
        for edge in outgoing {
            graph.connect(source_id, edge.source_port, edge.sink, edge.sink_port);
        }

        // Update the graph-level sink / source pointers.
        if graph.sink == Some(node_id) {
            graph.sink = Some(source_id);
        }
        if graph.source == Some(node_id) {
            graph.source = Some(source_id);
        }
    }

    /// Recursively expand one macro definition into `graph`.
    ///
    /// Adds all leaf descendant nodes and their internal edges.  Returns the
    /// [`ExpansionResult`] that lets the caller rewire connections through
    /// virtual ports.
    fn expand_macro_into_graph(
        &self,
        graph: &mut IRGraph,
        m: &Macro,
        instance_name: &str,
        parent_prefix: &str,
        params: &Object,
        outer_namespace: &str,
        depth: u8,
    ) -> ExpansionResult {
        if depth > MAXIMUM_DEPTH {
            panic!(
                "Max macro depth ({}) exceeded at {}::{}",
                MAXIMUM_DEPTH, parent_prefix, instance_name
            );
        }

        let current_prefix = if parent_prefix.is_empty() {
            instance_name.to_string()
        } else {
            format!("{}.{}", parent_prefix, instance_name)
        };

        // Merge default params, then override with call-site params.
        let mut resolved_params = m.default_params.clone().unwrap_or_default();
        for (k, v) in params {
            resolved_params.insert(k.clone(), v.clone());
        }

        // Build a local symbol table: local alias → ExpansionResult.
        // For leaf nodes the virtual_inputs map is empty and source_id
        // is the node's own id.
        let mut local: HashMap<String, ExpansionResult> = HashMap::new();

        for scope in &m.declarations.clone() {
            let ns = if scope.namespace.is_empty() {
                outer_namespace
            } else {
                &scope.namespace
            };

            for decl in &scope.declarations {
                let local_alias = decl.alias.as_ref().unwrap_or(&decl.node_type).clone();

                if let Some(inner_macro) = graph.macro_registry.get(&decl.node_type).cloned() {
                    let mut inner_params = decl.params.clone().unwrap_or_default();
                    Self::substitute_templates(&mut inner_params, &resolved_params);

                    let result = self.expand_macro_into_graph(
                        graph,
                        &inner_macro,
                        &local_alias,
                        &current_prefix,
                        &inner_params,
                        ns,
                        depth + 1,
                    );
                    local.insert(local_alias, result);
                } else {
                    let fqn = format!("{}.{}", current_prefix, local_alias);
                    let mut node_params = decl.params.clone().unwrap_or_default();
                    Self::substitute_templates(&mut node_params, &resolved_params);

                    let id = graph.add_node(
                        IRNodeKind::Leaf,
                        ns,
                        decl.node_type.clone(),
                        fqn,
                        node_params,
                        decl.pipes.clone(),
                    );
                    local.insert(
                        local_alias,
                        ExpansionResult {
                            source_id: id,
                            virtual_inputs: IndexMap::new(),
                        },
                    );
                }
            }
        }

        // Process this macro's internal connections.
        // Virtual connections populate the virtual_inputs map returned to the
        // caller; real connections are committed to the graph immediately.
        let mut virtual_inputs: IndexMap<String, (NodeId, Port)> = IndexMap::new();

        for conn in &m.connections {
            if m.virtual_ports_in.contains(&conn.source.node) {
                // Virtual port: record where it routes, for the caller to use
                // when rewiring incoming edges.
                let target = local.get(&conn.sink.node).unwrap_or_else(|| {
                    panic!(
                        "Virtual port '{}' routes to unknown node '{}' in macro '{}'",
                        conn.source.node, conn.sink.node, m.name
                    )
                });

                let (target_id, target_port) = match &conn.sink.port {
                    Port::Named(port_name) => {
                        // If the target itself is a macro with virtual inputs,
                        // follow through to the actual leaf.
                        target
                            .virtual_inputs
                            .get(port_name)
                            .map_or((target.source_id, conn.sink.port.clone()), |(id, port)| {
                                (*id, port.clone())
                            })
                    }
                    _ => (target.source_id, conn.sink.port.clone()),
                };

                virtual_inputs.insert(conn.source.node.clone(), (target_id, target_port));
            } else {
                // Normal internal connection.
                let src = local.get(&conn.source.node).unwrap_or_else(|| {
                    panic!(
                        "Source '{}' not found in macro '{}' scope",
                        conn.source.node, m.name
                    )
                });
                let snk = local.get(&conn.sink.node).unwrap_or_else(|| {
                    panic!(
                        "Sink '{}' not found in macro '{}' scope",
                        conn.sink.node, m.name
                    )
                });

                let source_id = src.source_id;
                let (sink_id, sink_port) = if snk.virtual_inputs.is_empty() {
                    (snk.source_id, conn.sink.port.clone())
                } else {
                    match &conn.sink.port {
                        Port::Named(name) => snk
                            .virtual_inputs
                            .get(name)
                            .map_or((snk.source_id, Port::None), |(id, port)| {
                                (*id, port.clone())
                            }),
                        Port::Index(i) => snk
                            .virtual_inputs
                            .get_index(*i)
                            .map_or((snk.source_id, Port::None), |(_, (id, port))| {
                                (*id, port.clone())
                            }),
                        Port::None => (snk.source_id, Port::None),
                        Port::Slice(_, _) => panic!("Slices are not supported on virtual ports"),
                    }
                };

                graph.connect(source_id, conn.source.port.clone(), sink_id, sink_port);
            }
        }

        let sink_result = local
            .get(&m.sink)
            .unwrap_or_else(|| panic!("Sink alias '{}' not found in macro '{}'", m.sink, m.name));

        ExpansionResult {
            source_id: sink_result.source_id,
            virtual_inputs,
        }
    }

    /// Replace every `Value::Template("$key")` in `params` with the
    /// matching value from `lookup`.
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

// ---------------------------------------------------------------------------
// object! — lives here because it constructs ir::Object values
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Internal helper
// ---------------------------------------------------------------------------

/// The result of expanding one macro instance.
///
/// Used only within [`MacroExpansionPass`]; not part of the public API.
struct ExpansionResult {
    /// The NodeId of the leaf that serves as this macro's audio output
    /// (i.e. the macro's `sink` leaf after full recursive expansion).
    source_id: NodeId,
    /// Virtual port name → (target NodeId, target Port).
    virtual_inputs: IndexMap<String, (NodeId, Port)>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use indexmap::IndexSet;

    use super::*;

    fn make_voice_macro() -> Macro {
        Macro {
            name: "voice".into(),
            default_params: Some(object! {
                "freq" => 440.0f32,
                "attack" => 100.0f32,
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
                    },
                    NodeDeclaration {
                        node_type: "adsr".into(),
                        alias: Some("env".into()),
                        params: Some(object! {
                            "attack" => Value::Template("$attack".into()),
                            "release" => Value::Template("$release".into())
                        }),
                        pipes: vec![],
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
                    },
                    NodeDeclaration {
                        node_type: "track_mixer".into(),
                        alias: Some("mixer".into()),
                        params: None,
                        pipes: vec![],
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

        // Connection is present verbatim — virtual ports not resolved.
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

    // -----------------------------------------------------------------------
    // MacroExpansionPass tests (mirror the original lower.rs tests)
    // -----------------------------------------------------------------------

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
                    params: Some(object!("freq" => 420.0f32)),
                    pipes: vec![],
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

        // Only the interior osc → env edge; virtual-port connections produce no edge.
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
                    }],
                },
                DeclarationScope {
                    namespace: "midi".into(),
                    declarations: vec![NodeDeclaration {
                        node_type: "poly_voice".into(),
                        alias: Some("poly".into()),
                        params: None,
                        pipes: vec![],
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
                    },
                    NodeDeclaration {
                        node_type: "voice".into(),
                        alias: Some("v2".into()),
                        params: Some(object! { "freq" => 880.0f32 }),
                        pipes: vec![],
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
                    },
                    NodeDeclaration {
                        node_type: "track_mixer".into(),
                        alias: Some("mixer".into()),
                        params: None,
                        pipes: vec![],
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
        assert_eq!(edges_to_mixer[0].source, env.id);
    }

    #[test]
    fn test_nested_macro_virtual_ports_and_connections() {
        let fm_osc = Macro {
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
                    },
                    NodeDeclaration {
                        node_type: "sine".into(),
                        alias: Some("carrier".into()),
                        params: Some(object! { "freq" => Value::Template("$freq".into()) }),
                        pipes: vec![],
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

        let voice = Macro {
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
                    },
                    NodeDeclaration {
                        node_type: "adsr".into(),
                        alias: Some("env".into()),
                        params: Some(object! { "attack" => Value::Template("$attack".into()) }),
                        pipes: vec![],
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
                    }],
                },
                DeclarationScope {
                    namespace: "midi".into(),
                    declarations: vec![NodeDeclaration {
                        node_type: "poly_voice".into(),
                        alias: Some("poly".into()),
                        params: None,
                        pipes: vec![],
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

        let mod_to_carrier =
            graph.find_edges_between("lead.osc_inst.modulator", "lead.osc_inst.carrier");
        assert_eq!(mod_to_carrier.len(), 1);
        assert_eq!(mod_to_carrier[0].sink_port, Port::Index(0));

        let osc_to_env = graph.find_edges_between("lead.osc_inst.carrier", "lead.env");
        assert_eq!(osc_to_env.len(), 1);
        assert_eq!(osc_to_env[0].sink_port, Port::Index(1));

        let poly_edges = graph.find_edges_from("poly");
        let freq_edge = poly_edges
            .iter()
            .find(|e| e.source_port == Port::Named("freq".into()))
            .unwrap();
        assert_eq!(freq_edge.sink, carrier.id);
        assert_eq!(freq_edge.sink_port, Port::Named("freq".into()));

        let gate_edge = poly_edges
            .iter()
            .find(|e| e.source_port == Port::Named("gate".into()))
            .unwrap();
        assert_eq!(gate_edge.sink, env.id);
        assert_eq!(gate_edge.sink_port, Port::Named("gate".into()));

        // 1 fm_osc interior + 1 voice interior + 2 external = 4
        assert_eq!(graph.edge_count(), 4);
    }

    // -----------------------------------------------------------------------
    // Graph algorithm tests
    // -----------------------------------------------------------------------

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
                    },
                    NodeDeclaration {
                        node_type: "filter".into(),
                        alias: Some("mid".into()),
                        params: None,
                        pipes: vec![],
                    },
                    NodeDeclaration {
                        node_type: "output".into(),
                        alias: Some("snk".into()),
                        params: None,
                        pipes: vec![],
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

        // No macros → default pipeline is a no-op expansion, graph is unchanged.
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
                    },
                    NodeDeclaration {
                        node_type: "output".into(),
                        alias: Some("snk".into()),
                        params: None,
                        pipes: vec![],
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
