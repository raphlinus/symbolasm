//! Generation of assembly language.
//!
//! This approach is clunky, and is going to run into problems when types are
//! needed.

use std::io::Write;

use crate::{
    ir::{Assign, BinOp, Body, Expr, Ir, UnaryOp},
    lex::{Error, TokBody, Token},
    regmap::{Regmap, parse_register},
    stmt::Stmt,
    types::{Type, TypeHandle, TypePool},
};

pub struct GenCtx<'a, W: Write> {
    types: &'a mut TypePool,
    w: &'a mut W,
}

#[derive(Clone, Copy, PartialEq)]
enum WithFlags {
    No,
    DontCare,
    Yes,
}

impl<'a, W: Write> GenCtx<'a, W> {
    pub fn new(types: &'a mut TypePool, w: &'a mut W) -> Self {
        Self { types, w }
    }

    pub fn gen_from_ir(&mut self, ir: &Ir) -> Result<(), Error> {
        //println!("{ir:?}");
        match ir {
            Ir::Assign(assign) => self.gen_assign(assign),
            Ir::WithFlags(expr) => self.gen_withflags(expr),
        }
    }

    fn gen_assign(&mut self, assign: &Assign) -> Result<(), Error> {
        match assign {
            Assign {
                with_flags,
                lhs:
                    Expr {
                        body: Body::Reg(lhs),
                        ..
                    },
                rhs,
            } => self.gen_assign_reg(*lhs, rhs, (*with_flags).into()),
            Assign {
                with_flags: _,
                lhs:
                    Expr {
                        body: Body::Unary(UnaryOp::Deref, addr),
                        ..
                    },
                rhs,
            } => self.gen_store(addr, rhs),
            Assign {
                with_flags: _,
                lhs:
                    Expr {
                        body: Body::Field(base, field),
                        ty,
                    },
                rhs,
            } => self.gen_store_field(*ty, base, *field, rhs),
            _ => todo!(),
        }
    }

    fn gen_assign_reg(&mut self, lhs: u8, rhs: &Expr, with_flags: WithFlags) -> Result<(), Error> {
        match &rhs.body {
            Body::Reg(r) => {
                write!(self.w, "    mov{} ", s(with_flags))?;
                write_reg(lhs, self.w)?;
                write!(self.w, ", ")?;
                write_reg(*r, self.w)?;
                writeln!(self.w)?;
            }
            Body::Imm(n) => {
                // TODO: more sophistication about immediate size
                // can be mvn if !n is imm12
                // falls back to ldr
                if *n < 0x1_0000 || is_imm8m(*n) {
                    write!(self.w, "    mov{} ", s(with_flags))?;
                    write_reg(lhs, self.w)?;
                    writeln!(self.w, ", #{n}")?;
                } else if is_imm8m(!n) {
                    write!(self.w, "    mvn{} ", s(with_flags))?;
                    write_reg(lhs, self.w)?;
                    writeln!(self.w, ", #{}", !n)?;
                } else {
                    // TODO: fail if with_flags
                    write!(self.w, "    ldr ")?;
                    write_reg(lhs, self.w)?;
                    writeln!(self.w, ", ={n}")?;
                }
            }
            Body::Unary(UnaryOp::Deref, rhs) => {
                let insn = ldr_for_ty(self.types.pointee(rhs.ty));
                write!(self.w, "    {insn} ")?;
                write_reg(lhs, self.w)?;
                write!(self.w, ", ")?;
                self.gen_addr(rhs)?;
                writeln!(self.w)?;
            }
            Body::Unary(UnaryOp::Neg, rhs) => {
                if let Body::Reg(r) = &rhs.body {
                    write!(self.w, "    rsb{} ", s(with_flags))?;
                    write_reg(lhs, self.w)?;
                    write!(self.w, ", ")?;
                    write_reg(*r, self.w)?;
                    writeln!(self.w, ", #0")?;
                } else {
                    Err("negation must be applied to register")?;
                }
            }
            Body::Unary(UnaryOp::Not, rhs) => {
                write!(self.w, "    mvn{} ", s(with_flags))?;
                write_reg(lhs, self.w)?;
                write!(self.w, ", ")?;
                self.gen_operand2(rhs)?;
                writeln!(self.w)?;
            }
            Body::Binop(a, op, b) => {
                let ty = self.types.get(a.ty);
                write!(self.w, "    {}{} ", insn_for_binop(*op, ty), s(with_flags))?;
                write_reg(lhs, self.w)?;
                write!(self.w, ", ")?;
                if let Body::Reg(r) = &a.body {
                    write_reg(*r, self.w)?;
                } else {
                    // TODO: rsb
                    Err("left operand of binop must be reg")?;
                }
                write!(self.w, ", ")?;
                // TODO: shifts aren't operand2; add and sub allow imm12
                // also match orn and bic
                self.gen_operand2(b)?;
                writeln!(self.w)?;
            }
            Body::Field(base, offset) => {
                let insn = ldr_for_ty(Some(self.types.get(rhs.ty)));
                if let Body::Reg(r) = &base.body {
                    write!(self.w, "    {insn} ")?;
                    write_reg(lhs, self.w)?;
                    write!(self.w, ", [")?;
                    write_reg(*r, self.w)?;
                    write!(self.w, ", #{offset}]")?;
                    writeln!(self.w)?;
                } else {
                    Err("base must be register")?;
                }
            }
            _ => todo!(),
        }
        Ok(())
    }

    fn gen_store(&mut self, addr: &Expr, rhs: &Expr) -> Result<(), Error> {
        if let Body::Reg(r) = &rhs.body {
            let insn = str_for_ty(self.types.pointee(addr.ty));
            write!(self.w, "    {insn} ")?;
            write_reg(*r, self.w)?;
            write!(self.w, ", ")?;
            self.gen_addr(addr)?;
            writeln!(self.w)?;
        } else {
            Err("store instructions only take registers")?;
        }
        Ok(())
    }

    fn gen_store_field(
        &mut self,
        ty: TypeHandle,
        base: &Expr,
        offset: usize,
        rhs: &Expr,
    ) -> Result<(), Error> {
        if let Body::Reg(rn) = &base.body
            && let Body::Reg(rd) = &rhs.body
        {
            let insn = str_for_ty(Some(self.types.get(ty)));
            write!(self.w, "    {insn} ")?;
            write_reg(*rd, self.w)?;
            write!(self.w, ", [")?;
            write_reg(*rn, self.w)?;
            write!(self.w, ", #{offset}]")?;
            writeln!(self.w)?;
        } else {
            Err("base and value must both be registers")?;
        }
        Ok(())
    }

    fn gen_withflags(&mut self, expr: &Expr) -> Result<(), Error> {
        match &expr.body {
            Body::Binop(a, op, b) => {
                let insn = test_insn_for_binop(*op).ok_or("unhandled binop for test")?;
                write!(self.w, "    {insn} ",)?;
                if let Body::Reg(r) = &a.body {
                    write_reg(*r, self.w)?;
                } else {
                    // TODO: rsb
                    Err("left operand of binop must be reg")?;
                }
                write!(self.w, ", ")?;
                self.gen_operand2(b)?;
                writeln!(self.w)?;
            }
            _ => Err("test expr must be binop")?,
        }
        Ok(())
    }

    fn gen_operand2(&mut self, expr: &Expr) -> Result<(), Error> {
        match &expr.body {
            Body::Reg(r) => write_reg(*r, self.w)?,
            Body::Imm(val) => {
                // we can validate it's suitable for operand2 here, or be looser for mov, add, sub
                write!(self.w, "#{val}")?;
            }
            Body::Binop(lhs, op, rhs) => {
                let opsh = match op {
                    BinOp::Shl => "lsl",
                    BinOp::Shr => {
                        if self.types.get(lhs.ty).is_signed() {
                            "asr"
                        } else {
                            "lsr"
                        }
                    }
                    _ => Err("invalid binop in operand2")?,
                };
                let Body::Reg(basereg) = &lhs.body else {
                    return Err("lhs of operand2 must be register")?;
                };
                write_reg(*basereg, self.w)?;
                write!(self.w, ", {opsh} ")?;
                self.gen_shift_amt(rhs)?;
            }
            _ => Err("unhandled operand2")?,
        }
        Ok(())
    }

    fn gen_shift_amt(&mut self, expr: &Expr) -> Result<(), Error> {
        match &expr.body {
            Body::Reg(r) => write_reg(*r, self.w)?,
            Body::Imm(val) => write!(self.w, "#{val}")?,
            _ => Err("invalid shift amount")?,
        }
        Ok(())
    }

    fn gen_addr(&mut self, addr: &Expr) -> Result<(), Error> {
        match &addr.body {
            Body::Reg(r) => {
                write!(self.w, "[")?;
                write_reg(*r, self.w)?;
                write!(self.w, "]")?;
            }
            _ => todo!(),
        }
        Ok(())
    }
}

// This should probably also move into GenCtx, but will still take regmap
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
            crate::stmt::Insn::Cbz(reg, target) => {
                if let Some(id) = reg.as_ident() {
                    write!(w, "    cbz ")?;
                    write_var_reg(id, regmap, w)?;
                    writeln!(w, ", {target}")?;
                } else {
                    Err("bx target must be register")?;
                }
            }
            crate::stmt::Insn::Cbnz(reg, target) => {
                if let Some(id) = reg.as_ident() {
                    write!(w, "    cbnz ")?;
                    write_var_reg(id, regmap, w)?;
                    writeln!(w, ", {target}")?;
                } else {
                    Err("bx target must be register")?;
                }
            }
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

fn s(with_flags: WithFlags) -> &'static str {
    if with_flags == WithFlags::Yes {
        "s"
    } else {
        ""
    }
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

fn insn_for_binop(binop: BinOp, ty: &Type) -> &'static str {
    match binop {
        BinOp::Add => "add",
        BinOp::Sub => "sub",
        BinOp::Mul => "mul",
        BinOp::Div => "div",
        BinOp::And => "and",
        BinOp::Orr => "orr",
        BinOp::Eor => "eor",
        BinOp::Shl => "lsl",
        BinOp::Shr => {
            if ty.is_signed() {
                "asr"
            } else {
                "lsr"
            }
        }
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

fn write_reg(reg: u8, w: &mut impl Write) -> Result<(), Error> {
    match reg {
        13 => write!(w, "sp")?,
        14 => write!(w, "lr")?,
        15 => write!(w, "pc")?,
        _ => write!(w, "r{reg}")?,
    }
    Ok(())
}

impl From<bool> for WithFlags {
    fn from(value: bool) -> Self {
        if value { WithFlags::Yes } else { WithFlags::No }
    }
}

fn ldr_for_ty(ty: Option<&Type>) -> &'static str {
    match ty {
        Some(Type::U8) => "ldrb",
        Some(Type::U16) => "ldrh",
        Some(Type::I8) => "ldrsb",
        Some(Type::I16) => "ldrsh",
        _ => "ldr",
    }
}

fn str_for_ty(ty: Option<&Type>) -> &'static str {
    match ty {
        Some(Type::U8 | Type::I8) => "strb",
        Some(Type::U16 | Type::I16) => "strh",
        _ => "str",
    }
}
