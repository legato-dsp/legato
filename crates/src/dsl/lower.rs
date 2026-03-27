use crate::dsl::ir::*;
use indexmap::IndexMap;
use std::collections::HashMap;

const MAXIMUM_DEPTH: u8 = 16;

/// A single, named transformation of an [`IRGraph`].
pub trait GraphPass {
    fn name(&self) -> &'static str;
    fn run(&self, graph: IRGraph) -> IRGraph;
}

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
        Self::new()
            .add_pass(MacroExpansionPass::default())
            .add_pass(SpawnKNodesPass::default())
    }
}

/// Convert the ASTMacro to the IRMacro
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
        body.connect_multi(
            src,
            conn.source.node_selector.clone(),
            conn.source.port.clone(),
            snk,
            conn.sink.node_selector.clone(),
            conn.sink.port.clone(),
        );
    }

    let virtual_input_map = ast_macro
        .connections
        .iter()
        .filter(|c| ast_macro.virtual_ports_in.contains(&c.source.node))
        .map(|c| {
            let target_id = local_alias_to_id[&c.sink.node];
            (
                c.source.node.clone(),
                (target_id, c.sink.node_selector.clone(), c.sink.port.clone()),
            )
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
        let src = *alias_to_id
            .get(&conn.source.node)
            .unwrap_or_else(|| panic!("ast_to_graph: source '{}' not found", conn.source.node));
        let snk = *alias_to_id
            .get(&conn.sink.node)
            .unwrap_or_else(|| panic!("ast_to_graph: sink '{}' not found", conn.sink.node));
        graph.connect_multi(
            src,
            conn.source.node_selector.clone(),
            conn.source.port.clone(),
            snk,
            conn.sink.node_selector.clone(),
            conn.sink.port.clone(),
        );
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
                self.expand_macro(&mut graph, id);
            }
            depth += 1;
        }
        graph
    }
}

impl MacroExpansionPass {
    fn expand_macro(&self, graph: &mut IRGraph, node_id: NodeId) {
        let node = graph.get_node(node_id).unwrap().clone();

        let ir_macro = graph
            .macro_registry
            .get(&node.node_type)
            .cloned()
            .unwrap_or_else(|| panic!("Macro '{}' not found in registry", node.node_type));

        let mut resolved_params = ir_macro.default_params.clone().unwrap_or_default();
        for (k, v) in &node.params {
            resolved_params.insert(k.clone(), v.clone());
        }

        let incoming: Vec<IREdge> = graph.incoming_edges(node_id).cloned().collect();
        let outgoing: Vec<IREdge> = graph.outgoing_edges(node_id).cloned().collect();
        graph.remove_node(node_id);

        // Expand n=count instances, each with a distinct alias prefix.
        let mut new_sinks: Vec<NodeId> = Vec::with_capacity(node.count as usize);

        for i in 0..node.count as usize {
            let instance_alias = if node.count == 1 {
                node.alias.clone()
            } else {
                format!("{}.{}", node.alias, i)
            };

            let id_map = self.clone_body_into(graph, &ir_macro, &instance_alias, &resolved_params);
            let new_sink = id_map[&ir_macro.sink];
            new_sinks.push(new_sink);

            let remapped_virtual: IndexMap<String, (NodeId, NodeSelector, Port)> = ir_macro
                .virtual_input_map
                .iter()
                .map(|(name, (id, sel, port))| {
                    (name.clone(), (id_map[id], sel.clone(), port.clone()))
                })
                .collect();

            // Rewire incoming edges into each instance.
            for edge in &incoming {
                let (target_id, target_selector, target_port) = match &edge.sink_port {
                    Port::Named(name) => remapped_virtual.get(name).map_or(
                        (new_sink, NodeSelector::Single, edge.sink_port.clone()),
                        |(id, sel, port)| (*id, sel.clone(), port.clone()),
                    ),
                    Port::Index(i) => remapped_virtual.get_index(*i).map_or(
                        (new_sink, NodeSelector::Single, edge.sink_port.clone()),
                        |(_, (id, sel, port))| (*id, sel.clone(), port.clone()),
                    ),
                    Port::None => remapped_virtual.get_index(0).map_or(
                        (new_sink, NodeSelector::Single, Port::None),
                        |(_, (id, sel, port))| (*id, sel.clone(), port.clone()),
                    ),
                    Port::Slice(..) | Port::Stride { .. } => panic!(
                        "Slice/Stride not supported on virtual ports (macro '{}')",
                        node.node_type
                    ),
                };
                graph.connect_multi(
                    edge.source,
                    edge.source_selector.clone(),
                    edge.source_port.clone(),
                    target_id,
                    target_selector,
                    target_port,
                );
            }
        }

        // Rewire outgoing edges from the last instance
        for edge in &outgoing {
            let srcs = edge.source_selector.select(&new_sinks).to_vec();
            for &src in &srcs {
                graph.connect_multi(
                    src,
                    NodeSelector::Single,
                    edge.source_port.clone(),
                    edge.sink,
                    edge.sink_selector.clone(),
                    edge.sink_port.clone(),
                );
            }
        }

        if graph.sink == Some(node_id) {
            graph.sink = new_sinks.last().copied();
        }
        if graph.source == Some(node_id) {
            graph.source = new_sinks.first().copied();
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
            graph.reconnect(id_map[&edge.source], id_map[&edge.sink], edge);
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

#[derive(Default)]
pub struct SpawnKNodesPass;

impl GraphPass for SpawnKNodesPass {
    fn name(&self) -> &'static str {
        "SpawnKNodesPass"
    }

    fn run(&self, graph: IRGraph) -> IRGraph {
        self.expand_nodes(graph)
    }
}

impl SpawnKNodesPass {
    fn expand_nodes(&self, mut graph: IRGraph) -> IRGraph {
        let multi: Vec<(NodeId, IRNode)> = graph
            .nodes()
            .filter(|n| n.count > 1)
            .map(|n| (n.id, n.clone()))
            .collect();

        if multi.is_empty() {
            return graph;
        }

        let multi_ids: std::collections::HashSet<NodeId> =
            multi.iter().map(|(id, _)| *id).collect();

        // ── Phase 1: spawn N instances for every multi-node ────────────────
        let mut expansion: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        for (orig_id, node) in &multi {
            let mut instances = Vec::with_capacity(node.count as usize);
            for i in 0..node.count as usize {
                let alias = format!("{}.{}", node.alias, i);
                let new_id = graph.add_node(
                    node.kind.clone(),
                    node.namespace.clone(),
                    node.node_type.clone(),
                    alias,
                    node.params.clone(),
                    node.pipes.clone(),
                    1,
                );
                instances.push(new_id);
            }
            expansion.insert(*orig_id, instances);
        }

        // ── Phase 2: expand edges that touch any multi-node ────────────────
        let snapshot: Vec<IREdge> = graph.edges().to_vec();

        for edge in &snapshot {
            let src_multi = multi_ids.contains(&edge.source);
            let snk_multi = multi_ids.contains(&edge.sink);
            if !src_multi && !snk_multi {
                continue; // unaffected — left in place
            }

            let src_pool: Vec<NodeId> = if src_multi {
                expansion[&edge.source].clone()
            } else {
                vec![edge.source]
            };
            let snk_pool: Vec<NodeId> = if snk_multi {
                expansion[&edge.sink].clone()
            } else {
                vec![edge.sink]
            };

            let srcs = edge.source_selector.select(&src_pool).to_vec();
            let snks = edge.sink_selector.select(&snk_pool).to_vec();

            Self::expand_edge(&mut graph, edge, &srcs, &snks);
        }

        // ── Phase 3: remove originals (also removes their incident edges) ──
        for (orig_id, _) in &multi {
            if graph.sink == Some(*orig_id) {
                // Last instance is the natural graph output.
                graph.sink = expansion[orig_id].last().copied();
            }
            if graph.source == Some(*orig_id) {
                graph.source = expansion[orig_id].first().copied();
            }
            graph.remove_node(*orig_id);
        }

        graph
    }

    /// Wire up a concrete set of source and sink NodeIds according to the
    /// original edge's port configuration.
    fn expand_edge(graph: &mut IRGraph, edge: &IREdge, srcs: &[NodeId], snks: &[NodeId]) {
        match &edge.sink_port {
            Port::Slice(start, end) => {
                assert_eq!(
                    srcs.len(),
                    end - start,
                    "SpawnKNodesPass: source instance count ({}) must equal \
                     port slice width ({}) for edge to {:?}",
                    srcs.len(),
                    end - start,
                    edge.sink,
                );
                let snk = snks[0];
                for (i, &src) in srcs.iter().enumerate() {
                    graph.connect(src, edge.source_port.clone(), snk, Port::Index(start + i));
                }
            }
            _ => match (srcs.len(), snks.len()) {
                (1, _) => {
                    // Broadcast: one source -> all sinks.
                    for &snk in snks {
                        graph.connect(
                            srcs[0],
                            edge.source_port.clone(),
                            snk,
                            edge.sink_port.clone(),
                        );
                    }
                }
                (_, 1) => {
                    // Fan-in: all sources -> single sink.
                    for &src in srcs {
                        graph.connect(
                            src,
                            edge.source_port.clone(),
                            snks[0],
                            edge.sink_port.clone(),
                        );
                    }
                }
                (n, m) => {
                    // Automap / zip.
                    assert_eq!(
                        n, m,
                        "SpawnKNodesPass: cannot automap nodes with different \
                         instance counts ({n} vs {m})"
                    );
                    for (&src, &snk) in srcs.iter().zip(snks.iter()) {
                        graph.connect(src, edge.source_port.clone(), snk, edge.sink_port.clone());
                    }
                }
            },
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
        $( _map.insert($key.to_string(), $crate::dsl::ir::Value::from($value)); )*
        _map
    }};
}

#[cfg(test)]
mod spawn_tests {
    use super::*;

    fn expand(ast: Ast) -> IRGraph {
        Pipeline::default().run_from_ast(ast)
    }

    fn multi_decl(node_type: &str, alias: &str, count: u32) -> NodeDeclaration {
        NodeDeclaration {
            node_type: node_type.into(),
            alias: Some(alias.into()),
            count,
            ..Default::default()
        }
    }

    fn scope(ns: &str, decls: Vec<NodeDeclaration>) -> DeclarationScope {
        DeclarationScope {
            namespace: ns.into(),
            declarations: decls,
        }
    }

    fn conn(
        src: &str,
        src_sel: NodeSelector,
        src_port: Port,
        snk: &str,
        snk_sel: NodeSelector,
        snk_port: Port,
    ) -> Connection {
        Connection {
            source: Endpoint {
                node: src.into(),
                node_selector: src_sel,
                port: src_port,
            },
            sink: Endpoint {
                node: snk.into(),
                node_selector: snk_sel,
                port: snk_port,
            },
        }
    }

    #[test]
    fn test_spawn_creates_n_instances() {
        let ast = Ast {
            declarations: vec![scope("audio", vec![multi_decl("sine", "osc", 4)])],
            sink: "osc".into(),
            ..Default::default()
        };
        let graph = expand(ast);

        assert_eq!(graph.node_count(), 4);
        for i in 0..4 {
            assert!(
                graph.find_node_by_alias(&format!("osc.{i}")).is_some(),
                "missing osc.{i}"
            );
        }
        // No edges declared -> no edges produced.
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn test_spawn_instances_inherit_params() {
        let ast = Ast {
            declarations: vec![scope(
                "audio",
                vec![NodeDeclaration {
                    node_type: "sine".into(),
                    alias: Some("osc".into()),
                    count: 3,
                    params: Some(object! { "freq" => 220.0f32 }),
                    ..Default::default()
                }],
            )],
            sink: "osc".into(),
            ..Default::default()
        };
        let graph = expand(ast);

        for i in 0..3 {
            assert_eq!(
                graph
                    .find_node_by_alias(&format!("osc.{i}"))
                    .unwrap()
                    .params
                    .get("freq"),
                Some(&Value::F32(220.0)),
                "osc.{i} should have freq=220.0"
            );
        }
    }

    // ── automap (*) >> (*) ─────────────────────────────────────────────────

    #[test]
    fn test_automap_all_to_all() {
        // my_mod(*) >> my_carrier(*)  — both count=4
        let ast = Ast {
            declarations: vec![scope(
                "audio",
                vec![
                    multi_decl("sine", "modulator", 4),
                    multi_decl("sine", "carrier", 4),
                ],
            )],
            connections: vec![conn(
                "modulator",
                NodeSelector::All,
                Port::None,
                "carrier",
                NodeSelector::All,
                Port::Index(0),
            )],
            sink: "carrier".into(),
            ..Default::default()
        };
        let graph = expand(ast);

        assert_eq!(graph.node_count(), 8);
        // One edge per pair: 4 modulator->carrier edges.
        assert_eq!(graph.edge_count(), 4);
        for i in 0..4 {
            let src_alias = format!("modulator.{i}");
            let snk_alias = format!("carrier.{i}");
            let edges = graph.find_edges_between(&src_alias, &snk_alias);
            assert_eq!(edges.len(), 1, "expected edge {src_alias} -> {snk_alias}");
            assert_eq!(edges[0].sink_port, Port::Index(0));
        }
    }

    // ── range selector ─────────────────────────────────────────────────────

    #[test]
    fn test_range_selector_partial_zip() {
        // my_source(1..3).out >> my_sink(1..3).audio_in — pick instances 1 and 2
        let ast = Ast {
            declarations: vec![scope(
                "audio",
                vec![multi_decl("osc", "src", 4), multi_decl("filter", "snk", 4)],
            )],
            connections: vec![conn(
                "src",
                NodeSelector::Range(1, 3),
                Port::Named("out".into()),
                "snk",
                NodeSelector::Range(1, 3),
                Port::Named("audio_in".into()),
            )],
            sink: "snk".into(),
            ..Default::default()
        };
        let graph = expand(ast);

        assert_eq!(graph.edge_count(), 2);
        for i in 1..3 {
            let edges = graph.find_edges_between(&format!("src.{i}"), &format!("snk.{i}"));
            assert_eq!(edges.len(), 1, "expected src.{i} -> snk.{i}");
            assert_eq!(edges[0].source_port, Port::Named("out".into()));
            assert_eq!(edges[0].sink_port, Port::Named("audio_in".into()));
        }
        // src.0 and src.3 should have no edges.
        assert!(graph.find_edges_from("src.0").is_empty());
        assert!(graph.find_edges_from("src.3").is_empty());
    }

    #[test]
    fn test_source_range_to_sink_port_slice() {
        // my_example(0..2).out >> mixer[0..2]
        let ast = Ast {
            declarations: vec![scope(
                "audio",
                vec![multi_decl("osc", "src", 4), multi_decl("mixer", "mixer", 1)],
            )],
            connections: vec![conn(
                "src",
                NodeSelector::Range(0, 2),
                Port::Named("out".into()),
                "mixer",
                NodeSelector::Single,
                Port::Slice(0, 2),
            )],
            sink: "mixer".into(),
            ..Default::default()
        };

        let graph = expand(ast);

        assert_eq!(graph.edge_count(), 2);

        let edges_to_mixer = graph.find_edges_to("mixer");
        assert_eq!(edges_to_mixer.len(), 2);

        let slot0 = edges_to_mixer
            .iter()
            .find(|e| e.sink_port == Port::Index(0))
            .expect("[0] missing");

        let slot1 = edges_to_mixer
            .iter()
            .find(|e| e.sink_port == Port::Index(1))
            .expect("[1] missing");

        let src0 = graph.find_node_by_alias("src.0").unwrap();
        let src1 = graph.find_node_by_alias("src.1").unwrap();
        assert_eq!(slot0.source, src0.id);
        assert_eq!(slot1.source, src1.id);
    }

    #[test]
    fn test_broadcast_single_source_to_multi_sink() {
        let ast = Ast {
            declarations: vec![scope(
                "audio",
                vec![
                    NodeDeclaration {
                        node_type: "lfo".into(),
                        alias: Some("lfo".into()),
                        count: 1,
                        ..Default::default()
                    },
                    multi_decl("filter", "filt", 4),
                ],
            )],
            connections: vec![conn(
                "lfo",
                NodeSelector::Single,
                Port::None,
                "filt",
                NodeSelector::All,
                Port::Named("cutoff".into()),
            )],
            sink: "filt".into(),
            ..Default::default()
        };
        let graph = expand(ast);

        assert_eq!(graph.edge_count(), 4);
        let lfo = graph.find_node_by_alias("lfo").unwrap();
        for i in 0..4 {
            let filt = graph.find_node_by_alias(&format!("filt.{i}")).unwrap();
            let edges = graph.find_edges_between("lfo", &format!("filt.{i}"));
            assert_eq!(edges.len(), 1, "expected lfo -> filt.{i}");
            assert_eq!(edges[0].source, lfo.id);
            assert_eq!(edges[0].sink, filt.id);
            assert_eq!(edges[0].sink_port, Port::Named("cutoff".into()));
        }
    }

    #[test]
    fn test_multi_node_inside_macro_expands_with_fqn() {
        // patch poly_voice { sine: osc * 4 { freq: 440.0 } }
        let poly_voice = AstMacro {
            name: "poly_voice".into(),
            default_params: Some(object! { "freq" => 440.0f32 }),
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "sine".into(),
                    alias: Some("osc".into()),
                    count: 4,
                    params: Some(object! { "freq" => Value::Template("$freq".into()) }),
                    ..Default::default()
                }],
            }],
            sink: "osc".into(),
            ..Default::default()
        };

        let ast = Ast {
            macros: vec![poly_voice],
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "poly_voice".into(),
                    alias: Some("lead".into()),
                    params: Some(object! { "freq" => 880.0f32 }),
                    count: 1,
                    ..Default::default()
                }],
            }],
            sink: "lead".into(),
            ..Default::default()
        };

        dbg!(&ast);

        let graph = expand(ast);
        assert_eq!(graph.node_count(), 4);

        for i in 0..4 {
            let alias = format!("lead.osc.{i}");
            let node = graph
                .find_node_by_alias(&alias)
                .unwrap_or_else(|| panic!("missing {alias}"));
            // Param substituted through the macro.
            assert_eq!(node.params.get("freq"), Some(&Value::F32(880.0)));
        }
    }

    #[test]
    fn test_index_selector_connects_single_instance() {
        // connect only osc(2) -> filter
        let ast = Ast {
            declarations: vec![scope(
                "audio",
                vec![multi_decl("osc", "osc", 4), multi_decl("filter", "filt", 1)],
            )],
            connections: vec![conn(
                "osc",
                NodeSelector::Index(2),
                Port::None,
                "filt",
                NodeSelector::Single,
                Port::None,
            )],
            sink: "filt".into(),
            ..Default::default()
        };
        let graph = expand(ast);

        assert_eq!(graph.edge_count(), 1);
        let edges = graph.find_edges_between("osc.2", "filt");
        assert_eq!(edges.len(), 1);
    }
}
