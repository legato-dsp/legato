use legato_dsl::parse::*;
use pest::Parser;

// Here we have a number of tests to cover basic primitives and edge cases
// I am sure this will expand over time

// If it's too error-prone we will just use JSON

fn parse_ok(rule: Rule, input: &str) {
    match LegatoParser::parse(rule, input) {
        Ok(pairs) => {
            println!("\n=== {:?} ===", rule);
            for pair in pairs {
                print_pair(&pair, 0);
            }
        }
        Err(e) => panic!("Parse failed for {:?}: {}", rule, e),
    }
}

fn print_pair(pair: &pest::iterators::Pair<Rule>, indent: usize) {
    println!(
        "{:indent$}{:?}: {:?}",
        "",
        pair.as_rule(),
        pair.as_str(),
        indent = indent * 2
    );
    for inner in pair.clone().into_inner() {
        print_pair(&inner, indent + 1);
    }
}

#[test]
fn parse_values() {
    parse_ok(Rule::uint, "42");
    parse_ok(Rule::int, "-42");
    parse_ok(Rule::float, "3.14");
    parse_ok(Rule::string, "\"hello\"");
    parse_ok(Rule::true_keyword, "true");
    parse_ok(Rule::false_keyword, "false");
    parse_ok(Rule::object, "{ a: 1, b: 2 }");
    parse_ok(
        Rule::object,
        r#"{ a: 1, 
              b: 2 
            }
        "#,
    );
    parse_ok(Rule::array, "[1, 2, 3]");
}

#[test]
fn parse_object() {
    parse_ok(Rule::object, "{ feedback: 0.3, pre_delay: 0.3, size: 0.8 }");
}

#[test]
fn parse_single_node() {
    parse_ok(Rule::add_node, "io: audio_in { chans: 2 }");
}

#[test]
fn parse_multiple_nodes() {
    parse_ok(
        Rule::add_nodes,
        r#"io: audio_in { chans: 2 },
        param { min: 0, max: 1.5 }
    "#,
    );
}

#[test]
fn parse_multiple_nodes_with_pipe() {
    parse_ok(
        Rule::add_nodes,
        r#"io: audio_in { chans: 2 },
        params: param { min: 0, max: 1.5, alg: lerp } | replicate(8)
    "#,
    );
}

#[test]
fn parse_scope_flat() {
    parse_ok(
        Rule::scope_block,
        "control { io: audio_in { chans: 2 }, param: params { min: 0, max: 1.5, alg: lerp }}",
    )
}

#[test]
fn parse_scope() {
    parse_ok(
        Rule::scope_block,
        r#"control {
                io: audio_in { chans: 2 },
                param: params { min: 0, max: 1.5, alg: lerp }
            }
        "#,
    )
}

#[test]
fn parse_scope_with_pipe_and_args() {
    parse_ok(
        Rule::scope_block,
        r#"control {
                io: audio_in { chans: 2 },
                param: params { min: 0, max: 1.5, alg: lerp } | replicate(8)
            }
        "#,
    )
}

#[test]
fn parse_scope_with_pipe_no_args() {
    parse_ok(
        Rule::scope_block,
        r#"control {
                io: audio_in | replicate(4),
                lfo | offset({ param: feedback, amount: 0.1, alg: random })
            }
        "#,
    )
}

#[test]
fn parse_connection_basic() {
    parse_ok(Rule::connection, "audio_in.stereo >> looper.audio.stereo");
}

#[test]
fn parse_object_with_comment() {
    parse_ok(
        Rule::object,
        "{ feedback: 0.3, pre_delay: 0.3, size: 0.8 } // Example config ",
    );
}

#[test]
fn parse_full_graph() {
    let src = r#"control {
                io: audio_in { chans: 2 },
                param: params { min: 0, max: 1.5, alg: lerp } | replicate(8)
            }

            user {
                looper { chans: 8 },
                my_reverb: reverbs { feedback: 0.3, pre_delay: 0.3, size: 0.8 }
                    | replicate(8)
                    | offset({ param: feedback, amount: 0.1, alg: random })
            }

            nodes {
                gain: looper_gains | replicate(8),
            }

            audio_in.stereo >> looper.stereo
            params >> looper.control

            { params }
        "#;

    parse_ok(Rule::graph, src);
}

#[test]
fn parse_with_mixed_comments_and_whitespace() {
    parse_ok(
        Rule::graph,
        r#"
            // Adding a bit of whitespace here!
            control { // inline
                io: audio_in { chans: 2 }, /* block comment */
                param: params { min: 0, max: 1.5 } // trailing
            }

            /* multi
            line
            comment */

            user {
                looper { chans: 8 } // custom
            }

            // connection comment
            audio_in >> looper.audio
            { params }
            "#,
    );
}

#[test]
fn parse_identifiers_and_aliases() {
    parse_ok(Rule::ident, "_internal123");
    parse_ok(Rule::ident, "AUDIO_MIXER");
    parse_ok(Rule::add_node, "node_type_1: alias_42 { val: 1 }");
}

#[test]
fn parse_numeric_edge_cases() {
    parse_ok(Rule::float, "-0.001");
    parse_ok(Rule::float, "0.0");
    parse_ok(Rule::int, "-123456789");
    parse_ok(Rule::uint, "00042");
}

#[test]
fn parse_string_with_escapes() {
    parse_ok(Rule::string, r#""Hello \"World\"!""#);
    parse_ok(Rule::string, r#""Path: C:\\Program Files\\Legato""#);
}

#[test]
fn parse_nested_object_and_array() {
    parse_ok(
        Rule::object,
        r#"{ config: { depth: 3, params: [1, 2, 3, { feedback: 0.5 }] } }"#,
    );
}

#[test]
fn parse_connection_with_varying_port_indices() {
    parse_ok(Rule::connection, "mix[0] >> master[1]");
    parse_ok(Rule::connection, "mix[10]>>master[20]");
    parse_ok(Rule::connection, "a.b >> c.[2]");
    parse_ok(Rule::connection, "out[3] >> gain.r");
}

#[test]
fn parse_multiline_connections_with_varying_ports() {
    parse_ok(
        Rule::connections,
        r#"audio_in.stereo >> looper.audio.stereo
        params >> looper.control
        "#,
    );
}

#[test]
fn parse_inline_connection() {
    parse_ok(
        Rule::connections,
        r#"audio_in.stereo >> looper.audio.stereo >> params >> looper.control
        "#,
    );
}

#[test]
fn parse_empty_scope() {
    parse_ok(Rule::scope_block, "empty_scope {}");
}

#[test]
fn parse_sink() {
    parse_ok(Rule::sink, r#"{ osc_one }"#);
}

#[test]
fn parse_node_without_params_or_pipe() {
    parse_ok(Rule::add_node, "oscillator");
}

#[test]
fn parse_node_with_multiple_pipes_and_args() {
    parse_ok(
        Rule::add_node,
        r#"noise_gen { seed: 123 } 
            | filter({ type: "highpass", freq: 200 }) 
            | replicate(4)
            "#,
    );
}
