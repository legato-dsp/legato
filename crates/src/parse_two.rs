//! This is a parser for JSON.
//! Run it with the following command:
//! cargo run --example json -- examples/sample.json

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::{extra::Err, prelude::*};
use std::{collections::{BTreeMap, HashMap}, env, fs};

use crate::ast::Object;

#[derive(Clone, Debug, PartialEq)]
enum Value {
    U32(u32),
    I32(i32),
    F32(f32),
    Bool(bool),
    Ident(String),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>)
}

#[derive(Clone, Debug, PartialEq)]
enum AST {
    DeclarationScope((String, Value))
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
        
        let f32 = just('-').or_not()
            .then(text::int(10))
            .then(just('.').then(digits.clone()))
            .to_slice()
            .map(|s: &str| Value::F32(s.parse().unwrap()));

        let i32 = just('-')
            .then(digits.clone())
            .to_slice()
            .map(|s: &str| Value::I32(s.parse().unwrap()))
            .boxed();

        let u32 = digits.to_slice().map(|s: &str| Value::U32(s.parse().unwrap()));

        let ident_raw = text::ascii::ident().map(ToString::to_string);
        let ident_value = ident_raw.clone().map(|s| match s.as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::Ident(s),
        });

        let kv = ident_raw.then_ignore(just(':').padded()).then(value.clone());
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
            object,
            array,
            ident_value,
        )).padded().boxed()
    })
}

fn ast_parser<'a>() -> impl Parser<'a, &'a str, Vec<AST>, Err<Rich<'a, char>>> {
    let ident = text::ascii::ident().map(ToString::to_string);

    let scope_block = ident
        .then_ignore(just('{').padded())
        .then(value_parser())
        .then_ignore(just('}').padded())
        .map(|(name, val)| AST::DeclarationScope((name, val)));

    scope_block
        .repeated()
        .collect()
        .padded()
        .then_ignore(end())
}

#[cfg(test)]
mod test_two {
    use super::*;
    use ariadne::{Color, Label, Report, ReportKind, Source};
    use std::collections::BTreeMap;

    fn assert_parse_equals_value(input: &str, expected: Value) {
        let parser = value_parser();
        match parser.parse(input).into_result() {
            Ok(output) => assert_eq!(output, expected, "Parsed result didn't match expectation"),
            Err(errors) => {
                let errors_pretty = errors.into_iter().for_each(|e| {
                    Report::build(ReportKind::Error, ((), e.span().into_range()))
                        .with_config(ariadne::Config::new().with_index_type(ariadne::IndexType::Byte))
                        .with_message(e.to_string())
                        .with_label(
                            Label::new(((), e.span().into_range()))
                                .with_message(e.reason().to_string())
                                .with_color(Color::Red),
                        )
                        .finish()
                        .print(Source::from(&input))
                        .unwrap()
                });
                println!("{:?}", errors_pretty);
                panic!("Parse failed (see report above)");
            }
        }
    }

    fn assert_parse_equals_ast(input: &str, expected: Vec<AST>) {
        let parser = ast_parser();
        match parser.parse(input).into_result() {
            Ok(output) => assert_eq!(output, expected, "Parsed result didn't match expectation"),
            Err(errors) => {
                let errors_pretty = errors.into_iter().for_each(|e| {
                    Report::build(ReportKind::Error, ((), e.span().into_range()))
                        .with_config(ariadne::Config::new().with_index_type(ariadne::IndexType::Byte))
                        .with_message(e.to_string())
                        .with_label(
                            Label::new(((), e.span().into_range()))
                                .with_message(e.reason().to_string())
                                .with_color(Color::Red),
                        )
                        .finish()
                        .print(Source::from(&input))
                        .unwrap()
                });
                println!("{:?}", errors_pretty);
                panic!("Parse failed (see report above)");
            }
        }
    }

    #[test]
    fn test_debug_values() {
        // Test the string specifically
        assert_parse_equals_value(
            r#""Line\nNew""#, 
            Value::String("Line\nNew".to_string())
        );

        // Test the number specifically
        assert_parse_equals_value("32", Value::U32(32));
        
        // Test the object
        let input = r#"{ bio: "Rust\nRules" }"#;
        let mut map = BTreeMap::new();
        map.insert("bio".to_string(), Value::String("Rust\nRules".to_string()));
        assert_parse_equals_value(input, Value::Object(map));
    }

    #[test]
    fn parse_values() {
        let cases = [
            ("32", Value::U32(32)),
            ("42.0", Value::F32(42.0)),
            ("-64", Value::I32(-64)),
            ("false", Value::Bool(false)),
            ("true", Value::Bool(true)),
            ("bob", Value::Ident("bob".into())),
            ("[42.0, 31.0, 24.0]", Value::Array(vec![Value::F32(42.0), Value::F32(31.0), Value::F32(24.0)])),
            (r#"
                {
                    version: 42.0,
                    settings: { 
                        enabled: true 
                    }
                }
            "#, Value::Object(BTreeMap::from([
            ("version".to_string(), Value::F32(42.0)),
            ("settings".to_string(), Value::Object(BTreeMap::from([
                ("enabled".to_string(), Value::Bool(true))
            ])))
        ])))
        ];

        for (input, expected) in cases {            
            assert_parse_equals_value(input, expected);
        }
    }

    #[test]
    fn test_bogus(){
        // Should fail as we need keyword or closing string
        let missing_quote = r#"
            MyScope {
                {
                    version: 1,
                    settings: { 
                        enabled: "true 
                    }
                }
            }
        "#;

        let parser = value_parser();
        let res = parser.parse(&missing_quote);
        
        assert_eq!(res.output().is_none(), true);
    }

    #[test]
    fn test_declaration_scope() {
        let src = r#"
            MyScope {
                {
                    version: 1,
                    settings: { 
                        enabled: true 
                    }
                }
            }
        "#;

        let expected_map = BTreeMap::from([
            ("version".to_string(), Value::U32(1)),
            ("settings".to_string(), Value::Object(BTreeMap::from([
                ("enabled".to_string(), Value::Bool(true))
            ])))
        ]);

        let expected_ast = vec![
            AST::DeclarationScope((
                "MyScope".to_string(),
                Value::Object(expected_map)
            ))
        ];

        assert_parse_equals_ast(src, expected_ast);
    }

    #[test]
    fn test_ident_keys_string_values() {
        let input = r#"{ 
            username: "Alice",
            bio: "Software Engineer\nRust Enthusiast",
            lucky_number: 7
        }"#;

        let mut expected_map = BTreeMap::new();
        expected_map.insert("username".to_string(), Value::String("Alice".to_string()));
        expected_map.insert("bio".to_string(), Value::String("Software Engineer\nRust Enthusiast".to_string()));
        expected_map.insert("lucky_number".to_string(), Value::U32(7));

        assert_parse_equals_value(input, Value::Object(expected_map));
    }

}