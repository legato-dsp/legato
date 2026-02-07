use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::{extra::Err, prelude::*};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Null,
    U32(u32),
    I32(i32),
    F32(f32),
    Bool(bool),
    Ident(String),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

pub type Object = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ASTPipe {
    pub name: String,
    pub params: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodeDeclaration {
    pub node_type: String,
    pub alias: Option<String>,
    pub params: Option<Object>,
    pub pipes: Vec<ASTPipe>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeclarationScope {
    pub namespace: String,
    pub declarations: Vec<NodeDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ast {
    pub declarations: Vec<DeclarationScope>,
    pub connections: Vec<Connection>,
    // When chaining executors/graphs, this is the entry point
    pub source: Option<String>,
    // The exit point that the runtime/executor delivers samples from
    pub sink: String,
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
            .then(just('.').then(digits.clone()))
            .to_slice()
            .map(|s: &str| Value::F32(s.parse().unwrap()));

        let i32 = just('-')
            .then(digits.clone())
            .to_slice()
            .map(|s: &str| Value::I32(s.parse().unwrap()))
            .boxed();

        let u32 = digits
            .to_slice()
            .map(|s: &str| Value::U32(s.parse().unwrap()));

        let ident_raw = text::ascii::ident().map(ToString::to_string);
        let ident_value = ident_raw.clone().map(|s| match s.as_str() {
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

        choice((f32, i32, u32, string_value, object, array, ident_value))
            .padded()
            .boxed()
    })
}

fn node_declaration<'a>() -> impl Parser<'a, &'a str, NodeDeclaration, Err<Rich<'a, char>>> {
    let ident = text::ascii::ident().map(ToString::to_string);

    let alias = just(':').padded().ignore_then(ident.clone()).or_not();

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
        .ignore_then(ident.clone())
        .then(
            value_parser()
                .delimited_by(just('(').padded(), just(')').padded())
                .or_not(),
        )
        .map(|(name, params)| ASTPipe { name, params });

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

#[derive(Debug, Clone, PartialEq)]
pub enum Port {
    Named(String),
    Index(u32),
    Slice(u32, u32),
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Endpoint {
    pub node: String,
    pub port: Port,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub source: Endpoint,
    pub sink: Endpoint,
}

fn endpoint_parser<'a>() -> impl Parser<'a, &'a str, Endpoint, Err<Rich<'a, char>>> {
    let ident = text::ascii::ident().map(ToString::to_string);
    let uint = text::digits(10)
        .to_slice()
        .map(|s: &str| s.parse::<u32>().unwrap());

    let port = choice((
        // node.mono
        just('.').ignore_then(ident).map(Port::Named),
        // node[0..2]
        uint.then_ignore(just(".."))
            .then(uint)
            .delimited_by(just('['), just(']'))
            .map(|(s, e)| Port::Slice(s, e)),
        // node[0]
        uint.delimited_by(just('['), just(']')).map(Port::Index),
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

    let scope = ident
        .then_ignore(just('{').padded())
        .then(
            node_declaration()
                .separated_by(just(',').padded())
                .allow_trailing()
                .collect(),
        )
        .then_ignore(just('}').padded())
        .map(|(namespace, declarations)| DeclarationScope {
            namespace,
            declarations,
        });

    scope
}

/// Just matches { string }, used in source and sink.
fn scope_or_sink<'a>() -> impl Parser<'a, &'a str, String, Err<Rich<'a, char>>> {
    text::ascii::ident()
        .map(ToString::to_string)
        .delimited_by(just('{').padded(), just('}').padded())
}

/// The main entrypoint for the Legato parser.
fn main_parser<'a>() -> impl Parser<'a, &'a str, Ast, Err<Rich<'a, char>>> {
    let source = scope_or_sink().or_not();

    let declarations = scope_parser().padded().repeated().collect();

    let connections = connection_parser()
        .padded()
        .repeated()
        .collect::<Vec<Vec<Connection>>>()
        .map(|v| v.into_iter().flatten().collect::<Vec<Connection>>())
        .or_not(); // Connections optional

    let sink = scope_or_sink();

    source
        .then(declarations)
        .then(connections)
        .then(sink)
        .map(|(((source, declarations), connections), sink)| Ast {
            source,
            declarations,
            connections: connections.unwrap_or(Vec::new()),
            sink,
        })
        .padded()
        .then_ignore(end())
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
        let parser = main_parser();
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
            source: None,
            sink: "sine".into(),
            connections: Vec::new(),
        };

        assert_parse_equals_ast(src, expected);
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

        let parser = main_parser();
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
        let res = main_parser().parse(broken_src);
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

        let parser = main_parser();
        let ast = parser.parse(src).into_result().unwrap();

        assert_eq!(ast.connections.len(), 2);
        assert_eq!(ast.connections[0].source.node, "osc");
        assert_eq!(ast.connections[0].sink.node, "gain");
        assert_eq!(ast.connections[1].source.node, "gain");
        assert_eq!(ast.connections[1].sink.node, "output");
        // Sink logic
        assert_eq!(ast.sink, "output".to_string());
        assert!(ast.source.is_none());
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

        let parser = main_parser();
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
}
