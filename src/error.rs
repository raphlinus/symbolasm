// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Errors, including errors with source locations and their reporting.

use crate::lex::Loc;

pub type Error = Box<dyn std::error::Error>;

/// An error fattened with the source location where it occurred.
///
/// Most code returns plain errors; locations are attached at the points that
/// iterate over statements and items.
#[derive(Debug)]
pub struct LocError {
    pub loc: Loc,
    pub err: Error,
}

impl std::fmt::Display for LocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.err.fmt(f)
    }
}

impl std::error::Error for LocError {}

pub trait WithLoc<T> {
    /// Attach a location, unless the error already has a more precise one.
    fn at(self, loc: &Loc) -> Result<T, Error>;
}

impl<T> WithLoc<T> for Result<T, Error> {
    fn at(self, loc: &Loc) -> Result<T, Error> {
        self.map_err(|err| {
            if err.is::<LocError>() {
                err
            } else {
                Box::new(LocError {
                    loc: loc.clone(),
                    err,
                }) as Error
            }
        })
    }
}

/// Print an error, with file, line, column and the source line if it has a
/// location. `files` are the source files' paths and their start offsets
/// in `src`, the files concatenated, in order.
pub fn report_error(files: &[(String, usize)], src: &str, err: &(dyn std::error::Error + 'static)) {
    let Some(loc_err) = err.downcast_ref::<LocError>() else {
        eprintln!("error: {err}");
        return;
    };
    let offset = loc_err.loc.offset.min(src.len());
    let (path, file_start) = files
        .iter()
        .rev()
        .find(|(_, start)| *start <= offset)
        .map_or(("", 0), |(path, start)| (path.as_str(), *start));
    let line_start = src[..offset]
        .rfind('\n')
        .map_or(0, |i| i + 1)
        .max(file_start);
    let line_end = src[offset..].find('\n').map_or(src.len(), |i| offset + i);
    let line = src[file_start..offset].matches('\n').count() + 1;
    let col = src[line_start..offset].chars().count() + 1;
    eprintln!("{path}:{line}:{col}: error: {}", loc_err.err);
    eprintln!("{:>5} | {}", line, &src[line_start..line_end]);
    eprintln!("      | {:>col$}", "^");
}
