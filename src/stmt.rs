use crate::{lex::Token, parse::Expr, regmap::parse_register};

#[derive(Debug)]
pub enum Stmt {
    Label(String),
    /// Includes assignment ops as well as `Equals`
    Assign(Expr, Token, Expr),
    /// var @ reg = expr
    AssignPlace(Expr, Token, Expr),
    WithFlagsExpr(Expr),
    WithFlagsAssign(Expr, Token, Expr),
    WithFlagsAssignPlace(Expr, Token, Expr),
    Insn(Insn),
}

#[derive(Debug)]
pub enum Insn {
    Bx(Expr),
    BCond(String, String),
    B(String),
    Bl(String),
}

pub enum Var {
    Var(String),
    Reg(u8),
}

impl Var {
    fn parse(expr: &Expr) -> Option<Self> {
        if let Some(id) = expr.as_ident() {
            if let Some(r) = parse_register(id) {
                Some(Var::Reg(r))
            } else {
                Some(Var::Var(id.to_owned()))
            }
        } else {
            None
        }
    }
}
