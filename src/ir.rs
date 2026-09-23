// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::lex::Token;
use crate::parse;
use crate::regmap::parse_register;
use crate::svd::Peripherals;
use crate::typeinf::TypeMap;
use crate::{
    lex::{Error, TokBody},
    regmap::Regmap,
    stmt::Stmt,
    types::{Type, TypeHandle, TypePool},
};

/// Intermediate representation of a single instruction.
///
/// Each node is assigned a type. However, variables are resolved.
#[derive(Debug)]
pub enum Ir {
    Assign(Assign),
    WithFlags(Expr),
    WithAddrUpdate(Box<Ir>, i32),
}

#[derive(Debug)]
pub struct Assign {
    pub with_flags: bool,
    pub lhs: Expr,
    pub rhs: Expr,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub ty: TypeHandle,
    pub body: Body,
}

#[derive(Clone, Debug)]
pub enum Body {
    Reg(u8),
    Imm(u32),
    Binop(Box<Expr>, BinOp, Box<Expr>),
    Unary(UnaryOp, Box<Expr>),
    // a legit question is whether this should be a separate expr
    // or whether it should be *(base + offset)
    Field(Box<Expr>, usize),
    Slice(Box<Expr>, usize, usize),
    Tuple(Vec<Expr>),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    And,
    Orr,
    Eor,
    Shl,
    Shr,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum UnaryOp {
    Deref,
    Ref,
    Neg,
    Not,
}

pub struct IrCtx<'a> {
    regmap: &'a Regmap,
    types: &'a mut TypePool,
    typemap: &'a TypeMap,
    // This is peripherals for now, but will grow to data in global scope.
    peripherals: Option<&'a Peripherals>,
}

impl BinOp {
    fn from_tok(tok: &TokBody) -> Option<Self> {
        Some(match tok {
            TokBody::Plus => BinOp::Add,
            TokBody::Asterisk => BinOp::Mul,
            TokBody::Minus => BinOp::Sub,
            TokBody::Slash => BinOp::Div,
            TokBody::Ampersand => BinOp::And,
            TokBody::Pipe => BinOp::Orr,
            TokBody::Caret => BinOp::Eor,
            TokBody::LessLess => BinOp::Shl,
            TokBody::GreaterGreater => BinOp::Shr,
            _ => return None,
        })
    }

    fn from_assign_tok(tok: &TokBody) -> Option<Self> {
        Some(match tok {
            TokBody::PlusEquals => BinOp::Add,
            TokBody::AsteriskEquals => BinOp::Mul,
            TokBody::MinusEquals => BinOp::Sub,
            _ => return None,
        })
    }
}

impl UnaryOp {
    fn from_tok(tok: &TokBody) -> Option<Self> {
        Some(match tok {
            TokBody::Minus => UnaryOp::Neg,
            TokBody::Exclamation => UnaryOp::Not,
            TokBody::Asterisk => UnaryOp::Deref,
            _ => return None,
        })
    }
}

impl<'a> IrCtx<'a> {
    pub fn new(
        regmap: &'a Regmap,
        types: &'a mut TypePool,
        typemap: &'a TypeMap,
        peripherals: Option<&'a Peripherals>,
    ) -> Self {
        Self {
            regmap,
            types,
            typemap,
            peripherals,
        }
    }

    pub fn can_lower(stmt: &Stmt) -> bool {
        matches!(
            stmt,
            Stmt::Assign(_, _, _)
                | Stmt::WithFlagsAssign(_, _, _)
                | Stmt::WithFlagsExpr(_)
                | Stmt::WithAddrUpdate(_, _, _, _)
        )
    }

    pub fn lower(&mut self, stmt: &Stmt) -> Result<Ir, Error> {
        match stmt {
            Stmt::Assign(lhs, op, rhs) => self.lower_assign(lhs, &op.tok, rhs, false),
            Stmt::WithFlagsExpr(expr) => {
                let expr = self.lower_expr(expr)?;
                Ok(Ir::WithFlags(expr))
            }
            Stmt::WithFlagsAssign(lhs, op, rhs) => self.lower_assign(lhs, &op.tok, rhs, true),
            Stmt::WithAddrUpdate(stmt, lhs, sign, incr) => {
                self.lower_addr_update(stmt, lhs, *sign, incr)
            }
            _ => todo!("either we shouldn't try to lower, or we need to impl"),
        }
    }

    fn lower_assign(
        &mut self,
        lhs: &parse::Expr,
        op: &TokBody,
        rhs: &parse::Expr,
        with_flags: bool,
    ) -> Result<Ir, Error> {
        let lhs = self.lower_expr(lhs)?;
        let rhs = self.lower_expr(rhs)?;
        if *op == TokBody::Equals {
            Ok(Ir::Assign(Assign {
                with_flags,
                lhs,
                rhs,
            }))
        } else {
            let op = BinOp::from_assign_tok(op).ok_or("unknown assignment op")?;
            let ty = lhs.ty;
            let body = Body::Binop(lhs.clone().into(), op, rhs.into());
            Ok(Ir::Assign(Assign {
                with_flags,
                lhs,
                rhs: Expr { ty, body },
            }))
        }
    }

    fn lower_expr(&mut self, expr: &parse::Expr) -> Result<Expr, Error> {
        match expr {
            parse::Expr::Ident(tok) => {
                let id = tok.as_ident().unwrap();
                let ty = self.type_of_ident(id).ok_or("type lookup failed")?;
                let reg = if let Some(reg) = parse_register(id) {
                    reg
                } else {
                    self.regmap
                        .lookup(id)
                        .ok_or(format!("variable {id} not found"))?
                };
                let body = Body::Reg(reg);
                Ok(Expr { ty, body })
            }
            parse::Expr::Literal(lit) => {
                if let TokBody::Number(n) = &lit.tok {
                    let ty = TypeHandle::default();
                    let body = Body::Imm(*n as u32);
                    Ok(Expr { ty, body })
                } else {
                    Err("malformed literal")?
                }
            }
            parse::Expr::Binop(lhs, op, rhs) => {
                if op.tok == TokBody::At {
                    // It's possible these checks move earlier
                    let Some(id) = lhs.as_ident() else {
                        return Err("placed variable must be identifier")?;
                    };
                    let ty = self.type_of_ident(id).ok_or("type lookup failed")?;
                    let Some(place) = rhs.as_ident() else {
                        return Err("place must be identifier")?;
                    };
                    let Some(reg) = parse_register(place) else {
                        return Err("place must be a register")?;
                    };
                    let body = Body::Reg(reg);
                    Ok(Expr { ty, body })
                } else {
                    let lhs = self.lower_expr(lhs)?;
                    let rhs = self.lower_expr(rhs)?;
                    let op = BinOp::from_tok(&op.tok).ok_or("unknown binop")?;
                    let ty = lhs.ty;
                    let body = Body::Binop(lhs.into(), op, rhs.into());
                    Ok(Expr { ty, body })
                }
            }
            parse::Expr::Unary(op, expr) => {
                let expr = self.lower_expr(expr)?;
                let op = UnaryOp::from_tok(&op.tok).ok_or("unknown unary op")?;
                let ty = match op {
                    UnaryOp::Deref => match self.types.get(expr.ty) {
                        Type::Ptr(target) => *target,
                        _ => TypeHandle::default(),
                    },
                    UnaryOp::Neg | UnaryOp::Not => expr.ty,
                    _ => todo!("unhandled unary op"),
                };
                let body = Body::Unary(op, expr.into());
                Ok(Expr { ty, body })
            }
            parse::Expr::Cast(lhs, ty) => {
                let mut expr = self.lower_expr(lhs)?;
                let ty = self.types.intern_from_ast(ty)?;
                // Here we choose not to have a separate cast expr, but that might
                // be useful later.
                expr.ty = ty;
                Ok(expr)
            }
            parse::Expr::Field(expr, field) => {
                if expr.as_ident() == Some("peripherals") {
                    return self.lower_peripheral(field);
                }
                let expr = self.lower_expr(expr)?;
                // TODO: support s.a.b, in which case the type is a struct
                let Type::Ptr(struct_ty) = self.types.get(expr.ty) else {
                    return Err("base must be pointer")?;
                };
                let Type::Struct(struct_handle) = self.types.get(*struct_ty) else {
                    return Err("base must be pointer to struct")?;
                };
                let field = self
                    .types
                    .get_field(*struct_handle, field.as_ident().unwrap())
                    .ok_or("field not found")?;
                let ty = field.ty;
                let body = Body::Field(expr.into(), field.offset);
                Ok(Expr { ty, body })
            }
            parse::Expr::Slice(expr, start, end) => {
                let expr = self.lower_expr(expr)?;
                let ty = expr.ty;
                let body = Body::Slice(expr.into(), *start, *end);
                Ok(Expr { ty, body })
            }
            parse::Expr::Tuple(exps) => {
                let mut irs = vec![];
                let mut types = vec![];
                for expr in exps {
                    let ir = self.lower_expr(expr)?;
                    types.push(ir.ty);
                    irs.push(ir);
                }
                let ty = self.types.intern_tuple(&types);
                let body = Body::Tuple(irs);
                Ok(Expr { ty, body })
            }
        }
    }

    fn lower_peripheral(&mut self, field: &Token) -> Result<Expr, Error> {
        if let Some(name) = field.as_ident()
            && let Some(p) = self.peripherals
        {
            if let Some(periph) = p.periphs.get(name) {
                let ty = periph.ty;
                let body = Body::Imm(periph.base_address);
                Ok(Expr { ty, body })
            } else {
                Err(format!("peripheral {name} not found"))?
            }
        } else {
            Err("peripherals not set up properly")?
        }
    }

    fn lower_addr_update(
        &mut self,
        stmt: &Stmt,
        lhs: &parse::Expr,
        sign: i32,
        incr: &parse::Expr,
    ) -> Result<Ir, Error> {
        let ir = self.lower(stmt)?;
        let _lhs = self.lower_expr(lhs)?;
        // TODO: check that _lhs matches addr in the stmt
        let rhs = self.lower_expr(incr)?;
        let Body::Imm(incr) = &rhs.body else {
            return Err("addr increment must be integer")?;
        };
        Ok(Ir::WithAddrUpdate(ir.into(), *incr as i32 * sign))
    }

    fn type_of_ident(&mut self, id: &str) -> Option<TypeHandle> {
        // TODO: symbol lookup etc
        if let Some(ty) = self.typemap.lookup(id) {
            Some(ty)
        } else {
            Some(TypeHandle::default())
        }
    }
}
