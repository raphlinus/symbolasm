// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Evaluation of constant expressions.
//!
//! Arithmetic is 32 bit wrapping unsigned, like the machine.

use std::collections::HashMap;

use crate::{
    error::Error,
    ir::BinOp,
    lex::TokBody,
    parse::Expr,
    types::{Type, TypeHandle, TypePool},
};

/// Evaluate a constant expression: literals, consts, arithmetic, and the
/// intrinsics `size`, `count`, `lower16` and `upper16`.
///
/// `statics` are the statics `size` and `count` may refer to.
pub fn eval(
    expr: &Expr,
    types: &mut TypePool,
    statics: &HashMap<String, TypeHandle>,
) -> Result<u32, Error> {
    match expr {
        Expr::Literal(lit) => {
            let TokBody::Number(n) = lit.tok else {
                return Err("only integers are constant expressions, not f32s")?;
            };
            u32::try_from(n).map_err(|_| format!("{n} doesn't fit in 32 bits").into())
        }
        Expr::Ident(id) => {
            let id = id.as_ident().unwrap();
            types
                .const_value(id)
                .ok_or_else(|| format!("{id} is not a constant").into())
        }
        Expr::Binop(a, op, b) => {
            let op = BinOp::from_tok(&op.tok).ok_or("not a constant expression")?;
            let a = eval(a, types, statics)?;
            let b = eval(b, types, statics)?;
            fold_binop(a, op, b)
        }
        Expr::Unary(op, a) => {
            let a = eval(a, types, statics)?;
            match op.tok {
                TokBody::Minus => Ok(a.wrapping_neg()),
                TokBody::Exclamation => Ok(!a),
                _ => Err("not a constant expression")?,
            }
        }
        Expr::Cast(a, _) => eval(a, types, statics),
        Expr::Call(name, args) => {
            let name = name.as_ident().unwrap();
            let [arg] = &args[..] else {
                return Err(format!("{name} takes one argument"))?;
            };
            match name {
                "size" | "count" => size_or_count(name, arg, types, statics),
                "lower16" => Ok(eval(arg, types, statics)? & 0xffff),
                "upper16" => Ok(eval(arg, types, statics)? >> 16),
                _ => Err(format!("unknown intrinsic {name}"))?,
            }
        }
        _ => Err("not a constant expression")?,
    }
}

/// `size(x)` is the size in bytes of a static or a named type, and
/// `count(x)` the number of elements of one that is an array.
fn size_or_count(
    name: &str,
    arg: &Expr,
    types: &mut TypePool,
    statics: &HashMap<String, TypeHandle>,
) -> Result<u32, Error> {
    let Some(id) = arg.as_ident() else {
        return Err(format!("{name} takes the name of a static or a type"))?;
    };
    let ty = match statics.get(id) {
        Some(ty) => *ty,
        None => types
            .named_type(id)
            .ok_or_else(|| format!("{id} is not a static or a type"))?,
    };
    if name == "count" {
        let &Type::Array(_, n) = types.get(ty) else {
            return Err(format!("count({id}): {id} is not an array"))?;
        };
        return Ok(n);
    }
    let info = types.info(ty);
    if info.align() == 0 {
        return Err(format!("size of {id} isn't known here; define it first"))?;
    }
    Ok(info.size() as u32)
}

/// Constant folding of an operation on two literals, with 32 bit wrapping
/// unsigned semantics.
pub fn fold_binop(a: u32, op: BinOp, b: u32) -> Result<u32, Error> {
    Ok(match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::Mul => a.wrapping_mul(b),
        BinOp::Div => a.checked_div(b).ok_or("division by zero in constant")?,
        BinOp::And => a & b,
        BinOp::Orr => a | b,
        BinOp::Eor => a ^ b,
        BinOp::Shl => a.checked_shl(b).ok_or("shift out of range in constant")?,
        BinOp::Shr => a.checked_shr(b).ok_or("shift out of range in constant")?,
    })
}
