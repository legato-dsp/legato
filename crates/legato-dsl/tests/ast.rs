use legato_dsl::{
    ast::{Ast, Value, build_ast, Sink},
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
    assert_eq!(params["freq"], Value::I32(440));
    assert_eq!(params["gain"], Value::F32(0.2));

    let sink = ast.sink;
    assert_eq!(sink, Sink { name: String::from("osc") })
}
