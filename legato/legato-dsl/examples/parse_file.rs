use legato_dsl::parse::parse_legato_file;



fn main(){
    parse_legato_file("./example.legato").unwrap();
}