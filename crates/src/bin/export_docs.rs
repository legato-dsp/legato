fn main() {
    #[cfg(feature = "docs")]
    println!("{}", legato::docs::export_nodes_json());
}
