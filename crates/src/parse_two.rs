//! This is a parser for JSON.
//! Run it with the following command:
//! cargo run --example json -- examples/sample.json

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::{extra::Err, prelude::*};
use std::{collections::{BTreeMap, HashMap}, env, fs};

use crate::ast::Object;

#[derive(Clone, Debug)]
pub enum Json {
    Invalid,
    Null,
    Bool(bool),
    Str(String),
    Num(f64),
    Array(Vec<Json>),
    Object(HashMap<String, Json>),
}

fn parser<'a>() -> impl Parser<'a, &'a str, Json, extra::Err<Rich<'a, char>>> {
    recursive(|value| {
        let digits = text::digits(10).to_slice();

        let frac = just('.').then(digits);

        let exp = just('e')
            .or(just('E'))
            .then(one_of("+-").or_not())
            .then(digits);

        let number = just('-')
            .or_not()
            .then(text::int(10))
            .then(frac.or_not())
            .then(exp.or_not())
            .to_slice()
            .map(|s: &str| s.parse().unwrap())
            .boxed();

        let escape = just('\\')
            .then(choice((
                just('\\'),
                just('/'),
                just('"'),
                just('b').to('\x08'),
                just('f').to('\x0C'),
                just('n').to('\n'),
                just('r').to('\r'),
                just('t').to('\t'),
                just('u').ignore_then(text::digits(16).exactly(4).to_slice().validate(
                    |digits, e, emitter| {
                        char::from_u32(u32::from_str_radix(digits, 16).unwrap()).unwrap_or_else(
                            || {
                                emitter.emit(Rich::custom(e.span(), "invalid unicode character"));
                                '\u{FFFD}' // unicode replacement character
                            },
                        )
                    },
                )),
            )))
            .ignored()
            .boxed();

        let string = none_of("\\\"")
            .ignored()
            .or(escape)
            .repeated()
            .to_slice()
            .map(ToString::to_string)
            .delimited_by(just('"'), just('"'))
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
            .boxed();

        let member = string.clone().then_ignore(just(':').padded()).then(value);
        let object = member
            .clone()
            .separated_by(just(',').padded().recover_with(skip_then_retry_until(
                any().ignored(),
                one_of(",}").ignored(),
            )))
            .collect()
            .padded()
            .delimited_by(
                just('{'),
                just('}')
                    .ignored()
                    .recover_with(via_parser(end()))
                    .recover_with(skip_then_retry_until(any().ignored(), end())),
            )
            .boxed();

        choice((
            just("null").to(Json::Null),
            just("true").to(Json::Bool(true)),
            just("false").to(Json::Bool(false)),
            number.map(Json::Num),
            string.map(Json::Str),
            array.map(Json::Array),
            object.map(Json::Object),
        ))
        .recover_with(via_parser(nested_delimiters(
            '{',
            '}',
            [('[', ']')],
            |_| Json::Invalid,
        )))
        .recover_with(via_parser(nested_delimiters(
            '[',
            ']',
            [('{', '}')],
            |_| Json::Invalid,
        )))
        .recover_with(skip_then_retry_until(
            any().ignored(),
            one_of(",]}").ignored(),
        ))
        .padded()
    })
}


#[derive(Clone, Debug, PartialEq)]
enum Value {
    U32(u32),
    I32(i32),
    F32(f32),
    Bool(bool),
    Ident(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>)
}

#[derive(Clone, Debug, PartialEq)]
enum AST {
    DeclarationScope((String, Value))
}

fn value_parser<'a>() -> impl Parser<'a, &'a str, Value, Err<Rich<'a, char>>> {
    recursive(|value| {
        let digits = text::digits(10).to_slice();

        let frac = just('.').then(digits);

        let exp = just('e')
            .or(just('E'))
            .then(one_of("+-").or_not())
            .then(digits);

        let f32 = just('-')
            .or_not()
            .then(text::int(10))
            .then(frac)
            .then(exp.or_not())
            .to_slice()
            .map(|s: &str| s.parse().unwrap())
            .boxed();

        let i32 = just('-').then(digits).to_slice().map(|s: &str| s.parse().unwrap()).boxed();

        let u32 = digits.to_slice().map(|s: &str| s.parse().unwrap()).boxed();

        let ident_raw = text::ascii::ident().map(ToString::to_string);

        let ident_value = ident_raw.clone().map(|s| match s.as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::Ident(s),
        });

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
                .boxed();

        let kv = ident_raw
            .then_ignore(just(':').padded())
            .then(value);
                
        let object = kv
            .separated_by(just(',').padded())
            .collect::<BTreeMap<String, Value>>()
            .padded()
            .delimited_by(just('{'), just('}'))
            .boxed();        

        choice((
            f32.map(Value::F32),
            i32.map(Value::I32),
            u32.map(Value::U32),
            array.map(Value::Array),
            object.map(Value::Object),
            ident_value
        )).padded().boxed()
    })
}

fn ast_parser<'a>() -> impl Parser<'a, &'a str, Vec<AST>, Err<Rich<'a, char>>> {
    let ident = text::ascii::ident().map(ToString::to_string);

    // This matches: node_type : alias { params } | pipe()
    // For now, let's focus on the DeclarationScope (String, Value) logic
    let scope_block = ident
        .then_ignore(just('{').padded())
        .then(value_parser()) // Using your existing value_parser
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
            let result = value_parser().parse(input).into_result();
            
            assert_eq!(
                result.unwrap(), 
                expected, 
                "Failed to parse input: '{}'", input
            );
        }
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

        let result = ast_parser().parse(src.trim()).into_result();

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

        assert_eq!(result.unwrap(), expected_ast);
    }

}