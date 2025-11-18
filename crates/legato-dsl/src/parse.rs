use pest::{Parser, iterators::Pairs};
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "./grammar.pest"]
pub struct LegatoParser;

pub fn parse_legato_file<'a>(file: &'a str) -> Result<Pairs<'a, Rule>, Box<dyn std::error::Error>> {
    let pairs = LegatoParser::parse(Rule::graph, file)?;

    Ok(pairs.clone())
}

pub fn print_pair<'i>(pair: &'i pest::iterators::Pair<Rule>, indent: usize) {
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
