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
mod consts;
mod data;
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
    // Items are defined in source order, so a struct embedded in another,
    // a const used by another or in an array length, or a static named in
    // size() or count() must come first. Pointers to structs are fine in
    // any order.
    for item in &program.0 {
        match item {
            parse::Item::Struct(s) => types.populate_struct(s).at(&s.name.loc)?,
            parse::Item::Const(c) => {
                let name = c.name.as_ident().unwrap();
                let value = consts::eval(&c.value, &mut types, &globals.statics).at(&c.name.loc)?;
                if globals.is_defined(name) || !types.define_const(name, value) {
                    return Err(format!("{name} is already defined").into()).at(&c.name.loc);
                }
            }
            parse::Item::Symbol(e) => {
                let name = e.name.as_ident().unwrap();
                let ty = types.intern_from_ast(&e.ty).at(&e.name.loc)?;
                if globals.is_defined(name) || types.const_value(name).is_some() {
                    return Err(format!("{name} is already defined").into()).at(&e.name.loc);
                }
                globals.symbols.insert(name.to_owned(), ty);
            }
            parse::Item::Static(s) => {
                let name = s.name.as_ident().unwrap();
                let ty = types.intern_from_ast(&s.ty).at(&s.name.loc)?;
                if globals.is_defined(name) || types.const_value(name).is_some() {
                    return Err(format!("{name} is already defined").into()).at(&s.name.loc);
                }
                globals.statics.insert(name.to_owned(), ty);
                if !s.mutable {
                    globals.immutable.insert(name.to_owned());
                }
            }
            parse::Item::Function(_) => (),
        }
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

    // Statics, in source order
    for item in &program.0 {
        if let parse::Item::Static(s) = item {
            data::emit_static(s, &mut types, &globals, w).at(&s.name.loc)?;
        }
    }
    Ok(())
}
