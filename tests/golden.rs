// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Golden-file tests.
//!
//! Each `tests/golden/*.sy` is compiled and compared against the `.s` file
//! next to it. Run with `UPDATE_GOLDEN=1` to (re)write the expected output.
//! If `arm-none-eabi-as` is on the path, the output is also assembled.
//!
//! Each `tests/errors/*.sy` must fail to compile, with stderr containing the
//! text after `// error: ` on its first line.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sy_files(dir: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "sy"))
        .collect();
    files.sort();
    files
}

fn compile(path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_symbolasm"))
        .arg(path)
        .output()
        .unwrap()
}

fn assemble(asm: &[u8], name: &str) -> Option<String> {
    let dir = std::env::temp_dir().join(format!("symbolasm-golden-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let s = dir.join(format!("{name}.s"));
    std::fs::write(&s, asm).unwrap();
    let out = Command::new("arm-none-eabi-as")
        .args(["-mcpu=cortex-m33", "-mthumb", "-o"])
        .arg(dir.join(format!("{name}.o")))
        .arg(&s)
        .output()
        .ok()?;
    if out.status.success() {
        None
    } else {
        Some(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

#[test]
fn golden() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut failures = vec![];
    for path in sy_files("tests/golden") {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let out = compile(&path);
        if !out.status.success() {
            failures.push(format!(
                "{name}: compile failed\n{}",
                String::from_utf8_lossy(&out.stderr)
            ));
            continue;
        }
        let expected_path = path.with_extension("s");
        if update {
            std::fs::write(&expected_path, &out.stdout).unwrap();
        } else {
            match std::fs::read(&expected_path) {
                Ok(expected) if expected == out.stdout => (),
                Ok(_) => failures.push(format!(
                    "{name}: output differs from {} (rerun with UPDATE_GOLDEN=1 if intended)",
                    expected_path.display()
                )),
                Err(_) => failures.push(format!("{name}: missing {}", expected_path.display())),
            }
        }
        if let Some(err) = assemble(&out.stdout, &name) {
            failures.push(format!("{name}: assembler rejected output\n{err}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn errors() {
    let mut failures = vec![];
    for path in sy_files("tests/errors") {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let src = std::fs::read_to_string(&path).unwrap();
        let expected = src
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("// error: "))
            .unwrap_or_else(|| panic!("{name}: first line must be `// error: <message>`"));
        let out = compile(&path);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if out.status.success() {
            failures.push(format!("{name}: compiled, expected error `{expected}`"));
        } else if !stderr.contains(expected) {
            failures.push(format!(
                "{name}: expected error `{expected}`, got\n{stderr}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
