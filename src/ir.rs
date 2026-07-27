use crate::lex::Token;
use crate::parse;
use crate::regmap::parse_register;
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
    // more to come
}

struct Place {
    var: String,
    place: u8,
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

impl Place {
    fn from_ast(lhs: &parse::Expr, place: &Token) -> Result<Self, Error> {
        let var = lhs
            .as_ident()
            .ok_or("placed expr must be ident")?
            .to_owned();
        if let TokBody::Idenfifier(place) = &place.tok {
            let place = parse_register(place).expect("place must be register");
            Ok(Self { var, place })
        } else {
            Err("place must be register name")?
        }
    }

    fn lookup(&self, id: &str) -> Option<u8> {
        if self.var == id {
            Some(self.place)
        } else {
            None
        }
    }
}

impl<'a> IrCtx<'a> {
    pub fn new(regmap: &'a Regmap, types: &'a mut TypePool, typemap: &'a TypeMap) -> Self {
        Self {
            regmap,
            types,
            typemap,
        }
    }

    pub fn can_lower(stmt: &Stmt) -> bool {
        matches!(
            stmt,
            Stmt::Assign(_, _, _)
                | Stmt::AssignPlace(_, _, _)
                | Stmt::WithFlagsAssign(_, _, _)
                | Stmt::WithFlagsAssignPlace(_, _, _)
                | Stmt::WithFlagsExpr(_)
        )
    }

    pub fn lower(&mut self, stmt: &Stmt) -> Result<Ir, Error> {
        match stmt {
            Stmt::Assign(lhs, op, rhs) => self.lower_assign(lhs, None, &op.tok, rhs, false),
            Stmt::AssignPlace(lhs, place, rhs) => {
                let place = Place::from_ast(lhs, place)?;
                self.lower_assign(lhs, Some(&place), &TokBody::Equals, rhs, false)
            }
            Stmt::WithFlagsExpr(expr) => {
                let expr = self.lower_expr(expr, None)?;
                Ok(Ir::WithFlags(expr))
            }
            Stmt::WithFlagsAssign(lhs, op, rhs) => self.lower_assign(lhs, None, &op.tok, rhs, true),
            Stmt::WithFlagsAssignPlace(lhs, place, rhs) => {
                let place = Place::from_ast(lhs, place)?;
                self.lower_assign(lhs, Some(&place), &TokBody::Equals, rhs, true)
            }
            _ => todo!("either we shouldn't try to lower, or we need to impl"),
        }
    }

    fn lower_assign(
        &mut self,
        lhs: &parse::Expr,
        place: Option<&Place>,
        op: &TokBody,
        rhs: &parse::Expr,
        with_flags: bool,
    ) -> Result<Ir, Error> {
        let lhs = self.lower_expr(lhs, place)?;
        let rhs = self.lower_expr(rhs, None)?;
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

    fn lower_expr(&mut self, expr: &parse::Expr, place: Option<&Place>) -> Result<Expr, Error> {
        match expr {
            parse::Expr::Ident(tok) => {
                let id = tok.as_ident().unwrap();
                let ty = self.type_of_ident(id).ok_or("type lookup failed")?;
                let reg = if let Some(reg) = place.map(|p| p.lookup(id)).flatten() {
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
                let lhs = self.lower_expr(lhs, None)?;
                let rhs = self.lower_expr(rhs, None)?;
                let op = BinOp::from_tok(&op.tok).ok_or("unknown binop")?;
                let ty = lhs.ty;
                let body = Body::Binop(lhs.into(), op, rhs.into());
                Ok(Expr { ty, body })
            }
            parse::Expr::Unary(op, expr) => {
                let expr = self.lower_expr(expr, None)?;
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
                let mut expr = self.lower_expr(lhs, place)?;
                let ty = self.types.from_ast(ty)?;
                // Here we choose not to have a separate cast expr, but that might
                // be useful later.
                expr.ty = ty;
                Ok(expr)
            }
        }
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
