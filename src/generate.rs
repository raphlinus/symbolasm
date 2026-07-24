//! Generation of assembly language.
//!
//! This approach is clunky, and is going to run into problems when types are
//! needed.

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
        Stmt::Assign(
            Expr::Unary(
                Token {
                    tok: TokBody::Asterisk,
                    ..
                },
                lhs,
            ),
            op,
            rhs,
        ) => gen_store(lhs, &op.tok, rhs, regmap, w)?,
        Stmt::Assign(_, _, _) => todo!("non-ident lhs nyi"),
        Stmt::AssignPlace(_lhs, place, rhs) => {
            gen_assign(place, &TokBody::Equals, rhs, regmap, false, w)?
        }
        Stmt::WithFlagsExpr(expr) => todo!(),
        Stmt::WithFlagsAssign(Expr::Ident(lhs), op, rhs) => {
            gen_assign(lhs, &op.tok, rhs, regmap, true, w)?
        }
        Stmt::WithFlagsAssignPlace(expr, token, expr1) => todo!(),
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
            crate::stmt::Insn::BCond(cond, target) => writeln!(w, "    b{cond} {target}")?,
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
                check_lit_ok(insn, lit)?;
                write!(w, "    {insn}{} ", s(with_flags))?;
                write_var_reg(id, regmap, w)?;
                write!(w, ", ")?;
                write_literal(lit, w)?;
                writeln!(w)?;
            }
            Expr::Binop(expr, token, expr1) => todo!(),
            Expr::Unary(tok, expr) => {
                if tok.tok == TokBody::Asterisk {
                    write!(w, "    ldr ")?;
                    write_var_reg(id, regmap, w)?;
                    write!(w, ", ")?;
                    write_addr(expr, regmap, w)?;
                    writeln!(w)?;
                } else {
                    Err("unhandled unary op")?;
                }
            }
        }
    } else {
        todo!("non-ident lhs in assign")
    }
    Ok(())
}

fn gen_store(
    lhs: &Expr,
    op: &TokBody,
    rhs: &Expr,
    regmap: &Regmap,
    w: &mut impl Write,
) -> Result<(), Error> {
    if let Some(id) = rhs.as_ident()
        && op == &TokBody::Equals
    {
        write!(w, "    str ")?;
        write_var_reg(id, regmap, w)?;
        write!(w, ", ")?;
        write_addr(lhs, regmap, w)?;
        writeln!(w)?;
        Ok(())
    } else {
        todo!()
    }
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

fn write_addr(expr: &Expr, regmap: &Regmap, w: &mut impl Write) -> Result<(), Error> {
    if let Expr::Ident(id) = expr {
        write!(w, "[")?;
        write_var_reg(id.as_ident().unwrap(), regmap, w)?;
        write!(w, "]")?;
    } else {
        todo!("can do lots of other address modes!");
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

fn check_lit_ok(insn: &str, lit: &Token) -> Result<(), Error> {
    if let TokBody::Number(num) = &lit.tok {
        // TODO: overflows
        let val = *num as u32;
        let ok = match insn {
            "mov" => val < 0x1_0000 || is_imm8m(val),
            "add" | "sub" => val < 0x1000 || is_imm8m(val),
            _ => is_imm8m(val),
        };
        if ok {
            Ok(())
        } else {
            Err("literal out of encoding range")?
        }
    } else {
        Err("expected number in literal token")?
    }
}

fn is_imm8m(val: u32) -> bool {
    val < 0x100
        || (val << val.leading_zeros()) & 0xff_ffff == 0
        || val == (val & 0xff) * 0x1_0001
        || val == (val & 0xff) * 0x0101_0101
        || val == (val & 0xff00) * 0x1_0001
}
