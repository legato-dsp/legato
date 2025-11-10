use crate::ast::Ast;
use std::error::Error;
use pest::Parser;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "./ast.pest"]
struct LegatoParser;

pub fn parse_legato_file(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(file)?;
    let pairs = LegatoParser::parse(Rule::graph, &raw)?;

    // pretty print all rules recursively
    for pair in pairs {
        print_pair(&pair, 0);
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use pest::Parser;

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
        parse_ok(Rule::add_nodes, r#"
        io: audio_in { chans: 2 },
        param { min: 0, max: 1.5 }
    "#);
    }

     #[test]
    fn parse_multiple_nodes_with_pipe() {
        parse_ok(Rule::add_nodes, r#"
        io: audio_in { chans: 2 },
        params: param { min: 0, max: 1.5, alg: lerp } | replicate(8)
    "#);
    }

    #[test]
    fn parse_scope(){
        parse_ok(Rule::scope_block, r#"
            control {
                io: audio_in { chans: 2 },
                param: params { min: 0, max: 1.5, alg: lerp }
            }
        "#)
    }
    
    #[test]
    fn parse_scope_with_pipe(){
        parse_ok(Rule::scope_block, r#"
            control {
                io: audio_in { chans: 2 },
                param: params { min: 0, max: 1.5, alg: lerp } | replicate(8)
            }
        "#)
    }

    #[test]
    fn parse_connection_basic() {
        parse_ok(Rule::connection, "audio_in.stereo >> looper.audio.stereo");
    }

    #[test]
    fn parse_object_with_comment() {
        parse_ok(Rule::object, "{ feedback: 0.3, pre_delay: 0.3, size: 0.8 } // Example config ");
    }

    #[test]
    fn parse_export(){
        parse_ok(Rule::exports, "{ shimmer_reverb, fm_synth_one, stereo }");
    }

    #[test]
    fn parse_full_graph() {
        let src = r#"
            control {
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

            audio_in.stereo >> looper.audio.stereo
            params >> looper.control { automap: true }

            { params }
        "#;

        parse_ok(Rule::graph, src);
    }
}