use legato::{
    ast::{Ast, ExpandedNode, PortConnectionType, Sink, Value, build_ast},
    parse::{LegatoParser, Rule, print_pair},
    pipes::PipeRegistry,
};
use pest::Parser;

fn parse_ast(input: &str) -> Ast {
    let pairs = LegatoParser::parse(Rule::graph, input).expect("PEST failed");

    print_pair(&(pairs.clone().next().unwrap()), 4);

    let registry = PipeRegistry::default();

    build_ast(pairs, &registry).expect("AST lowering failed")
}

#[test]
fn ast_node_with_alias_and_params() {
    let ast = parse_ast(
        r#"
        audio {
            osc: square_wave_one { freq: 440, gain: 0.2 }
        }
        { osc }
    "#,
    );

    assert_eq!(ast.declarations.len(), 1);
    let scope = &ast.declarations[0];
    assert_eq!(scope.namespace, "audio");

    assert_eq!(scope.declarations.len(), 1);
    let node_one = &scope.declarations[0];

    match node_one {
        ExpandedNode::Node(inner) => {
            assert_eq!(inner.node_type, "osc");
            assert_eq!(inner.alias, "square_wave_one");

            assert_eq!(inner.params["freq"], Value::U32(440));
            assert_eq!(inner.params["gain"], Value::F32(0.2));
        }
        _ => panic!(),
    };

    let sink = ast.sink;
    assert_eq!(
        sink,
        Sink {
            name: String::from("osc")
        }
    )
}

#[test]
fn ast_graph_with_connections() {
    let graph = r#"
        audio {
            sine_mono: mod { freq: 891.0 },
            sine_stereo: carrier { freq: 440.0 },
            mult_mono: fm_gain { val: 1000.0 }
        }

        mod[0] >> fm_gain[0] >> carrier[0]

        { carrier }
    "#;

    let ast = parse_ast(graph);

    assert_eq!(ast.declarations.len(), 1);
    assert_eq!(ast.connections.len(), 2);

    let c0 = &ast.connections[0];
    let c1 = &ast.connections[1];

    assert!(matches!(
        (&c0.source_port, &c0.sink_port),
        (
            PortConnectionType::Indexed { port: 0 },
            PortConnectionType::Indexed { port: 0 }
        )
    ));

    assert!(matches!(
        (&c1.source_port, &c1.sink_port),
        (
            PortConnectionType::Indexed { port: 0 },
            PortConnectionType::Indexed { port: 0 }
        )
    ));
}

#[test]
fn ast_graph_with_port_slices() {
    let graph = r#"
        audio {
            osc: stereo_osc { freq: 440.0, chans: 2 },
            gain: stereo_gain { val: 0.5, chans: 4 }
        }

        // test new slice syntax
        stereo_osc[0..1] >> stereo_gain[2..4]

        { gain_stage }
    "#;

    let ast = parse_ast(graph);

    assert_eq!(ast.declarations.len(), 1);
    let scope = &ast.declarations[0];
    assert_eq!(scope.namespace, "audio");

    assert_eq!(scope.declarations.len(), 2);
    let osc = &scope.declarations[0];
    let gain = &scope.declarations[1];

    match osc {
        ExpandedNode::Node(inner) => {
            assert_eq!(inner.node_type, "osc");
            assert_eq!(inner.alias, "stereo_osc");
        }
        _ => panic!(),
    };

    match gain {
        ExpandedNode::Node(inner) => {
            assert_eq!(inner.node_type, "gain");
            assert_eq!(inner.alias, "stereo_gain");
        }
        _ => panic!(),
    };

    assert_eq!(ast.connections.len(), 1);
    let conn = &ast.connections[0];

    assert_eq!(conn.source_name, "stereo_osc");
    assert_eq!(conn.sink_name, "stereo_gain");

    assert_eq!(
        conn.source_port,
        PortConnectionType::Slice { start: 0, end: 1 }
    );
    assert_eq!(
        conn.sink_port,
        PortConnectionType::Slice { start: 2, end: 4 }
    );

    assert_eq!(
        ast.sink,
        Sink {
            name: "gain_stage".to_string()
        }
    );
}

#[test]
fn ast_node_with_pipe_expands_into_multiple_nodes_and_connects() {
    let graph = r#"
        audio {
            osc: stereo_osc { freq: 440.0 } | replicate(2),
            gain: stereo_gain { val: 0.5 }
        }

        // connect both expanded osc nodes into gain
        stereo_osc.0 >> stereo_gain[0]
        stereo_osc.1 >> stereo_gain[1]

        { gain }
    "#;

    let ast = parse_ast(graph);

    assert_eq!(ast.declarations.len(), 1);
    let scope = &ast.declarations[0];
    assert_eq!(scope.namespace, "audio");

    assert_eq!(scope.declarations.len(), 2);

    // First declaration has tuple "indexing"
    match &scope.declarations[0] {
        ExpandedNode::Multiple(nodes) => {
            assert_eq!(nodes.len(), 2);

            assert_eq!(nodes[0].node_type, "osc");
            assert_eq!(nodes[0].alias, "stereo_osc.0");

            assert_eq!(nodes[1].node_type, "osc");
            assert_eq!(nodes[1].alias, "stereo_osc.1");
        }
        _ => panic!("expected expanded node from pipe"),
    }

    // Second declaration is normal node
    match &scope.declarations[1] {
        ExpandedNode::Node(inner) => {
            assert_eq!(inner.node_type, "gain");
            assert_eq!(inner.alias, "stereo_gain");
        }
        _ => panic!("expected single gain node"),
    }

    // two explicit connections
    assert_eq!(ast.connections.len(), 2);

    let c0 = &ast.connections[0];
    let c1 = &ast.connections[1];

    assert_eq!(c0.source_name, "stereo_osc.0");
    assert_eq!(c0.sink_name, "stereo_gain");
    assert_eq!(c0.sink_port, PortConnectionType::Indexed { port: 0 });

    assert_eq!(c1.source_name, "stereo_osc.1");
    assert_eq!(c1.sink_name, "stereo_gain");
    assert_eq!(c1.sink_port, PortConnectionType::Indexed { port: 1 });

    // sink
    assert_eq!(
        ast.sink,
        Sink {
            name: "gain".to_string()
        }
    );
}

#[test]
fn ast_graph_with_replicate_tuple_index_and_port_slices_using_aliases() {
    let graph = r#"
        audio {
            osc: mono_osc { freq: 220.0 } | replicate(4),
            gain: mc_gain { chans: 4 }
        }

        // select a specific replicated node, then slice its ports
        mono_osc.2[0..2] >> mc_gain[1..3]

        { mc_gain }
    "#;

    let ast = parse_ast(graph);

    assert_eq!(ast.declarations.len(), 1);
    let scope = &ast.declarations[0];
    assert_eq!(scope.namespace, "audio");

    assert_eq!(scope.declarations.len(), 2);

    match &scope.declarations[0] {
        ExpandedNode::Multiple(nodes) => {
            assert_eq!(nodes.len(), 4);

            for (i, node) in nodes.iter().enumerate() {
                assert_eq!(node.node_type, "osc");
                assert_eq!(node.alias, format!("mono_osc.{}", i));
            }
        }
        _ => panic!("expected replicated mono_osc nodes"),
    }

    match &scope.declarations[1] {
        ExpandedNode::Node(inner) => {
            assert_eq!(inner.node_type, "gain");
            assert_eq!(inner.alias, "mc_gain");
        }
        _ => panic!("expected single mc_gain node"),
    }

    assert_eq!(ast.connections.len(), 1);
    let conn = &ast.connections[0];

    assert_eq!(conn.source_name, "mono_osc.2");
    assert_eq!(conn.sink_name, "mc_gain");

    assert_eq!(
        conn.source_port,
        PortConnectionType::Slice { start: 0, end: 2 }
    );

    assert_eq!(
        conn.sink_port,
        PortConnectionType::Slice { start: 1, end: 3 }
    );

    assert_eq!(
        ast.sink,
        Sink {
            name: "mc_gain".to_string()
        }
    );
}
