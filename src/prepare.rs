// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Rewrites of a function body before analysis
//!
//! A function's results are placed in their registers at each assignment,
//! so `q = x + 1` for a result q in r0 becomes `q @r0 = x + 1`. The amount
//! in `sp -= n` is folded to a number.

use crate::{
    error::{Error, WithLoc},
    lex::{TokBody, Token},
    parse::{Expr, Function},
    regmap::reg_name,
    stmt::Stmt,
    types::TypePool,
};

pub fn prepare(func: &mut Function, types: &mut TypePool) -> Result<(), Error> {
    let mut floats = vec![];
    for arg in &func.rets.0 {
        let float = match &arg.ty {
            Some(ty) => {
                let ty = types.intern_from_ast(ty).at(&arg.var.loc)?;
                types.get(ty).is_float()
            }
            None => false,
        };
        floats.push(float);
    }
    let rets: Vec<(String, u8)> = func
        .rets
        .0
        .iter()
        .zip(func.abi.ret_regs(&floats))
        .filter_map(|(arg, reg)| Some((arg.var.as_ident()?.to_owned(), reg)))
        .collect();
    for (stmt, loc) in func.body.iter_mut().zip(&func.locs) {
        place_rets(stmt, &rets).at(loc)?;
        if let Stmt::SpAdjust(amount, places) = stmt {
            let (negative, expr) = match amount {
                Expr::Unary(op, inner) if op.tok == TokBody::MinusEquals => (true, &**inner),
                _ => (false, &*amount),
            };
            let n = crate::consts::eval(expr, types, &Default::default()).at(loc)?;
            if n % 4 != 0 || n > 1 << 20 {
                return Err(format!("sp must move by a multiple of 4, not {n}").into()).at(loc);
            }
            if !places.is_empty() {
                let mut tys = vec![];
                for (_, ty) in places.iter() {
                    tys.push(match ty {
                        Some(ty) => types.intern_from_ast(ty).at(loc)?,
                        None => types.get_handle(&crate::types::Type::U32),
                    });
                }
                let tuple = types.intern_tuple(&tys);
                let size = types.info(tuple).size().next_multiple_of(4) as u32;
                if size != n {
                    return Err(format!(
                        "the variables placed take {size} bytes, but sp moves by {n}"
                    )
                    .into())
                    .at(loc);
                }
            }
            let n = if negative { -(n as i64) } else { n as i64 };
            let tok = Token {
                loc: loc.clone(),
                tok: TokBody::Number(n),
            };
            *amount = Expr::Literal(tok);
        }
    }
    Ok(())
}

/// Place each result assigned in `stmt` in its register
fn place_rets(stmt: &mut Stmt, rets: &[(String, u8)]) -> Result<(), Error> {
    match stmt {
        Stmt::Assign(lhs, op, _) | Stmt::WithFlagsAssign(lhs, op, _) => {
            if op.tok == TokBody::Equals {
                place_lhs(lhs, rets)?;
            }
        }
        Stmt::WithAddrUpdate(inner, _, _, _) => place_rets(inner, rets)?,
        Stmt::Call(call) => {
            for ret in &mut call.rets {
                place_lhs(ret, rets)?;
            }
        }
        _ => (),
    }
    Ok(())
}

fn place_lhs(lhs: &mut Expr, rets: &[(String, u8)]) -> Result<(), Error> {
    match lhs {
        Expr::Ident(tok) => {
            if let Some(id) = tok.as_ident()
                && let Some((_, reg)) = rets.iter().find(|(r, _)| r == id)
            {
                let at = Token {
                    loc: tok.loc.clone(),
                    tok: TokBody::At,
                };
                let place = Token {
                    loc: tok.loc.clone(),
                    tok: TokBody::Identifier(reg_name(*reg)),
                };
                let var = Expr::Ident(tok.clone());
                *lhs = Expr::Binop(Box::new(var), at, Box::new(Expr::Ident(place)));
            }
        }
        Expr::Binop(var, op, place) if op.tok == TokBody::At => {
            if let Some(id) = var.as_ident()
                && let Some((_, reg)) = rets.iter().find(|(r, _)| r == id)
                && place.as_ident() != Some(reg_name(*reg).as_str())
            {
                Err(format!(
                    "{id} is a result, so it's always placed in {}",
                    reg_name(*reg)
                ))?;
            }
        }
        Expr::Tuple(exps) => {
            for e in exps {
                place_lhs(e, rets)?;
            }
        }
        _ => (),
    }
    Ok(())
}
