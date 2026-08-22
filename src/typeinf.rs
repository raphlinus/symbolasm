//! Type inference (very basic for now).

use std::collections::HashMap;

use crate::{
    lex::{Error, TokBody},
    parse::{self, Function},
    stmt::Stmt,
    svd::Peripherals,
    types::{Type, TypeHandle, TypePool},
};

#[derive(Default)]
pub struct TypeMap {
    map: HashMap<String, TypeHandle>,
}

pub struct TypeInferCtx<'a> {
    map: TypeMap,
    types: &'a mut TypePool,
    peripherals: Option<&'a Peripherals>,
}

impl<'a> TypeInferCtx<'a> {
    pub fn new(types: &'a mut TypePool, peripherals: Option<&'a Peripherals>) -> Self {
        Self {
            map: TypeMap::default(),
            types,
            peripherals,
        }
    }

    pub fn infer(mut self, func: &Function) -> Result<TypeMap, Error> {
        for arg in &func.args.0 {
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
            match stmt {
                Stmt::Assign(lhs, op, rhs) => {
                    if op.tok == TokBody::Equals {
                        changed |= self.infer_assign(lhs, rhs);
                    }
                }
                Stmt::AssignPlace(lhs, _place, rhs) => changed |= self.infer_assign(lhs, rhs),
                Stmt::WithFlagsAssign(lhs, op, rhs) => {
                    if op.tok == TokBody::Equals {
                        changed |= self.infer_assign(lhs, rhs);
                    }
                }
                Stmt::WithFlagsAssignPlace(lhs, _place, rhs) => {
                    changed |= self.infer_assign(lhs, rhs)
                }
                _ => (),
            }
        }
        changed
    }

    fn infer_assign(&mut self, lhs: &parse::Expr, rhs: &parse::Expr) -> bool {
        let Some(id) = lhs.as_ident() else {
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
            parse::Expr::Ident(token) => token
                .as_ident()
                .and_then(|id| self.map.map.get(id).cloned()),
            parse::Expr::Literal(_token) => Some(TypeHandle::default()),
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
                _ => None,
            },
            parse::Expr::Cast(_expr, ty) => self.types.intern_from_ast(ty).ok(),
            parse::Expr::Field(expr, field) => {
                if expr.as_ident() == Some("peripherals") {
                    if let Some(name) = field.as_ident()
                        && let Some(p) = self.peripherals
                        && let Some(periph) = p.periphs.get(name)
                    {
                        Some(periph.ty)
                    } else {
                        None
                    }
                } else {
                    self.try_get_type(expr).and_then(|base| {
                        if let Type::Ptr(target) = self.types.get(base)
                            && let Type::Struct(s) = self.types.get(*target)
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
        }
    }
}

impl TypeMap {
    pub fn lookup(&self, id: &str) -> Option<TypeHandle> {
        self.map.get(id).cloned()
    }
}
