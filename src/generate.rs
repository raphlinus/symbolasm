//! Generation of assembly language.

use std::io::Write;

use crate::{
    lex::{Error, TokBody, Token},
    parse::Expr,
    regmap::{Regmap, parse_register},
    stmt::Stmt,
};

pub fn gen_stmt(stmt: &Stmt, regmap: &Regmap, w: &mut impl Write) -> Result<(), Error> {
    match stmt {
        Stmt::Label(l) => {
            writeln!(w, "{l}:")?;
        }
        Stmt::Assign(Expr::Ident(lhs), op, rhs) => gen_assign(lhs, &op.tok, rhs, regmap, false, w)?,
        Stmt::Assign(_, _, _) => todo!("non-ident lhs nyi"),
        Stmt::AssignPlace(_lhs, place, rhs) => {
            gen_assign(place, &TokBody::Equals, rhs, regmap, false, w)?
        }
        Stmt::WithFlagsExpr(expr) => (),
        Stmt::WithFlagsAssign(Expr::Ident(lhs), op, rhs) => {
            gen_assign(lhs, &op.tok, rhs, regmap, true, w)?
        }
        Stmt::WithFlagsAssignPlace(expr, token, expr1) => (),
        Stmt::Insn(insn) => match insn {
            crate::stmt::Insn::Bx(target) => {
                if let Some(id) = target.as_ident() {
                    write!(w, "    bx ")?;
                    write_var_reg(id, regmap, w)?;
                    writeln!(w)?;
                } else {
                    Err("bx target must be register")?;
                }
            }
            crate::stmt::Insn::BCond(cond, target) => {
                writeln!(w, "    b{cond} {target}")?;
            }
            crate::stmt::Insn::B(target) => writeln!(w, "    b {target}")?,
            crate::stmt::Insn::Bl(target) => writeln!(w, "    bl {target}")?,
        },
        _ => todo!("nyi"),
    }
    Ok(())
}

fn gen_assign(
    lhs: &Token,
    op: &TokBody,
    rhs: &Expr,
    regmap: &Regmap,
    with_flags: bool,
    w: &mut impl Write,
) -> Result<(), Error> {
    if let Some(id) = lhs.as_ident() {
        match rhs {
            Expr::Ident(rhs_tok) => {
                let insn = insn_for_assign_op(op)?;
                write!(w, "    {insn}{} ", s(with_flags))?;
                write_var_reg(id, regmap, w)?;
                let rhs_id = rhs_tok.as_ident().ok_or("expected ident")?;
                write!(w, ", ")?;
                write_var_reg(rhs_id, regmap, w)?;
                writeln!(w)?;
            }
            Expr::Literal(lit) => {
                let insn = insn_for_assign_op(op)?;
                write!(w, "    {insn}{} ", s(with_flags))?;
                write_var_reg(id, regmap, w)?;
                write!(w, ", ")?;
                write_literal(lit, w)?;
                writeln!(w)?;
            }
            Expr::Binop(expr, token, expr1) => todo!(),
        }
    } else {
        todo!("non-ident lhs in assign")
    }
    Ok(())
}

fn write_var_reg(id: &str, regmap: &Regmap, w: &mut impl Write) -> Result<(), Error> {
    if parse_register(id).is_some() {
        write!(w, "{id}")?;
    } else if let Some(r) = regmap.lookup(id) {
        write!(w, "r{r}")?;
    } else {
        Err(format!("no place for {id}"))?;
    }
    Ok(())
}

fn write_literal(lit: &Token, w: &mut impl Write) -> Result<(), Error> {
    if let TokBody::Number(n) = &lit.tok {
        write!(w, "#{n}",)?;
    } else {
        Err("expected number token in literal")?
    }
    Ok(())
}

fn s(with_flags: bool) -> &'static str {
    if with_flags { "s" } else { "" }
}

fn insn_for_assign_op(tok: &TokBody) -> Result<&'static str, Error> {
    Ok(match tok {
        TokBody::Equals => "mov",
        TokBody::PlusEquals => "add",
        TokBody::MinusEquals => "sub",
        TokBody::AsteriskEquals => "mul",
        _ => Err("no instruction for op")?,
    })
}
