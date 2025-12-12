use legato::{
    ast::{Ast, PortConnectionType, Sink, Value, build_ast},
    parse::{LegatoParser, Rule},
};
use pest::Parser;

fn parse_ast(input: &str) -> Ast {
    let pairs = LegatoParser::parse(Rule::graph, input).expect("PEST failed");
    build_ast(pairs).expect("AST lowering failed")
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
    let node = &scope.declarations[0];

    assert_eq!(node.node_type, "osc");
    assert_eq!(node.alias.as_deref(), Some("square_wave_one"));

    let params = node.params.as_ref().unwrap();
    assert_eq!(params["freq"], Value::U32(440));
    assert_eq!(params["gain"], Value::F32(0.2));

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
    let graph = String::from(
        r#"
        audio {
            sine_mono: mod { freq: 891.0 },
            sine_stereo: carrier { freq: 440.0 },
            mult_mono: fm_gain { val: 1000.0 }
        }

        mod[0] >> fm_gain[0] >> carrier[0]

        { carrier }
    "#,
    );

    let ast = parse_ast(&graph);

    assert_eq!(ast.connections.len(), 2);

    let audio_scope = &ast.declarations[0];

    assert_eq!(audio_scope.namespace, "audio");

    assert_eq!(ast.connections.len(), 2);
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

    // ----- scopes -----
    assert_eq!(ast.declarations.len(), 1);
    let scope = &ast.declarations[0];
    assert_eq!(scope.namespace, "audio");

    assert_eq!(scope.declarations.len(), 2);
    let osc = &scope.declarations[0];
    let gain = &scope.declarations[1];

    assert_eq!(osc.node_type, "osc");
    assert_eq!(osc.alias.as_deref(), Some("stereo_osc"));

    assert_eq!(gain.node_type, "gain");
    assert_eq!(gain.alias.as_deref(), Some("stereo_gain"));

    assert_eq!(ast.connections.len(), 1);
    let conn = &ast.connections[0];

    assert_eq!(conn.source_name, "stereo_osc");
    assert_eq!(conn.sink_name, "stereo_gain");

    assert_eq!(conn.source_port, PortConnectionType::Slice { start: 0, end: 1 });
    assert_eq!(conn.sink_port, PortConnectionType::Slice { start: 2, end: 4 });

    assert_eq!(
        ast.sink,
        Sink { name: "gain_stage".to_string() }
    );
}
