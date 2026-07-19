use crate::{compile::FnScope, lex::tokenize, parse::parse_program};

mod bitset;
mod compile;
mod generate;
mod lex;
mod parse;
mod precedence;
mod regmap;
mod stmt;

fn main() {
    let path = std::env::args().nth(1).expect("need filename");
    let src = std::fs::read_to_string(path).expect("error reading file");
    let mut tokens = tokenize(&src).expect("error tokenizing file");
    let program = parse_program(&mut tokens).expect("parse error");
    println!("{program:#?}");
    for item in &program.0 {
        match item {
            parse::Item::Function(func) => {
                let mut scope = FnScope::default();
                scope.analyze(func).unwrap();
                scope.gen_function(func, &mut std::io::stdout()).unwrap();
            }
        }
    }
}
