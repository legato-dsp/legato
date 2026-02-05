//! This is a parser for JSON.
//! Run it with the following command:
//! cargo run --example json -- examples/sample.json

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::{extra::Err, prelude::*};
use std::{collections::HashMap, env, fs};

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
}

fn parser_two<'a>() -> impl Parser<'a, &'a str, Value, Err<Rich<'a, char>>> {
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

    choice((
        f32.map(Value::F32),
        i32.map(Value::I32),
        u32.map(Value::U32),
    ))
}

#[cfg(test)]
mod test_two {
    use super::*;

    #[test]
    fn parse_numbers(){
        let (num_u32, _errors) = parser_two().parse("32").into_output_errors();

        assert_eq!(num_u32.unwrap(), Value::U32(32));

        let (num_f32, _errors) = parser_two().parse("42.0").into_output_errors();

        assert_eq!(num_f32.unwrap(), Value::F32(42.0));

        let (num_i32, _errors) = parser_two().parse("-64").into_output_errors();

        assert_eq!(num_i32.unwrap(), Value::I32(-64));
    }
}








#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test() {
        let src = r#"
            {
                "name": "example"
            }
        "#;

        let (json, errs) = parser().parse(src.trim()).into_output_errors();

        println!("{json:#?}");

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


    }
}

