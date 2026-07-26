use onenote_parser::Parser;
use std::env;
use typed_path::TypedPath;

fn main() {
    let path = env::args().nth(1).expect("usage: parse <file>");
    let path = TypedPath::derive(&path);

    let parser = Parser::new();
    if path.extension() == Some(b"onetoc2") {
        let notebook = parser.parse_notebook(path).unwrap();
        println!("{:#?}", notebook);
    } else {
        let section = parser.parse_section(path).unwrap();
        println!("{:#?}", section);
    }
}
