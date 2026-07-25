//! Generation of assembly language.
//!
//! This approach is clunky, and is going to run into problems when types are
//! needed.

use std::io::Write;

use crate::{
    ir::{Assign, BinOp, Body, Expr, Ir, UnaryOp},
    lex::{Error, TokBody, Token},
    parse,
    regmap::{Regmap, parse_register},
    stmt::Stmt,
};

pub fn gen_from_ir(ir: &Ir, w: &mut impl Write) -> Result<(), Error> {
    match ir {
        Ir::Assign(assign) => gen_assign(assign, w),
        Ir::WithFlags(expr) => gen_withflags(expr, w),
        _ => todo!(),
    }
}

fn gen_assign(assign: &Assign, w: &mut impl Write) -> Result<(), Error> {
    match assign {
        Assign {
            with_flags,
            lhs: Expr {
                body: Body::Reg(lhs),
                ..
            },
            rhs,
        } => gen_assign_reg(*lhs, rhs, *with_flags, w),
        Assign {
            with_flags: _,
            lhs:
                Expr {
                    body: Body::Unary(UnaryOp::Deref, addr),
                    ..
                },
            rhs,
        } => gen_store(addr, rhs, w),
        _ => todo!(),
    }
}

fn gen_assign_reg(lhs: u8, rhs: &Expr, with_flags: bool, w: &mut impl Write) -> Result<(), Error> {
    match &rhs.body {
        Body::Reg(r) => writeln!(w, "    mov{} r{lhs}, r{r}", s(with_flags))?,
        Body::Unary(UnaryOp::Deref, rhs) => {
            write!(w, "    ldr ")?;
            write_reg(lhs, w)?;
            write!(w, ", ")?;
            gen_addr(rhs, w)?;
            writeln!(w)?;
        }
        Body::Binop(a, op, b) => {
            write!(w, "    {}{} ", insn_for_binop(*op), s(with_flags))?;
            write_reg(lhs, w)?;
            write!(w, ", ")?;
            if let Body::Reg(r) = &a.body {
                write_reg(*r, w)?;
            } else {
                // TODO: rsb
                Err("left operand of binop must be reg")?;
            }
            write!(w, ", ")?;
            gen_operand2(b, w)?;
            writeln!(w)?;
        }
        _ => todo!(),
    }
    Ok(())
}

fn gen_store(addr: &Expr, rhs: &Expr, w: &mut impl Write) -> Result<(), Error> {
    if let Body::Reg(r) = &rhs.body {
        write!(w, "    str ")?;
        write_reg(*r, w)?;
        write!(w, ", ")?;
        gen_addr(addr, w)?;
        writeln!(w)?;
    } else {
        Err("store instructions only take registers")?;
    }
    Ok(())
}

fn gen_withflags(expr: &Expr, w: &mut impl Write) -> Result<(), Error> {
    match &expr.body {
        Body::Binop(a, op, b) => {
            let insn = test_insn_for_binop(*op).ok_or("unhandled binop for test")?;
            write!(w, "    {insn} ",)?;
            if let Body::Reg(r) = &a.body {
                write_reg(*r, w)?;
            } else {
                // TODO: rsb
                Err("left operand of binop must be reg")?;
            }
            write!(w, ", ")?;
            gen_operand2(b, w)?;
            writeln!(w)?;
        }
        _ => Err("test expr must be binop")?,
    }
    Ok(())
}

fn gen_operand2(expr: &Expr, w: &mut impl Write) -> Result<(), Error> {
    match &expr.body {
        Body::Reg(r) => {
            write_reg(*r, w)?;
        }
        Body::Imm(val) => {
            // we can validate it's suitable for operand2 here, or be looser for mov, add, sub
            write!(w, "#{val}")?;
        }
        _ => Err("unhandled operand2")?,
    }
    Ok(())
}

pub fn gen_stmt(stmt: &Stmt, regmap: &Regmap, w: &mut impl Write) -> Result<(), Error> {
    match stmt {
        Stmt::Label(l) => {
            writeln!(w, "{l}:")?;
        }
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

fn write_var_reg(id: &str, regmap: &Regmap, w: &mut impl Write) -> Result<(), Error> {
    if let Some(r) = parse_register(id) {
        write_reg(r, w)?;
    } else if let Some(r) = regmap.lookup(id) {
        write_reg(r, w)?;
    } else {
        Err(format!("no place for {id}"))?;
    }
    Ok(())
}

fn s(with_flags: bool) -> &'static str {
    if with_flags { "s" } else { "" }
}

#[expect(unused)]
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
    val == (val & 0xff) * 0x1_0001
        || val == (val & 0xff) * 0x0101_0101
        || val == (val & 0xff00) * 0x1_0001
        || (val << val.leading_zeros()) & 0xff_ffff == 0
}

fn insn_for_binop(binop: BinOp) -> &'static str {
    match binop {
        BinOp::Add => "add",
        BinOp::Sub => "sub",
        BinOp::Mul => "mul",
        BinOp::Div => "div",
        BinOp::And => "and",
        BinOp::Orr => "orr",
        BinOp::Eor => "eor",
        BinOp::Shl => "lsl",
        // TODO: sign
        BinOp::Shr => "lsr",
    }
}

fn test_insn_for_binop(binop: BinOp) -> Option<&'static str> {
    Some(match binop {
        BinOp::Add => "cmn",
        BinOp::Sub => "cmp",
        BinOp::And => "tst",
        BinOp::Eor => "teq",
        _ => return None,
    })
}

fn gen_addr(addr: &Expr, w: &mut impl Write) -> Result<(), Error> {
    match &addr.body {
        Body::Reg(r) => {
            write!(w, "[")?;
            write_reg(*r, w)?;
            write!(w, "]")?;
        }
        _ => todo!(),
    }
    Ok(())
}

fn write_reg(reg: u8, w: &mut impl Write) -> Result<(), Error> {
    match reg {
        13 => write!(w, "sp")?,
        14 => write!(w, "lr")?,
        15 => write!(w, "pc")?,
        _ => write!(w, "r{reg}")?,
    }
    Ok(())
}
