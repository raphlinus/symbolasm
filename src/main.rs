use crate::{lex::tokenize, parse::parse_program};

mod lex;
mod parse;
mod precedence;

fn main() {
    let path = std::env::args().nth(1).expect("need filename");
    let src = std::fs::read_to_string(path).expect("error reading file");
    let mut tokens = tokenize(&src).expect("error tokenizing file");
    let program = parse_program(&mut tokens).expect("parse error");
    println!("{program:#?}");
}
