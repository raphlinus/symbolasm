// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Generation of FPU instructions, for the single precision FPU (FPv5-SP)
//! of the Cortex-M33.
//!
//! An assignment to an s register, a load or store of one, or a compare of
//! one comes here. f32 arithmetic is only done in s registers, and integer
//! arithmetic only in core registers; moves between the two are vmov.

use std::io::Write;

use super::{GenCtx, write_reg};
use crate::{
    error::Error,
    ifthen::IfState,
    ir::{BinOp, Body, Expr, UnaryOp},
    regmap::{FPSCR, is_sreg, reg_name},
    types::TypeHandle,
};

/// Whether an f32 can be a `vmov.f32` immediate: ±(16 to 31)/16 × 2^n for
/// n from -3 to 4, which is a sign, 3 bits of exponent and 4 of mantissa.
fn is_vmov_imm(bits: u32) -> bool {
    let exp = (bits >> 23) & 0xff;
    bits & 0x7_ffff == 0 && (124..=131).contains(&exp)
}

/// The register an operand is in, which must be an s register
fn sreg(e: &Expr, what: &str) -> Result<u8, Error> {
    match e.body {
        Body::Reg(r) if is_sreg(r) => Ok(r),
        Body::Reg(r) => Err(format!(
            "{what} works on s registers, and {} is a core register",
            reg_name(r)
        ))?,
        _ => Err(format!("{what} operands must be s registers"))?,
    }
}

/// Whether an expression reads an s register
pub fn has_sreg(e: &Expr) -> bool {
    match &e.body {
        Body::Reg(r) => is_sreg(*r),
        Body::Binop(a, _, b) => has_sreg(a) || has_sreg(b),
        Body::Unary(_, a) | Body::Field(a, _) | Body::Slice(a, _, _) | Body::Cvt(a) => has_sreg(a),
        Body::Tuple(els) | Body::Intrinsic(_, els) => els.iter().any(has_sreg),
        Body::Imm(_) | Body::Float(_) | Body::Sym(_) | Body::SymHalf(..) => false,
    }
}

/// The f32 intrinsics with one argument and their instructions
fn unary_insn(name: &str) -> Option<&'static str> {
    Some(match name {
        "abs" => "vabs.f32",
        "sqrt" => "vsqrt.f32",
        "floor" => "vrintm.f32",
        "ceil" => "vrintp.f32",
        "round" => "vrinta.f32",
        "round_ties_even" => "vrintn.f32",
        "trunc" => "vrintz.f32",
        _ => return None,
    })
}

/// The Armv8 instructions, which can't be in an IT block
fn unconditional(insn: &str) -> bool {
    [
        "vrinta", "vrintn", "vrintp", "vrintm", "vminnm", "vmaxnm", "vcvta", "vcvtn", "vcvtp",
        "vcvtm",
    ]
    .iter()
    .any(|i| insn.starts_with(i))
}

impl<W: Write> GenCtx<'_, W> {
    fn is_float(&self, ty: TypeHandle) -> bool {
        self.types.get(ty).is_float()
    }

    /// Start an FPU instruction, checking that it can be conditional if
    /// it's in an IT block
    fn start_vinsn(&mut self, insn: &str, if_state: &IfState) -> Result<(), Error> {
        if !matches!(if_state, IfState::Default) && unconditional(insn) {
            Err(format!("{insn} can't be conditional"))?;
        }
        self.start_insn(insn, if_state)
    }

    /// An instruction whose operands are all registers
    fn vinsn(&mut self, insn: &str, regs: &[u8], if_state: &IfState) -> Result<(), Error> {
        self.start_vinsn(insn, if_state)?;
        for (i, r) in regs.iter().enumerate() {
            write!(self.w, "{}", if i == 0 { " " } else { ", " })?;
            write_reg(*r, self.w)?;
        }
        writeln!(self.w)?;
        Ok(())
    }

    /// `lhs = rhs` where lhs is an s register, of type `ty`
    pub(super) fn gen_assign_sreg(
        &mut self,
        lhs: u8,
        ty: TypeHandle,
        rhs: &Expr,
        if_state: &IfState,
    ) -> Result<(), Error> {
        if lhs == FPSCR {
            return self.gen_assign_fpscr(rhs, if_state);
        }
        // Whether the arithmetic is on f32s
        let float = self.is_float(rhs.ty);
        match &rhs.body {
            Body::Reg(r) if *r == lhs && matches!(if_state, IfState::Default) => (),
            Body::Reg(r) if is_sreg(*r) => self.vinsn("vmov.f32", &[lhs, *r], if_state)?,
            Body::Reg(r) => self.vinsn("vmov", &[lhs, *r], if_state)?,
            Body::Imm(_) if self.is_float(ty) => {
                Err("an f32 constant needs a decimal point, as in 1.0")?;
            }
            Body::Float(bits) | Body::Imm(bits) => {
                if matches!(rhs.body, Body::Float(_)) && is_vmov_imm(*bits) {
                    self.start_vinsn("vmov.f32", if_state)?;
                    write!(self.w, " ")?;
                    write_reg(lhs, self.w)?;
                    writeln!(self.w, ", #{:?}", f32::from_bits(*bits))?;
                } else {
                    self.start_vinsn("vldr", if_state)?;
                    write!(self.w, " ")?;
                    write_reg(lhs, self.w)?;
                    writeln!(self.w, ", ={bits:#x}")?;
                }
            }
            Body::Unary(UnaryOp::Deref, addr) => self.gen_vldr_vstr("vldr", lhs, addr, if_state)?,
            Body::Field(base, offset) => {
                let addr = field_addr(base, *offset);
                self.gen_vldr_vstr("vldr", lhs, &addr, if_state)?
            }
            Body::Cvt(a) => self.gen_cvt(lhs, rhs.ty, a, if_state)?,
            _ if !float => Err(format!(
                "integer arithmetic is done in core registers, not {}",
                reg_name(lhs)
            ))?,
            Body::Unary(UnaryOp::Neg, a) => {
                if let Body::Binop(x, BinOp::Mul, y) = &a.body {
                    let (x, y) = (sreg(x, "vnmul")?, sreg(y, "vnmul")?);
                    self.vinsn("vnmul.f32", &[lhs, x, y], if_state)?
                } else {
                    let a = sreg(a, "vneg")?;
                    self.vinsn("vneg.f32", &[lhs, a], if_state)?
                }
            }
            // The negated multiply accumulates: -x - a * b is vnmla, and
            // a * b - x (or -x + a * b) is vnmls
            Body::Binop(acc, op @ (BinOp::Add | BinOp::Sub), b)
                if let Body::Unary(UnaryOp::Neg, acc) = &acc.body
                    && let Body::Binop(x, BinOp::Mul, y) = &b.body =>
            {
                let insn = if *op == BinOp::Sub {
                    "vnmla.f32"
                } else {
                    "vnmls.f32"
                };
                self.gen_mac(insn, lhs, acc, x, y, if_state)?
            }
            Body::Binop(a, BinOp::Sub, acc) if let Body::Binop(x, BinOp::Mul, y) = &a.body => {
                self.gen_mac("vnmls.f32", lhs, acc, x, y, if_state)?
            }
            // Multiply accumulate, which rounds twice (vfma is fma())
            Body::Binop(acc, op @ (BinOp::Add | BinOp::Sub), b)
                if let Body::Binop(x, BinOp::Mul, y) = &b.body =>
            {
                let insn = if *op == BinOp::Add {
                    "vmla.f32"
                } else {
                    "vmls.f32"
                };
                if !matches!(acc.body, Body::Reg(r) if r == lhs) {
                    let lhs = reg_name(lhs);
                    Err(format!(
                        "{insn} accumulates into its destination, so write {lhs} = {lhs} + a * b"
                    ))?;
                }
                let (x, y) = (sreg(x, insn)?, sreg(y, insn)?);
                self.vinsn(insn, &[lhs, x, y], if_state)?
            }
            Body::Binop(a, op, b) => {
                let insn = match op {
                    BinOp::Add => "vadd.f32",
                    BinOp::Sub => "vsub.f32",
                    BinOp::Mul => "vmul.f32",
                    BinOp::Div => "vdiv.f32",
                    _ => Err("only + - * / apply to f32")?,
                };
                if matches!(b.body, Body::Float(_)) || matches!(a.body, Body::Float(_)) {
                    Err(format!(
                        "{insn} has no immediate form; load the constant into an s register"
                    ))?;
                }
                let (a, b) = (sreg(a, insn)?, sreg(b, insn)?);
                self.vinsn(insn, &[lhs, a, b], if_state)?
            }
            Body::Intrinsic(name, args) => self.gen_float_intrinsic(lhs, name, args, if_state)?,
            _ => Err(format!("can't compute this in {}", reg_name(lhs)))?,
        }
        Ok(())
    }

    /// `lhs = x as f32` or `lhs = x as i32`, the conversions. The result
    /// can only go in an s register.
    pub(super) fn gen_cvt(
        &mut self,
        lhs: u8,
        ty: TypeHandle,
        a: &Expr,
        if_state: &IfState,
    ) -> Result<(), Error> {
        if !is_sreg(lhs) {
            Err(format!(
                "vcvt converts in s registers, so the result can't go in {}",
                reg_name(lhs)
            ))?;
        }
        if self.is_float(ty) {
            let src = if self.types.get(a.ty).is_signed() {
                "s32"
            } else {
                "u32"
            };
            let a = sreg(a, "vcvt")?;
            self.vinsn(&format!("vcvt.f32.{src}"), &[lhs, a], if_state)
        } else {
            self.gen_cvt_to_int(lhs, ty, a, if_state)
        }
    }

    /// An f32 to an integer: by truncation, as in Rust, or rounded the way
    /// `floor`, `ceil`, `round` or `round_ties_even` round
    fn gen_cvt_to_int(
        &mut self,
        lhs: u8,
        ty: TypeHandle,
        a: &Expr,
        if_state: &IfState,
    ) -> Result<(), Error> {
        let dst = if self.types.get(ty).is_signed() {
            "s32"
        } else {
            "u32"
        };
        let (insn, a) = match &a.body {
            Body::Intrinsic(name, args) if args.len() == 1 => match *name {
                "floor" => ("vcvtm", &args[0]),
                "ceil" => ("vcvtp", &args[0]),
                "round" => ("vcvta", &args[0]),
                "round_ties_even" => ("vcvtn", &args[0]),
                "trunc" => ("vcvt", &args[0]),
                _ => ("vcvt", a),
            },
            _ => ("vcvt", a),
        };
        let a = sreg(a, insn)?;
        self.vinsn(&format!("{insn}.{dst}.f32"), &[lhs, a], if_state)
    }

    fn gen_float_intrinsic(
        &mut self,
        lhs: u8,
        name: &str,
        args: &[Expr],
        if_state: &IfState,
    ) -> Result<(), Error> {
        if let Some(insn) = unary_insn(name) {
            let a = sreg(&args[0], insn)?;
            return self.vinsn(insn, &[lhs, a], if_state);
        }
        match name {
            "min" | "max" => {
                let insn = if name == "min" {
                    "vminnm.f32"
                } else {
                    "vmaxnm.f32"
                };
                let (a, b) = (sreg(&args[0], insn)?, sreg(&args[1], insn)?);
                self.vinsn(insn, &[lhs, a, b], if_state)
            }
            // fma(a, b, c) is a * b + c with one rounding, and c is the
            // destination. fma(-a, b, c) is vfms, fma(-a, b, -c) vfnma and
            // fma(a, b, -c) vfnms.
            "fma" => {
                let (neg_a, a) = match &args[0].body {
                    Body::Unary(UnaryOp::Neg, a) => (true, &**a),
                    _ => (false, &args[0]),
                };
                let (neg_c, c) = match &args[2].body {
                    Body::Unary(UnaryOp::Neg, c) => (true, &**c),
                    _ => (false, &args[2]),
                };
                let insn = match (neg_a, neg_c) {
                    (false, false) => "vfma.f32",
                    (true, false) => "vfms.f32",
                    (true, true) => "vfnma.f32",
                    (false, true) => "vfnms.f32",
                };
                if !matches!(c.body, Body::Reg(r) if r == lhs) {
                    let lhs = reg_name(lhs);
                    let c = if neg_c {
                        format!("-{lhs}")
                    } else {
                        lhs.clone()
                    };
                    Err(format!(
                        "{insn} accumulates into its destination, so write {lhs} = fma(a, b, {c})"
                    ))?;
                }
                let (a, b) = (sreg(a, insn)?, sreg(&args[1], insn)?);
                self.vinsn(insn, &[lhs, a, b], if_state)
            }
            "cmp" | "cmpe" => Err(format!(
                "{name} sets the FPU's flags, so write fpscr = {name}(a, b)"
            ))?,
            _ => Err(format!("{name} isn't an f32 instruction"))?,
        }
    }

    /// A negated multiply accumulate, `lhs = -acc ± x * y`, where acc
    /// must be lhs
    fn gen_mac(
        &mut self,
        insn: &str,
        lhs: u8,
        acc: &Expr,
        x: &Expr,
        y: &Expr,
        if_state: &IfState,
    ) -> Result<(), Error> {
        if !matches!(acc.body, Body::Reg(r) if r == lhs) {
            let lhs = reg_name(lhs);
            let form = if insn == "vnmla.f32" {
                format!("-{lhs} - a * b")
            } else {
                format!("a * b - {lhs}")
            };
            Err(format!(
                "{insn} accumulates into its destination, so write {lhs} = {form}"
            ))?;
        }
        let (x, y) = (sreg(x, insn)?, sreg(y, insn)?);
        self.vinsn(insn, &[lhs, x, y], if_state)
    }

    /// `vldr` or `vstr` of an s register, at a core register plus a
    /// multiple of 4 up to 1020, or at a label
    pub(super) fn gen_vldr_vstr(
        &mut self,
        insn: &str,
        reg: u8,
        addr: &Expr,
        if_state: &IfState,
    ) -> Result<(), Error> {
        let (base, offset) = match &addr.body {
            Body::Sym(sym) if insn == "vldr" => {
                self.start_vinsn(insn, if_state)?;
                write!(self.w, " ")?;
                write_reg(reg, self.w)?;
                writeln!(self.w, ", {sym}")?;
                return Ok(());
            }
            Body::Sym(_) => {
                Err("cannot store to a symbol directly; load its address into a register")?
            }
            Body::Reg(r) => (*r, 0),
            Body::Binop(a, op @ (BinOp::Add | BinOp::Sub), b)
                if let (Body::Reg(r), Body::Imm(n)) = (&a.body, &b.body) =>
            {
                let n = if *op == BinOp::Sub {
                    n.wrapping_neg()
                } else {
                    *n
                };
                (*r, n as i32)
            }
            Body::Binop(_, BinOp::Add, _) => Err(format!(
                "{insn} has no register offset form; add the offset first"
            ))?,
            _ => Err(format!("unsupported address for {insn}"))?,
        };
        if is_sreg(base) {
            Err(format!(
                "an address must be in a core register, not {}",
                reg_name(base)
            ))?;
        }
        if offset % 4 != 0 || !(-1020..=1020).contains(&offset) {
            Err(format!(
                "{insn} offset {offset} must be a multiple of 4 from -1020 to 1020"
            ))?;
        }
        self.start_vinsn(insn, if_state)?;
        write!(self.w, " ")?;
        write_reg(reg, self.w)?;
        write!(self.w, ", [")?;
        write_reg(base, self.w)?;
        if offset != 0 {
            write!(self.w, ", #{offset}")?;
        }
        writeln!(self.w, "]")?;
        Ok(())
    }

    /// `vldm` or `vstm` of consecutive s registers, ascending from the
    /// address in a core register, which `incr` updates
    pub(super) fn gen_vldm_vstm(
        &mut self,
        insn: &str,
        regs: &[u8],
        addr: &Expr,
        incr: i32,
        if_state: &IfState,
    ) -> Result<(), Error> {
        let Body::Reg(base) = addr.body else {
            return Err(format!(
                "{insn} takes an address in a register, with no offset"
            ))?;
        };
        check_consecutive(insn, regs)?;
        if incr != 0 && incr != 4 * regs.len() as i32 {
            Err(format!(
                "{insn} can only update the address past the registers it transfers"
            ))?;
        }
        self.start_vinsn(insn, if_state)?;
        write!(self.w, " ")?;
        write_reg(base, self.w)?;
        if incr != 0 {
            write!(self.w, "!")?;
        }
        write!(self.w, ", {{")?;
        write_reglist(regs, self.w)?;
        writeln!(self.w, "}}")?;
        Ok(())
    }

    /// `fpscr = cmp(a, b)` or `cmpe(a, b)`: vcmp or vcmpe, which set the
    /// FPU's flags. `#(fpscr)` copies them to the APSR for conditions.
    fn gen_vcmp(&mut self, insn: &str, args: &[Expr], if_state: &IfState) -> Result<(), Error> {
        let [a, b] = args else { unreachable!() };
        let insn = format!("v{insn}.f32");
        let a = sreg(a, &insn)?;
        self.start_vinsn(&insn, if_state)?;
        write!(self.w, " ")?;
        write_reg(a, self.w)?;
        match b.body {
            Body::Float(0) | Body::Imm(0) => writeln!(self.w, ", #0")?,
            Body::Reg(r) if is_sreg(r) => {
                write!(self.w, ", ")?;
                write_reg(r, self.w)?;
                writeln!(self.w)?;
            }
            _ => Err(format!("{insn} compares with an s register or 0.0"))?,
        }
        Ok(())
    }

    /// An assignment to fpscr: a compare, or vmsr from a core register
    fn gen_assign_fpscr(&mut self, rhs: &Expr, if_state: &IfState) -> Result<(), Error> {
        match &rhs.body {
            Body::Intrinsic(insn @ ("cmp" | "cmpe"), args) => self.gen_vcmp(insn, args, if_state),
            Body::Reg(r) if !is_sreg(*r) && *r != FPSCR => {
                self.start_insn("vmsr", if_state)?;
                write!(self.w, " fpscr, ")?;
                write_reg(*r, self.w)?;
                writeln!(self.w)?;
                Ok(())
            }
            _ => Err("fpscr takes cmp(a, b), cmpe(a, b) or a core register")?,
        }
    }

    /// `vpush` or `vpop` of consecutive s registers
    pub(super) fn gen_vpush_vpop(
        &mut self,
        insn: &str,
        regs: &[u8],
        if_state: &IfState,
    ) -> Result<(), Error> {
        check_consecutive(insn, regs)?;
        self.start_insn(insn, if_state)?;
        write!(self.w, " {{")?;
        write_reglist(regs, self.w)?;
        writeln!(self.w, "}}")?;
        Ok(())
    }
}

/// The address of a struct field, `base + offset`
fn field_addr(base: &Expr, offset: usize) -> Expr {
    let offset = Expr {
        ty: TypeHandle::default(),
        body: Body::Imm(offset as u32),
    };
    Expr {
        ty: base.ty,
        body: Body::Binop(base.clone().into(), BinOp::Add, offset.into()),
    }
}

fn check_consecutive(insn: &str, regs: &[u8]) -> Result<(), Error> {
    if regs.is_empty() {
        Err(format!("{insn} needs at least one register"))?;
    }
    if !regs.iter().all(|r| is_sreg(*r)) {
        Err(format!("{insn} takes s registers, not core registers"))?;
    }
    if regs.windows(2).any(|w| w[1] != w[0] + 1) {
        Err(format!(
            "{insn} registers must be consecutive and ascending"
        ))?;
    }
    Ok(())
}

fn write_reglist(regs: &[u8], w: &mut impl Write) -> Result<(), Error> {
    for (i, r) in regs.iter().enumerate() {
        if i > 0 {
            write!(w, ", ")?;
        }
        write_reg(*r, w)?;
    }
    Ok(())
}
