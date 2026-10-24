// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Loading a program's files: the ones on the command line, and the ones
//! they name with `use "file"`, each loaded after the files it uses.
//!
//! The files are compiled as if concatenated in that order, and a file used
//! more than once is loaded once. The `use` lines are found here, before
//! tokenizing; the parser just checks their syntax.

use std::{
    collections::HashSet,
    io::Read,
    path::{Path, PathBuf},
};

pub struct Loader {
    /// A file whose text comes from stdin, as if it were on disk
    stdin: Option<(PathBuf, String)>,
    loaded: HashSet<PathBuf>,
    /// The canonical path of each of `files`
    canon: Vec<PathBuf>,
    /// The chain of uses being loaded, for reporting a cycle
    active: Vec<(PathBuf, String)>,
    /// Path as written (relative to the using file's directory), text, and
    /// whether it was named on the command line rather than only used
    pub files: Vec<(String, String, bool)>,
}

/// An error at a `use` line, or in reading a file named on the command line
pub struct LoadError {
    pub msg: String,
    /// The using file's path, line text, line and column
    pub at: Option<(String, String, usize, usize)>,
}

impl LoadError {
    pub fn report(&self) {
        match &self.at {
            Some((path, text, line, col)) => {
                eprintln!("{path}:{line}:{col}: error: {}", self.msg);
                eprintln!("{line:>5} | {text}");
                eprintln!("      | {:>col$}", "^");
            }
            None => eprintln!("error: {}", self.msg),
        }
    }
}

impl Loader {
    /// `stdin`: the path of a file to read from stdin instead of disk.
    pub fn new(stdin: Option<&str>) -> Result<Self, LoadError> {
        let stdin = match stdin {
            Some(path) => {
                let canon = canonical(path, None)?;
                let mut text = String::new();
                std::io::stdin()
                    .read_to_string(&mut text)
                    .map_err(|e| LoadError {
                        msg: format!("reading stdin: {e}"),
                        at: None,
                    })?;
                Some((canon, text))
            }
            None => None,
        };
        Ok(Loader {
            stdin,
            loaded: HashSet::new(),
            canon: vec![],
            active: vec![],
            files: vec![],
        })
    }

    /// Load `path` and the files it uses. `at` is the `use` naming it.
    pub fn load(
        &mut self,
        path: &str,
        at: Option<(String, String, usize, usize)>,
    ) -> Result<(), LoadError> {
        let canon = canonical(path, at.clone())?;
        if let Some(i) = self.active.iter().position(|(c, _)| *c == canon) {
            let mut chain: Vec<&str> = self.active[i..].iter().map(|(_, p)| p.as_str()).collect();
            chain.push(path);
            return Err(LoadError {
                msg: format!("use cycle: {}", chain.join(" -> ")),
                at,
            });
        }
        if self.loaded.contains(&canon) {
            if at.is_none() {
                let i = self.loaded_ix(&canon);
                self.files[i].2 = true;
            }
            return Ok(());
        }
        let text = match &self.stdin {
            Some((c, text)) if *c == canon => text.clone(),
            _ => std::fs::read_to_string(&canon).map_err(|e| LoadError {
                msg: format!("reading {path}: {e}"),
                at: at.clone(),
            })?,
        };
        self.active.push((canon.clone(), path.to_owned()));
        let dir = Path::new(path).parent().unwrap_or(Path::new(""));
        for (i, line) in text.lines().enumerate() {
            let Some((col, used)) = use_line(line) else {
                continue;
            };
            let used_path = dir.join(used).to_string_lossy().into_owned();
            let at = (path.to_owned(), line.to_owned(), i + 1, col);
            self.load(&used_path, Some(at))?;
        }
        self.active.pop();
        self.loaded.insert(canon.clone());
        self.canon.push(canon);
        self.files.push((path.to_owned(), text, at.is_none()));
        Ok(())
    }

    fn loaded_ix(&self, canon: &Path) -> usize {
        self.canon.iter().position(|c| c == canon).unwrap()
    }
}

fn canonical(path: &str, at: Option<(String, String, usize, usize)>) -> Result<PathBuf, LoadError> {
    std::fs::canonicalize(path).map_err(|e| LoadError {
        msg: format!("reading {path}: {e}"),
        at,
    })
}

/// For a line `use "file"`, the column of the name (1 based) and the name.
fn use_line(line: &str) -> Option<(usize, &str)> {
    let rest = line.trim_start().strip_prefix("use")?.trim_start();
    let name = rest.strip_prefix('"')?;
    let end = name.find('"')?;
    let col = line.len() - rest.len() + 1;
    Some((col, &name[..end]))
}
