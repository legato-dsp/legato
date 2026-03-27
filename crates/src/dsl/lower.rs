use crate::dsl::ir::*;
use std::collections::HashMap;

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
    use crate::dsl::pipeline::Pipeline;

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
