// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! A language server for symbolasm: compile errors as diagnostics, the
//! cycles of each statement that loses some as an inlay hint, hovers with the instructions a
//! statement compiles to and where its variables are placed, go to
//! definition and highlighting of a name's uses.
//!
//! Each open file is compiled as the root of a program, with the files it
//! uses, and open files' unsaved text in place of what's on disk.

mod text;

use std::{collections::HashMap, path::PathBuf, rc::Rc};

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, DocumentHighlight,
    DocumentHighlightKind, DocumentHighlightParams, GotoDefinitionParams, GotoDefinitionResponse,
    Hover, HoverContents, HoverParams, HoverProviderCapability, InitializeParams, InlayHint,
    InlayHintLabel, InlayHintParams, InlayHintTooltip, Location, MarkupContent, MarkupKind, OneOf,
    Position, PublishDiagnosticsParams, Range, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri,
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
        PublishDiagnostics,
    },
    request::{
        DocumentHighlightRequest, GotoDefinition, HoverRequest, InlayHintRefreshRequest,
        InlayHintRequest, Request as _,
    },
};
use serde::Deserialize;
use symbolasm::facts::{self, DefKind, Facts, Place, StmtFacts};

use crate::text::{LineIndex, occurrences, path_to_uri, uri_to_path, word_at};

type Error = Box<dyn std::error::Error + Send + Sync>;

/// From the client's initializationOptions
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Options {
    /// Command line arguments: only `--svd path` is used
    args: Vec<String>,
}

struct Doc {
    text: String,
    path: PathBuf,
}

/// A compile of one open file as the root of a program
struct Analysis {
    facts: Facts,
    /// This file's index into the source's files, if it loaded
    file: Option<usize>,
    /// The statements in this file, by 0-based line: indices into stmts
    by_line: HashMap<u32, Vec<usize>>,
}

struct Server {
    conn: Connection,
    svd: Option<String>,
    docs: HashMap<Uri, Doc>,
    /// Cleared whenever an open file changes, since any program may use it
    cache: HashMap<Uri, Rc<Analysis>>,
    /// The last analysis of each open file that compiled, for hints and
    /// hovers while an edit is in progress
    last_good: HashMap<Uri, Rc<Analysis>>,
    /// Diagnostics to republish before waiting for more messages
    stale: bool,
    refresh_hints: bool,
    next_id: i32,
}

fn main() -> Result<(), Error> {
    let (conn, io) = Connection::stdio();
    let caps = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        inlay_hint_provider: Some(OneOf::Left(true)),
        definition_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        ..Default::default()
    };
    let params = conn.initialize(serde_json::to_value(caps)?)?;
    let params: InitializeParams = serde_json::from_value(params)?;
    let refresh_hints = params
        .capabilities
        .workspace
        .and_then(|w| w.inlay_hint)
        .and_then(|h| h.refresh_support)
        .unwrap_or(false);
    let options: Options = params
        .initialization_options
        .and_then(|o| serde_json::from_value(o).ok())
        .unwrap_or_default();
    let svd = options
        .args
        .iter()
        .position(|a| a == "--svd" || a == "-s")
        .and_then(|i| options.args.get(i + 1).cloned());
    let mut server = Server {
        conn,
        svd,
        docs: HashMap::new(),
        cache: HashMap::new(),
        last_good: HashMap::new(),
        stale: false,
        refresh_hints,
        next_id: 0,
    };
    server.run()?;
    drop(server);
    io.join()?;
    Ok(())
}

impl Server {
    fn run(&mut self) -> Result<(), Error> {
        loop {
            // Republish only once the messages queued so far are handled,
            // so a burst of edits compiles once.
            if self.stale && self.conn.receiver.is_empty() {
                self.stale = false;
                self.publish_all()?;
            }
            let Ok(msg) = self.conn.receiver.recv() else {
                return Ok(());
            };
            match msg {
                Message::Request(req) => {
                    if self.conn.handle_shutdown(&req)? {
                        return Ok(());
                    }
                    self.request(req)?;
                }
                Message::Notification(note) => self.notification(note)?,
                Message::Response(_) => (),
            }
        }
    }

    fn notification(&mut self, note: Notification) -> Result<(), Error> {
        match note.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let p: lsp_types::DidOpenTextDocumentParams = serde_json::from_value(note.params)?;
                if let Some(path) = uri_to_path(&p.text_document.uri) {
                    let doc = Doc {
                        text: p.text_document.text,
                        path,
                    };
                    self.docs.insert(p.text_document.uri, doc);
                    self.changed();
                }
            }
            DidChangeTextDocument::METHOD => {
                let p: lsp_types::DidChangeTextDocumentParams =
                    serde_json::from_value(note.params)?;
                // Full sync, so the last change is the whole text
                if let (Some(doc), Some(change)) = (
                    self.docs.get_mut(&p.text_document.uri),
                    p.content_changes.into_iter().last(),
                ) {
                    doc.text = change.text;
                    self.changed();
                }
            }
            DidCloseTextDocument::METHOD => {
                let p: lsp_types::DidCloseTextDocumentParams = serde_json::from_value(note.params)?;
                self.docs.remove(&p.text_document.uri);
                self.last_good.remove(&p.text_document.uri);
                self.publish(&p.text_document.uri, vec![])?;
                // Programs using it now get it from disk
                self.changed();
            }
            _ => (),
        }
        Ok(())
    }

    fn changed(&mut self) {
        self.cache.clear();
        self.stale = true;
    }

    fn request(&mut self, req: Request) -> Result<(), Error> {
        let id = req.id.clone();
        let method = req.method.clone();
        // A bug here shouldn't take the server down with it
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.respond(req)));
        let resp = match result {
            Ok(Ok(Some(value))) => Response::new_ok(id, value),
            Ok(Ok(None)) => {
                let msg = format!("unhandled request {method}");
                Response::new_err(id, ErrorCode::MethodNotFound as i32, msg)
            }
            Ok(Err(e)) => Response::new_err(id, ErrorCode::InvalidParams as i32, e.to_string()),
            Err(_) => {
                let msg = format!("{method} panicked");
                Response::new_err(id, ErrorCode::InternalError as i32, msg)
            }
        };
        self.conn.sender.send(resp.into())?;
        Ok(())
    }

    /// The result of a request, or None for one not handled
    fn respond(&mut self, req: Request) -> Result<Option<serde_json::Value>, Error> {
        let result = match req.method.as_str() {
            HoverRequest::METHOD => {
                let p: HoverParams = serde_json::from_value(req.params)?;
                serde_json::to_value(self.hover(p))?
            }
            InlayHintRequest::METHOD => {
                let p: InlayHintParams = serde_json::from_value(req.params)?;
                serde_json::to_value(self.inlay_hints(p))?
            }
            GotoDefinition::METHOD => {
                let p: GotoDefinitionParams = serde_json::from_value(req.params)?;
                serde_json::to_value(self.definition(p))?
            }
            DocumentHighlightRequest::METHOD => {
                let p: DocumentHighlightParams = serde_json::from_value(req.params)?;
                serde_json::to_value(self.highlights(p))?
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }

    /// The program rooted at an open file, compiled
    fn analysis(&mut self, uri: &Uri) -> Option<Rc<Analysis>> {
        if let Some(a) = self.cache.get(uri) {
            return Some(a.clone());
        }
        let doc = self.docs.get(uri)?;
        let overrides: HashMap<PathBuf, String> = self
            .docs
            .values()
            .filter_map(|d| Some((std::fs::canonicalize(&d.path).ok()?, d.text.clone())))
            .collect();
        let root = std::fs::canonicalize(&doc.path).ok();
        let paths = [doc.path.to_string_lossy().into_owned()];
        let svd = self.svd.as_deref();
        let facts = std::panic::catch_unwind(|| facts::facts(&paths, overrides, svd))
            .unwrap_or_else(|panic| {
                let msg = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                Facts {
                    source: None,
                    stmts: vec![],
                    loops: vec![],
                    defs: vec![],
                    error: Some(facts::Diagnostic {
                        path: paths[0].clone(),
                        line: 1,
                        col: 1,
                        msg: format!("the compiler panicked: {msg}"),
                    }),
                }
            });
        let file = facts
            .source
            .as_ref()
            .and_then(|s| s.canon.iter().position(|c| Some(c) == root.as_ref()));
        let mut by_line: HashMap<u32, Vec<usize>> = HashMap::new();
        if let (Some(source), Some(file)) = (&facts.source, file) {
            for (i, stmt) in facts.stmts.iter().enumerate() {
                let (f, line, _) = source.location(stmt.offset);
                if f == file {
                    by_line.entry(line as u32 - 1).or_default().push(i);
                }
            }
        }
        let a = Rc::new(Analysis {
            facts,
            file,
            by_line,
        });
        self.cache.insert(uri.clone(), a.clone());
        if a.facts.error.is_none() {
            self.last_good.insert(uri.clone(), a.clone());
        }
        Some(a)
    }

    /// The analysis, or if it doesn't compile, the last one that did
    fn good_analysis(&mut self, uri: &Uri) -> Option<Rc<Analysis>> {
        let a = self.analysis(uri)?;
        if a.facts.error.is_some() {
            return self.last_good.get(uri).cloned().or(Some(a));
        }
        Some(a)
    }

    fn publish_all(&mut self) -> Result<(), Error> {
        let uris: Vec<Uri> = self.docs.keys().cloned().collect();
        for uri in uris {
            let diags = self.diagnostics(&uri);
            self.publish(&uri, diags)?;
        }
        // A change to one file can change the hints of any that uses it
        if self.refresh_hints {
            let id = RequestId::from(self.next_id);
            self.next_id += 1;
            let req = Request::new(id, InlayHintRefreshRequest::METHOD.into(), ());
            self.conn.sender.send(req.into())?;
        }
        Ok(())
    }

    fn publish(&self, uri: &Uri, diagnostics: Vec<Diagnostic>) -> Result<(), Error> {
        let params = PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics,
            version: None,
        };
        let note = Notification::new(PublishDiagnostics::METHOD.into(), params);
        self.conn.sender.send(note.into())?;
        Ok(())
    }

    fn diagnostics(&mut self, uri: &Uri) -> Vec<Diagnostic> {
        let Some(a) = self.analysis(uri) else {
            return vec![];
        };
        let Some(err) = &a.facts.error else {
            return vec![];
        };
        let doc = &self.docs[uri];
        let err_path = std::fs::canonicalize(&err.path).ok();
        let lines = LineIndex::new(&doc.text);
        if err_path.is_some() && err_path == std::fs::canonicalize(&doc.path).ok() {
            let range = lines.token_range(err.line - 1, err.col - 1);
            return vec![error(range, err.msg.clone(), None)];
        }
        // In a file this one uses: shown at the top, linked to the spot
        let related = err_path.and_then(|p| {
            let open = self
                .docs
                .values()
                .find(|d| std::fs::canonicalize(&d.path).ok().as_ref() == Some(&p));
            let text = match open {
                Some(d) => d.text.clone(),
                None => std::fs::read_to_string(&p).ok()?,
            };
            let range = LineIndex::new(&text).token_range(err.line - 1, err.col - 1);
            Some(vec![DiagnosticRelatedInformation {
                location: Location {
                    uri: path_to_uri(&p),
                    range,
                },
                message: err.msg.clone(),
            }])
        });
        let msg = format!("{}:{}:{}: {}", err.path, err.line, err.col, err.msg);
        vec![error(lines.token_range(0, 0), msg, related)]
    }

    fn inlay_hints(&mut self, p: InlayHintParams) -> Vec<InlayHint> {
        let uri = &p.text_document.uri;
        let Some(a) = self.good_analysis(uri) else {
            return vec![];
        };
        let lines = LineIndex::new(&self.docs[uri].text);
        let mut hints = vec![];
        for (&line, ixs) in &a.by_line {
            if line < p.range.start.line || line > p.range.end.line {
                continue;
            }
            let stmts: Vec<&StmtFacts> = ixs.iter().map(|&i| &a.facts.stmts[i]).collect();
            let Some(text) = label(&stmts) else {
                continue;
            };
            // Only lines that lose cycles: an instruction that costs more by
            // its nature (push, ldm, bl) isn't a problem
            if !stmts
                .iter()
                .flat_map(|s| &s.insns)
                .any(|i| !i.cyc_why.is_empty())
            {
                continue;
            }
            hints.push(InlayHint {
                position: lines.line_end(line as usize),
                label: InlayHintLabel::String(format!("  {text}")),
                kind: None,
                text_edits: None,
                tooltip: Some(InlayHintTooltip::MarkupContent(markdown(insn_table(
                    &stmts,
                )))),
                padding_left: Some(true),
                padding_right: None,
                data: None,
            });
        }
        hints.sort_by_key(|h| h.position.line);
        hints
    }

    fn hover(&mut self, p: HoverParams) -> Option<Hover> {
        let uri = &p.text_document_position_params.text_document.uri;
        let pos = p.text_document_position_params.position;
        let a = self.good_analysis(uri)?;
        let text = &self.docs[uri].text;
        let lines = LineIndex::new(text);
        let word = lines.offset(pos).and_then(|off| word_at(text, off));
        let stmts: Vec<&StmtFacts> = a
            .by_line
            .get(&pos.line)
            .map(|ixs| ixs.iter().map(|&i| &a.facts.stmts[i]).collect())
            .unwrap_or_default();
        let mut parts = vec![];
        if let Some((word, _)) = &word {
            if let Some(s) = stmts.first() {
                parts.extend(placement(word, s));
            }
            if parts.is_empty()
                && let Some(def) = self.lookup_def(&a, uri, pos, word)
            {
                parts.push(describe_def(&a, def));
            }
        }
        if stmts.iter().any(|s| s.elided || !s.insns.is_empty()) {
            parts.push(insn_table(&stmts));
        }
        // On a `loop {` or `while {` line, one iteration's cycles
        if let (Some(source), Some(file)) = (&a.facts.source, a.file) {
            for l in &a.facts.loops {
                let (f, line, _) = source.location(l.offset);
                if f == file && line as u32 - 1 == pos.line {
                    parts.push(format!("loop: {}", l.describe()));
                }
            }
        }
        if parts.is_empty() {
            return None;
        }
        let range = word.map(|(_, r)| lines.range(r));
        Some(Hover {
            contents: HoverContents::Markup(markdown(parts.join("\n\n---\n\n"))),
            range,
        })
    }

    /// The definition a name at `pos` refers to: a label in the enclosing
    /// function, or an item.
    fn lookup_def<'a>(
        &self,
        a: &'a Analysis,
        uri: &Uri,
        pos: Position,
        name: &str,
    ) -> Option<&'a facts::Def> {
        let source = a.facts.source.as_ref()?;
        let start = source.files[a.file?].1;
        let off = start + LineIndex::new(&self.docs[uri].text).offset(pos)?;
        let func = enclosing_fn(&a.facts, off);
        let mut item = None;
        for d in a.facts.defs.iter().filter(|d| d.name == name) {
            if d.kind != DefKind::Label {
                item = Some(d);
            } else if d.func.as_deref() == func {
                return Some(d);
            }
        }
        item
    }

    fn definition(&mut self, p: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let uri = &p.text_document_position_params.text_document.uri;
        let pos = p.text_document_position_params.position;
        let a = self.good_analysis(uri)?;
        let text = &self.docs[uri].text;
        let (word, _) = word_at(text, LineIndex::new(text).offset(pos)?)?;
        let def = self.lookup_def(&a, uri, pos, &word)?;
        let source = a.facts.source.as_ref()?;
        let (file, line, col) = source.location(def.offset);
        let start = source.files[file].1;
        let end = source.files.get(file + 1).map_or(source.src.len(), |f| f.1);
        let lines = LineIndex::new(&source.src[start..end]);
        let start = lines.position(line - 1, col - 1);
        let end = Position {
            line: start.line,
            character: start.character + def.name.encode_utf16().count() as u32,
        };
        Some(GotoDefinitionResponse::Scalar(Location {
            uri: path_to_uri(&source.canon[file]),
            range: Range { start, end },
        }))
    }

    /// The uses of a name: an item's in the whole file, anything else's
    /// (a variable, label or register) in the enclosing function
    fn highlights(&mut self, p: DocumentHighlightParams) -> Option<Vec<DocumentHighlight>> {
        let uri = &p.text_document_position_params.text_document.uri;
        let pos = p.text_document_position_params.position;
        // Not the last good one: its offsets are into older text
        let a = self.analysis(uri).filter(|a| a.facts.error.is_none());
        let text = &self.docs[uri].text;
        let lines = LineIndex::new(text);
        let off = lines.offset(pos)?;
        let (word, _) = word_at(text, off)?;
        let mut scope = 0..text.len();
        let is_item = a.as_ref().is_some_and(|a| {
            a.facts
                .defs
                .iter()
                .any(|d| d.name == word && d.kind != DefKind::Label)
        });
        if !is_item
            && let Some(a) = &a
            && let (Some(source), Some(file)) = (&a.facts.source, a.file)
        {
            let start = source.files[file].1;
            let mut fns: Vec<usize> = a
                .facts
                .defs
                .iter()
                .filter(|d| d.kind == DefKind::Fn && d.offset >= start)
                .map(|d| d.offset - start)
                .filter(|&o| o < text.len())
                .collect();
            fns.sort();
            // From the line of the function's name to the next function's
            if let Some(i) = fns.iter().rposition(|&o| o <= off) {
                let line_start = |o: usize| text[..o].rfind('\n').map_or(0, |n| n + 1);
                let to = fns.get(i + 1).map_or(text.len(), |&o| line_start(o));
                scope = line_start(fns[i])..to;
            }
        }
        let found = occurrences(text, &word, scope);
        Some(
            found
                .into_iter()
                .map(|r| DocumentHighlight {
                    range: lines.range(r),
                    kind: Some(DocumentHighlightKind::TEXT),
                })
                .collect(),
        )
    }
}

fn error(
    range: Range,
    message: String,
    related: Option<Vec<DiagnosticRelatedInformation>>,
) -> Diagnostic {
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("symbolasm".into()),
        message,
        related_information: related,
        ..Default::default()
    }
}

fn markdown(value: String) -> MarkupContent {
    MarkupContent {
        kind: MarkupKind::Markdown,
        value,
    }
}

/// The function a source offset is in, by the function names before it
fn enclosing_fn(facts: &Facts, off: usize) -> Option<&str> {
    facts
        .defs
        .iter()
        .filter(|d| d.kind == DefKind::Fn && d.offset <= off)
        .max_by_key(|d| d.offset)
        .map(|d| d.name.as_str())
}

/// A line's label: its cycles, "3", or "1/3" when a conditional branch is
/// in it, and the why codes. None for a line that emits nothing.
fn label(stmts: &[&StmtFacts]) -> Option<String> {
    let insns: Vec<_> = stmts.iter().flat_map(|s| &s.insns).collect();
    if insns.is_empty() {
        return None;
    }
    let (mut nt, mut t, mut exact) = (0, 0, true);
    for insn in &insns {
        let parts: Vec<Option<u32>> = insn.cyc.split('/').map(|n| n.parse().ok()).collect();
        match parts[..] {
            [Some(a)] => (nt, t) = (nt + a, t + a),
            [Some(a), Some(b)] => (nt, t) = (nt + a, t + b),
            _ => exact = false,
        }
    }
    let cyc = if !exact {
        insns
            .iter()
            .map(|i| i.cyc.as_str())
            .collect::<Vec<_>>()
            .join("+")
    } else if nt == t {
        format!("{nt}")
    } else {
        format!("{nt}/{t}")
    };
    let why = unique(insns.iter().flat_map(|i| i.cyc_why.chars()));
    Some(if why.is_empty() {
        cyc
    } else {
        format!("{cyc} {why}")
    })
}

fn unique(chars: impl Iterator<Item = char>) -> String {
    let mut s = String::new();
    for c in chars {
        if !s.contains(c) {
            s.push(c);
        }
    }
    s
}

/// The instructions of a line's statements (size in bits, so it doesn't
/// read as cycles, and its why, cycles and their why, asm), and what the
/// why codes mean
fn insn_table(stmts: &[&StmtFacts]) -> String {
    let insns: Vec<_> = stmts.iter().flat_map(|s| &s.insns).collect();
    let mut rows = vec![];
    if stmts.iter().any(|s| s.elided) {
        rows.push("--     elided: a move to the same register".to_string());
    }
    for i in &insns {
        let sw = i.size_why.unwrap_or(' ');
        rows.push(format!(
            "{} {sw} {:>5} {:<3} {}",
            i.size * 8,
            i.cyc,
            i.cyc_why,
            i.asm
        ));
    }
    let mut out = format!("```text\n{}\n```", rows.join("\n"));
    if insns.len() > 1 {
        let bytes: u32 = insns.iter().map(|i| i.size).sum();
        out.push_str(&format!("\n\n{} instructions, {bytes} bytes", insns.len()));
    }
    let size_whys = unique(insns.iter().filter_map(|i| i.size_why));
    let cyc_whys = unique(insns.iter().flat_map(|i| i.cyc_why.chars()));
    let (size_legend, cyc_legend) = legend();
    let mut notes = vec![];
    for (codes, legend, what) in [
        (&size_whys, &size_legend, "32 bit"),
        (&cyc_whys, &cyc_legend, "cycle"),
    ] {
        for c in codes.chars() {
            if let Some(desc) = legend.get(&c) {
                notes.push(format!("- `{c}` ({what}): {desc}"));
            }
        }
    }
    if !notes.is_empty() {
        out.push_str("\n\n");
        out.push_str(&notes.join("\n"));
    }
    out
}

/// The size and cycle why codes' meanings, from --analyze's legend
fn legend() -> (HashMap<char, String>, HashMap<char, String>) {
    let mut size = HashMap::new();
    let mut cyc = HashMap::new();
    let mut in_cyc = false;
    for line in symbolasm::analyze::LEGEND.lines() {
        if line.starts_with("cycle why") {
            in_cyc = true;
        }
        let Some(rest) = line.strip_prefix("  ") else {
            continue;
        };
        let mut chars = rest.chars();
        let (Some(c), Some(' ')) = (chars.next(), chars.next()) else {
            continue;
        };
        let desc = chars.as_str().trim().to_string();
        if in_cyc { &mut cyc } else { &mut size }.insert(c, desc);
    }
    (size, cyc)
}

fn reg_name(r: u8) -> String {
    match r {
        13 => "sp".into(),
        14 => "lr".into(),
        15 => "pc".into(),
        // s0-s31 follow the core registers, as in the compiler
        32.. => format!("s{}", r - 32),
        _ => format!("r{r}"),
    }
}

fn parse_reg(s: &str) -> Option<u8> {
    match s {
        "sp" => Some(13),
        "lr" => Some(14),
        "pc" => Some(15),
        _ if s.starts_with('s') => s[1..].parse().ok().filter(|&n| n < 32).map(|n: u8| n + 32),
        _ => s.strip_prefix('r')?.parse().ok().filter(|&n| n < 16),
    }
}

/// Where a variable is around the statement it's in, or for a register,
/// the variable it holds
fn placement(word: &str, s: &StmtFacts) -> Option<String> {
    let before = s.before.iter().find(|p| p.var == word);
    let after = s.after.iter().find(|p| p.var == word);
    let on_stack = s.stack.iter().find(|p| p.var == word);
    if word == "sp"
        && let Some(sp) = s.sp
    {
        let at = match sp {
            0 => "where it was at entry".to_owned(),
            n if n < 0 => format!("{} below where it was at entry", -n),
            n => format!("{n} above where it was at entry"),
        };
        let mut text = format!("`sp` is {at}");
        for p in &s.stack {
            text.push_str(&format!("\n\n`sp.{}` at sp + {}", p.var, p.offset));
        }
        return Some(text);
    }
    if before.is_none()
        && after.is_none()
        && let Some(p) = on_stack
    {
        let ty = p.ty.as_ref().map_or(String::new(), |t| format!(": {t}"));
        return Some(format!("`sp.{word}{ty}` at sp + {}", p.offset));
    }
    if let Some(p) = before.or(after) {
        let ty = p.ty.as_ref().map_or(String::new(), |t| format!(": {t}"));
        let at = match (before, after) {
            (Some(b), Some(a)) if a.reg == b.reg => format!("in {}", reg_name(b.reg)),
            (Some(b), Some(a)) => format!("in {}, then {}", reg_name(b.reg), reg_name(a.reg)),
            (Some(b), None) => format!("in {}, then not placed", reg_name(b.reg)),
            (None, Some(a)) => format!("placed in {} here", reg_name(a.reg)),
            (None, None) => unreachable!(),
        };
        let stack = on_stack.map_or(String::new(), |p| {
            format!(", and `sp.{word}` at sp + {}", p.offset)
        });
        return Some(format!("`{word}{ty}` {at}{stack}"));
    }
    let reg = parse_reg(word)?;
    let holds = |places: &[Place]| places.iter().find(|p| p.reg == reg).map(|p| p.var.clone());
    Some(match (holds(&s.before), holds(&s.after)) {
        (Some(b), Some(a)) if a == b => format!("`{word}` holds `{b}`"),
        (Some(b), Some(a)) => format!("`{word}` holds `{b}`, then `{a}`"),
        (Some(b), None) => format!("`{word}` holds `{b}`, then no placed variable"),
        (None, Some(a)) => format!("`{word}` holds `{a}` from here"),
        (None, None) => format!("`{word}` holds no placed variable"),
    })
}

fn describe_def(a: &Analysis, def: &facts::Def) -> String {
    let kind = match def.kind {
        DefKind::Fn => "fn",
        DefKind::Struct => "struct",
        DefKind::Const => "const",
        DefKind::Static => "static",
        DefKind::Symbol => "symbol",
        DefKind::Label => "label",
    };
    let mut out = format!("{kind} `{}`", def.name);
    if def.kind == DefKind::Fn {
        // Its size: the statements from its name to the next function's
        let next = a
            .facts
            .defs
            .iter()
            .filter(|d| d.kind == DefKind::Fn && d.offset > def.offset)
            .map(|d| d.offset)
            .min()
            .unwrap_or(usize::MAX);
        let insns: Vec<_> = a
            .facts
            .stmts
            .iter()
            .filter(|s| s.offset > def.offset && s.offset < next)
            .flat_map(|s| &s.insns)
            .collect();
        if !insns.is_empty() {
            let bytes: u32 = insns.iter().map(|i| i.size).sum();
            out.push_str(&format!(": {} instructions, {bytes} bytes", insns.len()));
        }
    }
    out
}
