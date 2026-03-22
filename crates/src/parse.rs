use crate::{builder::ValidationError, ir::*};
use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::{extra::Err, prelude::*};
use std::collections::BTreeMap;

fn comment<'a>() -> impl Parser<'a, &'a str, (), Err<Rich<'a, char>>> {
    // One line comment, just ignore until newline
    let line_comment = just("//").then(none_of('\n').repeated()).ignored();

    // Multiline, just ignore until ending
    let block_comment = just("/*")
        .then(any().and_is(just("*/").not()).repeated())
        .then(just("*/"))
        .ignored();

    line_comment.or(block_comment)
}

fn extra_padded<'a, P, O>(parser: P) -> impl Parser<'a, &'a str, O, Err<Rich<'a, char>>>
where
    P: Parser<'a, &'a str, O, Err<Rich<'a, char>>>,
{
    let skip = choice((comment(), text::whitespace().at_least(1).ignored()))
        .repeated()
        .ignored();

    parser.padded_by(skip)
}

fn value_parser<'a>() -> impl Parser<'a, &'a str, Value, Err<Rich<'a, char>>> {
    recursive(|value| {
        let escape = just('\\').ignore_then(choice((
            just('\\'),
            just('/'),
            just('"'),
            just('n').to('\n'),
            just('r').to('\r'),
            just('t').to('\t'),
        )));

        let string_value = none_of("\\\"")
            .or(escape)
            .repeated()
            .collect::<String>()
            .delimited_by(just('"'), just('"'))
            .map(Value::String);

        let digits = text::digits(10);

        let f32 = just('-')
            .or_not()
            .then(text::int(10))
            .then(just('.').then(digits))
            .to_slice()
            .map(|s: &str| Value::F32(s.parse().unwrap()));

        let i32 = just('-')
            .then(digits)
            .to_slice()
            .map(|s: &str| Value::I32(s.parse().unwrap()))
            .boxed();

        let u32 = digits
            .to_slice()
            .map(|s: &str| Value::U32(s.parse().unwrap()));

        let template = just('$')
            .then(text::ascii::ident())
            .to_slice()
            .map(|s: &str| Value::Template(s.to_string()));

        let ident_raw = text::ascii::ident().map(ToString::to_string);
        let ident_value = ident_raw.map(|s| match s.as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "null" => Value::Null,
            _ => Value::Ident(s),
        });

        let kv = ident_raw
            .then_ignore(just(':').padded())
            .then(value.clone());

        let object = kv
            .separated_by(just(',').padded())
            .allow_trailing()
            .collect::<BTreeMap<String, Value>>()
            .delimited_by(just('{').padded(), just('}').padded())
            .map(Value::Object)
            .boxed();

        let array = value
            .clone()
            .separated_by(just(',').padded().recover_with(skip_then_retry_until(
                any().ignored(),
                one_of(",]").ignored(),
            )))
            .allow_trailing()
            .collect()
            .padded()
            .delimited_by(
                just('['),
                just(']')
                    .ignored()
                    .recover_with(via_parser(end()))
                    .recover_with(skip_then_retry_until(any().ignored(), end())),
            )
            .map(Value::Array)
            .boxed();

        choice((
            f32,
            i32,
            u32,
            string_value,
            template,
            object,
            array,
            ident_value,
        ))
        .padded()
        .boxed()
    })
}

fn node_declaration<'a>() -> impl Parser<'a, &'a str, NodeDeclaration, Err<Rich<'a, char>>> {
    let ident = text::ascii::ident().map(ToString::to_string);

    let alias = just(':').padded().ignore_then(ident).or_not();

    let obj_parser = ident
        .then_ignore(just(':').padded())
        .then(value_parser())
        .separated_by(just(',').padded())
        .allow_trailing()
        .collect::<BTreeMap<String, Value>>();

    let params = obj_parser
        .delimited_by(just('{').padded(), just('}').padded())
        .or_not();

    let pipe = just('|')
        .padded()
        .ignore_then(ident)
        .then(
            value_parser()
                .padded()
                .or_not()
                .delimited_by(just('('), just(')'))
                .or_not(),
        )
        .map(|(name, params)| ASTPipe {
            name,
            params: params.flatten(),
        });

    ident
        .then(alias)
        .then(params)
        .then(pipe.repeated().collect())
        .map(|(((node_type, alias), params), pipes)| NodeDeclaration {
            node_type,
            alias,
            params,
            pipes,
        })
}

fn patch_parser<'a>() -> impl Parser<'a, &'a str, Macro, Err<Rich<'a, char>>> {
    let ident = text::ascii::ident().map(ToString::to_string);

    // Default params use = for intitial values
    let default_param = ident.then_ignore(just('=').padded()).then(value_parser());

    let default_params = default_param
        .separated_by(just(',').padded())
        .allow_trailing()
        .collect::<BTreeMap<String, Value>>()
        .delimited_by(just('(').padded(), just(')').padded())
        .or_not();

    // Virtual ports: `in gate freq_in`
    let virtual_port_ident = ident.padded_by(text::inline_whitespace());

    let virtual_ports = just("in")
        .then_ignore(text::inline_whitespace().at_least(1))
        .ignore_then(
            virtual_port_ident
                .repeated()
                .at_least(1)
                .collect::<Vec<String>>(),
        );

    // Interior connections is the same as the normal AST
    let inner_connections = extra_padded(connection_parser())
        .repeated()
        .collect::<Vec<Vec<Connection>>>()
        .map(|v| v.into_iter().flatten().collect::<Vec<Connection>>());

    let patch_body = extra_padded(virtual_ports)
        .or_not()
        .then(extra_padded(scope_parser()).repeated().collect::<Vec<_>>())
        .then(inner_connections.or_not())
        .then(extra_padded(scope_or_sink()))
        .delimited_by(extra_padded(just('{')), extra_padded(just('}')));

    // patch must be followed by whitespace
    just("patch")
        .then_ignore(text::whitespace().at_least(1))
        .ignore_then(ident)
        .then(extra_padded(default_params))
        .then(patch_body)
        .map(|((name, params), (((vports, decls), conns), sink))| Macro {
            name,
            default_params: params,
            virtual_ports_in: vports.unwrap_or_default().into_iter().collect(),
            declarations: decls,
            connections: conns.unwrap_or_default(),
            sink,
        })
}

fn endpoint_parser<'a>() -> impl Parser<'a, &'a str, Endpoint, Err<Rich<'a, char>>> {
    let ident = text::ascii::ident().map(ToString::to_string);
    let uint = text::digits(10)
        .to_slice()
        .map(|s: &str| s.parse::<u32>().unwrap());

    let port = choice((
        // node.mono
        just('.').ignore_then(ident).map(Port::Named),
        // port stride e.g [0:10:2]: this maps to [start:end:step].
        // NOTE: Unlike python we don't take implicit values, this is not good [::-1]!
        uint.then_ignore(just(":"))
            .then(uint)
            .then_ignore(just(":"))
            .then(uint)
            .delimited_by(just("["), just("]"))
            .map(|((start, end), stride)| Port::Stride {
                start: start as usize,
                end: end as usize,
                stride: stride as usize,
            }),
        // node[0..2]
        uint.then_ignore(just(".."))
            .then(uint)
            .delimited_by(just('['), just(']'))
            .map(|(s, e)| Port::Slice(s as usize, e as usize)),
        // node[0]
        uint.delimited_by(just('['), just(']'))
            .map(|x| Port::Index(x as usize)),
    ))
    .or_not()
    .map(|p| p.unwrap_or(Port::None));

    ident.then(port).map(|(node, port)| Endpoint { node, port })
}

fn connection_parser<'a>() -> impl Parser<'a, &'a str, Vec<Connection>, Err<Rich<'a, char>>> {
    endpoint_parser()
        .separated_by(just(">>").padded())
        .at_least(2)
        .collect::<Vec<_>>()
        .map(|endpoints| {
            endpoints
                .windows(2)
                .map(|w| Connection {
                    source: w[0].clone(),
                    sink: w[1].clone(),
                })
                .collect()
        })
}

fn scope_parser<'a>() -> impl Parser<'a, &'a str, DeclarationScope, Err<Rich<'a, char>>> {
    let ident = text::ascii::ident().map(ToString::to_string);

    ident
        .then_ignore(extra_padded(just('{')))
        .then(
            extra_padded(node_declaration())
                .separated_by(extra_padded(just(',')))
                .allow_trailing()
                .collect(),
        )
        .then_ignore(extra_padded(just('}')))
        .map(|(namespace, declarations)| DeclarationScope {
            namespace,
            declarations,
        })
}

/// Just matches { string }, used in source and sink.
fn scope_or_sink<'a>() -> impl Parser<'a, &'a str, String, Err<Rich<'a, char>>> {
    text::ascii::ident()
        .map(ToString::to_string)
        .delimited_by(just('{').padded(), just('}').padded())
}

/// The main entrypoint for the Legato parser.
pub fn legato_parser_inner<'a>() -> impl Parser<'a, &'a str, Ast, Err<Rich<'a, char>>> {
    // Use the extra_padded helper here
    let source = extra_padded(scope_or_sink()).or_not();

    let patches = extra_padded(patch_parser())
        .repeated()
        .collect::<Vec<Macro>>();

    let declarations = extra_padded(scope_parser()).repeated().collect();

    let connections = extra_padded(connection_parser())
        .repeated()
        .collect::<Vec<Vec<Connection>>>()
        .map(|v| v.into_iter().flatten().collect::<Vec<Connection>>())
        .or_not();

    let sink = extra_padded(scope_or_sink());

    source
        .then(patches)
        .then(declarations)
        .then(connections)
        .then(sink)
        .map(
            |((((source, macros), declarations), connections), sink)| Ast {
                source,
                declarations,
                connections: connections.unwrap_or_default(),
                macros,
                sink,
            },
        )
        .then_ignore(extra_padded(end()))
}

/// The Legato parser, using chumsky and ariande to handle errors.
pub fn legato_parser(src: &str) -> Result<Ast, ValidationError> {
    let (ast, errs) = legato_parser_inner().parse(src.trim()).into_output_errors();
    errs.into_iter().for_each(|e| {
        Report::build(ReportKind::Error, ((), e.span().into_range()))
            .with_config(ariadne::Config::new().with_index_type(ariadne::IndexType::Byte))
            .with_message(e.to_string())
            .with_label(
                Label::new(((), e.span().into_range()))
                    .with_message(e.reason().to_string())
                    .with_color(Color::Red),
            )
            .finish()
            .print(Source::from(&src))
            .unwrap()
    });

    ast.ok_or(ValidationError::ParseError(
        "Could not parse source. Please check error report.".into(),
    ))
}

#[cfg(test)]
mod test {
    use super::*;
    use ariadne::{Color, Label, Report, ReportKind, Source};
    use std::collections::BTreeMap;

    // Value parser helper
    fn assert_parse_equals_value(input: &str, expected: Value) {
        let parser = value_parser();
        match parser.parse(input).into_result() {
            Ok(output) => assert_eq!(output, expected, "Parsed Value didn't match expectation"),
            Err(errors) => {
                print_errors(input, errors);
                panic!("Value parse failed");
            }
        }
    }

    // AST parser helper
    fn assert_parse_equals_ast(input: &str, expected: Ast) {
        let parser = legato_parser_inner();
        match parser.parse(input).into_result() {
            Ok(output) => assert_eq!(output, expected, "Parsed AST didn't match expectation"),
            Err(errors) => {
                print_errors(input, errors);
                panic!("AST parse failed");
            }
        }
    }

    fn print_errors(input: &str, errors: Vec<Rich<char>>) {
        for e in errors {
            Report::build(ReportKind::Error, ((), e.span().into_range()))
                .with_config(ariadne::Config::new().with_index_type(ariadne::IndexType::Byte))
                .with_message(e.to_string())
                .with_label(
                    Label::new(((), e.span().into_range()))
                        .with_message(e.reason().to_string())
                        .with_color(Color::Red),
                )
                .finish()
                .print(Source::from(input))
                .unwrap();
        }
    }

    #[test]
    fn test_value_primitives() {
        assert_parse_equals_value("null", Value::Null);
        assert_parse_equals_value("true", Value::Bool(true));
        assert_parse_equals_value("42.5", Value::F32(42.5));
        assert_parse_equals_value("-10", Value::I32(-10));
        assert_parse_equals_value(
            r#""escaped\nline""#,
            Value::String("escaped\nline".to_string()),
        );
    }

    #[test]
    fn test_node_pipes_and_aliases() {
        let src = r#"
            audio {
                osc: sine { freq: 440 } | lowpass(100.5) | gain(null)
            }

            { sine }
        "#;

        let expected = Ast {
            declarations: vec![DeclarationScope {
                namespace: "audio".to_string(),
                declarations: vec![NodeDeclaration {
                    node_type: "osc".to_string(),
                    alias: Some("sine".to_string()),
                    params: Some(BTreeMap::from([("freq".to_string(), Value::U32(440))])),
                    pipes: vec![
                        ASTPipe {
                            name: "lowpass".to_string(),
                            params: Some(Value::F32(100.5)),
                        },
                        ASTPipe {
                            name: "gain".to_string(),
                            params: Some(Value::Null),
                        },
                    ],
                }],
            }],
            macros: Vec::new(),
            sink: "sine".into(),
            source: None,
            connections: Vec::new(),
        };

        assert_parse_equals_ast(src, expected);
    }

    #[test]
    fn test_port_stride() {
        let src = r#"test_node[0:10:2]"#;
        let res = endpoint_parser().parse(src).unwrap();

        assert_eq!(
            res,
            Endpoint {
                node: "test_node".into(),
                port: Port::Stride {
                    start: 0,
                    end: 10,
                    stride: 2
                }
            }
        )
    }

    #[test]
    fn test_multiple_scopes_and_nodes() {
        let src = r#"
            control {
                param { val: 255.0 }
            }

            audio {
                osc: square_wave_one { freq: 440.0, gain: 0.2 } | volume(0.8),
            }

            { square_wave_one }
        "#;

        let parser = legato_parser_inner();
        let ast = parser.parse(src).into_result().unwrap();

        assert_eq!(ast.declarations.len(), 2);
        assert_eq!(ast.declarations[0].namespace, "control");
        assert_eq!(ast.declarations[1].declarations.len(), 1);
        assert_eq!(
            ast.declarations[1].declarations[0].alias,
            Some("square_wave_one".to_string())
        );
        assert_eq!(ast.declarations[1].declarations[0].node_type, "osc");
        assert_eq!(
            ast.declarations[1].declarations[0].pipes,
            vec![ASTPipe {
                name: "volume".into(),
                params: Some(Value::F32(0.8))
            }]
        );
    }

    #[test]
    fn test_bogus_syntax() {
        let broken_src = "bogus_scope { node { param: 1 ";
        let res = legato_parser_inner().parse(broken_src);
        assert!(res.into_result().is_err());
    }

    #[test]
    fn test_complex_object_nesting() {
        let input = r#"{ 
            meta: { author: "bob", active: true },
            tags: ["rust", "dsp"]
        }"#;

        let mut meta_map = BTreeMap::new();
        meta_map.insert("author".into(), Value::String("bob".into()));
        meta_map.insert("active".into(), Value::Bool(true));

        let expected_map = BTreeMap::from([
            ("meta".into(), Value::Object(meta_map)),
            (
                "tags".into(),
                Value::Array(vec![
                    Value::String("rust".into()),
                    Value::String("dsp".into()),
                ]),
            ),
        ]);

        assert_parse_equals_value(input, Value::Object(expected_map));
    }

    #[test]
    fn test_basic_connection() {
        let src = "osc >> gain";
        let parser = connection_parser();
        let result = parser.parse(src).into_result().unwrap();

        assert_eq!(result[0].source.node, "osc");
        assert_eq!(result[0].sink.node, "gain");
    }

    #[test]
    fn test_connections_in_ast() {
        let src = r#"
            audio {
                osc,
                gain,
                output
            }
            osc >> gain
            gain >> output

            { output }
        "#;

        let parser = legato_parser_inner();
        let ast = parser.parse(src).into_result().unwrap();

        assert_eq!(ast.connections.len(), 2);
        assert_eq!(ast.connections[0].source.node, "osc");
        assert_eq!(ast.connections[0].sink.node, "gain");
        assert_eq!(ast.connections[1].source.node, "gain");
        assert_eq!(ast.connections[1].sink.node, "output");
        // Sink logic
        assert_eq!(ast.sink, "output".to_string());
    }

    #[test]
    fn test_connection_whitespace() {
        let src = "osc   >>   gain";
        let parser = connection_parser().padded();
        let result = parser.parse(src).into_result().unwrap();
        assert_eq!(result[0].source.node, "osc");
    }

    #[test]
    fn test_connections_in_ast_nested() {
        let src = r#"
            audio {
                osc,
                gain,
                svf,
                output
            }
            osc >> gain >> svf
            gain >> output

            { output }
        "#;

        let parser = legato_parser_inner();
        let ast = parser.parse(src).into_result().unwrap();

        assert_eq!(ast.connections.len(), 3);
        assert_eq!(ast.connections[0].source.node, "osc");
        assert_eq!(ast.connections[0].sink.node, "gain");
        assert_eq!(ast.connections[1].source.node, "gain");
        assert_eq!(ast.connections[1].sink.node, "svf");
        assert_eq!(ast.connections[2].source.node, "gain");
        assert_eq!(ast.connections[2].sink.node, "output");

        assert_eq!(ast.sink, "output".to_string());
    }

    #[test]
    fn test_complex_ports() {
        let src = "audio_in.stereo >> looper[0..2] >> out[1]";
        let parser = connection_parser();
        let result = parser.parse(src).into_result().unwrap();

        assert_eq!(result.len(), 2);

        assert_eq!(result[0].source.port, Port::Named("stereo".into()));
        assert_eq!(result[0].sink.port, Port::Slice(0, 2));

        assert_eq!(result[1].source.node, "looper");
        assert_eq!(result[1].sink.port, Port::Index(1));
    }

    #[test]
    fn test_mixed_chain() {
        let src = "osc >> gain.input >> bus[0..2] >> master[1]";
        let parser = connection_parser();
        let result = parser.parse(src).into_result().unwrap();

        assert_eq!(result[0].source.port, Port::None);
        assert_eq!(result[0].sink.port, Port::Named("input".into()));
        assert_eq!(result[1].sink.port, Port::Slice(0, 2));
        assert_eq!(result[2].sink.port, Port::Index(1));
    }

    #[test]
    fn test_audio_graph_with_slices_and_pipes() {
        let src = r#"
        audio {
            sampler { sampler_name: "amen", chans: 2 } | logger(),
            allpass { delay_length: 200, feedback: 0.5, chans: 2 },
            track_mixer { tracks: 2, chans_per_track: 2, gain: [0.5, 0.5] },
        }

        sampler >> track_mixer[0..2]
        sampler >> allpass
        allpass >> track_mixer[2..4]

        { track_mixer }
    "#;

        let parser = legato_parser_inner();
        let ast = parser.parse(src).into_result().unwrap();

        assert_eq!(ast.declarations.len(), 1);
        let scope = &ast.declarations[0];
        assert_eq!(scope.namespace, "audio");
        assert_eq!(scope.declarations.len(), 3);

        let sampler = &scope.declarations[0];
        assert_eq!(sampler.node_type, "sampler");
        assert_eq!(sampler.alias, None);
        assert_eq!(sampler.pipes.len(), 1);
        assert_eq!(sampler.pipes[0].name, "logger");

        let track_mixer = &scope.declarations[2];
        let gain = track_mixer.params.as_ref().unwrap().get("gain").unwrap();

        assert_eq!(gain, &Value::Array(vec![Value::F32(0.5), Value::F32(0.5)]));

        assert_eq!(ast.connections.len(), 3);

        // sampler >> track_mixer[0..2]
        assert_eq!(ast.connections[0].source.node, "sampler");
        assert_eq!(ast.connections[0].sink.node, "track_mixer");
        assert_eq!(ast.connections[0].sink.port, Port::Slice(0, 2));

        // sampler >> allpass
        assert_eq!(ast.connections[1].source.node, "sampler");
        assert_eq!(ast.connections[1].sink.node, "allpass");

        // allpass >> track_mixer[2..4]
        assert_eq!(ast.connections[2].source.node, "allpass");
        assert_eq!(ast.connections[2].sink.node, "track_mixer");
        assert_eq!(ast.connections[2].sink.port, Port::Slice(2, 4));

        assert_eq!(ast.sink, "track_mixer".to_string());
    }

    // New patch tests

    #[test]
    fn test_template_value() {
        assert_parse_equals_value("$freq", Value::Template("$freq".into()));
        assert_parse_equals_value("$attack_time", Value::Template("$attack_time".into()));
    }

    #[test]
    fn test_patch_minimal() {
        // just a scope and sink
        let src = r#"
            patch simple_gain() {
                audio {
                    gain { amount: 0.5 }
                }
                { gain }
            }
            { gain }
        "#;
        let ast = legato_parser_inner().parse(src).into_result().unwrap();

        assert_eq!(ast.macros.len(), 1);
        let m = &ast.macros[0];
        assert_eq!(m.name, "simple_gain");
        assert!(
            m.default_params
                .as_ref()
                .map(|p| p.is_empty())
                .unwrap_or(true)
        );
        assert!(m.virtual_ports_in.is_empty());
        assert_eq!(m.sink, "gain");
    }

    #[test]
    fn test_patch_default_params() {
        let src = r#"
            patch voice(freq = 440.0, attack = 100.0) {
                audio {
                    sine: osc { freq: $freq },
                    adsr: env { attack: $attack }
                }
                { env }
            }
            { env }
        "#;
        let ast = legato_parser_inner().parse(src).into_result().unwrap();

        let m = &ast.macros[0];
        assert_eq!(m.name, "voice");

        let params = m.default_params.as_ref().unwrap();
        assert_eq!(params.get("freq"), Some(&Value::F32(440.0)));
        assert_eq!(params.get("attack"), Some(&Value::F32(100.0)));

        // Template values should be present in interior node params
        let osc = &m.declarations[0].declarations[0];
        assert_eq!(
            osc.params.as_ref().unwrap().get("freq"),
            Some(&Value::Template("$freq".into()))
        );
    }

    #[test]
    fn test_patch_virtual_ports() {
        let src = r#"
            patch voice(freq = 440.0) {
                in gate freq_in

                audio {
                    sine: osc { freq: $freq },
                    adsr: env { attack: 100.0 }
                }

                freq_in >> osc.freq
                gate >> env.gate
                osc >> env[1]

                { env }
            }
            { env }
        "#;
        let ast = legato_parser_inner().parse(src).into_result().unwrap();

        let m = &ast.macros[0];

        // Virtual ports in declaration order
        assert_eq!(m.virtual_ports_in.len(), 2);
        assert_eq!(m.virtual_ports_in[0], "gate");
        assert_eq!(m.virtual_ports_in[1], "freq_in");

        // All three connections parsed
        assert_eq!(m.connections.len(), 3);

        let freq_conn = m
            .connections
            .iter()
            .find(|c| c.source.node == "freq_in")
            .unwrap();
        assert_eq!(freq_conn.sink.node, "osc");
        assert_eq!(freq_conn.sink.port, Port::Named("freq".into()));

        let gate_conn = m
            .connections
            .iter()
            .find(|c| c.source.node == "gate")
            .unwrap();
        assert_eq!(gate_conn.sink.node, "env");
        assert_eq!(gate_conn.sink.port, Port::Named("gate".into()));

        let audio_conn = m
            .connections
            .iter()
            .find(|c| c.source.node == "osc")
            .unwrap();
        assert_eq!(audio_conn.sink.node, "env");
        assert_eq!(audio_conn.sink.port, Port::Index(1));
    }

    #[test]
    fn test_patch_in_full_ast() {
        // End-to-end: definition, instantiation scope, external connections, sink
        let src = r#"
            patch voice(freq = 440.0) {
                in gate freq_in

                audio {
                    sine: osc { freq: $freq },
                    adsr: env { attack: 100.0 }
                }

                freq_in >> osc.freq
                gate >> env.gate
                osc >> env[1]

                { env }
            }

            patches {
                voice: v1 { freq: 880.0 },
                voice: v2 { freq: 220.0 }
            }

            midi {
                poly_voice { chan: 0 }
            }

            poly_voice.freq >> v1.freq_in
            poly_voice.gate >> v1.gate

            { v1 }
        "#;

        let ast = legato_parser_inner().parse(src).into_result().unwrap();

        assert_eq!(ast.macros.len(), 1);
        assert_eq!(ast.macros[0].name, "voice");

        // patches + midi scopes
        assert_eq!(ast.declarations.len(), 2);
        assert_eq!(ast.declarations[0].namespace, "patches");
        assert_eq!(ast.declarations[0].declarations.len(), 2);

        // External connections
        assert_eq!(ast.connections.len(), 2);
        assert_eq!(ast.connections[0].source.node, "poly_voice");
        assert_eq!(ast.connections[0].source.port, Port::Named("freq".into()));
        assert_eq!(ast.connections[0].sink.node, "v1");
        assert_eq!(ast.connections[0].sink.port, Port::Named("freq_in".into()));

        assert_eq!(ast.sink, "v1");
    }
}
