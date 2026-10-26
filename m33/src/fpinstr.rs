//! The single precision FPU (FPv5-SP): data processing, moves, loads and
//! stores, and FPSCR. Double precision encodings (sz = 1) and the fixed
//! point and half precision conversions aren't implemented.

use crate::{
    cpu::Cpu,
    fp::{self, Mode, Rounding},
};

/// FPSCR fields
const FPSCR_DN: u32 = 1 << 25;
const FPSCR_FZ: u32 = 1 << 24;
const RMODE_SHIFT: u32 = 22;
const NZCV: u32 = 0xf000_0000;

impl Cpu {
    /// The FPSCR mode for operations that don't round or flush
    fn fp_nan_mode(&self) -> Mode {
        Mode {
            dn: self.regs.fpscr & FPSCR_DN != 0,
        }
    }

    /// The FPSCR mode for arithmetic, which must be round to nearest
    /// without flush to zero
    fn fp_mode(&self) -> Mode {
        let fpscr = self.regs.fpscr;
        if fpscr & FPSCR_FZ != 0 || (fpscr >> RMODE_SHIFT) & 3 != 0 {
            panic!(
                "at pc {:#x}: FPSCR {fpscr:#x} sets flush to zero or a rounding mode, which isn't modeled",
                self.regs.r[15]
            );
        }
        Mode {
            dn: fpscr & FPSCR_DN != 0,
        }
    }

    /// FPU instructions with first halfword 0xec00 to 0xefff
    pub(crate) fn fp_instr(&mut self, inst: u16, inst_2: u16) {
        if inst_2 & 0xe00 != 0xa00 {
            todo!("coprocessor inst {inst:04x} {inst_2:04x} not implemented");
        }
        let double = inst_2 & 0xf00 == 0xb00;
        // A single precision FPU only loads, stores and moves d registers
        if double && (inst & 0xf00 == 0xe00 || inst & 0xfe0 == 0xc40) {
            todo!("double precision inst {inst:04x} {inst_2:04x} not implemented");
        }
        if inst & 0xf00 == 0xe00 {
            if inst_2 & 0x10 == 0 {
                self.fp_data_processing(inst, inst_2);
            } else {
                self.fp_transfer(inst, inst_2);
            }
        } else if inst & 0xfe0 == 0xc40 {
            self.fp_transfer_64(inst, inst_2);
        } else {
            self.fp_load_store(inst, inst_2, double);
        }
    }

    /// The Armv8 FPU instructions, which are unconditional, with first
    /// halfword 0xfe00 to 0xfeff: vrint and vcvt with a rounding mode,
    /// vmaxnm and vminnm, and vsel
    pub(crate) fn fp_unconditional(&mut self, inst: u16, inst_2: u16) {
        if inst_2 & 0xf10 != 0xa00 {
            todo!("inst {inst:04x} {inst_2:04x} not implemented");
        }
        let (d, n, m) = sregs(inst, inst_2);
        let mode = self.fp_nan_mode();
        let rm_rounding = |rm: u16| match rm & 3 {
            0 => Rounding::TiesAway,
            1 => Rounding::TiesEven,
            2 => Rounding::Up,
            _ => Rounding::Down,
        };
        let sm = self.regs.s[m];
        if inst & 0xff80 == 0xfe00 {
            // vsel: eq, vs, ge, gt
            let cond = match (inst >> 4) & 3 {
                0 => 0,
                1 => 6,
                2 => 10,
                _ => 12,
            };
            self.regs.s[d] = if self.eval_cond(cond) {
                self.regs.s[n]
            } else {
                sm
            };
        } else if inst & 0xffb0 == 0xfe80 {
            let max = inst_2 & 0x40 == 0;
            self.regs.s[d] = mode.max_min_num(self.regs.s[n], sm, max);
        } else if inst & 0xffbc == 0xfeb8 {
            self.regs.s[d] = mode.round_int(sm, rm_rounding(inst));
        } else if inst & 0xffbc == 0xfebc {
            let signed = inst_2 & 0x80 != 0;
            self.regs.s[d] = fp::to_int(sm, rm_rounding(inst), signed);
        } else {
            todo!("inst {inst:04x} {inst_2:04x} not implemented");
        }
    }

    /// Data processing: opc1 in bits 23 and 21:20, opc2 the Vn field, and
    /// opc3 bits 7:6 (Armv8-M ARM, floating-point data-processing)
    fn fp_data_processing(&mut self, inst: u16, inst_2: u16) {
        let (d, n, m) = sregs(inst, inst_2);
        let opc1 = ((inst >> 5) & 4) | ((inst >> 4) & 3);
        let op = inst_2 & 0x40 != 0;
        let (sd, sn, sm) = (self.regs.s[d], self.regs.s[n], self.regs.s[m]);
        let result = match (opc1, op) {
            (0, _) | (1, _) => {
                let mode = self.fp_mode();
                let product = mode.mul(sn, sm);
                // vmla, vmls, vnmls, vnmla
                let (neg_acc, neg_product) = match (opc1, op) {
                    (0, false) => (false, false),
                    (0, true) => (false, true),
                    (_, false) => (true, false),
                    (_, true) => (true, true),
                };
                let acc = if neg_acc { fp::neg(sd) } else { sd };
                let product = if neg_product {
                    fp::neg(product)
                } else {
                    product
                };
                mode.add(acc, product)
            }
            (2, false) => self.fp_mode().mul(sn, sm),
            (2, true) => fp::neg(self.fp_mode().mul(sn, sm)),
            (3, false) => self.fp_mode().add(sn, sm),
            (3, true) => self.fp_mode().sub(sn, sm),
            (4, false) => self.fp_mode().div(sn, sm),
            // vfnms, vfnma, vfma, vfms
            (5, _) | (6, _) => {
                let neg_acc = opc1 == 5;
                let neg_n = op;
                let acc = if neg_acc { fp::neg(sd) } else { sd };
                let sn = if neg_n { fp::neg(sn) } else { sn };
                self.fp_mode().mul_add(acc, sn, sm)
            }
            (7, _) => match self.fp_other(inst, inst_2, sd, sm) {
                Some(result) => result,
                None => return,
            },
            _ => todo!("inst {inst:04x} {inst_2:04x} not implemented"),
        };
        self.regs.s[d] = result;
    }

    /// opc1 = 0b1x11: immediate moves, the one operand ops, compares and
    /// conversions. None when the instruction doesn't write Sd.
    fn fp_other(&mut self, inst: u16, inst_2: u16, sd: u32, sm: u32) -> Option<u32> {
        if inst_2 & 0x40 == 0 {
            let imm8 = (((inst & 0xf) << 4) | (inst_2 & 0xf)) as u32;
            return Some(fp::expand_imm(imm8));
        }
        let opc2 = inst & 0xf;
        let bit7 = inst_2 & 0x80 != 0;
        let fpscr = self.regs.fpscr;
        let fpscr_rounding = Rounding::from_rmode(fpscr >> RMODE_SHIFT);
        Some(match (opc2, bit7) {
            (0, false) => sm,
            (0, true) => fp::abs(sm),
            (1, false) => fp::neg(sm),
            (1, true) => self.fp_mode().sqrt(sm),
            // vcmp and vcmpe, which differ only in the exceptions they
            // signal
            (4, _) | (5, _) => {
                let other = if opc2 == 5 { 0 } else { sm };
                let nzcv = fp::compare(sd, other);
                self.regs.fpscr = (fpscr & !NZCV) | nzcv;
                return None;
            }
            // vrintr and vrintz; vrintx (inexact) rounds like vrintr
            (6, _) | (7, false) => {
                let rounding = if opc2 == 6 && bit7 {
                    Rounding::Zero
                } else {
                    fpscr_rounding
                };
                self.fp_nan_mode().round_int(sm, rounding)
            }
            (8, signed) => fp::from_int(sm, fpscr_rounding, signed),
            // vcvt (toward zero) and vcvtr (FPSCR's rounding)
            (12, z) | (13, z) => {
                let rounding = if z { Rounding::Zero } else { fpscr_rounding };
                fp::to_int(sm, rounding, opc2 == 13)
            }
            _ => todo!("inst {inst:04x} {inst_2:04x} not implemented"),
        })
    }

    /// vmov between a core register and an s register, and vmrs and vmsr
    fn fp_transfer(&mut self, inst: u16, inst_2: u16) {
        let rt = (inst_2 >> 12) as usize;
        if inst == 0xeef1 && inst_2 & 0xfff == 0xa10 {
            // vmrs: to APSR's flags when Rt is pc
            if rt == 15 {
                self.regs.psr = (self.regs.psr & !NZCV) | (self.regs.fpscr & NZCV);
            } else {
                self.regs.r[rt] = self.regs.fpscr;
            }
        } else if inst == 0xeee1 && inst_2 & 0xfff == 0xa10 {
            self.regs.fpscr = self.regs.r[rt];
        } else if inst & 0xffe0 == 0xee00 && inst_2 & 0xf7f == 0xa10 {
            let n = (((inst & 0xf) << 1) | ((inst_2 >> 7) & 1)) as usize;
            if inst & 0x10 != 0 {
                self.regs.r[rt] = self.regs.s[n];
            } else {
                self.regs.s[n] = self.regs.r[rt];
            }
        } else {
            todo!("inst {inst:04x} {inst_2:04x} not implemented");
        }
    }

    /// vmov between two core registers and two consecutive s registers
    fn fp_transfer_64(&mut self, inst: u16, inst_2: u16) {
        if inst_2 & 0xfd0 != 0xa10 {
            todo!("inst {inst:04x} {inst_2:04x} not implemented");
        }
        let rt = (inst_2 >> 12) as usize;
        let rt2 = (inst & 0xf) as usize;
        let m = (((inst_2 & 0xf) << 1) | ((inst_2 >> 5) & 1)) as usize;
        if inst & 0x10 != 0 {
            self.regs.r[rt] = self.regs.s[m];
            self.regs.r[rt2] = self.regs.s[m + 1];
        } else {
            self.regs.s[m] = self.regs.r[rt];
            self.regs.s[m + 1] = self.regs.r[rt2];
        }
    }

    /// vldr, vstr, vldm, vstm, vpush and vpop, of s registers or (when
    /// double) d registers, each a pair of s registers
    fn fp_load_store(&mut self, inst: u16, inst_2: u16, double: bool) {
        let rn = (inst & 0xf) as usize;
        let load = inst & 0x10 != 0;
        let vd = ((inst_2 >> 12) & 0xf) as usize;
        let bit_d = ((inst >> 6) & 1) as usize;
        // The first s register
        let d = if double {
            if bit_d != 0 {
                panic!("d16-d31 don't exist, in inst {inst:04x} {inst_2:04x}");
            }
            vd << 1
        } else {
            (vd << 1) | bit_d
        };
        let imm8 = (inst_2 & 0xff) as u32;
        let up = inst & 0x80 != 0;
        let pre = inst & 0x100 != 0;
        let wback = inst & 0x20 != 0;
        // The base; pc reads as this instruction's address plus 4, word
        // aligned, and r15 is already past it
        let base = if rn == 15 {
            self.regs.r[15] & !3
        } else {
            self.regs.r[rn]
        };
        if pre && !wback {
            let addr = if up {
                base.wrapping_add(imm8 * 4)
            } else {
                base.wrapping_sub(imm8 * 4)
            };
            self.check_aligned(addr);
            for i in 0..1 + double as usize {
                let addr = addr.wrapping_add(4 * i as u32);
                if load {
                    self.regs.s[d + i] = self.load_u32(addr);
                } else {
                    self.store_u32(addr, self.regs.s[d + i]);
                }
            }
            return;
        }
        // For d registers imm8 is twice their number, so it's the number of
        // words either way
        // Multiple: increment after, or decrement before with writeback
        let n = imm8;
        let start = match (pre, up) {
            (false, true) => base,
            (true, false) if wback => base.wrapping_sub(4 * n),
            _ => todo!("inst {inst:04x} {inst_2:04x} not implemented"),
        };
        if d + n as usize > 32 || n == 0 {
            panic!("bad register list in inst {inst:04x} {inst_2:04x}");
        }
        self.check_aligned(start);
        for i in 0..n {
            let addr = start.wrapping_add(4 * i);
            let reg = d + i as usize;
            if load {
                self.regs.s[reg] = self.load_u32(addr);
            } else {
                self.store_u32(addr, self.regs.s[reg]);
            }
        }
        if wback {
            self.regs.r[rn] = if up { base.wrapping_add(4 * n) } else { start };
        }
    }
}

/// The s registers of a data processing instruction: Vd:D, Vn:N and Vm:M
fn sregs(inst: u16, inst_2: u16) -> (usize, usize, usize) {
    let d = (((inst_2 >> 12) & 0xf) << 1) | ((inst >> 6) & 1);
    let n = ((inst & 0xf) << 1) | ((inst_2 >> 7) & 1);
    let m = ((inst_2 & 0xf) << 1) | ((inst_2 >> 5) & 1);
    (d as usize, n as usize, m as usize)
}
