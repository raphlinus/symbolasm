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

mod analyze;
mod bitset;
mod compile;
mod consts;
mod data;
mod error;
mod flags;
mod generate;
mod globals;
mod ifthen;
mod ir;
mod lex;
mod load;
mod parse;
mod precedence;
mod regmap;
mod stmt;
mod svd;
mod typeinf;
mod types;

#[derive(Parser)]
struct Args {
    /// Source files, compiled as one program, as if concatenated in order,
    /// each after the files it names with `use "file"`.
    #[arg(required = true)]
    paths: Vec<String>,

    /// Read this file's text from stdin instead of from disk (for an
    /// editor's unsaved buffer). Its uses still resolve from its path.
    #[arg(long)]
    stdin: Option<String>,

    #[arg(short, long)]
    svd: Option<String>,

    /// Instead of asm, print a per-line size and cycle analysis of the
    /// functions in the files named here, not those only reached by use.
    #[arg(long)]
    analyze: bool,

    /// With --analyze, only this function.
    #[arg(long = "fn")]
    func: Option<String>,

    /// With --analyze, the size column: n (2 or 4), nib (first hex digit
    /// of each halfword), or hex.
    #[arg(long, default_value = "nib")]
    size: String,

    /// With --analyze, the columns to show, comma separated, from
    /// line,size,szwhy,cycwhy,cyc,asm,src.
    #[arg(long, default_value = "line,size,szwhy,cycwhy,cyc,asm,src")]
    cols: String,

    /// With --analyze, print the legend for the why columns first.
    #[arg(long)]
    legend: bool,
}

fn main() {
    let args = Args::parse();
    let mut loader = match load::Loader::new(args.stdin.as_deref()) {
        Ok(loader) => loader,
        Err(e) => {
            e.report();
            std::process::exit(1);
        }
    };
    for path in &args.paths {
        if let Err(e) = loader.load(path, None) {
            e.report();
            std::process::exit(1);
        }
    }
    let mut src = String::new();
    let mut files = vec![];
    // Byte ranges of the files named on the command line, the ones
    // --analyze reports
    let mut named = vec![];
    for (path, text, is_named) in loader.files {
        let start = src.len();
        files.push((path, start));
        src.push_str(&text);
        if !text.ends_with('\n') {
            src.push('\n');
        }
        if is_named {
            named.push(start..src.len());
        }
    }
    if let Err(err) = run(&args, &src, &files, &named) {
        report_error(&files, &src, &*err);
        std::process::exit(1);
    }
}

fn run(
    args: &Args,
    src: &str,
    files: &[(String, usize)],
    named: &[std::ops::Range<usize>],
) -> Result<(), Error> {
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
            parse::Item::Function(f) => {
                let name = f.name.as_ident().unwrap();
                if globals.is_defined(name) || types.const_value(name).is_some() {
                    return Err(format!("{name} is already defined").into()).at(&f.name.loc);
                }
                globals.fns.insert(name.to_owned());
            }
        }
    }
    let mut out = Vec::new();
    let w = &mut out;
    writeln!(w, ".cpu cortex-m33")?;
    writeln!(w, ".syntax unified")?;
    writeln!(w, ".thumb")?;

    for item in &program.0 {
        match item {
            parse::Item::Function(func) => {
                let mut scope = FnScope::default();
                scope.analyze(func)?;
                scope.gen_function(func, &mut types, &globals, args.analyze, w)?;
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
    if args.analyze {
        let fns = program
            .0
            .iter()
            .filter_map(|item| match item {
                parse::Item::Function(f) => {
                    Some((f.name.as_ident()?.to_owned(), f.name.loc.offset))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let opts = analyze::Opts::parse(&args.size, &args.cols, args.func.clone(), args.legend)?;
        let asm = String::from_utf8(out)?;
        analyze::analyze(&asm, src, files, named, &fns, &opts)?;
    } else {
        std::io::stdout().write_all(&out)?;
    }
    Ok(())
}
