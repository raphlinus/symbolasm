// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Tests of `facts`, what the language server knows about a program.
//! They need `arm-none-eabi-as` (for sizes and cycles), and are skipped
//! without it.

use std::{collections::HashMap, path::Path, process::Command};

use symbolasm::facts::{DefKind, Facts, facts};

fn have_as() -> bool {
    Command::new("arm-none-eabi-as")
        .arg("--version")
        .output()
        .is_ok()
}

fn use_sy() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/use.sy");
    path.to_string_lossy().into_owned()
}

/// use.sy with its text replaced, as an editor's unsaved buffer
fn with_text(text: &str) -> Facts {
    let path = use_sy();
    let canon = std::fs::canonicalize(&path).unwrap();
    facts(&[path], HashMap::from([(canon, text.to_owned())]), None)
}

/// The statement starting with `text`
fn stmt<'a>(f: &'a Facts, text: &str) -> &'a symbolasm::facts::StmtFacts {
    let src = &f.source.as_ref().unwrap().src;
    f.stmts
        .iter()
        .find(|s| src[s.offset..].starts_with(text))
        .unwrap_or_else(|| panic!("no statement {text}"))
}

#[test]
fn placements_and_insns() {
    if !have_as() {
        return;
    }
    let f = facts(&[use_sy()], HashMap::new(), None);
    assert!(f.error.is_none());
    let s = stmt(&f, "p.x = x");
    let places: Vec<_> = s
        .before
        .iter()
        .map(|p| (p.var.as_str(), p.reg, p.ty.as_deref()))
        .collect();
    assert_eq!(places, [("p", 0, Some("*Point")), ("x", 1, Some("u32"))]);
    assert_eq!(s.insns.len(), 1);
    assert_eq!(s.insns[0].asm, "str r1, [r0, #0]");
    assert_eq!(s.insns[0].size, 2);
    // x is placed by its statement
    let s = stmt(&f, "x @r1");
    assert!(s.before.iter().all(|p| p.var != "x"));
    assert!(s.after.iter().any(|p| p.var == "x" && p.reg == 1));
}

#[test]
fn defs_across_uses() {
    if !have_as() {
        return;
    }
    let f = facts(&[use_sy()], HashMap::new(), None);
    let source = f.source.as_ref().unwrap();
    let def = |name: &str| f.defs.iter().find(|d| d.name == name).unwrap();
    assert_eq!(def("set_x").kind, DefKind::Fn);
    assert_eq!(def("ORIGIN_X").kind, DefKind::Const);
    let point = def("Point");
    assert_eq!(point.kind, DefKind::Struct);
    let (file, line, col) = source.location(point.offset);
    assert!(source.files[file].0.ends_with("point.sy"));
    assert_eq!((line, col), (1, 8));
}

#[test]
fn unsaved_text_and_errors() {
    if !have_as() {
        return;
    }
    let f = with_text("use \"use/point.sy\"\n\nfn f(p: *Point) {\n    p.z = 1\n}\n");
    let err = f.error.as_ref().expect("an error");
    assert!(err.path.ends_with("use.sy"));
    assert_eq!(err.line, 4);
    assert!(f.stmts.is_empty());
    // A missing use, and a label
    let f = with_text("use \"use/nope.sy\"\n");
    let err = f.error.as_ref().expect("an error");
    assert_eq!((err.line, err.col), (1, 5));
    let f = with_text("fn f() {\nhere:\n    goto here\n}\n");
    assert!(f.error.is_none());
    let here = f.defs.iter().find(|d| d.name == "here").unwrap();
    assert_eq!(
        (here.kind, here.func.as_deref()),
        (DefKind::Label, Some("f"))
    );
}
