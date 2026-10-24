// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::consts::fold_binop;
use crate::globals::Globals;
use crate::lex::Token;
use crate::parse;
use crate::regmap::parse_register;
use crate::typeinf::TypeMap;
use crate::{
    error::Error,
    lex::TokBody,
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
    /// Address of a linker symbol.
    Sym(String),
    /// Half of a linker symbol's address, resolved by the linker.
    SymHalf(Half, String),
    /// An instruction written as a function call, like `clz(x)`
    Intrinsic(&'static str, Vec<Expr>),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Half {
    Lower,
    Upper,
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
    globals: &'a Globals,
}

impl BinOp {
    pub fn from_tok(tok: &TokBody) -> Option<Self> {
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
            TokBody::AmpersandEquals => BinOp::And,
            TokBody::PipeEquals => BinOp::Orr,
            TokBody::CaretEquals => BinOp::Eor,
            TokBody::LessLessEquals => BinOp::Shl,
            TokBody::GreaterGreaterEquals => BinOp::Shr,
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
        globals: &'a Globals,
    ) -> Self {
        Self {
            regmap,
            types,
            typemap,
            globals,
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
        let mut ir = self.lower_raw(stmt)?;
        canonicalize_ir(&mut ir)?;
        Ok(ir)
    }

    /// `*p++ = x` or `x = *p++`: update the address by the size of the
    /// transfer, which for a tuple is the whole tuple.
    fn post_inc(&mut self, ir: Ir, sign: i32, is_store: bool) -> Result<Ir, Error> {
        let Ir::Assign(assign) = &ir else {
            unreachable!()
        };
        let (deref, other) = if is_store {
            (&assign.lhs, &assign.rhs)
        } else {
            (&assign.rhs, &assign.lhs)
        };
        let size = if let Body::Tuple(_) = other.body {
            self.types.info(other.ty).size()
        } else {
            self.types.info(deref.ty).size()
        };
        if let Body::Tuple(_) = other.body
            && sign < 0
        {
            return Err("a tuple can't be transferred with a post-decrement")?;
        }
        Ok(Ir::WithAddrUpdate(ir.into(), sign * size as i32))
    }

    fn is_ptr(&self, ty: TypeHandle) -> bool {
        matches!(self.types.get(ty), Type::Ptr(_))
    }

    fn lower_raw(&mut self, stmt: &Stmt) -> Result<Ir, Error> {
        match stmt {
            Stmt::Assign(lhs, op, rhs) if op.tok == TokBody::Equals => {
                if let Some((deref, sign)) = strip_post_inc(lhs) {
                    let ir = self.lower_assign(&deref, &op.tok, rhs, false)?;
                    return self.post_inc(ir, sign, true);
                }
                if let Some((deref, sign)) = strip_post_inc(rhs) {
                    let ir = self.lower_assign(lhs, &op.tok, &deref, false)?;
                    return self.post_inc(ir, sign, false);
                }
                self.lower_assign(lhs, &op.tok, rhs, false)
            }
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
                } else if let Some(reg) = self.regmap.lookup(id) {
                    reg
                } else if let Some(n) = self.types.const_value(id) {
                    let body = Body::Imm(n);
                    return Ok(Expr { ty, body });
                } else if self.globals.symbol_ty(id).is_some() || self.globals.fns.contains_key(id)
                {
                    let body = Body::Sym(id.to_owned());
                    return Ok(Expr { ty, body });
                } else if let Some(static_ty) = self.globals.static_ty(id) {
                    // An immutable scalar static can be loaded by value,
                    // PC-relative. A static mut is only reachable through
                    // its address, since it lives in RAM.
                    if !self.globals.is_immutable(id) {
                        return Err(format!("static mut {id} can only be used as &{id}"))?;
                    }
                    if !matches!(
                        self.types.get(static_ty),
                        Type::U32
                            | Type::I32
                            | Type::U16
                            | Type::I16
                            | Type::U8
                            | Type::I8
                            | Type::Ptr(_)
                    ) {
                        return Err(format!("{id} isn't a scalar; use &{id}"))?;
                    }
                    let addr_ty = self.types.get_handle(&Type::Ptr(static_ty));
                    let addr = Expr {
                        ty: addr_ty,
                        body: Body::Sym(id.to_owned()),
                    };
                    let body = Body::Unary(UnaryOp::Deref, addr.into());
                    return Ok(Expr {
                        ty: static_ty,
                        body,
                    });
                } else if let Some(why) = self.regmap.lost(id) {
                    return Err(format!("{id} is no longer in a register: {why}"))?;
                } else {
                    return Err(format!("variable {id} not found"))?;
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
                    // Pointer arithmetic keeps the pointer's type, whichever
                    // side it's on.
                    let ty = if self.is_ptr(rhs.ty) && !self.is_ptr(lhs.ty) {
                        rhs.ty
                    } else {
                        lhs.ty
                    };
                    let body = Body::Binop(lhs.into(), op, rhs.into());
                    Ok(Expr { ty, body })
                }
            }
            parse::Expr::Unary(op, expr) if op.tok == TokBody::Ampersand => {
                let mut addr = self.address_of(expr)?;
                // A pointer to an array is a pointer to its first element.
                if let Type::Ptr(target) = self.types.get(addr.ty) {
                    let target = self.types.decay(*target);
                    addr.ty = self.types.get_handle(&Type::Ptr(target));
                }
                Ok(addr)
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
                // The base is a pointer to a struct, or, for p.a.b, a field
                // that is itself a struct, whose offset is folded in.
                let (struct_handle, base, offset) = match self.types.get(expr.ty) {
                    Type::Struct(s) => {
                        let Body::Field(base, offset) = expr.body else {
                            return Err("base must be pointer")?;
                        };
                        (*s, base, offset)
                    }
                    Type::Ptr(struct_ty) => {
                        let Type::Struct(s) = self.types.get(*struct_ty) else {
                            return Err("base must be pointer to struct")?;
                        };
                        (*s, Box::new(expr), 0)
                    }
                    _ => return Err("base must be pointer")?,
                };
                let field = self
                    .types
                    .get_field(struct_handle, field.as_ident().unwrap())
                    .ok_or("field not found")?;
                let ty = field.ty;
                let body = Body::Field(base, offset + field.offset);
                Ok(Expr { ty, body })
            }
            parse::Expr::Slice(expr, start, end) => {
                let expr = self.lower_expr(expr)?;
                let ty = expr.ty;
                let body = Body::Slice(expr.into(), *start, *end);
                Ok(Expr { ty, body })
            }
            parse::Expr::Call(name, args) => self.lower_call(name, args),
            parse::Expr::PostInc(..) => {
                Err("++ and -- only apply to an address being loaded or stored, as in *p++")?
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

    /// The address of a place: a static (`&dvi`), or a field of a struct
    /// reached through a pointer, a static, a peripheral, or an embedded
    /// struct (`&p.lent`, `&dvi.queue`, `&peripherals.PPB.NVIC_ISER0`,
    /// `&p.a.b`). The result is typed as a pointer to the place.
    ///
    /// A field's address is the base plus the field's offset: an add for a
    /// register base, a constant for a peripheral, and a linker expression
    /// (`sym + offset`) for a static.
    fn address_of(&mut self, expr: &parse::Expr) -> Result<Expr, Error> {
        match expr {
            parse::Expr::Ident(tok) => {
                let id = tok.as_ident().unwrap();
                let target = self.globals.static_ty(id).ok_or(format!(
                    "& only applies to statics and fields, {id} is not one"
                ))?;
                let ty = self.types.get_handle(&Type::Ptr(target));
                let body = Body::Sym(id.to_owned());
                Ok(Expr { ty, body })
            }
            parse::Expr::Field(base, field) => {
                let base = if base.as_ident() == Some("peripherals") {
                    return self.lower_peripheral(field);
                } else if let Some(id) = base.as_ident()
                    && self.globals.static_ty(id).is_some()
                    && self.regmap.lookup(id).is_none()
                {
                    // the struct is the static itself
                    self.address_of(base)?
                } else if let parse::Expr::Field(..) = &**base
                    && let Ok(inner) = self.address_of(base)
                    && let Type::Ptr(inner_ty) = self.types.get(inner.ty)
                    && let Type::Struct(_) = self.types.get(*inner_ty)
                {
                    // an embedded struct
                    inner
                } else {
                    // a pointer to the struct
                    self.lower_expr(base)?
                };
                let Type::Ptr(struct_ty) = self.types.get(base.ty) else {
                    return Err("base must be pointer")?;
                };
                let Type::Struct(struct_handle) = self.types.get(*struct_ty) else {
                    return Err("base must be pointer to struct")?;
                };
                let field = self
                    .types
                    .get_field(*struct_handle, field.as_ident().unwrap())
                    .ok_or("field not found")?;
                let (field_ty, offset) = (field.ty, field.offset as u32);
                let ty = self.types.get_handle(&Type::Ptr(field_ty));
                let body = match base.body {
                    Body::Imm(a) => Body::Imm(a.wrapping_add(offset)),
                    _ if offset == 0 => base.body,
                    _ => {
                        let offset = Expr {
                            ty: TypeHandle::default(),
                            body: Body::Imm(offset),
                        };
                        Body::Binop(base.into(), BinOp::Add, offset.into())
                    }
                };
                Ok(Expr { ty, body })
            }
            _ => Err("& only applies to statics and fields")?,
        }
    }

    fn lower_peripheral(&mut self, field: &Token) -> Result<Expr, Error> {
        if let Some(name) = field.as_ident()
            && let Some(p) = &self.globals.peripherals
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

    fn lower_call(&mut self, name: &Token, args: &[parse::Expr]) -> Result<Expr, Error> {
        let id = name.as_ident().unwrap();
        if let Some(insn) = INSN_INTRINSICS.iter().find(|i| **i == id) {
            let arity = if matches!(*insn, "pkhbt" | "pkhtb" | "ror") {
                2
            } else {
                1
            };
            if args.len() != arity {
                return Err(format!("{insn} takes {arity} argument(s)"))?;
            }
            let mut irs = vec![];
            for arg in args {
                irs.push(self.lower_expr(arg)?);
            }
            let ty = irs[0].ty;
            let body = Body::Intrinsic(insn, irs);
            return Ok(Expr { ty, body });
        }
        let half = match id {
            "lower16" => Some(Half::Lower),
            "upper16" => Some(Half::Upper),
            _ => None,
        };
        let ty = TypeHandle::default();
        if let Some(half) = half
            && let [parse::Expr::Ident(arg)] = args
            && let Some(arg) = arg.as_ident()
            && self.globals.symbol_ty(arg).is_some()
        {
            let body = Body::SymHalf(half, arg.to_owned());
            return Ok(Expr { ty, body });
        }
        // Otherwise it must be a constant: size(), count(), or a half of
        // a constant.
        let call = parse::Expr::Call(name.clone(), args.to_vec());
        let n = crate::consts::eval(&call, self.types, &self.globals.statics)?;
        let body = Body::Imm(n);
        Ok(Expr { ty, body })
    }

    fn lower_addr_update(
        &mut self,
        stmt: &Stmt,
        lhs: &parse::Expr,
        sign: i32,
        incr: &parse::Expr,
    ) -> Result<Ir, Error> {
        let ir = self.lower_raw(stmt)?;
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
        } else if let Some(ty) = self.globals.symbol_ty(id) {
            Some(ty)
        } else if let Some(ty) = self.globals.fns.get(id) {
            Some(*ty)
        } else {
            Some(TypeHandle::default())
        }
    }
}

/// Split `*p++` into `*p` and the direction of the update.
fn strip_post_inc(expr: &parse::Expr) -> Option<(parse::Expr, i32)> {
    if let parse::Expr::Unary(op, inner) = expr
        && op.tok == TokBody::Asterisk
        && let parse::Expr::PostInc(addr, sign) = &**inner
    {
        Some((parse::Expr::Unary(op.clone(), addr.clone()), *sign))
    } else {
        None
    }
}

/// Instructions written as function calls, which compute a value.
const INSN_INTRINSICS: &[&str] = &[
    "clz", "rbit", "rev", "rev16", "revsh", "pkhbt", "pkhtb", "ror",
];

fn canonicalize_ir(ir: &mut Ir) -> Result<(), Error> {
    match ir {
        Ir::Assign(assign) => {
            canonicalize(&mut assign.lhs)?;
            canonicalize(&mut assign.rhs)
        }
        Ir::WithFlags(expr) => canonicalize(expr),
        Ir::WithAddrUpdate(ir, _) => canonicalize_ir(ir),
    }
}

/// Rewrite an expression into the forms code generation expects.
///
/// Children are canonicalized first. Then:
/// - an operation on two literals is folded,
/// - a commutative operation with a literal or compound left operand and a
///   register right operand is swapped, so the register comes first,
/// - multiplication by a power of two becomes a left shift, so
///   `*(p + i * 4)` is the same as `*(p + (i << 2))`,
/// - `(a + c1) + c2` with literals c1 and c2 becomes `a + (c1 + c2)`.
fn canonicalize(expr: &mut Expr) -> Result<(), Error> {
    match &mut expr.body {
        Body::Binop(a, _, b) => {
            canonicalize(a)?;
            canonicalize(b)?;
        }
        Body::Unary(_, a) | Body::Field(a, _) | Body::Slice(a, _, _) => canonicalize(a)?,
        Body::Tuple(els) | Body::Intrinsic(_, els) => {
            for el in els {
                canonicalize(el)?;
            }
        }
        Body::Reg(_) | Body::Imm(_) | Body::Sym(_) | Body::SymHalf(_, _) => return Ok(()),
    }
    if let Body::Unary(op @ (UnaryOp::Not | UnaryOp::Neg), a) = &expr.body
        && let Body::Imm(n) = a.body
    {
        let n = if *op == UnaryOp::Not {
            !n
        } else {
            n.wrapping_neg()
        };
        expr.body = Body::Imm(n);
        return Ok(());
    }
    let Body::Binop(a, op, b) = &mut expr.body else {
        return Ok(());
    };
    if let (Body::Imm(x), Body::Imm(y)) = (&a.body, &b.body) {
        expr.body = Body::Imm(fold_binop(*x, *op, *y)?);
        return Ok(());
    }
    let commutative = matches!(
        op,
        BinOp::Add | BinOp::Mul | BinOp::And | BinOp::Orr | BinOp::Eor
    );
    if commutative && !matches!(a.body, Body::Reg(_)) && matches!(b.body, Body::Reg(_)) {
        std::mem::swap(a, b);
    }
    if *op == BinOp::Mul
        && let Body::Imm(n) = b.body
        && n.is_power_of_two()
    {
        if n == 1 {
            let a = (**a).clone();
            expr.body = a.body;
            return Ok(());
        }
        *op = BinOp::Shl;
        b.body = Body::Imm(n.trailing_zeros());
    }
    if *op == BinOp::Add
        && let Body::Imm(c2) = b.body
        && let Body::Binop(inner_a, BinOp::Add, inner_b) = &a.body
        && let Body::Imm(c1) = inner_b.body
    {
        let inner_a = (**inner_a).clone();
        **a = inner_a;
        b.body = Body::Imm(c1.wrapping_add(c2));
    }
    Ok(())
}
