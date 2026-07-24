//! Parsing

use crate::{
    lex::{Error, TokBody, TokBuf, Token},
    precedence::Precedence,
    stmt::{Insn, Stmt},
};

#[derive(Debug)]
pub struct Program(pub Vec<Item>);

#[derive(Debug)]
pub enum Item {
    Function(Function),
}

#[derive(Debug)]
pub struct Function {
    pub name: Token,
    pub args: Args,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub struct Args(pub Vec<Arg>);

#[derive(Debug)]
pub struct Arg {
    pub var: Token,
    // TODO: optional type
}

#[derive(Debug)]
pub enum Expr {
    Ident(Token),
    Literal(Token),
    Binop(Box<Expr>, Token, Box<Expr>),
    Unary(Token, Box<Expr>),
}

pub fn parse_program(toks: &mut TokBuf) -> Result<Program, Error> {
    let mut items = vec![];
    while let Some(tok) = toks.peek() {
        if tok.tok == TokBody::Newline {
            toks.next();
            continue;
        }
        if tok.match_str("fn") {
            let _fn_tok = toks.next().unwrap();
            let name = toks.next().ok_or("expected function name")?;
            if !matches!(name.tok, TokBody::Idenfifier(_)) {
                Err("function name must be identifier")?
            }
            let name = name.clone();
            let args = parse_args(toks)?;
            let body = parse_body(toks)?;
            let f = Function { name, args, body };
            items.push(Item::Function(f));
        } else {
            todo!("unexpected token {tok:?}");
        }
    }
    Ok(Program(items))
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
    if matches!(tok.tok, TokBody::Idenfifier(_)) {
        Ok(Arg { var: tok.clone() })
    } else {
        Err("expected arg to be identifier")?
    }
}

fn parse_body(toks: &mut TokBuf) -> Result<Vec<Stmt>, Error> {
    let mut stmts = vec![];
    toks.expect(&TokBody::OpenBrace)?;
    toks.expect(&TokBody::Newline)?;
    loop {
        toks.eat_newlines();
        if toks.expect_opt(&TokBody::CloseBrace) {
            break;
        }
        stmts.push(parse_stmt(toks)?);
    }
    Ok(stmts)
}

fn parse_stmt(toks: &mut TokBuf) -> Result<Stmt, Error> {
    let first = toks.next().ok_or("unexpected eof in stmt")?.clone();
    if let TokBody::Idenfifier(ident) = first.tok {
        if toks.expect_opt(&TokBody::Colon) {
            toks.expect(&TokBody::Newline)?;
            return Ok(Stmt::Label(ident));
        }
        // TODO: a bit more backtracking, among other things this precludes "b" as variable name
        match ident.as_str() {
            "bx" => {
                let dst = parse_expr(toks)?;
                toks.expect(&TokBody::Newline)?;
                return Ok(Stmt::Insn(Insn::Bx(dst)));
            }
            "bcc" | "bcs" | "beq" | "bge" | "bgt" | "bhi" | "bhs" | "ble" | "blo" | "bls"
            | "blt" | "bmi" | "bne" | "bpl" | "bvc" | "bvs" => {
                let dst = toks.next().ok_or("expected label")?;
                let TokBody::Idenfifier(label) = dst.tok.clone() else {
                    return Err("branch target must be identifier")?;
                };
                let cond = ident[1..].to_string();
                toks.expect(&TokBody::Newline)?;
                return Ok(Stmt::Insn(Insn::BCond(cond, label)));
            }
            "b" => {
                let dst = toks.next().ok_or("expected label")?;
                let TokBody::Idenfifier(label) = dst.tok.clone() else {
                    return Err("branch target must be identifier")?;
                };
                toks.expect(&TokBody::Newline)?;
                return Ok(Stmt::Insn(Insn::B(label)));
            }
            "bl" => {
                let dst = toks.next().ok_or("expected label")?;
                let TokBody::Idenfifier(label) = dst.tok.clone() else {
                    return Err("branch target must be identifier")?;
                };
                toks.expect(&TokBody::Newline)?;
                return Ok(Stmt::Insn(Insn::Bl(label)));
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
                toks.expect(&TokBody::Newline)?;
                return Ok(Stmt::Assign(lhs, op.clone(), rhs));
            } else if op.tok == TokBody::At {
                _ = toks.next();
                let reg = toks.next().ok_or("expected reg")?.clone();
                // TODO: ensure reg is valid register
                toks.expect(&TokBody::Equals)?;
                let rhs = parse_expr(toks)?;
                toks.expect(&TokBody::Newline)?;
                return Ok(Stmt::AssignPlace(lhs, reg, rhs));
            }
        }
    } else if first.tok == TokBody::Asterisk {
        toks.back_one();
        let lhs = parse_expr(toks)?;
        let op = toks.next().ok_or("eof in assignment")?.clone();
        // TODO: maybe validate
        let rhs = parse_expr(toks)?;
        return Ok(Stmt::Assign(lhs, op, rhs));
    } else if first.tok == TokBody::Octothorpe {
        return parse_withflags(toks);
    }
    todo!()
}

fn parse_expr(toks: &mut TokBuf) -> Result<Expr, Error> {
    parse_expr_rec(toks, Precedence::Loosest)
}

fn parse_expr_rec(toks: &mut TokBuf, precedence: Precedence) -> Result<Expr, Error> {
    let mut lhs = parse_expr_unary(toks)?;
    while let Some(op) = toks.next() {
        if let Some(p) = Precedence::of(&op.tok) {
            if p <= precedence {
                let op = op.clone();
                let rhs = parse_expr_rec(toks, p)?;
                lhs = Expr::Binop(Box::new(lhs), op, Box::new(rhs));
            } else {
                toks.back_one();
                break;
            }
        } else {
            toks.back_one();
            break;
        }
    }
    Ok(lhs)
}

fn parse_expr_unary(toks: &mut TokBuf) -> Result<Expr, Error> {
    let first = toks.next().ok_or("unexpected eof in expr")?;
    match first.tok {
        TokBody::OpenParen => {
            let expr = parse_expr(toks)?;
            // TODO: handle comma for tuple formation
            toks.expect(&TokBody::CloseParen)?;
            Ok(expr)
        }
        TokBody::Idenfifier(_) => Ok(Expr::Ident(first.clone())),
        TokBody::Number(_) => Ok(Expr::Literal(first.clone())),
        TokBody::Asterisk => {
            let first = first.clone();
            let expr = parse_expr(toks)?;
            Ok(Expr::Unary(first, expr.into()))
        }
        _ => Err("unknown token for expr")?,
    }
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
    } else if op.tok == TokBody::At {
        let reg = toks.next().ok_or("expected reg")?.clone();
        // TODO: ensure reg is valid register
        toks.expect(&TokBody::Equals)?;
        let rhs = parse_expr(toks)?;
        toks.expect(&TokBody::CloseParen)?;
        toks.expect(&TokBody::Newline)?;
        return Ok(Stmt::WithFlagsAssignPlace(lhs, reg, rhs));
    } else if op.tok == TokBody::CloseParen {
        toks.expect(&TokBody::Newline)?;
        return Ok(Stmt::WithFlagsExpr(lhs));
    }
    todo!()
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
