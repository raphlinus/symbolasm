// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Parsing

use crate::{
    error::{Error, WithLoc},
    lex::{Loc, TokBody, TokBuf, Token},
    precedence::Precedence,
    stmt::{Insn, Stmt},
};

#[derive(Debug)]
pub struct Program(pub Vec<Item>);

#[derive(Debug)]
pub enum Item {
    Function(Function),
    Struct(Struct),
    Symbol(SymbolDecl),
    Static(StaticDecl),
    Const(ConstDecl),
}

/// Data defined in this file: `static [mut] name: type [= init]`.
///
/// A `static mut` without an initializer is zero initialized.
#[derive(Debug)]
pub struct StaticDecl {
    pub name: Token,
    pub mutable: bool,
    pub ty: Type,
    pub init: Option<Init>,
    pub link_section: Option<String>,
    /// Minimum alignment from `#[align = n]`, raising the type's
    pub align: Option<u32>,
}

/// An initializer for a static.
#[derive(Debug)]
pub enum Init {
    Expr(Expr),
    /// `[a, b, c]`
    List(Vec<Init>),
    /// `[x; n]`
    Repeat(Box<Init>, Expr),
}

/// A named constant, `const name = expr`, folded to an immediate.
#[derive(Debug)]
pub struct ConstDecl {
    pub name: Token,
    pub value: Expr,
}

/// A symbol satisfied by the linker: `symbol name: type`.
///
/// Unlike a C extern, the name's value is the symbol's address.
#[derive(Debug)]
pub struct SymbolDecl {
    pub name: Token,
    pub ty: Type,
}

#[derive(Debug)]
pub struct Function {
    pub name: Token,
    pub link_section: Option<String>,
    /// Alignment of the function's start from `#[align = n]`
    pub align: Option<u32>,
    pub args: Args,
    pub body: Vec<Stmt>,
    /// Location of the first token of each statement, parallel to `body`.
    pub locs: Vec<Loc>,
}

#[derive(Debug)]
pub struct Args(pub Vec<Arg>);

#[derive(Debug)]
pub struct Arg {
    pub var: Token,
    pub ty: Option<Type>,
}

#[derive(Clone, Debug)]
pub enum Type {
    Ident(Token),
    Ptr(Box<Type>),
    /// `[T; n]`, where n is a constant expression
    Array(Box<Type>, Box<Expr>),
}

#[derive(Clone, Debug)]
pub enum Expr {
    Ident(Token),
    Literal(Token),
    Binop(Box<Expr>, Token, Box<Expr>),
    Unary(Token, Box<Expr>),
    Cast(Box<Expr>, Type),
    Field(Box<Expr>, Token),
    Slice(Box<Expr>, usize, usize),
    Tuple(Vec<Expr>),
    /// Function call syntax, currently only used for intrinsics.
    Call(Token, Vec<Expr>),
    /// `p++` (1) or `p--` (-1), only valid as `*p++`: the address is
    /// updated by the size of the transfer.
    PostInc(Box<Expr>, i32),
}

#[derive(Debug)]
pub struct Struct {
    pub name: Token,
    pub fields: Vec<Field>,
}

#[derive(Debug)]
pub struct Field {
    pub name: Token,
    pub ty: Type,
}

/// Attributes, which apply to the next item.
#[derive(Default)]
struct Attrs {
    link_section: Option<String>,
    align: Option<u32>,
    /// Location of the first attribute
    loc: Option<Loc>,
}

pub fn parse_program(toks: &mut TokBuf) -> Result<Program, Error> {
    let mut items = vec![];
    let mut attrs = Attrs::default();
    while let Some(tok) = toks.next() {
        if tok.tok == TokBody::Newline {
            continue;
        }
        let loc = tok.loc.clone();
        if tok.tok == TokBody::Octothorpe {
            parse_attr(toks, &mut attrs).at(&loc)?;
            attrs.loc.get_or_insert(loc);
            continue;
        }
        let is_fn_or_static = tok.match_str("fn") || tok.match_str("static");
        if let Some(attr_loc) = &attrs.loc
            && !is_fn_or_static
        {
            return Err("attributes only apply to fn and static".into()).at(attr_loc);
        }
        if tok.match_str("fn") {
            let mut f = parse_function(toks).at(&loc)?;
            let attrs = std::mem::take(&mut attrs);
            f.link_section = attrs.link_section;
            f.align = attrs.align;
            items.push(Item::Function(f));
        } else if tok.match_str("struct") {
            let s = parse_struct(toks).at(&loc)?;
            items.push(Item::Struct(s));
        } else if tok.match_str("static") {
            let mut s = parse_static(toks).at(&loc)?;
            let attrs = std::mem::take(&mut attrs);
            s.link_section = attrs.link_section;
            s.align = attrs.align;
            items.push(Item::Static(s));
        } else if tok.match_str("const") {
            let c = parse_const(toks).at(&loc)?;
            items.push(Item::Const(c));
        } else if tok.match_str("symbol") {
            let e = parse_symbol(toks).at(&loc)?;
            items.push(Item::Symbol(e));
        } else {
            return Err("expected fn, struct, static, const or symbol".into()).at(&loc);
        }
    }
    if let Some(attr_loc) = attrs.loc {
        return Err("attributes must be followed by fn or static".into()).at(&attr_loc);
    }
    Ok(Program(items))
}

// Note: "#" has already been consumed
fn parse_attr(toks: &mut TokBuf, attrs: &mut Attrs) -> Result<(), Error> {
    toks.expect(&TokBody::OpenBracket)?;
    let name = toks.next().ok_or("expected attribute name")?.clone();
    if !name.match_str("link_section") && !name.match_str("align") {
        Err("the attributes are #[link_section = \"name\"] and #[align = n]")?;
    }
    toks.expect(&TokBody::Equals)?;
    if name.match_str("link_section") {
        let Some(TokBody::Str(section)) = toks.next().map(|t| t.tok.clone()) else {
            return Err("expected section name string")?;
        };
        let valid = section
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'$'));
        if section.is_empty() || !valid {
            Err(format!("invalid section name {section:?}"))?;
        }
        if attrs.link_section.replace(section).is_some() {
            Err("duplicate link_section")?;
        }
    } else {
        let Some(TokBody::Number(n)) = toks.next().map(|t| t.tok.clone()) else {
            return Err("expected alignment number")?;
        };
        if n <= 0 || n > 1 << 16 || n & (n - 1) != 0 {
            Err(format!("alignment {n} isn't a power of two up to 65536"))?;
        }
        if attrs.align.replace(n as u32).is_some() {
            Err("duplicate align")?;
        }
    }
    toks.expect(&TokBody::CloseBracket)?;
    toks.expect(&TokBody::Newline)?;
    Ok(())
}

// Note: "fn" keyword has already been consumed
fn parse_function(toks: &mut TokBuf) -> Result<Function, Error> {
    let name = toks.next().ok_or("expected function name")?;
    if !name.is_ident() {
        Err("function name must be identifier")?
    }
    let name = name.clone();
    let args = parse_args(toks).at(&name.loc)?;
    let (body, locs) = parse_body(toks, name.as_ident().unwrap())?;
    Ok(Function {
        name,
        link_section: None,
        align: None,
        args,
        body,
        locs,
    })
}

fn parse_args(toks: &mut TokBuf) -> Result<Args, Error> {
    toks.expect(&TokBody::OpenParen)?;
    let mut args = vec![];
    while !toks.expect_opt(&TokBody::CloseParen) {
        args.push(parse_arg(toks)?);
        if toks.expect_opt(&TokBody::Comma) {
            continue;
        } else if toks.expect_opt(&TokBody::CloseParen) {
            break;
        } else {
            Err("syntax error in args")?;
        }
    }
    Ok(Args(args))
}

fn parse_arg(toks: &mut TokBuf) -> Result<Arg, Error> {
    let tok = toks.next().ok_or("unexpected eof in arg")?;
    if tok.is_ident() {
        let var = tok.clone();
        let mut ty = None;
        if toks.expect_opt(&TokBody::Colon) {
            ty = Some(parse_type(toks)?);
        }
        Ok(Arg { var, ty })
    } else {
        Err("expected arg to be identifier")?
    }
}

fn parse_type(toks: &mut TokBuf) -> Result<Type, Error> {
    let first = toks.next().ok_or("expected type")?;
    match &first.tok {
        TokBody::Identifier(_) => Ok(Type::Ident(first.clone())),
        TokBody::Asterisk => {
            let expr = parse_type(toks)?;
            Ok(Type::Ptr(expr.into()))
        }
        TokBody::OpenBracket => {
            let elem = parse_type(toks)?;
            toks.expect(&TokBody::Semicolon)?;
            let count = parse_expr(toks)?;
            toks.expect(&TokBody::CloseBracket)?;
            Ok(Type::Array(elem.into(), count.into()))
        }
        _ => Err("unknown type")?,
    }
}

/// A block open while parsing a function body.
enum OpenBlock {
    /// `if #cc { }`, compiled to an IT block
    It,
    /// `if { }`, which scopes the labels `else` and `end`, or `loop { }`,
    /// which scopes `loop` and `break`
    Labels {
        id: usize,
        is_loop: bool,
        has_else: bool,
        uses_else: bool,
    },
}

fn parse_body(toks: &mut TokBuf, fn_name: &str) -> Result<(Vec<Stmt>, Vec<Loc>), Error> {
    let mut stmts = vec![];
    let mut locs = vec![];
    toks.expect(&TokBody::OpenBrace)?;
    toks.expect(&TokBody::Newline)?;
    let mut blocks: Vec<OpenBlock> = vec![];
    let mut n_label_blocks = 0;
    let block_label = |id: usize, which: &str| format!(".L{fn_name}_{id}_{which}");
    loop {
        toks.eat_newlines();
        if blocks.is_empty() && toks.expect_opt(&TokBody::CloseBrace) {
            break;
        }
        let tok = toks.peek().ok_or("unexpected eof in function body")?;
        let loc = tok.loc.clone();
        // `if {` and `loop {` open a label block
        let opens_label_block = (tok.match_str("if") || tok.match_str("loop"))
            && toks.tokens.get(toks.ix + 1).map(|t| &t.tok) == Some(&TokBody::OpenBrace);
        if opens_label_block {
            if let Some(OpenBlock::It) = blocks.last() {
                return Err("blocks are not allowed inside if #cc { }".into()).at(&loc);
            }
            let is_loop = tok.match_str("loop");
            toks.next();
            toks.next();
            toks.expect(&TokBody::Newline)?;
            let id = n_label_blocks;
            n_label_blocks += 1;
            blocks.push(OpenBlock::Labels {
                id,
                is_loop,
                has_else: false,
                uses_else: false,
            });
            if is_loop {
                stmts.push(Stmt::Label(block_label(id, "loop")));
                locs.push(loc);
            }
            continue;
        }
        let mut stmt = parse_stmt(toks).at(&loc)?;
        match (&stmt, blocks.last_mut()) {
            (Stmt::StartIf(_), _) => blocks.push(OpenBlock::It),
            (Stmt::Else | Stmt::EndBlock, None) => {
                return Err("unmatched close brace".into()).at(&loc);
            }
            (Stmt::Else | Stmt::EndBlock, Some(OpenBlock::It)) => {
                if let Stmt::EndBlock = stmt {
                    blocks.pop();
                }
            }
            (
                Stmt::Else,
                Some(OpenBlock::Labels {
                    id,
                    is_loop,
                    has_else,
                    ..
                }),
            ) => {
                if *is_loop {
                    return Err("loop block can't have an else".into()).at(&loc);
                }
                if *has_else {
                    return Err("block already has an else".into()).at(&loc);
                }
                // One statement is one instruction, so no branch is
                // inserted; the then part must end with one. A conditional
                // branch is accepted, as it signals that falling through
                // into the else part is intended.
                if !stmts.last().is_some_and(is_branch) {
                    return Err("then part falls through into else; end it with goto end".into())
                        .at(&loc);
                }
                *has_else = true;
                stmt = Stmt::Label(block_label(*id, "else"));
            }
            (Stmt::EndBlock, Some(OpenBlock::Labels { id, .. })) => {
                let id = *id;
                let Some(OpenBlock::Labels {
                    is_loop,
                    has_else,
                    uses_else,
                    ..
                }) = blocks.pop()
                else {
                    unreachable!()
                };
                if uses_else && !has_else {
                    return Err("goto else in a block with no else".into()).at(&loc);
                }
                stmt = Stmt::Label(block_label(id, if is_loop { "break" } else { "end" }));
            }
            (Stmt::Insn(_), _) => {
                // `loop` and `break` refer to the innermost `loop { }`, `else`
                // and `end` to the innermost `if { }`. Outside any label
                // block they are ordinary named labels.
                if let Some(label) = branch_target_mut(&mut stmt)
                    && matches!(label.as_str(), "loop" | "break" | "else" | "end")
                    && blocks.iter().any(|b| matches!(b, OpenBlock::Labels { .. }))
                {
                    let want_loop = matches!(label.as_str(), "loop" | "break");
                    let target = blocks.iter_mut().rev().find_map(|b| match b {
                        OpenBlock::Labels {
                            id,
                            is_loop,
                            uses_else,
                            ..
                        } if *is_loop == want_loop => Some((*id, uses_else)),
                        _ => None,
                    });
                    let Some((id, uses_else)) = target else {
                        let msg = match label.as_str() {
                            "end" => "goto end outside an if block; use goto break to leave a loop"
                                .to_string(),
                            "else" => "goto else outside an if block".to_string(),
                            _ => format!("goto {label} outside a loop block"),
                        };
                        return Err(msg.into()).at(&loc);
                    };
                    if label == "else" {
                        *uses_else = true;
                    }
                    *label = block_label(id, label);
                }
            }
            _ => (),
        }
        stmts.push(stmt);
        locs.push(loc);
    }
    Ok((stmts, locs))
}

fn is_branch(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Insn(Insn::B(_) | Insn::BCond(..) | Insn::Cbz(..) | Insn::Cbnz(..) | Insn::Bx(_)) => {
            true
        }
        Stmt::Insn(Insn::Pop(regs)) => regs.iter().any(|r| r.as_ident() == Some("pc")),
        _ => false,
    }
}

fn branch_target_mut(stmt: &mut Stmt) -> Option<&mut String> {
    match stmt {
        Stmt::Insn(
            Insn::B(label) | Insn::BCond(_, label) | Insn::Cbz(_, label) | Insn::Cbnz(_, label),
        ) => Some(label),
        _ => None,
    }
}

fn parse_label(toks: &mut TokBuf) -> Result<String, Error> {
    let dst = toks.next().ok_or("expected label")?;
    let TokBody::Identifier(label) = dst.tok.clone() else {
        return Err("branch target must be identifier")?;
    };
    Ok(label)
}

const CONDS: &[&str] = &[
    "eq", "ne", "cs", "hs", "cc", "lo", "mi", "pl", "vs", "vc", "hi", "ls", "ge", "lt", "gt", "le",
];

// Note: "if" has already been consumed, and it isn't `if {`
fn parse_if(toks: &mut TokBuf) -> Result<Stmt, Error> {
    if toks.expect_opt(&TokBody::Octothorpe) {
        let cond = toks.next().ok_or("unexpected eof in if stmt")?.clone();
        if toks.expect_opt(&TokBody::OpenBrace) {
            toks.expect(&TokBody::Newline)?;
            return Ok(Stmt::StartIf(cond));
        }
        let name = cond.as_ident().ok_or("expected condition after #")?;
        if !CONDS.contains(&name) {
            Err(format!("unknown condition #{name}"))?;
        }
        if !toks.peek().is_some_and(|t| t.match_str("goto")) {
            Err("expected { or goto after condition")?;
        }
        toks.next();
        let label = parse_label(toks)?;
        toks.expect(&TokBody::Newline)?;
        return Ok(Stmt::Insn(Insn::BCond(name.to_owned(), label)));
    }
    // `if x == 0 goto label` (cbz) and `if x != 0 goto label` (cbnz)
    let reg = parse_expr(toks)?;
    let op = toks.next().ok_or("unexpected eof in if stmt")?.tok.clone();
    let zero = toks.next().ok_or("unexpected eof in if stmt")?;
    if zero.tok != TokBody::Number(0) {
        Err("only comparison with 0 is supported (cbz/cbnz)")?;
    }
    if !toks.peek().is_some_and(|t| t.match_str("goto")) {
        Err("expected goto")?;
    }
    toks.next();
    let label = parse_label(toks)?;
    toks.expect(&TokBody::Newline)?;
    match op {
        TokBody::EqualsEquals => Ok(Stmt::Insn(Insn::Cbz(reg, label))),
        TokBody::ExclamationEquals => Ok(Stmt::Insn(Insn::Cbnz(reg, label))),
        _ => Err("expected == or != in if")?,
    }
}

fn parse_stmt(toks: &mut TokBuf) -> Result<Stmt, Error> {
    let first = toks.next().ok_or("unexpected eof in stmt")?.clone();
    match first.tok {
        TokBody::Identifier(ident) => {
            if toks.expect_opt(&TokBody::Colon) {
                toks.expect(&TokBody::Newline)?;
                return Ok(Stmt::Label(ident));
            }
            // A mnemonic followed by something that can only continue an
            // expression (`b = 1`, `bl.next`) is a variable, not an instruction.
            let continues_expr = toks.peek().is_some_and(|t| {
                t.tok.is_assign_op()
                    || matches!(t.tok, TokBody::At | TokBody::Period | TokBody::OpenBracket)
            });
            match ident.as_str() {
                _ if continues_expr => (),
                "bx" => {
                    let dst = parse_expr(toks)?;
                    toks.expect(&TokBody::Newline)?;
                    return Ok(Stmt::Insn(Insn::Bx(dst)));
                }
                "blx" => {
                    let dst = parse_expr(toks)?;
                    toks.expect(&TokBody::Newline)?;
                    return Ok(Stmt::Insn(Insn::Blx(dst)));
                }
                "bcc" | "bcs" | "beq" | "bge" | "bgt" | "bhi" | "bhs" | "ble" | "blo" | "bls"
                | "blt" | "bmi" | "bne" | "bpl" | "bvc" | "bvs" => {
                    let dst = toks.next().ok_or("expected label")?;
                    let TokBody::Identifier(label) = dst.tok.clone() else {
                        return Err("branch target must be identifier")?;
                    };
                    let cond = ident[1..].to_string();
                    toks.expect(&TokBody::Newline)?;
                    return Ok(Stmt::Insn(Insn::BCond(cond, label)));
                }
                "b" => {
                    let dst = toks.next().ok_or("expected label")?;
                    let TokBody::Identifier(label) = dst.tok.clone() else {
                        return Err("branch target must be identifier")?;
                    };
                    toks.expect(&TokBody::Newline)?;
                    return Ok(Stmt::Insn(Insn::B(label)));
                }
                "bl" => {
                    let dst = toks.next().ok_or("expected label")?;
                    let TokBody::Identifier(label) = dst.tok.clone() else {
                        return Err("branch target must be identifier")?;
                    };
                    toks.expect(&TokBody::Newline)?;
                    return Ok(Stmt::Insn(Insn::Bl(label)));
                }
                "cbz" | "cbnz" => {
                    let reg = parse_expr(toks)?;
                    toks.expect(&TokBody::Comma)?;
                    let dst = toks.next().ok_or("expected label")?;
                    let TokBody::Identifier(label) = dst.tok.clone() else {
                        return Err("branch target must be identifier")?;
                    };
                    toks.expect(&TokBody::Newline)?;
                    return Ok(match ident.as_str() {
                        "cbz" => Stmt::Insn(Insn::Cbz(reg, label)),
                        "cbnz" => Stmt::Insn(Insn::Cbnz(reg, label)),
                        _ => unreachable!(),
                    });
                }
                "wfe" | "wfi" | "sev" | "nop" | "dmb" | "dsb" | "isb" => {
                    toks.expect(&TokBody::Newline)
                        .map_err(|_| format!("{ident} takes no operands"))?;
                    return Ok(Stmt::Insn(Insn::NoOperand(ident)));
                }
                "if" => return parse_if(toks),
                "goto" => {
                    let label = parse_label(toks)?;
                    toks.expect(&TokBody::Newline)?;
                    return Ok(Stmt::Insn(Insn::B(label)));
                }
                _ => (),
            }
            toks.back_one();
            let lhs = parse_expr(toks)?;
            if let Some(op) = toks.peek() {
                if op.tok.is_assign_op() {
                    // TODO: validate that lhs is assignable
                    let op = toks.next().unwrap().clone();
                    let rhs = parse_expr(toks)?;
                    let stmt = Stmt::Assign(lhs, op.clone(), rhs);
                    addr_update_helper(toks, stmt)
                } else if let Expr::Call(name, args) = lhs {
                    // Instructions written as function calls, without a result
                    toks.expect(&TokBody::Newline)?;
                    match name.as_ident().unwrap() {
                        "push" => Ok(Stmt::Insn(Insn::Push(args))),
                        "pop" => Ok(Stmt::Insn(Insn::Pop(args))),
                        n @ ("cpsie" | "cpsid") => {
                            let [arg] = &args[..] else {
                                return Err(format!("{n} takes one argument, i or f"))?;
                            };
                            let flags = arg
                                .as_ident()
                                .filter(|f| matches!(*f, "i" | "f" | "if"))
                                .ok_or(format!("{n} argument must be i, f or if"))?;
                            Ok(Stmt::Insn(Insn::Cps(n == "cpsie", flags.to_owned())))
                        }
                        n => Err(format!("unknown instruction {n}"))?,
                    }
                } else {
                    Err("unhandled syntax")?
                }
            } else {
                Err("unexpected eof in stmt")?
            }
        }
        TokBody::Asterisk | TokBody::OpenParen => {
            toks.back_one();
            let lhs = parse_expr(toks)?;
            let op = toks.next().ok_or("eof in assignment")?.clone();
            // TODO: maybe validate
            let rhs = parse_expr(toks)?;
            let stmt = Stmt::Assign(lhs, op, rhs);
            addr_update_helper(toks, stmt)
        }
        TokBody::Octothorpe => parse_withflags(toks),
        TokBody::CloseBrace => {
            let tok = toks.next().ok_or("unexpected eof after close brace")?;
            match &tok.tok {
                TokBody::Newline => Ok(Stmt::EndBlock),
                TokBody::Identifier(id) => {
                    if id != "else" {
                        Err("unexpected identifier after close brace")?
                    }
                    toks.expect(&TokBody::OpenBrace)?;
                    toks.expect(&TokBody::Newline)?;
                    Ok(Stmt::Else)
                }
                _ => Err("unexpected token after close brace")?,
            }
        }
        _ => Err(format!("unhandled token to begin stmt {:?}", first.tok))?,
    }
}

fn addr_update_helper(toks: &mut TokBuf, stmt: Stmt) -> Result<Stmt, Error> {
    let tok = toks.next().ok_or("missing newline on stmt")?;
    match &tok.tok {
        TokBody::Newline => Ok(stmt),
        TokBody::Semicolon => {
            let lhs = parse_expr(toks)?;
            let assign_op = toks.next().ok_or("eof in addr update")?;
            let sign = match assign_op.tok {
                TokBody::PlusEquals => 1,
                TokBody::MinusEquals => -1,
                _ => Err("addr update must be += or -=")?,
            };
            let rhs = parse_expr(toks)?;
            toks.expect(&TokBody::Newline)?;
            Ok(Stmt::WithAddrUpdate(stmt.into(), lhs, sign, rhs))
        }
        _ => Err("unexpected token after stmt")?,
    }
}

fn parse_expr(toks: &mut TokBuf) -> Result<Expr, Error> {
    parse_expr_rec(toks, Precedence::Loosest)
}

fn parse_expr_rec(toks: &mut TokBuf, precedence: Precedence) -> Result<Expr, Error> {
    let mut lhs = parse_expr_unary(toks)?;
    while let Some(op) = toks.next() {
        if let Some(p) = Precedence::of(&op.tok) {
            // Strict comparison makes same-precedence ops left associative.
            if p < precedence {
                let op = op.clone();
                let rhs = parse_expr_rec(toks, p)?;
                lhs = Expr::Binop(Box::new(lhs), op, Box::new(rhs));
            } else {
                toks.back_one();
                break;
            }
        } else if op.as_ident() == Some("as") && Precedence::Cast <= precedence {
            let ty = parse_type(toks)?;
            lhs = Expr::Cast(Box::new(lhs), ty);
        } else {
            toks.back_one();
            break;
        }
    }
    Ok(lhs)
}

fn parse_expr_unary(toks: &mut TokBuf) -> Result<Expr, Error> {
    let tok = toks.peek().ok_or("unexpected eof in expr")?;
    match tok.tok {
        TokBody::Asterisk | TokBody::Minus | TokBody::Exclamation | TokBody::Ampersand => {
            let first = toks.next().unwrap().clone();
            let expr = parse_expr_unary(toks)?;
            Ok(Expr::Unary(first, expr.into()))
        }
        _ => parse_trailer_expr(toks),
    }
}

fn parse_trailer_expr(toks: &mut TokBuf) -> Result<Expr, Error> {
    let first = toks.next().ok_or("unexpected eof in expr")?;
    let mut expr = match first.tok {
        TokBody::OpenParen => {
            let expr = parse_expr(toks)?;
            let next = toks.next().ok_or("unexpected eof inside parens")?;
            match &next.tok {
                TokBody::CloseParen => expr,
                TokBody::Comma => {
                    let mut exprs = vec![expr];
                    // allows a trailing comma
                    while !toks.expect_opt(&TokBody::CloseParen) {
                        exprs.push(parse_expr(toks)?);
                        let sep = toks.next().ok_or("unexpected eof inside parens")?;
                        match sep.tok {
                            TokBody::Comma => (),
                            TokBody::CloseParen => break,
                            _ => Err("expected comma or close paren in tuple")?,
                        }
                    }
                    Expr::Tuple(exprs)
                }
                _ => Err("unknown separator in parens")?,
            }
        }
        TokBody::Identifier(_) => {
            let name = first.clone();
            if toks.expect_opt(&TokBody::OpenParen) {
                Expr::Call(name, parse_call_args(toks)?)
            } else {
                Expr::Ident(name)
            }
        }
        TokBody::Number(_) => Expr::Literal(first.clone()),
        _ => Err("unknown token for expr")?,
    };
    while let Some(first) = toks.peek() {
        match &first.tok {
            TokBody::PlusPlus | TokBody::MinusMinus => {
                let sign = if first.tok == TokBody::PlusPlus {
                    1
                } else {
                    -1
                };
                toks.next().unwrap();
                expr = Expr::PostInc(expr.into(), sign);
            }
            TokBody::Period => {
                toks.next().unwrap();
                let field = toks.next().ok_or("expected field name")?;
                if !field.is_ident() {
                    Err("field must be identifier")?
                }
                expr = Expr::Field(expr.into(), field.clone());
            }
            TokBody::OpenBracket => {
                toks.next().unwrap();
                let start = toks.next().ok_or("unexpected eof in slice")?;
                let &TokBody::Number(start) = &start.tok else {
                    return Err("slice start must be number")?;
                };
                toks.expect(&TokBody::DotDot)?;
                let end = toks.next().ok_or("unexpected eof in slice")?;
                let &TokBody::Number(end) = &end.tok else {
                    return Err("slice end must be number")?;
                };
                toks.expect(&TokBody::CloseBracket)?;
                expr = Expr::Slice(expr.into(), start as usize, end as usize);
            }
            _ => break,
        }
    }
    Ok(expr)
}

// Note: open paren has already been consumed
fn parse_call_args(toks: &mut TokBuf) -> Result<Vec<Expr>, Error> {
    let mut args = vec![];
    while !toks.expect_opt(&TokBody::CloseParen) {
        args.push(parse_expr(toks)?);
        let sep = toks.next().ok_or("unexpected eof in call")?;
        match sep.tok {
            TokBody::Comma => (),
            TokBody::CloseParen => break,
            _ => Err("expected comma or close paren in call")?,
        }
    }
    Ok(args)
}

// Note: "static" keyword has already been consumed
fn parse_static(toks: &mut TokBuf) -> Result<StaticDecl, Error> {
    let mutable = toks.expect_opt_ident("mut");
    let name = toks.next().ok_or("expected static name")?.clone();
    if !name.is_ident() {
        Err("static name must be identifier")?
    }
    toks.expect(&TokBody::Colon)?;
    let ty = parse_type(toks)?;
    let init = if toks.expect_opt(&TokBody::Equals) {
        Some(parse_init(toks)?)
    } else if !mutable {
        Err("an immutable static needs an initializer")?
    } else {
        None
    };
    toks.expect(&TokBody::Newline)?;
    Ok(StaticDecl {
        name,
        mutable,
        ty,
        init,
        link_section: None,
        align: None,
    })
}

/// An initializer: an expression, `[a, b, c]` or `[x; n]`. Inside the
/// brackets newlines are ignored and a trailing comma is allowed.
fn parse_init(toks: &mut TokBuf) -> Result<Init, Error> {
    if !toks.expect_opt(&TokBody::OpenBracket) {
        return Ok(Init::Expr(parse_expr(toks)?));
    }
    toks.eat_newlines();
    let mut items = vec![];
    if toks.expect_opt(&TokBody::CloseBracket) {
        return Ok(Init::List(items));
    }
    let first = parse_init(toks)?;
    toks.eat_newlines();
    if toks.expect_opt(&TokBody::Semicolon) {
        toks.eat_newlines();
        let count = parse_expr(toks)?;
        toks.eat_newlines();
        toks.expect(&TokBody::CloseBracket)?;
        return Ok(Init::Repeat(first.into(), count));
    }
    items.push(first);
    loop {
        if toks.expect_opt(&TokBody::CloseBracket) {
            break;
        }
        toks.expect(&TokBody::Comma)?;
        toks.eat_newlines();
        if toks.expect_opt(&TokBody::CloseBracket) {
            break;
        }
        items.push(parse_init(toks)?);
        toks.eat_newlines();
    }
    Ok(Init::List(items))
}

// Note: "const" keyword has already been consumed
fn parse_const(toks: &mut TokBuf) -> Result<ConstDecl, Error> {
    let name = toks.next().ok_or("expected const name")?.clone();
    if !name.is_ident() {
        Err("const name must be identifier")?
    }
    if toks.peek().is_some_and(|t| t.tok == TokBody::Colon) {
        Err("consts are untyped: `const name = value`")?
    }
    toks.expect(&TokBody::Equals)?;
    let value = parse_expr(toks)?;
    toks.expect(&TokBody::Newline)?;
    Ok(ConstDecl { name, value })
}

// Note: "symbol" keyword has already been consumed
fn parse_symbol(toks: &mut TokBuf) -> Result<SymbolDecl, Error> {
    let name = toks.next().ok_or("expected symbol name")?.clone();
    if !name.is_ident() {
        Err("symbol name must be identifier")?
    }
    toks.expect(&TokBody::Colon)?;
    let ty = parse_type(toks)?;
    toks.expect(&TokBody::Newline)?;
    Ok(SymbolDecl { name, ty })
}

fn parse_withflags(toks: &mut TokBuf) -> Result<Stmt, Error> {
    toks.expect(&TokBody::OpenParen)?;
    let lhs = parse_expr(toks)?;
    let op = toks.next().ok_or("unexpected eof in #()")?;
    if op.tok.is_assign_op() {
        let op = op.clone();
        let rhs = parse_expr(toks)?;
        toks.expect(&TokBody::CloseParen)?;
        toks.expect(&TokBody::Newline)?;
        return Ok(Stmt::WithFlagsAssign(lhs, op.clone(), rhs));
    } else if op.tok == TokBody::CloseParen {
        toks.expect(&TokBody::Newline)?;
        return Ok(Stmt::WithFlagsExpr(lhs));
    }
    Err("expected assignment or close paren in #()")?
}

fn parse_field(toks: &mut TokBuf, field_name: &Token) -> Result<Type, Error> {
    if !field_name.is_ident() {
        Err("expected field name to be identifier")?;
    }
    toks.expect(&TokBody::Colon)?;
    let ty = parse_type(toks)?;
    toks.expect(&TokBody::Newline)?;
    Ok(ty)
}

// Note: "struct" keyword has already been consumed
fn parse_struct(toks: &mut TokBuf) -> Result<Struct, Error> {
    let name = toks.next().ok_or("expected struct name")?.clone();
    if !name.is_ident() {
        Err("struct name must be identifier")?
    }
    toks.expect(&TokBody::OpenBrace)?;
    toks.expect(&TokBody::Newline)?;
    let mut fields = vec![];
    loop {
        toks.eat_newlines();
        if toks.expect_opt(&TokBody::CloseBrace) {
            break;
        }
        let field_name = toks.next().ok_or("unexpected eof in struct")?.clone();
        let loc = field_name.loc.clone();
        let ty = parse_field(toks, &field_name).at(&loc)?;
        fields.push(Field {
            name: field_name,
            ty,
        });
    }
    Ok(Struct { name, fields })
}

impl Expr {
    pub fn as_ident(&self) -> Option<&str> {
        if let Expr::Ident(tok) = self {
            tok.as_ident()
        } else {
            None
        }
    }
}
