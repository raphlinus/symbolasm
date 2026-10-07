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

#[derive(Debug)]
pub enum Type {
    Ident(Token),
    Ptr(Box<Type>),
}

#[derive(Debug)]
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

pub fn parse_program(toks: &mut TokBuf) -> Result<Program, Error> {
    let mut items = vec![];
    while let Some(tok) = toks.next() {
        if tok.tok == TokBody::Newline {
            continue;
        }
        let loc = tok.loc.clone();
        if tok.match_str("fn") {
            let f = parse_function(toks).at(&loc)?;
            items.push(Item::Function(f));
        } else if tok.match_str("struct") {
            let s = parse_struct(toks).at(&loc)?;
            items.push(Item::Struct(s));
        } else if tok.match_str("symbol") {
            let e = parse_symbol(toks).at(&loc)?;
            items.push(Item::Symbol(e));
        } else {
            return Err("expected fn, struct or symbol".into()).at(&loc);
        }
    }
    Ok(Program(items))
}

// Note: "fn" keyword has already been consumed
fn parse_function(toks: &mut TokBuf) -> Result<Function, Error> {
    let name = toks.next().ok_or("expected function name")?;
    if !name.is_ident() {
        Err("function name must be identifier")?
    }
    let name = name.clone();
    let args = parse_args(toks).at(&name.loc)?;
    let (body, locs) = parse_body(toks)?;
    Ok(Function {
        name,
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
        _ => Err("unknown type")?,
    }
}

fn parse_body(toks: &mut TokBuf) -> Result<(Vec<Stmt>, Vec<Loc>), Error> {
    let mut stmts = vec![];
    let mut locs = vec![];
    toks.expect(&TokBody::OpenBrace)?;
    toks.expect(&TokBody::Newline)?;
    let mut depth = 0;
    loop {
        toks.eat_newlines();
        if depth == 0 && toks.expect_opt(&TokBody::CloseBrace) {
            break;
        }
        let loc = toks
            .peek()
            .ok_or("unexpected eof in function body")?
            .loc
            .clone();
        stmts.push(parse_stmt(toks, &mut depth).at(&loc)?);
        locs.push(loc);
    }
    Ok((stmts, locs))
}

fn parse_stmt(toks: &mut TokBuf, depth: &mut usize) -> Result<Stmt, Error> {
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
                "if" => {
                    toks.expect(&TokBody::Octothorpe)?;
                    let cond = toks.next().ok_or("unexpected eof in if stmt")?.clone();
                    toks.expect(&TokBody::OpenBrace)?;
                    toks.expect(&TokBody::Newline)?;
                    *depth += 1;
                    return Ok(Stmt::StartIf(cond));
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
                TokBody::Newline => {
                    *depth -= 1;
                    Ok(Stmt::EndBlock)
                }
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
        TokBody::Asterisk | TokBody::Minus | TokBody::Exclamation => {
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
