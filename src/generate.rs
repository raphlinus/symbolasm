// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Generation of assembly language.
//!
//! This approach is clunky, and is going to run into problems when types are
//! needed.

use std::{borrow::Cow, collections::HashMap, io::Write, ops::Deref};

use crate::{
    error::Error,
    flags,
    ifthen::IfState,
    ir::{Assign, BinOp, Body, Expr, Half, Ir, UnaryOp},
    lex::{TokBody, Token},
    regmap::{Regmap, parse_register},
    stmt::Stmt,
    types::{Type, TypeHandle, TypePool},
};

/// The labels defined in a function, which are local to it.
pub struct LocalLabels<'a> {
    pub fn_name: &'a str,
    pub labels: &'a HashMap<String, usize>,
}

impl LocalLabels<'_> {
    /// The assembler name for a label or branch target. Labels defined in
    /// the function become `.L<fn>.<label>`, so they don't collide across
    /// functions or leave symbols in the object file. Other targets (other
    /// functions, for a tail call) and block labels, which are already
    /// local, pass through.
    fn asm_name<'s>(&self, label: &'s str) -> Cow<'s, str> {
        if !label.starts_with(".L") && self.labels.contains_key(label) {
            format!(".L{}.{label}", self.fn_name).into()
        } else {
            label.into()
        }
    }
}

pub struct GenCtx<'a, W: Write> {
    types: &'a mut TypePool,
    w: &'a mut W,
    /// Flags read after this statement, from `flags::live_after`.
    live_flags: u8,
}

#[derive(Clone, Copy, PartialEq)]
enum WithFlags {
    No,
    DontCare,
    Yes,
}

impl<'a, W: Write> GenCtx<'a, W> {
    pub fn new(types: &'a mut TypePool, w: &'a mut W) -> Self {
        Self {
            types,
            w,
            live_flags: flags::NZCV,
        }
    }

    pub fn set_live_flags(&mut self, live_flags: u8) {
        self.live_flags = live_flags;
    }

    /// Whether to use the flag-setting form of an instruction, for its 16
    /// bit encoding: flags are don't-care here, and the ones it writes are
    /// dead.
    /// Like `narrow`, but leaves an explicit Yes or No as it is. `writes`
    /// is 0xff when there's no 16 bit flag-setting encoding.
    fn narrow_or(&self, with_flags: WithFlags, writes: u8) -> WithFlags {
        match with_flags {
            WithFlags::DontCare if writes != 0xff => self.narrow(with_flags, writes),
            WithFlags::DontCare => WithFlags::No,
            _ => with_flags,
        }
    }

    fn narrow(&self, with_flags: WithFlags, writes: u8) -> WithFlags {
        if with_flags == WithFlags::DontCare && writes & self.live_flags == 0 {
            WithFlags::Yes
        } else {
            WithFlags::No
        }
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
            } => {
                // Outside an IT block, flags not explicitly set are
                // don't-care; inside one, the 16 bit encodings are the
                // ones that don't set flags.
                let with_flags = if *with_flags {
                    WithFlags::Yes
                } else if matches!(if_state, IfState::Default) {
                    WithFlags::DontCare
                } else {
                    WithFlags::No
                };
                self.gen_assign_reg(*lhs, rhs, with_flags, if_state)
            }
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
                // movs has no 16 bit immediate form
                let imm16_ok = *n < 0x1_0000 && with_flags != WithFlags::Yes;
                let with_flags = if *n < 0x100 && lhs < 8 {
                    // movs rd, #imm8
                    match with_flags {
                        WithFlags::DontCare => self.narrow(with_flags, flags::NZ),
                        _ => with_flags,
                    }
                } else if with_flags == WithFlags::DontCare {
                    WithFlags::No
                } else {
                    with_flags
                };
                if imm16_ok || (is_imm8m(*n) && !n >= 0x100) {
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
                    if with_flags == WithFlags::Yes {
                        Err(format!("movs can't encode {n:#x}"))?;
                    }
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
                    // negs rd, rn
                    let with_flags = if lhs < 8 && *r < 8 {
                        self.narrow_or(with_flags, flags::NZCV)
                    } else {
                        self.narrow_or(with_flags, 0xff)
                    };
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
                // mvns rd, rm
                let writes = match rhs.body {
                    Body::Reg(r) if lhs < 8 && r < 8 => flags::NZ,
                    _ => 0xff,
                };
                let with_flags = self.narrow_or(with_flags, writes);
                self.start_insn_flags("mvn", with_flags, if_state)?;
                write!(self.w, " ")?;
                write_reg(lhs, self.w)?;
                write!(self.w, ", ")?;
                self.gen_operand2(rhs)?;
                writeln!(self.w)?;
            }
            Body::Intrinsic(insn, args) => {
                let with_flags = self.narrow_or(with_flags, 0xff);
                self.gen_intrinsic(lhs, insn, args, with_flags, if_state)?
            }
            // Multiply accumulate: a + b * c is mla, a - b * c is mls
            Body::Binop(acc, op @ (BinOp::Add | BinOp::Sub), b)
                if let Body::Binop(x, BinOp::Mul, y) = &b.body =>
            {
                let insn = if *op == BinOp::Add { "mla" } else { "mls" };
                if with_flags == WithFlags::Yes {
                    Err(format!("{insn} can't set flags"))?;
                }
                let regs = [acc, x, y].map(|e| match e.body {
                    Body::Reg(r) => Some(r),
                    _ => None,
                });
                let [Some(ra), Some(rn), Some(rm)] = regs else {
                    return Err(format!("{insn} operands must all be registers"))?;
                };
                self.start_insn(insn, if_state)?;
                write!(self.w, " ")?;
                for r in [lhs, rn, rm] {
                    write_reg(r, self.w)?;
                    write!(self.w, ", ")?;
                }
                write_reg(ra, self.w)?;
                writeln!(self.w)?;
            }
            // The address of a static's field, resolved by the linker
            Body::Binop(a, BinOp::Add, b)
                if let Body::Sym(sym) = &a.body
                    && let Body::Imm(offset) = b.body =>
            {
                if with_flags == WithFlags::Yes {
                    Err("loading a symbol address cannot set flags")?;
                }
                self.start_insn("ldr", if_state)?;
                write!(self.w, " ")?;
                write_reg(lhs, self.w)?;
                writeln!(self.w, ", ={sym}+{offset}")?;
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
                let fitted;
                if let Body::Imm(n) = rhs.body {
                    let (fit_insn, fit_n) = fit_imm(insn, n, with_flags == WithFlags::Yes)?;
                    insn = fit_insn;
                    fitted = Expr {
                        ty: rhs.ty,
                        body: Body::Imm(fit_n),
                    };
                    rhs = &fitted;
                }
                // The 16 bit flag-setting encodings: low registers, and for
                // the logical ops and register shifts, rd the same as rn.
                // A commutative op with rd the same as rm is swapped to fit.
                let mut a = a.deref();
                let rn = if let Body::Reg(r) = a.body { r } else { 0xff };
                let commutative = matches!(insn, "and" | "orr" | "eor");
                let low = lhs < 8 && rn < 8;
                let writes = match (insn, &rhs.body) {
                    ("add" | "sub", Body::Imm(n))
                        if low && (*n <= 7 || (lhs == rn && *n <= 255)) =>
                    {
                        flags::NZCV
                    }
                    ("add" | "sub", Body::Reg(rm)) if low && *rm < 8 => flags::NZCV,
                    ("and" | "orr" | "eor" | "bic", Body::Reg(rm)) if low && *rm < 8 => {
                        if lhs == rn || (commutative && lhs == *rm) {
                            flags::NZ
                        } else {
                            0xff
                        }
                    }
                    ("lsl" | "lsr" | "asr", Body::Imm(_)) if low => flags::NZC,
                    ("lsl" | "lsr" | "asr", Body::Reg(rm)) if low && lhs == rn && *rm < 8 => {
                        flags::NZC
                    }
                    _ => 0xff,
                };
                let with_flags = self.narrow_or(with_flags, writes);
                if with_flags == WithFlags::Yes
                    && commutative
                    && lhs != rn
                    && let Body::Reg(rm) = rhs.body
                    && rm == lhs
                {
                    std::mem::swap(&mut a, &mut rhs);
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
                if with_flags == WithFlags::Yes {
                    Err("bit field extracts can't set flags")?;
                }
                let signed = self.types.get(a.ty).is_signed();
                // TODO: ensure end > start, otherwise error
                let width = end - start;
                // The low byte or halfword has its own instruction, which
                // has a 16 bit encoding for low registers.
                let extend = match (*start, width, signed) {
                    (0, 8, false) => Some("uxtb"),
                    (0, 8, true) => Some("sxtb"),
                    (0, 16, false) => Some("uxth"),
                    (0, 16, true) => Some("sxth"),
                    _ => None,
                };
                if let Body::Reg(r) = &a.body
                    && *start > 0
                    && *end == 32
                    && lhs < 8
                    && *r < 8
                    && self.narrow(with_flags, flags::NZC) == WithFlags::Yes
                {
                    // y[n..32] is y >> n, and lsrs/asrs have 16 bit encodings
                    let insn = if signed { "asrs" } else { "lsrs" };
                    self.start_insn(insn, if_state)?;
                    write!(self.w, " ")?;
                    write_reg(lhs, self.w)?;
                    write!(self.w, ", ")?;
                    write_reg(*r, self.w)?;
                    writeln!(self.w, ", #{start}")?;
                } else if let Body::Reg(r) = &a.body {
                    let insn = extend.unwrap_or(if signed { "sbfx" } else { "ubfx" });
                    self.start_insn(insn, if_state)?;
                    write!(self.w, " ")?;
                    write_reg(lhs, self.w)?;
                    write!(self.w, ", ")?;
                    write_reg(*r, self.w)?;
                    if extend.is_none() {
                        write!(self.w, ", #{start}, #{width}")?;
                    }
                    writeln!(self.w)?;
                } else {
                    Err("slice source must be register")?;
                }
            }
            _ => todo!(),
        }
        Ok(())
    }

    /// Instructions written as calls: `clz(a)`, `rbit(a)`, `rev(a)`,
    /// `rev16(a)`, `revsh(a)`, `ror(a, n)`, `pkhbt(a, b << n)` and
    /// `pkhtb(a, b >> n)`.
    fn gen_intrinsic(
        &mut self,
        lhs: u8,
        insn: &str,
        args: &[Expr],
        with_flags: WithFlags,
        if_state: &IfState,
    ) -> Result<(), Error> {
        let reg = |e: &Expr| match e.body {
            Body::Reg(r) => Ok(r),
            _ => Err(format!("{insn} arguments must be registers")),
        };
        let a = reg(&args[0])?;
        let flags = if insn == "ror" {
            with_flags
        } else {
            WithFlags::No
        };
        if with_flags == WithFlags::Yes && flags != WithFlags::Yes {
            Err(format!("{insn} can't set flags"))?;
        }
        self.start_insn_flags(insn, flags, if_state)?;
        write!(self.w, " ")?;
        write_reg(lhs, self.w)?;
        write!(self.w, ", ")?;
        write_reg(a, self.w)?;
        match (insn, args.get(1).map(|b| &b.body)) {
            (_, None) => (),
            ("ror", Some(Body::Imm(n))) => {
                if !(1..=31).contains(n) {
                    Err(format!("ror by {n} is out of range (1 to 31)"))?;
                }
                write!(self.w, ", #{n}")?;
            }
            ("ror", Some(Body::Reg(b))) => {
                write!(self.w, ", ")?;
                write_reg(*b, self.w)?;
            }
            ("pkhbt", Some(Body::Reg(b))) => {
                write!(self.w, ", ")?;
                write_reg(*b, self.w)?;
            }
            ("pkhbt", Some(Body::Binop(b, BinOp::Shl, n)))
                if let (Body::Reg(b), Body::Imm(n)) = (&b.body, &n.body) =>
            {
                if *n > 31 {
                    Err(format!("pkhbt shift {n} is out of range (0 to 31)"))?;
                }
                write!(self.w, ", ")?;
                write_reg(*b, self.w)?;
                write!(self.w, ", lsl #{n}")?;
            }
            ("pkhtb", Some(Body::Binop(b, BinOp::Shr, n)))
                if let (Body::Reg(b), Body::Imm(n)) = (&b.body, &n.body) =>
            {
                // pkhtb shifts arithmetically, whatever the type
                if !(1..=32).contains(n) {
                    Err(format!("pkhtb shift {n} is out of range (1 to 32)"))?;
                }
                write!(self.w, ", ")?;
                write_reg(*b, self.w)?;
                write!(self.w, ", asr #{n}")?;
            }
            ("pkhbt", _) => Err("pkhbt's second argument must be b or b << n")?,
            ("pkhtb", _) => Err("pkhtb's second argument must be b >> n")?,
            _ => Err("ror's second argument must be a register or 1 to 31")?,
        }
        writeln!(self.w)?;
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
                if insn == "str" && self.gen_single_ldm_stm("stm", *r, addr, incr, if_state)? {
                    return Ok(());
                }
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

    /// Use 16 bit `ldmia rb!, {rt}` / `stmia rb!, {rt}` for a post-incremented
    /// word transfer when the encoding allows it. Unlike `ldr rt, [rb], #4`,
    /// this requires the address to be word aligned.
    fn gen_single_ldm_stm(
        &mut self,
        insn: &str,
        rt: u8,
        addr: &Expr,
        incr: i32,
        if_state: &IfState,
    ) -> Result<bool, Error> {
        let Body::Reg(rb) = addr.body else {
            return Ok(false);
        };
        // The 16 bit encodings only take low registers, and for ldm the base
        // can't be in the list if it's written back.
        if incr != 4 || rt >= 8 || rb >= 8 || (insn == "ldm" && rt == rb) {
            return Ok(false);
        }
        self.start_insn(insn, if_state)?;
        write!(self.w, " ")?;
        write_reg(rb, self.w)?;
        write!(self.w, "!, {{")?;
        write_reg(rt, self.w)?;
        writeln!(self.w, "}}")?;
        Ok(true)
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
        if let Body::Binop(_, add_or_sub, offset) = &addr.body
            && let Body::Imm(n) = offset.body
        {
            let n = if *add_or_sub == BinOp::Sub {
                n.wrapping_neg()
            } else {
                n
            } as i32;
            if n % 4 != 0 || !(-1020..=1020).contains(&n) {
                Err(format!(
                    "{op}rd offset {n} must be a multiple of 4 from -1020 to 1020"
                ))?;
            }
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
                let mut insn = test_insn_for_binop(*op).ok_or("unhandled binop for test")?;
                let mut b = b.deref().clone();
                if let Body::Imm(n) = b.body {
                    let (fit_insn, fit_n) = fit_imm(insn, n, true)?;
                    insn = fit_insn;
                    b.body = Body::Imm(fit_n);
                }
                self.start_insn(insn, if_state)?;
                write!(self.w, " ")?;
                if let Body::Reg(r) = &a.body {
                    write_reg(*r, self.w)?;
                } else {
                    // TODO: rsb
                    Err("left operand of binop must be reg")?;
                }
                write!(self.w, ", ")?;
                self.gen_operand2(&b)?;
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
                if insn == "ldr" && self.gen_single_ldm_stm("ldm", *r, addr, incr, if_state)? {
                    return Ok(());
                }
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
                if let Body::Imm(n) = rhs.body {
                    check_shift(*op, n)?;
                } else {
                    Err("a shifted operand needs a constant shift")?;
                }
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
            Body::Binop(lhs, BinOp::Sub, rhs)
                if let Body::Reg(r) = lhs.body
                    && let Body::Imm(n) = rhs.body =>
            {
                if incr != 0 {
                    Err("an address with an offset can't also be updated")?;
                }
                if !(1..=255).contains(&n) {
                    Err(format!(
                        "address offset -{n} is out of range (-255 to 4095)"
                    ))?;
                }
                write!(self.w, "[")?;
                write_reg(r, self.w)?;
                write!(self.w, ", #-{n}]")?;
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
                        let offset = *n as i32;
                        if !(-255..=4095).contains(&offset) {
                            Err(format!(
                                "address offset {offset} is out of range (-255 to 4095)"
                            ))?;
                        }
                        write!(self.w, "[")?;
                        write_reg(*lhs_reg, self.w)?;
                        write!(self.w, ", #{offset}]")?;
                    }
                    Body::Binop(offset, BinOp::Shl, shift) => {
                        if let Body::Reg(offset_reg) = &offset.body
                            && let Body::Imm(shift) = &shift.body
                        {
                            if *shift > 3 {
                                Err(format!(
                                    "index stride {} isn't encodable; only 1, 2, 4 and 8 are",
                                    1u64 << shift
                                ))?;
                            }
                            write!(self.w, "[")?;
                            write_reg(*lhs_reg, self.w)?;
                            write!(self.w, ", ")?;
                            write_reg(*offset_reg, self.w)?;
                            write!(self.w, ", lsl #{shift}]")?;
                        } else {
                            Err("must be register left shifted by immediate")?;
                        }
                    }
                    Body::Binop(_, BinOp::Mul, stride) => {
                        if let Body::Imm(n) = stride.body {
                            Err(format!(
                                "index stride {n} isn't encodable; only 1, 2, 4 and 8 are, so compute the offset first"
                            ))?;
                        }
                        Err("an index must be a register times a constant")?;
                    }
                    _ => Err("unsupported address offset")?,
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
        labels: &LocalLabels,
        if_state: &IfState,
    ) -> Result<(), Error> {
        match stmt {
            Stmt::Label(l) => {
                writeln!(self.w, "{}:", labels.asm_name(l))?;
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
                crate::stmt::Insn::Blx(target) => {
                    if let Some(id) = target.as_ident() {
                        self.start_insn("blx", if_state)?;
                        write!(self.w, " ")?;
                        write_var_reg(id, regmap, self.w)?;
                        writeln!(self.w)?;
                    } else {
                        Err("blx target must be register")?;
                    }
                }
                crate::stmt::Insn::BCond(cond, target) => {
                    let insn = format!("b{cond}");
                    self.start_insn(&insn, if_state)?;
                    writeln!(self.w, " {}", labels.asm_name(target))?;
                }
                crate::stmt::Insn::B(target) => {
                    self.start_insn("b", if_state)?;
                    writeln!(self.w, " {}", labels.asm_name(target))?;
                }
                crate::stmt::Insn::Push(regs) => {
                    self.gen_reglist("push", regs, regmap, if_state)?
                }
                crate::stmt::Insn::Pop(regs) => self.gen_reglist("pop", regs, regmap, if_state)?,
                crate::stmt::Insn::Cps(enable, flags) => {
                    self.start_insn(if *enable { "cpsie" } else { "cpsid" }, if_state)?;
                    writeln!(self.w, " {flags}")?;
                }
                crate::stmt::Insn::NoOperand(insn) => {
                    if matches!(insn.as_str(), "dmb" | "dsb" | "isb")
                        && !matches!(if_state, IfState::Default)
                    {
                        Err(format!("{insn} can't be conditional"))?;
                    }
                    self.start_insn(insn, if_state)?;
                    writeln!(self.w)?;
                }
                crate::stmt::Insn::Bl(target) => {
                    self.start_insn("bl", if_state)?;
                    writeln!(self.w, " {target}")?;
                }
                crate::stmt::Insn::Cbz(reg, target) => {
                    if let Some(id) = reg.as_ident() {
                        self.start_insn("cbz", if_state)?;
                        write!(self.w, " ")?;
                        write_var_reg(id, regmap, self.w)?;
                        writeln!(self.w, ", {}", labels.asm_name(target))?;
                    } else {
                        Err("bx target must be register")?;
                    }
                }
                crate::stmt::Insn::Cbnz(reg, target) => {
                    if let Some(id) = reg.as_ident() {
                        self.start_insn("cbnz", if_state)?;
                        write!(self.w, " ")?;
                        write_var_reg(id, regmap, self.w)?;
                        writeln!(self.w, ", {}", labels.asm_name(target))?;
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
            let n = if let Some(id) = r.as_ident() {
                parse_register(id)
                    .or_else(|| regmap.lookup(id))
                    .ok_or(format!("no place for {id}"))?
            } else if let Some(place) = pop_binding(insn, r) {
                // `pop(x @r1)` restores into r1 and places x there
                parse_register(place).ok_or(format!("{place} is not a register"))?
            } else {
                Err(format!("{insn} arguments must be registers"))?
            };
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

/// Pick an encodable form for a data processing instruction with an
/// immediate operand, or report that there isn't one.
///
/// Thumb-2 encodes modified immediates (imm8m), and add and sub also take
/// any 12 bit immediate, though only without setting flags. When a value
/// doesn't fit, the paired instruction may fit with the negated or
/// inverted value: add/sub, cmp/cmn, and/bic, orr/orn. The pairs set the
/// same flags, except for 0 and 0x80000000, whose negations aren't
/// different values; those are never swapped.
fn fit_imm(insn: &'static str, n: u32, with_flags: bool) -> Result<(&'static str, u32), Error> {
    match insn {
        "lsl" => return check_shift(BinOp::Shl, n).map(|_| (insn, n)),
        "lsr" | "asr" => return check_shift(BinOp::Shr, n).map(|_| (insn, n)),
        "mul" | "udiv" | "sdiv" => Err(format!("{insn} has no immediate form"))?,
        _ => (),
    }
    let fits = |insn: &str, n: u32| {
        is_imm8m(n) || (matches!(insn, "add" | "sub") && !with_flags && n < 0x1000)
    };
    if fits(insn, n) {
        return Ok((insn, n));
    }
    let swapped = match insn {
        "add" => Some(("sub", n.wrapping_neg())),
        "sub" => Some(("add", n.wrapping_neg())),
        "cmp" => Some(("cmn", n.wrapping_neg())),
        "cmn" => Some(("cmp", n.wrapping_neg())),
        "and" => Some(("bic", !n)),
        "bic" => Some(("and", !n)),
        "orr" => Some(("orn", !n)),
        "orn" => Some(("orr", !n)),
        _ => None,
    };
    if let Some((other, m)) = swapped
        && n != 0x8000_0000
        && fits(other, m)
    {
        return Ok((other, m));
    }
    Err(format!(
        "{n:#x} isn't an encodable immediate for {insn}; load it into a register"
    ))?
}

/// Shift amounts: lsl takes 0 to 31, lsr and asr 1 to 32.
fn check_shift(op: BinOp, n: u32) -> Result<(), Error> {
    let ok = match op {
        BinOp::Shl => n < 32,
        _ => (1..=32).contains(&n),
    };
    if !ok {
        Err(format!("shift by {n} is out of range"))?;
    }
    Ok(())
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

/// The register of a `x @rN` binding in a pop list.
fn pop_binding<'a>(insn: &str, r: &'a crate::parse::Expr) -> Option<&'a str> {
    match r {
        crate::parse::Expr::Binop(var, op, place)
            if insn == "pop" && op.tok == crate::lex::TokBody::At && var.as_ident().is_some() =>
        {
            place.as_ident()
        }
        _ => None,
    }
}
