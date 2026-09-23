// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::io::Write;

use clap::Parser;

use crate::{compile::FnScope, lex::tokenize, parse::parse_program, types::TypePool};

mod bitset;
mod compile;
mod generate;
mod ifthen;
mod ir;
mod lex;
mod parse;
mod precedence;
mod regmap;
mod stmt;
mod svd;
mod typeinf;
mod types;

#[derive(Parser)]
struct Args {
    path: String,

    #[arg(short, long)]
    svd: Option<String>,
}

fn main() {
    let args = Args::parse();
    let src = std::fs::read_to_string(&args.path).expect("error reading file");
    let mut tokens = tokenize(&src).expect("error tokenizing file");
    let program = parse_program(&mut tokens).expect("parse error");
    let mut types = TypePool::new();
    let mut peripherals = None;

    if let Some(svd) = &args.svd {
        peripherals = Some(svd::parse_svd(svd, &mut types).unwrap());
    }
    //println!("{program:#?}");
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

    let w = &mut std::io::stdout();
    writeln!(w, ".cpu cortex-m33").unwrap();
    writeln!(w, ".syntax unified").unwrap();
    writeln!(w, ".thumb").unwrap();

    for item in &program.0 {
        match item {
            parse::Item::Function(func) => {
                let mut scope = FnScope::default();
                scope.analyze(func).unwrap();
                scope
                    .gen_function(func, &mut types, peripherals.as_ref(), w)
                    .unwrap();
            }
            _ => (),
        }
    }
}
