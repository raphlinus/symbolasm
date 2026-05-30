//! Parsing

use crate::lex::{Error, TokBody, TokBuf, Token};

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
    Insn(Insn),
}

#[derive(Debug)]
pub enum Insn {
    Bx(Expr),
}

#[derive(Debug)]
pub enum Expr {
    Ident(Token),
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
                return Err("function name must be identifier".into());
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
    todo!()
}
