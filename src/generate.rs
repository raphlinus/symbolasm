// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Generation of assembly language.
//!
//! This approach is clunky, and is going to run into problems when types are
//! needed.

use std::{io::Write, ops::Deref};

use crate::{
    error::Error,
    ifthen::IfState,
    ir::{Assign, BinOp, Body, Expr, Half, Ir, UnaryOp},
    lex::{TokBody, Token},
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

    pub fn gen_from_ir(&mut self, ir: &Ir, if_state: &IfState) -> Result<(), Error> {
        //println!("{ir:?}");
        match ir {
            Ir::Assign(assign) => self.gen_assign(assign, if_state),
            Ir::WithFlags(expr) => self.gen_withflags(expr, if_state),
            Ir::WithAddrUpdate(ir, inc) => self.gen_addr_update(ir, *inc, if_state),
        }
    }

    fn gen_assign(&mut self, assign: &Assign, if_state: &IfState) -> Result<(), Error> {
        match assign {
            Assign {
                with_flags,
                lhs:
                    Expr {
                        body: Body::Reg(lhs),
                        ..
                    },
                rhs,
            } => self.gen_assign_reg(*lhs, rhs, (*with_flags).into(), if_state),
            Assign {
                with_flags: _,
                lhs:
                    Expr {
                        body: Body::Unary(UnaryOp::Deref, addr),
                        ..
                    },
                rhs,
            } => self.gen_store(addr, rhs, 0, if_state),
            Assign {
                with_flags: _,
                lhs:
                    Expr {
                        body: Body::Field(base, field),
                        ty,
                    },
                rhs,
            } => self.gen_store_field(*ty, base, *field, rhs, if_state),
            Assign {
                with_flags: _,
                lhs:
                    Expr {
                        body: Body::Slice(a, start, end),
                        ..
                    },
                rhs,
            } => self.gen_assign_slice(a, *start, *end, rhs, if_state),
            Assign {
                with_flags: _,
                lhs:
                    Expr {
                        body: Body::Tuple(els),
                        ..
                    },
                rhs,
            } => self.gen_load_tuple(els, rhs, if_state),
            _ => todo!(),
        }
    }

    fn gen_assign_reg(
        &mut self,
        lhs: u8,
        rhs: &Expr,
        with_flags: WithFlags,
        if_state: &IfState,
    ) -> Result<(), Error> {
        match &rhs.body {
            // A move to the same register only renames, so emits nothing.
            // Inside an IT block it is kept, since the block's instruction
            // count was computed per statement.
            Body::Reg(r)
                if *r == lhs
                    && with_flags != WithFlags::Yes
                    && matches!(if_state, IfState::Default) => {}
            Body::Reg(r) => {
                self.start_insn_flags("mov", with_flags, if_state)?;
                write!(self.w, " ")?;
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
                    self.start_insn_flags("mov", with_flags, if_state)?;
                    write!(self.w, " ")?;
                    write_reg(lhs, self.w)?;
                    writeln!(self.w, ", #{n}")?;
                } else if is_imm8m(!n) {
                    self.start_insn_flags("mvn", with_flags, if_state)?;
                    write!(self.w, " ")?;
                    write_reg(lhs, self.w)?;
                    writeln!(self.w, ", #{}", !n)?;
                } else {
                    // TODO: fail if with_flags
                    self.start_insn("ldr", if_state)?;
                    write!(self.w, " ")?;
                    write_reg(lhs, self.w)?;
                    writeln!(self.w, ", ={n}")?;
                }
            }
            Body::Sym(sym) => {
                // adr would be shorter, but GNU as rejects it for symbols not
                // defined in the same file, and its range is only +/-4095.
                if with_flags == WithFlags::Yes {
                    Err("loading a symbol address cannot set flags")?;
                }
                self.start_insn("ldr", if_state)?;
                write!(self.w, " ")?;
                write_reg(lhs, self.w)?;
                writeln!(self.w, ", ={sym}")?;
            }
            Body::SymHalf(half, sym) => {
                if with_flags == WithFlags::Yes {
                    Err("movw cannot set flags")?;
                }
                self.start_insn("movw", if_state)?;
                write!(self.w, " ")?;
                write_reg(lhs, self.w)?;
                writeln!(self.w, ", #:{}:{sym}", half_reloc(*half))?;
            }
            Body::Unary(UnaryOp::Deref, rhs) => {
                let insn = ldr_for_ty(self.types.pointee(rhs.ty));
                self.start_insn(insn, if_state)?;
                write!(self.w, " ")?;
                write_reg(lhs, self.w)?;
                write!(self.w, ", ")?;
                self.gen_addr(rhs, 0)?;
                writeln!(self.w)?;
            }
            Body::Unary(UnaryOp::Neg, rhs) => {
                if let Body::Reg(r) = &rhs.body {
                    self.start_insn_flags("rsb", with_flags, if_state)?;
                    write!(self.w, " ")?;
                    write_reg(lhs, self.w)?;
                    write!(self.w, ", ")?;
                    write_reg(*r, self.w)?;
                    writeln!(self.w, ", #0")?;
                } else {
                    Err("negation must be applied to register")?;
                }
            }
            Body::Unary(UnaryOp::Not, rhs) => {
                self.start_insn_flags("mvn", with_flags, if_state)?;
                write!(self.w, " ")?;
                write_reg(lhs, self.w)?;
                write!(self.w, ", ")?;
                self.gen_operand2(rhs)?;
                writeln!(self.w)?;
            }
            Body::Binop(a, op, b) => {
                let ty = self.types.get(a.ty);
                let mut insn = insn_for_binop(*op, ty);
                let mut rhs = b.deref();
                if let Body::Unary(UnaryOp::Not, b) = &rhs.body {
                    insn = match op {
                        BinOp::And => "bic",
                        BinOp::Orr => "orn",
                        _ => Err("unary not in rhs only works with and/or")?,
                    };
                    rhs = b.deref();
                }
                self.start_insn_flags(insn, with_flags, if_state)?;
                write!(self.w, " ")?;
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
                self.gen_operand2(rhs)?;
                writeln!(self.w)?;
            }
            Body::Field(base, offset) => {
                let insn = ldr_for_ty(Some(self.types.get(rhs.ty)));
                if let Body::Reg(r) = &base.body {
                    self.start_insn_flags(insn, with_flags, if_state)?;
                    write!(self.w, " ")?;
                    write_reg(lhs, self.w)?;
                    write!(self.w, ", [")?;
                    write_reg(*r, self.w)?;
                    write!(self.w, ", #{offset}]")?;
                    writeln!(self.w)?;
                } else {
                    Err("base must be register")?;
                }
            }
            Body::Slice(a, start, end) => {
                // TODO: handle 0..8 and 0..16 (shorter encodings)
                let insn = if self.types.get(a.ty).is_signed() {
                    "sbfx"
                } else {
                    "ubfx"
                };
                // TODO: ensure end > start, otherwise error
                let width = end - start;
                if let Body::Reg(r) = &a.body {
                    self.start_insn_flags(insn, with_flags, if_state)?;
                    write!(self.w, " ")?;
                    write_reg(lhs, self.w)?;
                    write!(self.w, ", ")?;
                    write_reg(*r, self.w)?;
                    writeln!(self.w, ", #{start}, #{width}")?;
                } else {
                    Err("slice source must be register")?;
                }
            }
            _ => todo!(),
        }
        Ok(())
    }

    fn gen_assign_slice(
        &mut self,
        a: &Expr,
        start: usize,
        end: usize,
        rhs: &Expr,
        if_state: &IfState,
    ) -> Result<(), Error> {
        if let Body::Reg(r) = &a.body {
            // TODO: ensure end > start, otherwise error
            let width = end - start;
            let is_top_half = start == 16 && end == 32;
            match &rhs.body {
                Body::Imm(n) if *n != 0 => {
                    if !is_top_half || *n > 0xffff {
                        Err(
                            "only 16 bit immediates into [16..32] (movt), otherwise slices can only be cleared",
                        )?;
                    }
                    self.start_insn("movt", if_state)?;
                    write!(self.w, " ")?;
                    write_reg(*r, self.w)?;
                    writeln!(self.w, ", #{n}")?;
                }
                Body::SymHalf(half, sym) => {
                    if !is_top_half {
                        Err("symbol halves can only be assigned to [16..32] (movt)")?;
                    }
                    self.start_insn("movt", if_state)?;
                    write!(self.w, " ")?;
                    write_reg(*r, self.w)?;
                    writeln!(self.w, ", #:{}:{sym}", half_reloc(*half))?;
                }
                Body::Imm(_) => {
                    self.start_insn("bfc", if_state)?;
                    write!(self.w, " ")?;
                    write_reg(*r, self.w)?;
                    writeln!(self.w, ", #{start}, #{width}")?;
                }
                Body::Reg(rhs) => {
                    self.start_insn("bfi", if_state)?;
                    write!(self.w, " ")?;
                    write_reg(*r, self.w)?;
                    write!(self.w, ", ")?;
                    write_reg(*rhs, self.w)?;
                    writeln!(self.w, ", #{start}, #{width}")?;
                }
                _ => todo!(),
            }
        } else {
            Err("slice assignment must be to register")?;
        }
        Ok(())
    }

    fn gen_store(
        &mut self,
        addr: &Expr,
        rhs: &Expr,
        incr: i32,
        if_state: &IfState,
    ) -> Result<(), Error> {
        if let Body::Sym(_) = &addr.body {
            Err("cannot store to a symbol directly; load its address into a register")?;
        }
        match &rhs.body {
            Body::Reg(r) => {
                let insn = str_for_ty(self.types.pointee(addr.ty));
                self.start_insn(insn, if_state)?;
                write!(self.w, " ")?;
                write_reg(*r, self.w)?;
                write!(self.w, ", ")?;
                self.gen_addr(addr, incr)?;
                writeln!(self.w)?;
            }
            Body::Tuple(els) => {
                self.gen_loadstore_tuple(els, addr, incr, if_state, "st")?;
            }
            _ => {
                println!("{rhs:?}");
                Err("store instructions only take registers")?;
            }
        }
        Ok(())
    }

    fn gen_load_tuple(
        &mut self,
        els: &[Expr],
        rhs: &Expr,
        if_state: &IfState,
    ) -> Result<(), Error> {
        if let Body::Unary(UnaryOp::Deref, addr) = &rhs.body {
            self.gen_loadstore_tuple(els, addr, 0, if_state, "ld")
        } else {
            Err("tuple load must be from deref of address")?
        }
    }

    fn gen_loadstore_tuple(
        &mut self,
        els: &[Expr],
        addr: &Expr,
        incr: i32,
        if_state: &IfState,
        op: &str,
    ) -> Result<(), Error> {
        // multiple store; can be either stm or strd
        // try stm first (has a chance at 16 bit encoding)
        // TODO: allow decrement also
        let mut stm_ok =
            matches!(addr.body, Body::Reg(_)) && (incr == 0 || incr == 4 * els.len() as i32);
        let mut last_r = None;
        for el in els {
            let Body::Reg(r) = &el.body else {
                return Err("tuple element for store must be register")?;
            };
            if let Some(last_r) = last_r
                && r <= last_r
            {
                stm_ok = false;
            }
            last_r = Some(r);
        }
        if stm_ok {
            self.start_insn(&format!("{op}m"), if_state)?;
            write!(self.w, " ")?;
            if let Body::Reg(addr_r) = &addr.body {
                write_reg(*addr_r, self.w)?;
            }
            if incr != 0 {
                write!(self.w, "!")?;
            }
            write!(self.w, ", {{")?;
            let mut comma = false;
            for el in els {
                if comma {
                    write!(self.w, ", ")?;
                }
                if let Body::Reg(r) = &el.body {
                    write_reg(*r, self.w)?;
                }
                comma = true;
            }
            writeln!(self.w, "}}")?;
            return Ok(());
        }
        // Now try strd
        if els.len() != 2 {
            Err("tuple store not eligible for stm, strd only does pairs")?;
        }
        if let Body::Binop(_, _, offset) = &addr.body
            && !matches!(offset.body, Body::Imm(_))
        {
            Err(format!(
                "{op}rd has no register offset form in Thumb; only immediate offsets"
            ))?;
        }
        self.start_insn(&format!("{op}rd"), if_state)?;
        write!(self.w, " ")?;
        for el in els {
            if let Body::Reg(r) = &el.body {
                write_reg(*r, self.w)?;
            }
            write!(self.w, ", ")?;
        }
        self.gen_addr(addr, incr)?;
        writeln!(self.w)?;
        Ok(())
    }

    fn gen_store_field(
        &mut self,
        ty: TypeHandle,
        base: &Expr,
        offset: usize,
        rhs: &Expr,
        if_state: &IfState,
    ) -> Result<(), Error> {
        if let Body::Reg(rn) = &base.body
            && let Body::Reg(rd) = &rhs.body
        {
            let insn = str_for_ty(Some(self.types.get(ty)));
            self.start_insn(insn, if_state)?;
            write!(self.w, " ")?;
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

    fn gen_withflags(&mut self, expr: &Expr, if_state: &IfState) -> Result<(), Error> {
        match &expr.body {
            Body::Binop(a, op, b) => {
                let insn = test_insn_for_binop(*op).ok_or("unhandled binop for test")?;
                self.start_insn(insn, if_state)?;
                write!(self.w, " ")?;
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

    fn gen_addr_update(&mut self, ir: &Ir, incr: i32, if_state: &IfState) -> Result<(), Error> {
        let Ir::Assign(Assign { lhs, rhs, .. }) = ir else {
            return Err("statement with addr update must be assignment")?;
        };
        if let Body::Unary(UnaryOp::Deref, addr) = &lhs.body {
            self.gen_store(addr, rhs, incr, if_state)
        } else if let Body::Unary(UnaryOp::Deref, addr) = &rhs.body {
            if let Body::Reg(r) = &lhs.body {
                let insn = ldr_for_ty(self.types.pointee(addr.ty));
                self.start_insn(insn, if_state)?;
                write!(self.w, " ")?;
                write_reg(*r, self.w)?;
                write!(self.w, ", ")?;
                self.gen_addr(addr, incr)?;
                writeln!(self.w)?;
                Ok(())
            } else if let Body::Tuple(els) = &lhs.body {
                self.gen_loadstore_tuple(els, addr, incr, if_state, "ld")
            } else {
                Err("load must assign to register")?
            }
        } else {
            Err("addr update must be either load or store")?
        }
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

    fn gen_addr(&mut self, addr: &Expr, incr: i32) -> Result<(), Error> {
        match &addr.body {
            Body::Sym(sym) => {
                // PC-relative literal load
                if incr != 0 {
                    Err("cannot update a symbol address")?;
                }
                write!(self.w, "{sym}")?;
            }
            Body::Reg(r) => {
                write!(self.w, "[")?;
                write_reg(*r, self.w)?;
                write!(self.w, "]")?;
                if incr != 0 {
                    write!(self.w, ", #{incr}")?;
                }
            }
            Body::Binop(lhs, BinOp::Add, rhs) => {
                // TODO: incr must be 0
                let Body::Reg(lhs_reg) = &lhs.body else {
                    return Err("left addend must be register")?;
                };
                match &rhs.body {
                    Body::Reg(rhs_reg) => {
                        write!(self.w, "[")?;
                        write_reg(*lhs_reg, self.w)?;
                        write!(self.w, ", ")?;
                        write_reg(*rhs_reg, self.w)?;
                        write!(self.w, "]")?;
                    }
                    Body::Imm(n) => {
                        // TODO: validation of immediate
                        write!(self.w, "[")?;
                        write_reg(*lhs_reg, self.w)?;
                        write!(self.w, ", #{n}]")?;
                    }
                    Body::Binop(offset, BinOp::Shl, shift) => {
                        if let Body::Reg(offset_reg) = &offset.body
                            && let Body::Imm(shift) = &shift.body
                        {
                            // TODO: validate (somewhere) that shift is 0..=3
                            write!(self.w, "[")?;
                            write_reg(*lhs_reg, self.w)?;
                            write!(self.w, ", ")?;
                            write_reg(*offset_reg, self.w)?;
                            write!(self.w, ", lsl #{shift}]")?;
                        } else {
                            Err("must be register left shifted by immediate")?;
                        }
                    }
                    _ => todo!(),
                }
            }
            _ => Err("unsupported address expression")?,
        }
        Ok(())
    }

    pub fn gen_stmt(
        &mut self,
        stmt: &Stmt,
        regmap: &Regmap,
        if_state: &IfState,
    ) -> Result<(), Error> {
        match stmt {
            Stmt::Label(l) => {
                writeln!(self.w, "{l}:")?;
            }
            Stmt::Insn(insn) => match insn {
                crate::stmt::Insn::Bx(target) => {
                    if let Some(id) = target.as_ident() {
                        self.start_insn("bx", if_state)?;
                        write!(self.w, " ")?;
                        write_var_reg(id, regmap, self.w)?;
                        writeln!(self.w)?;
                    } else {
                        Err("bx target must be register")?;
                    }
                }
                crate::stmt::Insn::BCond(cond, target) => {
                    let insn = format!("b{cond}");
                    self.start_insn(&insn, if_state)?;
                    writeln!(self.w, " {target}")?;
                }
                crate::stmt::Insn::B(target) => {
                    self.start_insn("b", if_state)?;
                    writeln!(self.w, " {target}")?;
                }
                crate::stmt::Insn::Push(regs) => {
                    self.gen_reglist("push", regs, regmap, if_state)?
                }
                crate::stmt::Insn::Pop(regs) => self.gen_reglist("pop", regs, regmap, if_state)?,
                crate::stmt::Insn::Bl(target) => {
                    self.start_insn("bl", if_state)?;
                    writeln!(self.w, " {target}")?;
                }
                crate::stmt::Insn::Cbz(reg, target) => {
                    if let Some(id) = reg.as_ident() {
                        self.start_insn("cbz", if_state)?;
                        write!(self.w, " ")?;
                        write_var_reg(id, regmap, self.w)?;
                        writeln!(self.w, ", {target}")?;
                    } else {
                        Err("bx target must be register")?;
                    }
                }
                crate::stmt::Insn::Cbnz(reg, target) => {
                    if let Some(id) = reg.as_ident() {
                        self.start_insn("cbnz", if_state)?;
                        write!(self.w, " ")?;
                        write_var_reg(id, regmap, self.w)?;
                        writeln!(self.w, ", {target}")?;
                    } else {
                        Err("bx target must be register")?;
                    }
                }
            },
            Stmt::StartIf(cond) => {
                if let Some(cond) = cond.as_ident() {
                    self.start_insn("", if_state)?;
                    writeln!(self.w, " {cond}")?;
                }
            }
            Stmt::Else | Stmt::EndBlock => (),
            _ => todo!("nyi"),
        }
        Ok(())
    }

    fn gen_reglist(
        &mut self,
        insn: &str,
        regs: &[crate::parse::Expr],
        regmap: &Regmap,
        if_state: &IfState,
    ) -> Result<(), Error> {
        let mut nums = vec![];
        for r in regs {
            let id = r
                .as_ident()
                .ok_or(format!("{insn} arguments must be registers"))?;
            let n = parse_register(id)
                .or_else(|| regmap.lookup(id))
                .ok_or(format!("no place for {id}"))?;
            if nums.last().is_some_and(|&last| n <= last) {
                Err(format!("{insn} registers must be in ascending order"))?;
            }
            nums.push(n);
        }
        if nums.is_empty() {
            Err(format!("{insn} needs at least one register"))?;
        }
        self.start_insn(insn, if_state)?;
        write!(self.w, " {{")?;
        for (i, n) in nums.iter().enumerate() {
            if i > 0 {
                write!(self.w, ", ")?;
            }
            write_reg(*n, self.w)?;
        }
        writeln!(self.w, "}}")?;
        Ok(())
    }

    fn start_insn(&mut self, insn: &str, if_state: &IfState) -> Result<(), Error> {
        match if_state {
            IfState::Default => write!(self.w, "    {insn}")?,
            IfState::Ift(items) => {
                write!(self.w, "    i")?;
                for is_then in items {
                    let c = if *is_then { "t" } else { "e" };
                    write!(self.w, "{c}")?;
                }
            }
            IfState::Then(cond) => write!(self.w, "    {insn}{}", cond.to_str())?,
            IfState::Else(cond) => write!(self.w, "    {insn}{}", (!*cond).to_str())?,
        }
        Ok(())
    }

    fn start_insn_flags(
        &mut self,
        insn: &str,
        with_flags: WithFlags,
        if_state: &IfState,
    ) -> Result<(), Error> {
        let insn_s = format!("{insn}{}", s(with_flags));
        self.start_insn(&insn_s, if_state)
    }
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

fn half_reloc(half: Half) -> &'static str {
    match half {
        Half::Lower => "lower16",
        Half::Upper => "upper16",
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
        BinOp::Div => {
            if ty.is_signed() {
                "sdiv"
            } else {
                "udiv"
            }
        }
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
