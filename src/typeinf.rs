// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Type inference (very basic for now).

use std::collections::HashMap;

use crate::{
    error::Error,
    globals::Globals,
    ir::FLOAT_INTRINSICS,
    lex::TokBody,
    parse::{self, Function},
    stmt::Stmt,
    types::{Type, TypeHandle, TypePool},
};

#[derive(Default)]
pub struct TypeMap {
    map: HashMap<String, TypeHandle>,
}

pub struct TypeInferCtx<'a> {
    map: TypeMap,
    types: &'a mut TypePool,
    globals: &'a Globals,
}

impl<'a> TypeInferCtx<'a> {
    pub fn new(types: &'a mut TypePool, globals: &'a Globals) -> Self {
        Self {
            map: TypeMap::default(),
            types,
            globals,
        }
    }

    pub fn infer(mut self, func: &Function) -> Result<TypeMap, Error> {
        for arg in func.args.0.iter().chain(&func.rets.0) {
            let ty = if let Some(ast_ty) = &arg.ty.as_ref() {
                self.types.intern_from_ast(ast_ty)?
            } else {
                // Note: this is a choice; it could be default to allow more inference
                self.types.get_handle(&Type::U32)
            };
            let var = arg.var.as_ident().ok_or("arg must be ident")?;
            self.map.map.insert(var.to_owned(), ty);
        }
        loop {
            if !self.infer_pass(&func.body) {
                break;
            }
        }
        Ok(self.map)
    }

    // TODO: indicate whether additional passes might be effective
    // Also a thought. Doing this on AST raises the question whether it might be
    // better to do IR lowering in two phases. First attaches types to each node
    // while retaining variables (each variable has one type), second pass places
    // variables in registers. But we'll do this in the current structure.
    fn infer_pass(&mut self, body: &[Stmt]) -> bool {
        let mut changed = false;
        for stmt in body {
            changed |= self.infer_stmt(stmt);
        }
        changed
    }

    fn infer_stmt(&mut self, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Assign(lhs, op, rhs) => {
                if op.tok == TokBody::Equals {
                    self.infer_assign(lhs, rhs)
                } else {
                    false
                }
            }
            Stmt::WithFlagsAssign(lhs, op, rhs) => {
                if op.tok == TokBody::Equals {
                    self.infer_assign(lhs, rhs)
                } else {
                    false
                }
            }
            Stmt::WithAddrUpdate(stmt, _, _, _) => self.infer_stmt(stmt),
            Stmt::Call(call) => {
                let name = call.target.as_ident().unwrap_or_default();
                let ty = self
                    .map
                    .map
                    .get(name)
                    .copied()
                    .or_else(|| self.globals.fn_ty(name, self.types));
                let Some(rets) = ty.and_then(|t| self.types.sig(t)).map(|s| s.rets.clone()) else {
                    return false;
                };
                let mut changed = false;
                for (ret, ty) in call.rets.iter().zip(rets) {
                    if let Some(var) = assigned_var(ret)
                        && !self.map.map.contains_key(var)
                    {
                        self.map.map.insert(var.to_owned(), ty);
                        changed = true;
                    }
                }
                changed
            }
            Stmt::SpAdjust(_, places) => {
                let mut changed = false;
                for (var, ty) in places {
                    let Some(var) = var.as_ident() else { continue };
                    if self.map.map.contains_key(var) {
                        continue;
                    }
                    let ty = match ty {
                        Some(ty) => self.types.intern_from_ast(ty).ok(),
                        None => Some(self.types.get_handle(&Type::U32)),
                    };
                    if let Some(ty) = ty {
                        self.map.map.insert(var.to_owned(), ty);
                        changed = true;
                    }
                }
                changed
            }
            _ => false,
        }
    }

    fn infer_assign(&mut self, lhs: &parse::Expr, rhs: &parse::Expr) -> bool {
        // A multiple load, `(a, b) = *p`: each gets the pointee type
        if let parse::Expr::Tuple(vars) = lhs {
            let Some(ty) = self.try_get_type(rhs) else {
                return false;
            };
            let mut changed = false;
            for var in vars {
                if let Some(id) = assigned_var(var)
                    && !self.map.map.contains_key(id)
                {
                    self.map.map.insert(id.to_owned(), ty);
                    changed = true;
                }
            }
            return changed;
        }
        let Some(id) = assigned_var(lhs) else {
            return false;
        };
        if self.map.map.contains_key(id) {
            return false;
        }
        if let Some(ty) = self.try_get_type(rhs) {
            self.map.map.insert(id.to_owned(), ty);
            true
        } else {
            false
        }
    }

    fn try_get_type(&mut self, expr: &parse::Expr) -> Option<TypeHandle> {
        match expr {
            parse::Expr::Ident(token) => token.as_ident().and_then(|id| {
                self.map
                    .map
                    .get(id)
                    .cloned()
                    .or_else(|| self.globals.symbol_ty(id))
                    .or_else(|| self.globals.fns.get(id).copied())
                    .or_else(|| {
                        self.globals
                            .static_ty(id)
                            .filter(|_| self.globals.is_immutable(id))
                    })
                    .or_else(|| self.types.const_value(id).map(|_| TypeHandle::default()))
            }),
            parse::Expr::Literal(token) => match token.tok {
                TokBody::Float(_) => Some(self.types.get_handle(&Type::F32)),
                _ => Some(TypeHandle::default()),
            },
            parse::Expr::Binop(expr, _token, _expr1) => self.try_get_type(expr),
            parse::Expr::Unary(token, expr) => match &token.tok {
                TokBody::Asterisk => self.try_get_type(expr).and_then(|ptr| {
                    if let Type::Ptr(target) = self.types.get(ptr) {
                        Some(*target)
                    } else {
                        None
                    }
                }),
                TokBody::Minus | TokBody::Exclamation => self.try_get_type(expr),
                TokBody::Ampersand => {
                    let target = self.place_type(expr)?;
                    let target = self.types.decay(target);
                    Some(self.types.get_handle(&Type::Ptr(target)))
                }
                _ => None,
            },
            parse::Expr::Cast(_expr, ty) => self.types.intern_from_ast(ty).ok(),
            parse::Expr::Field(expr, field) if expr.as_ident() == Some("sp") => {
                self.map.map.get(field.as_ident()?).copied()
            }
            parse::Expr::Field(expr, field) => {
                if expr.as_ident() == Some("peripherals") {
                    if let Some(name) = field.as_ident()
                        && let Some(p) = &self.globals.peripherals
                        && let Some(periph) = p.periphs.get(name)
                    {
                        Some(periph.ty)
                    } else {
                        None
                    }
                } else {
                    self.try_get_type(expr).and_then(|base| {
                        // a pointer to a struct, or an embedded struct (p.a.b)
                        let target = match self.types.get(base) {
                            Type::Ptr(target) => *target,
                            _ => base,
                        };
                        if let Type::Struct(s) = self.types.get(target)
                            && let Some(field_name) = field.as_ident()
                            && let Some(field) = self.types.get_field(*s, field_name)
                        {
                            Some(field.ty)
                        } else {
                            None
                        }
                    })
                }
            }
            parse::Expr::Slice(expr, _, _) => self.try_get_type(expr),
            parse::Expr::Tuple(_) => None,
            parse::Expr::PostInc(expr, _) => self.try_get_type(expr),
            parse::Expr::Call(name, args) => match name.as_ident()? {
                "to_bits" | "f32_to_u32" => Some(self.types.get_handle(&Type::U32)),
                "f32_to_i32" => Some(self.types.get_handle(&Type::I32)),
                "from_bits" | "i32_to_f32" | "u32_to_f32" => {
                    Some(self.types.get_handle(&Type::F32))
                }
                id if FLOAT_INTRINSICS.iter().any(|(i, _)| *i == id) => {
                    self.try_get_type(args.first()?)
                }
                // lower16 and upper16 produce plain values
                _ => Some(TypeHandle::default()),
            },
        }
    }
}

impl TypeInferCtx<'_> {
    /// The type of a place that `&` can apply to: a static, or a field of a
    /// struct reached through a pointer, a static, a peripheral or an
    /// embedded struct.
    fn place_type(&mut self, expr: &parse::Expr) -> Option<TypeHandle> {
        match expr {
            parse::Expr::Ident(id) => self.globals.static_ty(id.as_ident()?),
            parse::Expr::Field(base, field) if base.as_ident() == Some("sp") => {
                self.map.map.get(field.as_ident()?).copied()
            }
            parse::Expr::Field(base, field) => {
                let struct_ty = if let parse::Expr::Field(inner, periph) = &**base
                    && inner.as_ident() == Some("peripherals")
                {
                    let p = self.globals.peripherals.as_ref()?;
                    let Type::Ptr(t) = self.types.get(p.periphs.get(periph.as_ident()?)?.ty) else {
                        return None;
                    };
                    *t
                } else if let Some(id) = base.as_ident()
                    && !self.map.map.contains_key(id)
                    && let Some(ty) = self.globals.static_ty(id)
                {
                    ty
                } else if let parse::Expr::Field(..) = &**base {
                    self.place_type(base)?
                } else {
                    let base_ty = self.try_get_type(base)?;
                    let Type::Ptr(t) = self.types.get(base_ty) else {
                        return None;
                    };
                    *t
                };
                let Type::Struct(s) = self.types.get(struct_ty) else {
                    return None;
                };
                Some(self.types.get_field(*s, field.as_ident()?)?.ty)
            }
            _ => None,
        }
    }
}

/// The variable assigned by an lhs, either `x` or placed `x @r0`.
fn assigned_var(lhs: &parse::Expr) -> Option<&str> {
    match lhs {
        parse::Expr::Binop(var, op, _place) if op.tok == TokBody::At => var.as_ident(),
        _ => lhs.as_ident(),
    }
}

impl TypeMap {
    pub fn lookup(&self, id: &str) -> Option<TypeHandle> {
        self.map.get(id).cloned()
    }
}
