//! Parsing

use crate::{lex::{Error, TokBody, TokBuf, Token}, precedence::{self, Precedence}};

#[derive(Debug)]
pub struct Program(Vec<Item>);

#[derive(Debug)]
pub enum Item {
    Function(Function),
}

#[derive(Debug)]
pub struct Function {
    name: Token,
    args: Args,
    body: Vec<Stmt>,
}

#[derive(Debug)]
pub struct Args;

#[derive(Debug)]
pub enum Stmt {
    Label(String),
    Assign(Expr, Expr),
    Insn(Insn),
}

#[derive(Debug)]
pub enum Insn {
    Bx(Expr),
    Assign(Expr, Expr),
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
                return Err("function name must be identifier")?;
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
            _ => (),
        }
        toks.back_one();
        let lhs = parse_expr(toks)?;
        if toks.expect_opt(&TokBody::Equals) {
            // TODO: validate that lhs is assignable
            let rhs = parse_expr(toks)?;
            toks.expect(&TokBody::Newline)?;
            return Ok(Stmt::Assign(lhs, rhs));
        }
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
    return Ok(lhs);
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
