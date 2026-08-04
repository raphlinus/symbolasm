use crate::{compile::FnScope, lex::tokenize, parse::parse_program, types::TypePool};

mod bitset;
mod compile;
mod generate;
mod ir;
mod lex;
mod parse;
mod precedence;
mod regmap;
mod stmt;
mod typeinf;
mod types;

fn main() {
    let path = std::env::args().nth(1).expect("need filename");
    let src = std::fs::read_to_string(path).expect("error reading file");
    let mut tokens = tokenize(&src).expect("error tokenizing file");
    let program = parse_program(&mut tokens).expect("parse error");
    let mut types = TypePool::new();
    println!("{program:#?}");
    for item in &program.0 {
        if let parse::Item::Struct(s) = item {
            types.register_struct(s).unwrap();
        }
    }
    // Note: this requires define before use for struct inclusion.
    // No problem for pointers though, just inclusion.
    // To loosen this, we'd need topo sort.
    for item in &program.0 {
        if let parse::Item::Struct(s) = item {
            types.populate_struct(s).unwrap();
        }
    }

    for item in &program.0 {
        match item {
            parse::Item::Function(func) => {
                let mut scope = FnScope::default();
                scope.analyze(func).unwrap();
                scope
                    .gen_function(func, &mut types, &mut std::io::stdout())
                    .unwrap();
            }
            _ => (),
        }
    }
}
