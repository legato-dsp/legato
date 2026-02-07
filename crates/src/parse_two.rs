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
    pub connections: Vec<Connection>, // We can add connections, source, and sink here in the next stage
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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Connection {
    pub source_node: String,
    pub sink_node: String,
}

fn connection_parser<'a>() -> impl Parser<'a, &'a str, Connection, Err<Rich<'a, char>>> {
    let ident = text::ascii::ident().map(ToString::to_string);

    let connection = ident
        .then_ignore(just(">>").padded())
        .then(ident.padded())
        .map(|(source_node, sink_node)| Connection {
            source_node,
            sink_node,
        });

    connection
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

fn main_parser<'a>() -> impl Parser<'a, &'a str, Ast, Err<Rich<'a, char>>> {
    let declarations = scope_parser().padded().repeated().collect();

    let connections = connection_parser().padded().repeated().collect();

    let ast = declarations
        .then(connections.or_not())
        .map(|(declarations, connections)| Ast {
            declarations,
            connections: connections.unwrap_or(Vec::new()),
        })
        .padded()
        .then_ignore(end());

    ast
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

        assert_eq!(result.source_node, "osc");
        assert_eq!(result.sink_node, "gain");
    }

    #[test]
    fn test_connections_in_ast() {
        let src = r#"
            audio {
                osc,
                gain
            }
            osc >> gain
            gain >> output
        "#;

        let parser = main_parser();
        let ast = parser.parse(src).into_result().unwrap();

        assert_eq!(ast.connections.len(), 2);
        assert_eq!(ast.connections[0].source_node, "osc");
        assert_eq!(ast.connections[0].sink_node, "gain");
        assert_eq!(ast.connections[1].source_node, "gain");
        assert_eq!(ast.connections[1].sink_node, "output");
    }

    #[test]
    fn test_connection_whitespace() {
        let src = "osc   >>   gain";
        let parser = connection_parser().padded();
        let result = parser.parse(src).into_result().unwrap();
        assert_eq!(result.source_node, "osc");
    }
}
