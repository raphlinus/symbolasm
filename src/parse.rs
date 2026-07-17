//! Parsing

use crate::{
    lex::{Error, TokBody, TokBuf, Token},
    precedence::Precedence,
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
pub struct Args;

#[derive(Debug)]
pub enum Stmt {
    Label(String),
    /// Includes assignment ops as well as `Equals`
    Assign(Expr, Token, Expr),
    /// var @ reg = expr
    AssignPlace(Expr, Token, Expr),
    WithFlagsExpr(Expr),
    WithFlagsAssign(Expr, Token, Expr),
    WithFlagsAssignPlace(Expr, Token, Expr),
    Insn(Insn),
}

#[derive(Debug)]
pub enum Insn {
    Bx(Expr),
    BCond(String, String),
    B(String),
    Bl(String),
}

#[derive(Debug)]
pub enum Expr {
    Ident(Token),
    Binop(Box<Expr>, Token, Box<Expr>),
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
    toks.expect(&TokBody::CloseParen)?;
    Ok(Args)
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
    if let TokBody::Idenfifier(ident) = &first.tok {
        if toks.expect_opt(&TokBody::Colon) {
            toks.expect(&TokBody::Newline)?;
            return Ok(Stmt::Label(ident.clone()));
        }
        match ident.as_str() {
            "bx" => {
                let dst = parse_expr(toks)?;
                toks.expect(&TokBody::Newline)?;
                return Ok(Stmt::Insn(Insn::Bx(dst)));
            }
            // TODO: the rest of the conditions
            "bne" | "beq" | "bcc" | "bcs" | "bpl" | "bmi" => {
                let dst = toks.next().ok_or("expected label")?;
                let TokBody::Idenfifier(label) = &dst.tok else {
                    return Err("branch target must be identifier")?;
                };
                let cond = ident[1..].to_string();
                return Ok(Stmt::Insn(Insn::BCond(cond, label.clone())));
            }
            "b" => {
                let dst = toks.next().ok_or("expected label")?;
                let TokBody::Idenfifier(label) = &dst.tok else {
                    return Err("branch target must be identifier")?;
                };
                return Ok(Stmt::Insn(Insn::B(label.clone())));
            }
            "bl" => {
                let dst = toks.next().ok_or("expected label")?;
                let TokBody::Idenfifier(label) = &dst.tok else {
                    return Err("branch target must be identifier")?;
                };
                return Ok(Stmt::Insn(Insn::Bl(label.clone())));
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
    if first.tok == TokBody::OpenParen {
        let expr = parse_expr(toks)?;
        // TODO: handle comma for tuple formation
        toks.expect(&TokBody::CloseParen)?;
        return Ok(expr);
    }
    Ok(Expr::Ident(first.clone()))
}

fn parse_withflags(toks: &mut TokBuf) -> Result<Stmt, Error> {
    toks.expect(&TokBody::OpenParen)?;
    let lhs = parse_expr(toks)?;
    let op = toks.next().ok_or("unexpected eof in #()")?;
    if op.tok.is_assign_op() {
        let op = toks.next().unwrap().clone();
        let rhs = parse_expr(toks)?;
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
