// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::io::Write;

use clap::Parser;

use crate::{
    compile::FnScope,
    lex::{Error, LocError, WithLoc, tokenize},
    parse::parse_program,
    types::TypePool,
};

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
    let mut peripherals = None;

    if let Some(svd) = &args.svd {
        peripherals = Some(svd::parse_svd(svd, &mut types).map_err(|e| format!("{svd}: {e}"))?);
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

    let w = &mut std::io::stdout();
    writeln!(w, ".cpu cortex-m33")?;
    writeln!(w, ".syntax unified")?;
    writeln!(w, ".thumb")?;

    for item in &program.0 {
        match item {
            parse::Item::Function(func) => {
                let mut scope = FnScope::default();
                scope.analyze(func)?;
                scope.gen_function(func, &mut types, peripherals.as_ref(), w)?;
            }
            _ => (),
        }
    }
    Ok(())
}

/// Print an error, with line, column and the source line if it has a location.
fn report_error(path: &str, src: &str, err: &(dyn std::error::Error + 'static)) {
    let Some(loc_err) = err.downcast_ref::<LocError>() else {
        eprintln!("error: {err}");
        return;
    };
    let offset = loc_err.loc.offset.min(src.len());
    let line_start = src[..offset].rfind('\n').map_or(0, |i| i + 1);
    let line_end = src[offset..].find('\n').map_or(src.len(), |i| offset + i);
    let line = src[..offset].matches('\n').count() + 1;
    let col = src[line_start..offset].chars().count() + 1;
    eprintln!("{path}:{line}:{col}: error: {}", loc_err.err);
    eprintln!("{:>5} | {}", line, &src[line_start..line_end]);
    eprintln!("      | {:>col$}", "^");
}
