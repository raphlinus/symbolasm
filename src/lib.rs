// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The symbolasm compiler, as a library: for the command line tool, and for
//! the language server, which wants what the compiler knows (see `facts`).

use std::{collections::HashMap, io::Write, ops::Range, path::PathBuf};

use crate::{
    compile::{FnScope, StmtPlaces},
    error::{Error, WithLoc},
    globals::Globals,
    lex::tokenize,
    parse::parse_program,
    types::TypePool,
};

mod abi;
pub mod analyze;
mod bitset;
mod compile;
mod consts;
mod data;
pub mod error;
pub mod facts;
mod flags;
mod generate;
mod globals;
mod ifthen;
mod ir;
mod lex;
pub mod load;
mod parse;
mod precedence;
mod regmap;
mod stmt;
mod svd;
mod typeinf;
mod types;

/// A program's files, concatenated in the order they're compiled in.
pub struct Source {
    pub src: String,
    /// Each file's path, as given or as reached by `use`, and its start
    /// offset in `src`
    pub files: Vec<(String, usize)>,
    /// The canonical path of each of `files`
    pub canon: Vec<PathBuf>,
    /// Byte ranges of the files named, rather than only reached by use:
    /// the ones --analyze reports
    pub named: Vec<Range<usize>>,
}

impl Source {
    /// Load `paths` and the files they use. `overrides` are texts to use
    /// instead of files on disk, by canonical path.
    pub fn load(
        paths: &[String],
        overrides: HashMap<PathBuf, String>,
    ) -> Result<Source, load::LoadError> {
        let mut loader = load::Loader::new(overrides);
        for path in paths {
            loader.load(path, None)?;
        }
        let mut src = String::new();
        let mut files = vec![];
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
        Ok(Source {
            src,
            files,
            canon: loader.canon,
            named,
        })
    }
}

/// The type of a function definition's address
fn fn_type(f: &parse::Function, types: &mut TypePool) -> Result<types::TypeHandle, Error> {
    let mut list = |args: &parse::Args| -> Result<Vec<types::TypeHandle>, Error> {
        args.0
            .iter()
            .map(|a| match &a.ty {
                Some(t) => types.intern_from_ast(t),
                None => Ok(types.get_handle(&types::Type::U32)),
            })
            .collect()
    };
    let args = list(&f.args)?;
    let rets = list(&f.rets)?;
    types.intern_fn(f.abi, args, rets)
}

/// A function's declaration and definition must have the same type
fn check_same_fn(
    name: &str,
    ty: types::TypeHandle,
    other: types::TypeHandle,
    types: &TypePool,
) -> Result<(), Error> {
    if ty != other {
        Err(format!(
            "{name} is {} here, but {} elsewhere",
            types.display(ty),
            types.display(other)
        ))?;
    }
    Ok(())
}

/// What compiling produces.
pub struct Output {
    pub asm: Vec<u8>,
    /// Each function's name and the offset of its name in `src`
    pub fns: Vec<(String, usize)>,
    /// With `facts`, the placements around each statement
    pub places: Vec<StmtPlaces>,
    /// With `facts`, the top-level items and labels, for go to definition
    pub defs: Vec<facts::Def>,
}

/// Compile `source`. With `facts`, the asm has `@sy` markers before each
/// statement (for `analyze`), and placements and definitions are collected.
pub fn compile(source: &Source, svd: Option<&str>, facts: bool) -> Result<Output, Error> {
    let src = &source.src;
    let mut tokens = tokenize(src)?;
    let program = parse_program(&mut tokens)?;
    let mut types = TypePool::new();
    let mut globals = Globals::default();

    if let Some(svd) = svd {
        let peripherals = svd::parse_svd(svd, &mut types).map_err(|e| format!("{svd}: {e}"))?;
        globals.peripherals = Some(peripherals);
    }
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
                // A function may be declared as often as it's defined, as
                // long as the types match
                let other = globals.fn_ty(name, &types);
                if types.sig(ty).is_some() && other.is_some() {
                    check_same_fn(name, ty, other.unwrap(), &types).at(&e.name.loc)?;
                } else if globals.is_defined(name) || types.const_value(name).is_some() {
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
                let ty = fn_type(f, &mut types).at(&f.name.loc)?;
                if let Some(decl) = globals.symbol_ty(name)
                    && types.sig(decl).is_some()
                    && !globals.fns.contains_key(name)
                {
                    check_same_fn(name, ty, decl, &types).at(&f.name.loc)?;
                } else if globals.is_defined(name) || types.const_value(name).is_some() {
                    return Err(format!("{name} is already defined").into()).at(&f.name.loc);
                }
                globals.fns.insert(name.to_owned(), ty);
            }
        }
    }
    let mut defs = vec![];
    if facts {
        facts::item_defs(&program, &mut defs);
    }
    let mut out = Vec::new();
    let w = &mut out;
    writeln!(w, ".cpu cortex-m33")?;
    writeln!(w, ".syntax unified")?;
    writeln!(w, ".thumb")?;

    let mut places = vec![];
    for item in &program.0 {
        if let parse::Item::Function(func) = item {
            let mut scope = FnScope::default();
            scope.analyze(func)?;
            let sink = if facts {
                scope.label_defs(func, src, &mut defs);
                Some(&mut places)
            } else {
                None
            };
            scope.gen_function(func, &mut types, &globals, sink, w)?;
        }
    }

    // Statics, in source order
    for item in &program.0 {
        if let parse::Item::Static(s) = item {
            data::emit_static(s, &mut types, &globals, w).at(&s.name.loc)?;
        }
    }
    let fns = program
        .0
        .iter()
        .filter_map(|item| match item {
            parse::Item::Function(f) => Some((f.name.as_ident()?.to_owned(), f.name.loc.offset)),
            _ => None,
        })
        .collect::<Vec<_>>();
    Ok(Output {
        asm: out,
        fns,
        places,
        defs,
    })
}
