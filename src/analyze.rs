// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Per-line size and cycle analysis, for `--analyze`.
//!
//! The asm, with a `@sy <offset> <kind>` marker before each statement's
//! instructions, goes through the GNU assembler for a listing, which gives
//! each instruction's address and encoding. A rough Cortex-M33 timing model
//! (RP2350 datasheet 3.7.4.9) then runs over each function's instructions in
//! address order, as if it were straight-line code: the instruction before a
//! branch target is taken to be the one before it in memory.
//!
//! This is a first draft. Cases the datasheet doesn't settle are guesses,
//! to be checked against DWT measurements.

use std::{
    collections::{HashMap, HashSet},
    process::Command,
};

use crate::error::Error;

#[derive(Clone, Copy, PartialEq)]
enum SizeMode {
    N,
    Nib,
    Hex,
}

#[derive(Clone, Copy, PartialEq)]
enum Col {
    Line,
    Size,
    SizeWhy,
    CycWhy,
    Cyc,
    Asm,
    Src,
}

pub struct Opts {
    size: SizeMode,
    cols: Vec<Col>,
    func: Option<String>,
    legend: bool,
}

impl Opts {
    pub fn parse(
        size: &str,
        cols: &str,
        func: Option<String>,
        legend: bool,
    ) -> Result<Self, Error> {
        let size = match size {
            "n" => SizeMode::N,
            "nib" => SizeMode::Nib,
            "hex" => SizeMode::Hex,
            _ => Err(format!("unknown size mode {size}, expected n, nib or hex"))?,
        };
        let cols = cols
            .split(',')
            .map(|c| {
                Ok(match c.trim() {
                    "line" => Col::Line,
                    "size" => Col::Size,
                    "szwhy" => Col::SizeWhy,
                    "cycwhy" => Col::CycWhy,
                    "cyc" => Col::Cyc,
                    "asm" => Col::Asm,
                    "src" => Col::Src,
                    _ => Err(format!("unknown column {c}"))?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Opts {
            size,
            cols,
            func,
            legend,
        })
    }
}

const LEGEND: &str = "\
size why, when a 16 bit encoding was one change away:
  H  a high register (r8 and up)
  F  the 16 bit form sets flags (or, in an IT block, doesn't)
  3  the 16 bit form needs Rd to also be a source
cycle why, shown only when a cycle is lost:
  L  load-use: a source is the previous load's result
  R  result-use: a source is the previous multiply, divide or complex op's result
  S  a complex op (rotated immediate, shift not lsl #0-3) that sets flags
  U  the branch target (for bl, the return site) is 32 bit and not word aligned
  F  the branch isn't folded with the previous instruction
  K  folded, but the previous instruction sets flags, so taken still costs
  ?  the model isn't sure
cycles for a conditional branch are not-taken/taken; - in size is an elided statement
";

#[derive(Debug, Clone)]
enum Op {
    Reg(u8),
    Imm(i64),
    Shift,
    Mem {
        base: u8,
        index: Option<u8>,
        imm: Option<i64>,
        shifted: bool,
    },
    List(Vec<u8>),
    Sym(String),
}

struct Insn {
    func: usize,
    text: String,
    addr: u32,
    hw: Vec<u16>,
    /// Branched to, or the return site of a call.
    target: bool,
    base: String,
    s: bool,
    cond: Option<String>,
    ops: Vec<Op>,
    /// Writeback, with `!` or a post-indexed offset.
    wb: bool,
    cyc: String,
    cyc_why: String,
    size_why: Option<char>,
}

impl Insn {
    fn size(&self) -> u32 {
        self.hw.len() as u32 * 2
    }
}

struct Marker {
    func: usize,
    offset: usize,
    assign: bool,
    insns: Vec<usize>,
}

pub fn analyze(
    asm: &str,
    src: &str,
    files: &[(String, usize)],
    fns: &[(String, usize)],
    opts: &Opts,
) -> Result<(), Error> {
    let lines: Vec<&str> = asm.lines().collect();
    let listing = parse_listing(&assemble(asm)?, lines.len());
    let fn_index: HashMap<&str, usize> = fns
        .iter()
        .enumerate()
        .map(|(i, (name, _))| (name.as_str(), i))
        .collect();

    let mut insns: Vec<Insn> = vec![];
    let mut markers: Vec<Marker> = vec![];
    let mut labels: HashMap<String, usize> = HashMap::new();
    let mut pending_labels: Vec<String> = vec![];
    let mut cur_fn: Option<usize> = None;
    for (ix, line) in lines.iter().enumerate() {
        if line.starts_with(".section") {
            // Labels at the very end of a section don't name an instruction.
            pending_labels.clear();
            cur_fn = None;
        } else if let Some(rest) = line.strip_prefix("@sy ") {
            let mut parts = rest.split_whitespace();
            let offset = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
            let assign = parts.next() == Some("a");
            if let Some(func) = cur_fn {
                markers.push(Marker {
                    func,
                    offset,
                    assign,
                    insns: vec![],
                });
            }
        } else if let Some(label) = line.strip_suffix(':') {
            if !label.starts_with(' ') {
                if let Some(&f) = fn_index.get(label) {
                    cur_fn = Some(f);
                }
                pending_labels.push(label.to_owned());
            }
        } else if line.starts_with("    ")
            && line
                .trim_start()
                .starts_with(|c: char| c.is_ascii_lowercase())
        {
            let (Some(func), Some((addr, hw))) = (cur_fn, &listing[ix]) else {
                continue;
            };
            let text = line.trim().to_owned();
            let (mnem, rest) = text.split_once(' ').unwrap_or((&text, ""));
            let (base, s, cond) = parse_mnemonic(mnem);
            let ops = parse_ops(rest);
            let wb = rest.contains('!')
                || ops
                    .windows(2)
                    .any(|w| matches!((&w[0], &w[1]), (Op::Mem { .. }, Op::Imm(_))));
            let i = insns.len();
            for l in pending_labels.drain(..) {
                labels.insert(l, i);
            }
            if let Some(m) = markers.last_mut().filter(|m| m.func == func) {
                m.insns.push(i);
            }
            insns.push(Insn {
                func,
                text,
                addr: *addr,
                hw: hw.clone(),
                target: false,
                base,
                s,
                cond,
                ops,
                wb,
                cyc: String::new(),
                cyc_why: String::new(),
                size_why: None,
            });
        }
    }

    // Branch targets: referenced labels, and return sites of calls
    let mut referenced = HashSet::new();
    for insn in &insns {
        if is_branch(&insn.base) {
            for op in &insn.ops {
                if let Op::Sym(s) = op {
                    referenced.insert(s.clone());
                }
            }
        }
    }
    for l in &referenced {
        if let Some(&i) = labels.get(l) {
            insns[i].target = true;
        }
    }
    for i in 1..insns.len() {
        if matches!(insns[i - 1].base.as_str(), "bl" | "blx") {
            insns[i].target = true;
        }
    }

    model(&mut insns, &labels);
    for insn in &mut insns {
        insn.size_why = size_why(insn);
    }

    if opts.legend {
        print!("{LEGEND}\n");
    }
    let lines = SrcLines::new(src, files);
    let mut first = true;
    for (f, (name, offset)) in fns.iter().enumerate() {
        if opts.func.as_ref().is_some_and(|n| n != name) {
            continue;
        }
        let fn_markers: Vec<&Marker> = markers.iter().filter(|m| m.func == f).collect();
        if fn_markers.is_empty() {
            continue;
        }
        let start = lines.line_of(*offset);
        let last = fn_markers
            .iter()
            .map(|m| lines.line_of(m.offset))
            .max()
            .unwrap_or(start);
        let end = lines.fn_end(last);
        let fn_insns: Vec<&Insn> = insns.iter().filter(|i| i.func == f).collect();
        let bytes: u32 = fn_insns.iter().map(|i| i.size()).sum();
        let (path, line) = lines.file_line(start);
        if !first {
            println!();
        }
        first = false;
        println!(
            "== {name}  {path}:{line}  {bytes} bytes, {} insns",
            fn_insns.len()
        );
        let mut rows: HashMap<usize, Vec<Option<usize>>> = HashMap::new();
        for m in &fn_markers {
            let r = rows.entry(lines.line_of(m.offset)).or_default();
            if m.insns.is_empty() {
                if m.assign {
                    r.push(None);
                }
            } else {
                r.extend(m.insns.iter().map(|&i| Some(i)));
            }
        }
        for gl in start..=end {
            let (_, local) = lines.file_line(gl);
            let text = lines.text(gl);
            match rows.get(&gl) {
                Some(r) if !r.is_empty() => {
                    for (k, row) in r.iter().enumerate() {
                        let src = if k == 0 { text } else { "" };
                        print_row(opts, local, row.map(|i| &insns[i]), true, src);
                    }
                }
                _ => print_row(opts, local, None, false, text),
            }
        }
    }
    Ok(())
}

fn print_row(opts: &Opts, line: usize, insn: Option<&Insn>, stmt: bool, src: &str) {
    let mut out = String::new();
    for col in &opts.cols {
        let cell = match col {
            Col::Line => format!("{line:>5}:"),
            Col::Size => {
                let s = match (insn, stmt) {
                    (Some(i), _) => size_str(opts.size, &i.hw),
                    // An elided statement
                    (None, true) => "-".to_owned(),
                    (None, false) => String::new(),
                };
                let w = match opts.size {
                    SizeMode::N => 1,
                    SizeMode::Nib => 2,
                    SizeMode::Hex => 9,
                };
                format!("{s:<w$}")
            }
            Col::SizeWhy => format!("{}", insn.and_then(|i| i.size_why).unwrap_or(' ')),
            Col::CycWhy => format!("{:<3}", insn.map_or("", |i| &i.cyc_why)),
            Col::Cyc => format!("{:>5}", insn.map_or("", |i| &i.cyc)),
            Col::Asm => format!("{:<28}", insn.map_or("", |i| &i.text)),
            Col::Src => src.to_owned(),
        };
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&cell);
    }
    println!("{}", out.trim_end());
}

fn size_str(mode: SizeMode, hw: &[u16]) -> String {
    match mode {
        SizeMode::N => format!("{}", hw.len() * 2),
        SizeMode::Nib => hw.iter().map(|h| format!("{:x}", h >> 12)).collect(),
        SizeMode::Hex => hw
            .iter()
            .map(|h| format!("{h:04x}"))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// The source, split into lines, with the files they came from.
struct SrcLines<'a> {
    lines: Vec<&'a str>,
    starts: Vec<usize>,
    /// The first global line of each file, and its path
    files: Vec<(usize, &'a str)>,
}

impl<'a> SrcLines<'a> {
    fn new(src: &'a str, files: &'a [(String, usize)]) -> Self {
        let lines: Vec<&str> = src.split('\n').collect();
        let mut starts = vec![];
        let mut pos = 0;
        for l in &lines {
            starts.push(pos);
            pos += l.len() + 1;
        }
        let mut s = SrcLines {
            lines,
            starts,
            files: vec![],
        };
        s.files = files
            .iter()
            .map(|(path, start)| (s.line_of(*start), path.as_str()))
            .collect();
        s
    }

    fn line_of(&self, offset: usize) -> usize {
        match self.starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    fn file_line(&self, gl: usize) -> (&str, usize) {
        let (first, path) = self
            .files
            .iter()
            .rev()
            .find(|(first, _)| *first <= gl)
            .copied()
            .unwrap_or((0, ""));
        (path, gl - first + 1)
    }

    fn text(&self, gl: usize) -> &str {
        self.lines.get(gl).copied().unwrap_or("")
    }

    /// The function's closing brace, the first line at or after its last
    /// statement that starts with `}`.
    fn fn_end(&self, last: usize) -> usize {
        let (path, _) = self.file_line(last);
        for gl in last..self.lines.len() {
            if self.file_line(gl).0 != path {
                break;
            }
            if self.lines[gl].starts_with('}') {
                return gl;
            }
        }
        last
    }
}

fn assemble(asm: &str) -> Result<String, Error> {
    let dir = std::env::temp_dir().join(format!("symbolasm-analyze-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let s = dir.join("in.s");
    let lst = dir.join("in.lst");
    std::fs::write(&s, asm)?;
    let assembler = std::env::var("AS").unwrap_or_else(|_| "arm-none-eabi-as".into());
    let out = Command::new(&assembler)
        .arg("-mcpu=cortex-m33")
        .arg(format!("-aln={}", lst.display()))
        .arg("-o")
        .arg(dir.join("in.o"))
        .arg(&s)
        .output()
        .map_err(|e| format!("running {assembler}: {e}"))?;
    if !out.status.success() {
        Err(format!(
            "{assembler} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ))?;
    }
    let text = std::fs::read_to_string(&lst)?;
    _ = std::fs::remove_dir_all(&dir);
    Ok(text)
}

/// The address and halfwords of each asm line that emits bytes.
fn parse_listing(lst: &str, n: usize) -> Vec<Option<(u32, Vec<u16>)>> {
    let mut result = vec![None; n];
    for line in lst.lines() {
        let Some((prefix, _)) = line.split_once('\t') else {
            continue;
        };
        // Continuation lines, for data that doesn't fit, have no line number.
        if prefix.starts_with("     ") {
            continue;
        }
        let toks: Vec<&str> = prefix.split_whitespace().collect();
        if toks.len() < 3 {
            continue;
        }
        let (Ok(lineno), Ok(addr)) = (toks[0].parse::<usize>(), u32::from_str_radix(toks[1], 16))
        else {
            continue;
        };
        let hex = toks[2];
        let bytes: Vec<u8> = (0..hex.len() / 2)
            .filter_map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok())
            .collect();
        let hw = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if (1..=n).contains(&lineno) {
            result[lineno - 1] = Some((addr, hw));
        }
    }
    result
}

const BASES: &[&str] = &[
    "adc", "add", "addw", "adr", "and", "asr", "b", "bfc", "bfi", "bic", "bkpt", "bl", "blx", "bx",
    "cbnz", "cbz", "clz", "cmn", "cmp", "cpsid", "cpsie", "dmb", "dsb", "eor", "isb", "ldm",
    "ldmia", "ldmdb", "ldr", "ldrb", "ldrd", "ldrex", "ldrh", "ldrsb", "ldrsh", "lsl", "lsr",
    "mcr", "mcrr", "mla", "mls", "mov", "movt", "movw", "mrc", "mrrc", "mrs", "msr", "mul", "mvn",
    "neg", "nop", "orn", "orr", "pkhbt", "pkhtb", "pop", "push", "rbit", "rev", "rev16", "revsh",
    "ror", "rrx", "rsb", "sbc", "sbfx", "sdiv", "sel", "sev", "smlal", "smull", "stm", "stmia",
    "stmdb", "str", "strb", "strd", "strex", "strh", "sub", "subw", "svc", "sxtb", "sxth", "tbb",
    "tbh", "teq", "tst", "ubfx", "udiv", "umlal", "umull", "uxtb", "uxth", "wfe", "wfi", "yield",
];

/// Mnemonics that take an `s` suffix
const S_OK: &[&str] = &[
    "adc", "add", "and", "asr", "bic", "eor", "lsl", "lsr", "mov", "mul", "mvn", "neg", "orn",
    "orr", "ror", "rrx", "rsb", "sbc", "sub",
];

const CONDS: &[&str] = &[
    "eq", "ne", "cs", "hs", "cc", "lo", "mi", "pl", "vs", "vc", "hi", "ls", "ge", "lt", "gt", "le",
];

/// Base mnemonic, `s` suffix, and condition.
fn parse_mnemonic(m: &str) -> (String, bool, Option<String>) {
    let m = m.trim_end_matches(".w").trim_end_matches(".n");
    if m.len() <= 5 && m.starts_with("it") && m[2..].chars().all(|c| c == 't' || c == 'e') {
        return ("it".into(), false, None);
    }
    let mut bases: Vec<&str> = BASES.iter().copied().filter(|b| m.starts_with(b)).collect();
    bases.sort_by_key(|b| std::cmp::Reverse(b.len()));
    for b in bases {
        let rest = &m[b.len()..];
        let (s, rest) = match rest.strip_prefix('s') {
            Some(r) if S_OK.contains(&b) => (true, r),
            _ => (false, rest),
        };
        if rest.is_empty() {
            return (b.into(), s, None);
        }
        if CONDS.contains(&rest) {
            return (b.into(), s, Some(rest.into()));
        }
    }
    (m.into(), false, None)
}

fn split_top(s: &str) -> Vec<&str> {
    let mut parts = vec![];
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(s[start..i].trim());
                start = i + 1;
            }
            _ => (),
        }
    }
    if !s[start..].trim().is_empty() {
        parts.push(s[start..].trim());
    }
    parts
}

fn parse_reg(s: &str) -> Option<u8> {
    match s.trim().trim_end_matches('!') {
        "sp" => Some(13),
        "lr" => Some(14),
        "pc" => Some(15),
        "ip" => Some(12),
        "fp" => Some(11),
        "sl" => Some(10),
        "sb" => Some(9),
        s => {
            let n: u8 = s.strip_prefix('r')?.parse().ok()?;
            (n < 16).then_some(n)
        }
    }
}

fn parse_imm(s: &str) -> Option<i64> {
    let s = s.trim().strip_prefix('#')?;
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let v = match s.strip_prefix("0x") {
        Some(h) => i64::from_str_radix(h, 16).ok()?,
        None => s.parse().ok()?,
    };
    Some(if neg { -v } else { v })
}

fn parse_ops(s: &str) -> Vec<Op> {
    split_top(s).into_iter().map(parse_op).collect()
}

fn parse_op(s: &str) -> Op {
    if let Some(inner) = s.strip_prefix('[') {
        let inner = inner.trim_end_matches('!').trim_end_matches(']');
        let parts = split_top(inner);
        Op::Mem {
            base: parts.first().and_then(|p| parse_reg(p)).unwrap_or(15),
            index: parts.get(1).and_then(|p| parse_reg(p)),
            imm: parts.get(1).and_then(|p| parse_imm(p)),
            shifted: parts.len() > 2,
        }
    } else if let Some(inner) = s.strip_prefix('{') {
        let mut regs = vec![];
        for part in split_top(inner.trim_end_matches('}')) {
            match part.split_once('-') {
                Some((a, b)) => {
                    if let (Some(a), Some(b)) = (parse_reg(a), parse_reg(b)) {
                        regs.extend(a..=b);
                    }
                }
                None => regs.extend(parse_reg(part)),
            }
        }
        Op::List(regs)
    } else if let Some(r) = parse_reg(s) {
        Op::Reg(r)
    } else if let Some(v) = parse_imm(s) {
        Op::Imm(v)
    } else if ["lsl ", "lsr ", "asr ", "ror "]
        .iter()
        .any(|k| s.starts_with(k))
        || s == "rrx"
    {
        Op::Shift
    } else {
        Op::Sym(s.into())
    }
}

fn is_branch(base: &str) -> bool {
    matches!(base, "b" | "bl" | "cbz" | "cbnz")
}

fn op_regs(op: &Op) -> Vec<u8> {
    match op {
        Op::Reg(r) => vec![*r],
        Op::Mem { base, index, .. } => std::iter::once(*base).chain(*index).collect(),
        Op::List(l) => l.clone(),
        _ => vec![],
    }
}

fn reg_at(ops: &[Op], i: usize) -> Option<u8> {
    match ops.get(i) {
        Some(Op::Reg(r)) => Some(*r),
        _ => None,
    }
}

/// Mnemonics whose two-operand form reads Rd, as in `adds r0, #1`
const TWO_OP: &[&str] = &[
    "add", "sub", "and", "orr", "eor", "bic", "adc", "sbc", "lsl", "lsr", "asr", "ror", "orn",
    "mul", "rsb",
];

#[derive(Default)]
struct Effects {
    srcs: Vec<u8>,
    /// Sources read as a multiply-accumulate's accumulator, a cycle later
    accum: Vec<u8>,
    /// Results with a two-cycle latency
    late: Vec<u8>,
    late_is_load: bool,
    complex: bool,
}

fn effects(insn: &Insn) -> Effects {
    let ops = &insn.ops;
    let all: Vec<u8> = ops.iter().flat_map(op_regs).collect();
    let mem: Vec<u8> = ops
        .iter()
        .filter(|o| matches!(o, Op::Mem { .. }))
        .flat_map(op_regs)
        .collect();
    let lead: Vec<u8> = ops
        .iter()
        .take_while(|o| matches!(o, Op::Reg(_)))
        .flat_map(op_regs)
        .collect();
    let list = || -> Vec<u8> {
        ops.iter()
            .find_map(|o| match o {
                Op::List(l) => Some(l.clone()),
                _ => None,
            })
            .unwrap_or_default()
    };
    let last_of =
        |l: Vec<u8>| -> Vec<u8> { l.last().copied().filter(|&r| r != 15).into_iter().collect() };
    let mut e = Effects::default();
    match insn.base.as_str() {
        "cmp" | "cmn" | "tst" | "teq" | "bx" | "blx" | "cbz" | "cbnz" | "mcr" | "mcrr" => {
            e.srcs = all
        }
        "str" | "strb" | "strh" | "strd" | "strex" => e.srcs = mem,
        "ldr" | "ldrb" | "ldrh" | "ldrsb" | "ldrsh" | "ldrd" | "ldrex" => {
            e.srcs = mem;
            let literal = ops
                .iter()
                .any(|o| matches!(o, Op::Sym(s) if s.starts_with('=')));
            // `ldr rd, =n` can come out as a mov
            if !literal || is_ldr_literal(&insn.hw) {
                e.late = lead.into_iter().filter(|&r| r != 15).collect();
                e.late_is_load = true;
            }
        }
        "ldm" | "ldmia" | "ldmdb" | "pop" => {
            e.srcs = if insn.base == "pop" {
                vec![13]
            } else {
                reg_at(ops, 0).into_iter().collect()
            };
            e.late = last_of(list());
            e.late_is_load = true;
        }
        "stm" | "stmia" | "stmdb" | "push" => {
            e.srcs = if insn.base == "push" {
                vec![13]
            } else {
                reg_at(ops, 0).into_iter().collect()
            };
        }
        "mla" | "mls" => {
            e.srcs = all[1..].to_vec();
            e.accum = all.get(3).copied().into_iter().collect();
            e.late = all[..1].to_vec();
        }
        "umlal" | "smlal" => {
            e.srcs = all.clone();
            e.accum = all[..2].to_vec();
            e.late = all[..2].to_vec();
        }
        "umull" | "smull" => {
            e.srcs = all[2..].to_vec();
            e.late = all[..2].to_vec();
        }
        "mrc" | "mrrc" => (),
        b if all.is_empty() || is_branch(b) => (),
        b => {
            let n_ops = ops.iter().filter(|o| !matches!(o, Op::Shift)).count();
            let reads_rd =
                (TWO_OP.contains(&b) && n_ops == 2) || matches!(b, "bfi" | "bfc" | "movt");
            e.srcs = if reads_rd {
                all.clone()
            } else {
                all[1..].to_vec()
            };
            if matches!(b, "mul" | "udiv" | "sdiv") {
                e.late = all[..1].to_vec();
            }
            if is_complex(&insn.hw) {
                e.complex = true;
                e.late = all[..1].to_vec();
            }
        }
    }
    e
}

fn is_ldr_literal(hw: &[u16]) -> bool {
    match hw {
        [a] => a & 0xf800 == 0x4800,
        [a, _] => a & 0xff7f == 0xf85f,
        _ => false,
    }
}

/// A complex op, by the datasheet: a shifted operand other than lsl #0-3,
/// or a modified immediate that needs a rotation. Decoded from the
/// encoding, since the assembler picks between forms.
fn is_complex(hw: &[u16]) -> bool {
    let [a, b] = hw else { return false };
    let op = (a >> 5) & 0xf;
    let mov = op == 0b0010 && a & 0xf == 0xf;
    // Data processing, modified immediate
    if a & 0xfa00 == 0xf000 && b & 0x8000 == 0 {
        let i_imm3 = ((a >> 10) & 1) << 3 | ((b >> 12) & 7);
        return !mov && i_imm3 >= 4;
    }
    // Data processing, shifted register (MOV with a shift is lsl and co.)
    if a & 0xfe00 == 0xea00 {
        let amount = ((b >> 12) & 7) << 2 | ((b >> 6) & 3);
        let ty = (b >> 4) & 3;
        return !mov && !(ty == 0 && amount <= 3);
    }
    // sel
    a & 0xfff0 == 0xfaa0 && b & 0xf0f0 == 0xf080
}

fn sets_flags(insn: &Insn) -> bool {
    insn.s || matches!(insn.base.as_str(), "cmp" | "cmn" | "tst" | "teq")
}

/// What the timing of the next instruction needs from this one.
#[derive(Clone, Copy)]
struct Prev {
    size: u32,
    aligned: bool,
    target: bool,
    folded: bool,
    /// Folded with an IT; whether that stops a fold with the next
    /// instruction is a guess.
    it_folded: bool,
    sets_flags: bool,
    push_pop: bool,
}

fn model(insns: &mut [Insn], labels: &HashMap<String, usize>) {
    let mut prev: Option<Prev> = None;
    let mut late: Vec<u8> = vec![];
    let mut late_is_load = false;
    // An IT followed by a 16 bit instruction is folded into it.
    let mut it_folded = false;
    for i in 0..insns.len() {
        if i > 0 && insns[i - 1].func != insns[i].func {
            prev = None;
            late.clear();
            it_folded = false;
        }
        let next_32 = insns
            .get(i + 1)
            .filter(|n| n.func == insns[i].func)
            .map(|n| (n.size() == 4, n.addr % 4 != 0));
        // U for a branch to `label`: target 32 bit and not word aligned
        let u_for = |sym: Option<&str>, prev: &Option<Prev>| -> bool {
            let Some(&t) = sym.and_then(|s| labels.get(s)) else {
                return false;
            };
            insns[t].size() == 4 && insns[t].addr % 4 != 0 && !prev.is_some_and(|p| p.push_pop)
        };
        let insn = &insns[i];
        let e = effects(insn);
        let mut why = String::new();
        let mut extra = 0;
        let hit: Vec<u8> = e
            .srcs
            .iter()
            .copied()
            .filter(|r| late.contains(r) && !e.accum.contains(r))
            .collect();
        if !hit.is_empty() {
            extra += 1;
            why.push(if late_is_load { 'L' } else { 'R' });
        }
        let mut next_late = e.late.clone();
        let mut next_late_is_load = e.late_is_load;
        if e.complex && sets_flags(insn) {
            // Paid here whatever comes next, so nothing is left for the
            // next instruction to wait on (a guess).
            extra += 1;
            why.push('S');
            next_late.clear();
        }
        // F: folded with the previous instruction; the bool is "unsure"
        let fold = |prev: &Option<Prev>| -> (bool, bool) {
            match prev {
                None => (false, true),
                Some(p) if p.size != 2 => (false, false),
                Some(p) if p.folded => (false, p.it_folded),
                // Folds when it falls through, not when branched to
                Some(p) if p.target && !p.aligned => (true, true),
                Some(_) => (true, false),
            }
        };
        let sym = insn.ops.iter().find_map(|o| match o {
            Op::Sym(s) => Some(s.as_str()),
            _ => None,
        });
        let cond = insn.cond.is_some();
        let base = insn.base.as_str();
        let mut folded = false;
        let mut it_fold = false;
        let mut after_uncond = false;
        let cyc = match base {
            "it" => {
                let n = if next_32.is_some_and(|(w, _)| !w) {
                    0
                } else {
                    1
                };
                it_folded = n == 0;
                // Keep the late results of the instruction before the IT.
                next_late = late.clone();
                next_late_is_load = late_is_load;
                format!("{n}")
            }
            "nop" => {
                let (f, unsure) = fold(&prev);
                folded = f;
                if unsure {
                    why.push('?');
                }
                format!("{}", 1 - f as i32)
            }
            "b" | "bx" if base == "b" || reg_at(&insn.ops, 0) == Some(14) => {
                let (f, unsure) = fold(&prev);
                let k = !(cond && prev.is_some_and(|p| p.sets_flags));
                let u = base == "b" && u_for(sym, &prev);
                let taken = extra + 2 + u as i32 - (k && f) as i32;
                if u {
                    why.push('U');
                }
                if !f {
                    why.push('F');
                } else if !k {
                    why.push('K');
                }
                if unsure {
                    why.push('?');
                }
                if cond {
                    folded = f;
                    format!("{}/{taken}", extra + 1 - f as i32)
                } else {
                    after_uncond = true;
                    format!("{taken}")
                }
            }
            "bx" => {
                after_uncond = !cond;
                format!("{}", extra + 2)
            }
            "bl" | "blx" => {
                let u = base == "bl" && u_for(sym, &prev);
                let ret_u = next_32 == Some((true, true));
                if u || ret_u {
                    why.push('U');
                }
                after_uncond = true;
                format!("{}", extra + 2 + u as i32 + ret_u as i32)
            }
            "cbz" | "cbnz" => {
                let u = u_for(sym, &prev);
                if u {
                    why.push('U');
                }
                format!("{}/{}", extra + 1, extra + 2 + u as i32)
            }
            "pop"
                if insn
                    .ops
                    .iter()
                    .any(|o| matches!(o, Op::List(l) if l.contains(&15))) =>
            {
                let n = op_regs(&insn.ops[0]).len() as i32;
                after_uncond = true;
                // load time, + 2 + L
                format!("{}", extra + n + 3)
            }
            "udiv" | "sdiv" => format!("{}-{}", 2 + extra, 11 + extra),
            "ldm" | "ldmia" | "ldmdb" | "stm" | "stmia" | "stmdb" | "push" | "pop" => {
                let n = insn.ops.iter().find_map(|o| match o {
                    Op::List(l) => Some(l.len()),
                    _ => None,
                });
                format!("{}", extra + n.unwrap_or(1) as i32)
            }
            "ldrd" | "strd" => format!("{}", extra + 2),
            _ => format!("{}", extra + 1),
        };
        if base != "it" && std::mem::take(&mut it_folded) {
            folded = true;
            it_fold = true;
        }
        let this = Prev {
            size: insn.size(),
            aligned: insn.addr % 4 == 0,
            target: insn.target,
            folded,
            it_folded: it_fold,
            sets_flags: sets_flags(insn),
            push_pop: matches!(base, "push" | "pop"),
        };
        if after_uncond {
            prev = None;
            late.clear();
        } else {
            prev = Some(this);
            late = next_late;
            late_is_load = next_late_is_load;
        }
        let insn = &mut insns[i];
        insn.cyc = cyc;
        insn.cyc_why = why;
    }
}

/// Why a 32 bit instruction isn't 16 bit, when that's one change away.
fn size_why(insn: &Insn) -> Option<char> {
    if insn.hw.len() != 2 {
        return None;
    }
    let base = insn.base.as_str();
    let ops = &insn.ops;
    let in_it = insn.cond.is_some() && base != "b";
    // 16 bit data processing sets flags outside an IT block, and not inside
    let flags_wrong = insn.s == in_it;
    let lo = |r: &u8| *r < 8;
    let regs: Vec<u8> = ops
        .iter()
        .filter_map(|o| match o {
            Op::Reg(r) => Some(*r),
            _ => None,
        })
        .collect();
    let all_lo = regs.iter().all(lo);
    let shifted = ops.iter().any(|o| matches!(o, Op::Shift));
    let n = ops.len();
    let r = |i| reg_at(ops, i);
    let mut why = vec![];
    match base {
        "add" | "sub" => {
            if regs.iter().any(|&r| r == 13 || r == 15) || shifted {
                return None;
            }
            let (rd, rn, x) = if n == 2 {
                (r(0)?, r(0)?, &ops[1])
            } else {
                (r(0)?, r(1)?, &ops[2])
            };
            match x {
                Op::Imm(v) => {
                    let v = v.abs();
                    if v > 255 {
                        return None;
                    }
                    if rd != rn && v > 7 {
                        why.push('3');
                    }
                    if !all_lo {
                        why.push('H');
                    }
                    if flags_wrong {
                        why.push('F');
                    }
                }
                Op::Reg(rm) => {
                    if base == "add" && (rd == rn || rd == *rm) && !insn.s {
                        // add Rdn, Rm takes any registers and leaves flags
                        return None;
                    }
                    if !all_lo {
                        why.push('H');
                    }
                    if flags_wrong {
                        why.push('F');
                    }
                }
                _ => return None,
            }
        }
        "mov" | "mvn" => {
            if shifted {
                return None;
            }
            match ops.get(1)? {
                // mov Rd, Rm without flags takes any registers
                Op::Reg(_) if base == "mov" && !insn.s => return None,
                Op::Reg(_) => {
                    if !all_lo {
                        why.push('H');
                    }
                    if flags_wrong {
                        why.push('F');
                    }
                }
                Op::Imm(v) if base == "mov" && (0..=255).contains(v) => {
                    if !all_lo {
                        why.push('H');
                    }
                    if flags_wrong {
                        why.push('F');
                    }
                }
                _ => return None,
            }
        }
        "cmp" => match ops.get(1)? {
            Op::Imm(v) if (0..=255).contains(v) && !all_lo => why.push('H'),
            _ => return None,
        },
        "cmn" | "tst" if !shifted && regs.len() == 2 && !all_lo => why.push('H'),
        "and" | "orr" | "eor" | "bic" | "adc" | "sbc" | "mul" | "lsl" | "lsr" | "asr" | "ror"
            if !shifted && regs.len() == n =>
        {
            let (rd, rn, rm) = if n == 2 {
                (r(0)?, r(0)?, r(1)?)
            } else {
                (r(0)?, r(1)?, r(2)?)
            };
            let commutes = matches!(base, "and" | "orr" | "eor" | "adc" | "mul");
            if !(rd == rn || (commutes && rd == rm)) {
                why.push('3');
            }
            if !all_lo {
                why.push('H');
            }
            if flags_wrong {
                why.push('F');
            }
        }
        "lsl" | "lsr" | "asr" if n == 3 && matches!(ops[2], Op::Imm(_)) => {
            if !all_lo {
                why.push('H');
            }
            if flags_wrong {
                why.push('F');
            }
        }
        "rsb" if n == 3 && matches!(ops[2], Op::Imm(0)) => {
            if !all_lo {
                why.push('H');
            }
            if flags_wrong {
                why.push('F');
            }
        }
        "sxtb" | "sxth" | "uxtb" | "uxth" | "rev" | "rev16" | "revsh" if n == 2 && !all_lo => {
            why.push('H')
        }
        "ldr" | "str" | "ldrb" | "strb" | "ldrh" | "strh" | "ldrsb" | "ldrsh" if !insn.wb => {
            let rt = r(0)?;
            let Some(Op::Mem {
                base: rn,
                index,
                imm,
                shifted,
            }) = ops.get(1)
            else {
                return None;
            };
            let mut regs = vec![rt, *rn];
            if let Some(rm) = index {
                if *shifted {
                    return None;
                }
                regs.push(*rm);
            } else {
                let imm = imm.unwrap_or(0);
                let scale = match base {
                    "ldr" | "str" => 4,
                    "ldrh" | "strh" => 2,
                    "ldrb" | "strb" => 1,
                    _ => return None,
                };
                if *rn == 13 {
                    if scale != 4 || imm < 0 || imm > 1020 || imm % 4 != 0 {
                        return None;
                    }
                    regs = vec![rt];
                } else if imm < 0 || imm % scale != 0 || imm / scale > 31 {
                    return None;
                }
            }
            if !regs.iter().all(lo) {
                why.push('H');
            }
        }
        "ldm" | "ldmia" | "stm" | "stmia" => {
            let rn = r(0)?;
            let list = op_regs(ops.get(1)?);
            let wb_ok = insn.wb || (base.starts_with("ldm") && list.contains(&rn));
            if !wb_ok {
                return None;
            }
            if !(lo(&rn) && list.iter().all(lo)) {
                why.push('H');
            }
        }
        _ => return None,
    }
    match why[..] {
        [c] => Some(c),
        _ => None,
    }
}
