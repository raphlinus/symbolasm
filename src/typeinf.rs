//! Type inference (very basic for now).

use std::collections::HashMap;

use crate::{
    lex::{Error, TokBody},
    parse::{self, Function},
    stmt::Stmt,
    types::{Type, TypeHandle, TypePool},
};

pub struct TypeMap {
    map: HashMap<String, TypeHandle>,
}

impl TypeMap {
    pub fn infer(func: &Function, types: &mut TypePool) -> Result<Self, Error> {
        let mut map = HashMap::new();
        for arg in &func.args.0 {
            let ty = if let Some(ast_ty) = &arg.ty.as_ref() {
                types.from_ast(ast_ty)?
            } else {
                // Note: this is a choice; it could be default to allow more inference
                types.get_handle(&Type::U32)
            };
            let var = arg.var.as_ident().ok_or("arg must be ident")?;
            map.insert(var.to_owned(), ty);
        }
        let mut result = Self { map };
        loop {
            if !result.infer_pass(&func.body, types) {
                break;
            }
        }
        Ok(result)
    }

    pub fn lookup(&self, id: &str) -> Option<TypeHandle> {
        self.map.get(id).cloned()
    }

    // TODO: indicate whether additional passes might be effective
    // Also a thought. Doing this on AST raises the question whether it might be
    // better to do IR lowering in two phases. First attaches types to each node
    // while retaining variables (each variable has one type), second pass places
    // variables in registers. But we'll do this in the current structure.
    fn infer_pass(&mut self, body: &[Stmt], types: &mut TypePool) -> bool {
        let mut changed = false;
        for stmt in body {
            match stmt {
                Stmt::Assign(lhs, op, rhs) => {
                    if op.tok == TokBody::Equals {
                        changed |= self.infer_assign(lhs, rhs, types);
                    }
                }
                Stmt::AssignPlace(lhs, _place, rhs) => {
                    changed |= self.infer_assign(lhs, rhs, types)
                }
                Stmt::WithFlagsAssign(lhs, op, rhs) => {
                    if op.tok == TokBody::Equals {
                        changed |= self.infer_assign(lhs, rhs, types);
                    }
                }
                Stmt::WithFlagsAssignPlace(lhs, _place, rhs) => {
                    changed |= self.infer_assign(lhs, rhs, types)
                }
                _ => (),
            }
        }
        changed
    }

    fn infer_assign(&mut self, lhs: &parse::Expr, rhs: &parse::Expr, types: &mut TypePool) -> bool {
        let Some(id) = lhs.as_ident() else {
            return false;
        };
        if self.map.contains_key(id) {
            return false;
        }
        if let Some(ty) = self.try_get_type(rhs, types) {
            self.map.insert(id.to_owned(), ty);
            true
        } else {
            false
        }
    }

    fn try_get_type(&mut self, expr: &parse::Expr, types: &mut TypePool) -> Option<TypeHandle> {
        match expr {
            parse::Expr::Ident(token) => token.as_ident().and_then(|id| self.map.get(id).cloned()),
            parse::Expr::Literal(_token) => Some(TypeHandle::default()),
            parse::Expr::Binop(expr, _token, _expr1) => self.try_get_type(expr, types),
            parse::Expr::Unary(token, expr) => match &token.tok {
                TokBody::Asterisk => self.try_get_type(expr, types).and_then(|ptr| {
                    if let Type::Ptr(target) = types.get(ptr) {
                        Some(*target)
                    } else {
                        None
                    }
                }),
                TokBody::Minus | TokBody::Exclamation => self.try_get_type(expr, types),
                _ => None,
            },
            parse::Expr::Cast(_expr, ty) => types.from_ast(ty).ok(),
        }
    }
}
