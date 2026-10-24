// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Text positions: LSP's lines and UTF-16 columns, byte offsets, and the
//! compiler's 1-based lines and char columns. Also names in text, and
//! file URIs.

use std::{
    ops::Range,
    path::{Path, PathBuf},
    str::FromStr,
};

use lsp_types::{Position, Uri};

/// A text's line starts
pub struct LineIndex<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        LineIndex { text, starts }
    }

    /// A line's text, without its newline
    fn line(&self, line: usize) -> &'a str {
        let Some(&start) = self.starts.get(line) else {
            return "";
        };
        let end = self
            .starts
            .get(line + 1)
            .map_or(self.text.len(), |&e| e - 1);
        self.text[start..end]
            .strip_suffix('\r')
            .unwrap_or(&self.text[start..end])
    }

    /// The byte offset of an LSP position
    pub fn offset(&self, pos: Position) -> Option<usize> {
        let start = *self.starts.get(pos.line as usize)?;
        let line = self.line(pos.line as usize);
        let mut units = 0;
        for (i, c) in line.char_indices() {
            if units >= pos.character as usize {
                return Some(start + i);
            }
            units += c.len_utf16();
        }
        Some(start + line.len())
    }

    /// The LSP position of a byte offset
    pub fn pos_of(&self, offset: usize) -> Position {
        let line = self.starts.partition_point(|&s| s <= offset) - 1;
        let start = self.starts[line];
        let character = self.text[start..offset.min(self.text.len())]
            .encode_utf16()
            .count();
        Position::new(line as u32, character as u32)
    }

    pub fn range(&self, r: Range<usize>) -> lsp_types::Range {
        lsp_types::Range::new(self.pos_of(r.start), self.pos_of(r.end))
    }

    /// The LSP position of a 0-based line and char column
    pub fn position(&self, line: usize, col: usize) -> Position {
        let units: usize = self.line(line).chars().take(col).map(char::len_utf16).sum();
        Position::new(line as u32, units as u32)
    }

    pub fn line_end(&self, line: usize) -> Position {
        Position::new(line as u32, self.line(line).encode_utf16().count() as u32)
    }

    /// The token starting at a 0-based line and char column: a name or
    /// number, or one other character
    pub fn token_range(&self, line: usize, col: usize) -> lsp_types::Range {
        let text = self.line(line);
        let start = self.position(line, col);
        let rest: String = text.chars().skip(col).collect();
        let len: usize = match rest.chars().next() {
            Some(c) if is_word(c) => rest.chars().take_while(|&c| is_word(c)).count(),
            Some('"') => rest[1..]
                .find('"')
                .map_or(1, |e| rest[..e + 2].chars().count()),
            Some(_) => 1,
            None => 0,
        };
        let units: usize = rest.chars().take(len).map(char::len_utf16).sum();
        let end = Position::new(start.line, start.character + units as u32);
        lsp_types::Range::new(start, end)
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The name (or number) at a byte offset, and its byte range
pub fn word_at(text: &str, offset: usize) -> Option<(String, Range<usize>)> {
    let bytes = text.as_bytes();
    let is = |i: usize| bytes.get(i).is_some_and(|&b| is_word(b as char));
    let mut start = offset;
    // At the end of a name counts, as editors put the cursor after it
    if !is(start) && start > 0 && is(start - 1) {
        start -= 1;
    }
    if !is(start) {
        return None;
    }
    while start > 0 && is(start - 1) {
        start -= 1;
    }
    let mut end = start;
    while is(end) {
        end += 1;
    }
    if bytes[start].is_ascii_digit() {
        return None;
    }
    Some((text[start..end].to_owned(), start..end))
}

/// The whole-word occurrences of a name in a range of the text, skipping
/// comments and strings
pub fn occurrences(text: &str, name: &str, scope: Range<usize>) -> Vec<Range<usize>> {
    let mut found = vec![];
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        let base = line_start;
        line_start += line.len();
        if line_start <= scope.start || base >= scope.end {
            continue;
        }
        let code = code_part(line);
        for (i, _) in code.match_indices(name) {
            let (s, e) = (base + i, base + i + name.len());
            let before = code[..i].chars().next_back();
            let after = code[i + name.len()..].chars().next();
            if before.is_some_and(is_word) || after.is_some_and(is_word) {
                continue;
            }
            if !in_string(code, i) && s >= scope.start && e <= scope.end {
                found.push(s..e);
            }
        }
    }
    found
}

/// A line up to its `//` comment, if any (outside a string)
fn code_part(line: &str) -> &str {
    let mut in_str = false;
    let b = line.as_bytes();
    for i in 0..b.len() {
        match b[i] {
            b'"' => in_str = !in_str,
            b'/' if !in_str && b.get(i + 1) == Some(&b'/') => return &line[..i],
            _ => (),
        }
    }
    line
}

fn in_string(code: &str, i: usize) -> bool {
    code[..i].matches('"').count() % 2 == 1
}

pub fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    let rest = uri.as_str().strip_prefix("file://")?;
    let mut bytes = vec![];
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(hex) = rest.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            bytes.push(v);
            i += 3;
        } else {
            bytes.push(b[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(String::from_utf8(bytes).ok()?))
}

pub fn path_to_uri(path: &Path) -> Uri {
    let mut s = String::from("file://");
    for &b in path.to_string_lossy().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    Uri::from_str(&s).expect("a file URI")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_positions() {
        // é is one UTF-16 unit and two bytes; 𝕏 is two units and four bytes
        let text = "// é𝕏\n    x @r1 = 2\n";
        let lines = LineIndex::new(text);
        let x = text.find('x').unwrap();
        assert_eq!(lines.pos_of(x), Position::new(1, 4));
        assert_eq!(lines.offset(Position::new(1, 4)), Some(x));
        assert_eq!(lines.line_end(0), Position::new(0, 6));
        assert_eq!(lines.position(0, 5), Position::new(0, 6));
        let r = lines.token_range(1, 4);
        assert_eq!((r.start, r.end), (Position::new(1, 4), Position::new(1, 5)));
    }

    #[test]
    fn words() {
        let text = "a_1 = b2 + 3";
        assert_eq!(word_at(text, 0), Some(("a_1".into(), 0..3)));
        // Just after a name counts
        assert_eq!(word_at(text, 3), Some(("a_1".into(), 0..3)));
        assert_eq!(word_at(text, 11), None);
        assert_eq!(word_at(text, 5), None);
    }

    #[test]
    fn occurrences_skip_comments_and_strings() {
        let text = "x = x + 1 // x\ny = \"x\" + x2 + x\n";
        let found = occurrences(text, "x", 0..text.len());
        assert_eq!(found, [0..1, 4..5, 30..31]);
        // Within a scope
        assert_eq!(occurrences(text, "x", 2..text.len()), [4..5, 30..31]);
    }
}
