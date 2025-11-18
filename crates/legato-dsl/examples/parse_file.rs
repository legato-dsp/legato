use legato_dsl::ast::build_ast;
use legato_dsl::parse::{parse_legato_file, print_pair};

fn main() {
    let raw = std::fs::read_to_string("./example.legato").expect("Could not read example graph");
    let res = parse_legato_file(&raw).unwrap();

    for pair in res.clone() {
        print_pair(&pair, 4);
    }

    let ast = build_ast(res);
}
