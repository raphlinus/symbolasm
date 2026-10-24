// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! What the compiler knows about a program, for the language server: each
//! statement's instructions, with sizes and the cycle model, the placements
//! around it, and the definitions of items and labels.

use std::{collections::HashMap, path::PathBuf};

use crate::{
    Source,
    analyze::{self, InsnFacts},
    error::LocError,
    parse::{Item, Program},
};

pub struct Facts {
    /// None if loading failed
    pub source: Option<Source>,
    /// In source order, for all the program's files
    pub stmts: Vec<StmtFacts>,
    pub defs: Vec<Def>,
    pub error: Option<Diagnostic>,
}

pub struct StmtFacts {
    /// The statement's offset in the source
    pub offset: usize,
    /// It emits nothing (a move to the same register)
    pub elided: bool,
    pub insns: Vec<InsnFacts>,
    /// Placements just before and just after it
    pub before: Vec<Place>,
    pub after: Vec<Place>,
}

/// A variable placed in a register
pub struct Place {
    pub var: String,
    pub reg: u8,
    /// The variable's type, as written in source
    pub ty: Option<String>,
}

pub struct Def {
    pub name: String,
    pub kind: DefKind,
    /// The offset of the definition in the source
    pub offset: usize,
    /// For a label, the function it's in
    pub func: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DefKind {
    Fn,
    Struct,
    Const,
    Static,
    Symbol,
    Label,
}

/// An error, with its file and 1-based line and column (in chars)
pub struct Diagnostic {
    pub path: String,
    pub line: usize,
    pub col: usize,
    pub msg: String,
}

/// Compile `paths` (and the files they use) and collect what's known.
/// `overrides` are texts to use instead of files on disk, by canonical path.
/// Compiling stops at the first error, so with an error there are no
/// statements or definitions.
pub fn facts(paths: &[String], overrides: HashMap<PathBuf, String>, svd: Option<&str>) -> Facts {
    let source = match Source::load(paths, overrides) {
        Ok(source) => source,
        Err(e) => {
            let (path, line, col) = match &e.at {
                Some((path, _, line, col)) => (path.clone(), *line, *col),
                None => (paths.first().cloned().unwrap_or_default(), 1, 1),
            };
            return Facts {
                source: None,
                stmts: vec![],
                defs: vec![],
                error: Some(Diagnostic {
                    path,
                    line,
                    col,
                    msg: e.msg,
                }),
            };
        }
    };
    let result = crate::compile(&source, svd, true).and_then(|out| {
        let asm = String::from_utf8(out.asm)?;
        let insns = analyze::statements(&asm, &out.fns)?;
        Ok((insns, out.places, out.defs))
    });
    match result {
        Ok((insns, places, defs)) => {
            let mut places: HashMap<usize, _> = places.into_iter().map(|p| (p.offset, p)).collect();
            let stmts = insns
                .into_iter()
                .map(|s| {
                    let (before, after) = places
                        .remove(&s.offset)
                        .map_or((vec![], vec![]), |p| (p.before, p.after));
                    StmtFacts {
                        offset: s.offset,
                        elided: s.elided,
                        insns: s.insns,
                        before,
                        after,
                    }
                })
                .collect();
            Facts {
                source: Some(source),
                stmts,
                defs,
                error: None,
            }
        }
        Err(err) => {
            let error = Some(diagnostic(&source, &*err));
            Facts {
                source: Some(source),
                stmts: vec![],
                defs: vec![],
                error,
            }
        }
    }
}

fn diagnostic(source: &Source, err: &(dyn std::error::Error + 'static)) -> Diagnostic {
    match err.downcast_ref::<LocError>() {
        Some(e) => {
            let (file, line, col) = source.location(e.loc.offset);
            Diagnostic {
                path: source.files[file].0.clone(),
                line,
                col,
                msg: e.err.to_string(),
            }
        }
        None => Diagnostic {
            path: source
                .files
                .first()
                .map(|f| f.0.clone())
                .unwrap_or_default(),
            line: 1,
            col: 1,
            msg: err.to_string(),
        },
    }
}

impl Source {
    /// The file (an index into `files`), 1-based line and 1-based column
    /// (in chars) of an offset into `src`.
    pub fn location(&self, offset: usize) -> (usize, usize, usize) {
        let offset = offset.min(self.src.len());
        let file = self
            .files
            .iter()
            .rposition(|(_, start)| *start <= offset)
            .unwrap_or(0);
        let start = self.files.get(file).map_or(0, |f| f.1);
        let line_start = self.src[..offset]
            .rfind('\n')
            .map_or(0, |i| i + 1)
            .max(start);
        let line = self.src[start..offset].matches('\n').count() + 1;
        let col = self.src[line_start..offset].chars().count() + 1;
        (file, line, col)
    }
}

/// The top-level items' definitions
pub(crate) fn item_defs(program: &Program, defs: &mut Vec<Def>) {
    for item in &program.0 {
        let (name, kind) = match item {
            Item::Function(f) => (&f.name, DefKind::Fn),
            Item::Struct(s) => (&s.name, DefKind::Struct),
            Item::Const(c) => (&c.name, DefKind::Const),
            Item::Static(s) => (&s.name, DefKind::Static),
            Item::Symbol(s) => (&s.name, DefKind::Symbol),
        };
        if let Some(id) = name.as_ident() {
            defs.push(Def {
                name: id.to_owned(),
                kind,
                offset: name.loc.offset,
                func: None,
            });
        }
    }
}
