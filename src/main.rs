// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::io::Write;

use clap::Parser;

use crate::{
    compile::FnScope,
    error::{Error, WithLoc, report_error},
    globals::Globals,
    lex::tokenize,
    parse::parse_program,
    types::TypePool,
};

mod bitset;
mod compile;
mod error;
mod generate;
mod globals;
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
    let src = match std::fs::read_to_string(&args.path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("error: reading {}: {e}", args.path);
            std::process::exit(1);
        }
    };
    if let Err(err) = run(&args, &src) {
        report_error(&args.path, &src, &*err);
        std::process::exit(1);
    }
}

fn run(args: &Args, src: &str) -> Result<(), Error> {
    let mut tokens = tokenize(src)?;
    let program = parse_program(&mut tokens)?;
    let mut types = TypePool::new();
    let mut globals = Globals::default();

    if let Some(svd) = &args.svd {
        let peripherals = svd::parse_svd(svd, &mut types).map_err(|e| format!("{svd}: {e}"))?;
        globals.peripherals = Some(peripherals);
    }
    //println!("{program:#?}");
    for item in &program.0 {
        if let parse::Item::Struct(s) = item {
            types.register_struct(s).at(&s.name.loc)?;
        }
    }
    // Note: this requires define before use for struct inclusion.
    // No problem for pointers though, just inclusion.
    // To loosen this, we'd need topo sort.
    for item in &program.0 {
        if let parse::Item::Struct(s) = item {
            types.populate_struct(s).at(&s.name.loc)?;
        }
    }
    for item in &program.0 {
        if let parse::Item::Symbol(e) = item {
            let name = e.name.as_ident().unwrap();
            let ty = types.intern_from_ast(&e.ty).at(&e.name.loc)?;
            if globals.symbols.insert(name.to_owned(), ty).is_some() {
                return Err(format!("duplicate symbol {name}").into()).at(&e.name.loc);
            }
        }
        if let parse::Item::Static(s) = item {
            let name = s.name.as_ident().unwrap();
            let ty = types.intern_from_ast(&s.ty).at(&s.name.loc)?;
            if globals.statics.insert(name.to_owned(), ty).is_some() {
                return Err(format!("duplicate static {name}").into()).at(&s.name.loc);
            }
        }
    }
    if let Some(name) = globals
        .statics
        .keys()
        .find(|n| globals.symbols.contains_key(*n))
    {
        return Err(format!("{name} is both a symbol and a static").into());
    }

    let w = &mut std::io::stdout();
    writeln!(w, ".cpu cortex-m33")?;
    writeln!(w, ".syntax unified")?;
    writeln!(w, ".thumb")?;

    for item in &program.0 {
        match item {
            parse::Item::Function(func) => {
                let mut scope = FnScope::default();
                scope.analyze(func)?;
                scope.gen_function(func, &mut types, &globals, w)?;
            }
            _ => (),
        }
    }

    // Zero initialized statics, in source order
    for item in &program.0 {
        if let parse::Item::Static(s) = item {
            let name = s.name.as_ident().unwrap();
            let info = types.info(globals.statics[name]);
            if info.size() == 0 {
                return Err(format!("static {name} has zero size").into()).at(&s.name.loc);
            }
            writeln!(w, ".section .bss")?;
            writeln!(w, ".balign {}", info.align())?;
            writeln!(w, "{name}:")?;
            writeln!(w, "    .space {}", info.size())?;
        }
    }
    Ok(())
}
