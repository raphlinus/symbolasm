use crate::cpu::{Cpu, ShiftType};

impl Cpu {
    pub fn step_instr(&mut self) {
        let inst_16 = self.load_u16(self.regs.r[15]);
        if self.trace {
            println!("{:08x}: {inst_16:04x}", self.regs.r[15]);
        }
        // Inside an IT block, an instruction whose condition fails is
        // skipped. ITSTATE advances after each instruction in the block.
        if self.it_state != 0 {
            let cond = (self.it_state >> 4) as u16;
            let pass = self.eval_cond(cond);
            if !pass {
                let is_32 = inst_16 >> 11 >= 0x1d;
                self.regs.r[15] += if is_32 { 4 } else { 2 };
            } else {
                self.regs.r[15] += 2;
                self.dispatch(inst_16);
            }
            self.it_state = if self.it_state & 7 == 0 {
                0
            } else {
                (self.it_state & 0xe0) | ((self.it_state << 1) & 0x1f)
            };
            return;
        }
        self.regs.r[15] += 2;
        self.dispatch(inst_16);
    }

    fn dispatch(&mut self, inst_16: u16) {
        match inst_16 >> 12 {
            0x0 | 0x1 => self.inst_0(inst_16),
            0x2 => self.inst_2(inst_16),
            0x3 => self.inst_3(inst_16),
            0x4 => self.inst_4(inst_16),
            0x5 => self.inst_5(inst_16),
            0x6 => self.inst_6(inst_16),
            0x7 => self.inst_7(inst_16),
            0x8 => self.inst_8(inst_16),
            0x9 => self.inst_9(inst_16),
            0xa => self.inst_a(inst_16),
            0xb => self.inst_b(inst_16),
            0xc => self.inst_c(inst_16),
            0xd => self.inst_d(inst_16),
            0xe => self.inst_e(inst_16),
            0xf => self.inst_f(inst_16),
            _ => todo!("inst {inst_16:04x} not implemented"),
        }
    }

    /// Shift (immediate), add, subtract: the 16 bit encodings with top
    /// bits 000. They set flags outside an IT block.
    fn inst_0(&mut self, inst: u16) {
        let rd = (inst & 7) as usize;
        let rn = ((inst >> 3) & 7) as usize;
        let rrn = self.regs.r[rn];
        let set = !self.in_it();
        if inst >> 11 == 3 {
            // ADD/SUB (register or 3 bit immediate), T1 encodings
            let operand = if inst & 0x400 != 0 {
                ((inst >> 6) & 7) as u32
            } else {
                self.regs.r[((inst >> 6) & 7) as usize]
            };
            self.regs.r[rd] = if inst & 0x200 == 0 {
                self.add_flags(rrn, operand, 0, set)
            } else {
                self.add_flags(rrn, !operand, 1, set)
            };
            return;
        }
        let imm5 = ((inst >> 6) & 0x1f) as u32;
        let (val, carry) = match inst >> 11 {
            // LSL (immediate), imm5 0 is MOV (register) setting flags
            0 if imm5 == 0 => (rrn, None),
            0 => (rrn << imm5, Some((rrn >> (32 - imm5)) & 1)),
            // LSR (immediate), imm5 0 means 32
            1 => {
                let n = if imm5 == 0 { 32 } else { imm5 };
                (
                    ((rrn as u64) >> n) as u32,
                    Some(((rrn as u64) >> (n - 1)) as u32 & 1),
                )
            }
            // ASR (immediate)
            _ => {
                let n = if imm5 == 0 { 32 } else { imm5 };
                let v = rrn as i32 as i64;
                ((v >> n) as u32, Some((v >> (n - 1)) as u32 & 1))
            }
        };
        self.regs.r[rd] = val;
        if set {
            self.set_nz(val);
            if let Some(c) = carry {
                self.set_c(c);
            }
        }
    }

    // Load and store word (immediate), T1 encodings
    fn inst_6(&mut self, inst: u16) {
        let rt = (inst & 7) as usize;
        let rn = ((inst >> 3) & 7) as usize;
        let addr = self.regs.r[rn].wrapping_add(((inst >> 6) & 0x1f) as u32 * 4);
        if inst & 0x800 == 0 {
            self.store_u32(addr, self.regs.r[rt]);
        } else {
            self.regs.r[rt] = self.load_u32(addr);
        }
    }

    fn inst_2(&mut self, inst: u16) {
        let imm8 = (inst & 0xff) as u32;
        let rd = ((inst >> 8) & 7) as usize;
        if inst & 0x800 == 0 {
            // MOV (immediate, T1 encoding)
            self.regs.r[rd] = imm8;
            if !self.in_it() {
                self.set_nz(imm8);
            }
        } else {
            // CMP (immediate, T1 encoding)
            _ = self.adc(self.regs.r[rd], !imm8, 1);
        }
    }

    fn inst_3(&mut self, inst: u16) {
        let imm8 = (inst & 0xff) as u32;
        let rd = ((inst >> 8) & 7) as usize;
        let set = !self.in_it();
        if inst & 0x800 == 0 {
            // ADDS (immediate, T2 encoding)
            self.regs.r[rd] = self.add_flags(self.regs.r[rd], imm8, 0, set);
        } else {
            // SUBS (immediate, T2 encoding)
            self.regs.r[rd] = self.add_flags(self.regs.r[rd], !imm8, 1, set);
        }
    }

    fn inst_4(&mut self, inst: u16) {
        if inst & 0xc00 == 0 {
            // data processing, table 3-3; flags only outside an IT block
            let op = (inst >> 6) & 0xf;
            let rm = ((inst >> 3) & 7) as usize;
            let rdn = (inst & 7) as usize;
            let rrm = self.regs.r[rm];
            let rrn = self.regs.r[rdn];
            let set = !self.in_it();
            let carry_in = (self.regs.psr >> 29) & 1;
            let logical = |cpu: &mut Self, val: u32, write: bool| {
                if write {
                    cpu.regs.r[rdn] = val;
                }
                if set || !write {
                    cpu.set_nz(val);
                }
            };
            match op {
                0 => logical(self, rrn & rrm, true),              // ANDS
                1 => logical(self, rrn ^ rrm, true),              // EORS
                12 => logical(self, rrn | rrm, true),             // ORRS
                14 => logical(self, rrn & !rrm, true),            // BICS
                15 => logical(self, !rrm, true),                  // MVNS
                13 => logical(self, rrn.wrapping_mul(rrm), true), // MULS
                8 => logical(self, rrn & rrm, false),             // TST
                2..=4 | 7 => {
                    // LSLS, LSRS, ASRS, RORS (register)
                    let n = rrm & 0xff;
                    let (val, c) = match op {
                        2 if n == 0 => (rrn, None),
                        2 if n < 32 => (rrn << n, Some((rrn >> (32 - n)) & 1)),
                        2 => (0, Some(if n == 32 { rrn & 1 } else { 0 })),
                        3 if n == 0 => (rrn, None),
                        3 if n <= 32 => (
                            ((rrn as u64) >> n) as u32,
                            Some(((rrn as u64) >> (n - 1)) as u32 & 1),
                        ),
                        3 => (0, Some(0)),
                        4 if n == 0 => (rrn, None),
                        4 => {
                            let k = n.min(32);
                            let v = rrn as i32 as i64;
                            ((v >> k) as u32, Some((v >> (k - 1)) as u32 & 1))
                        }
                        _ if n == 0 => (rrn, None),
                        _ => {
                            let v = rrn.rotate_right(n % 32);
                            (v, Some(v >> 31))
                        }
                    };
                    self.regs.r[rdn] = val;
                    if set {
                        self.set_nz(val);
                        if let Some(c) = c {
                            self.set_c(c);
                        }
                    }
                }
                5 => self.regs.r[rdn] = self.add_flags(rrn, rrm, carry_in, set), // ADCS
                6 => self.regs.r[rdn] = self.add_flags(rrn, !rrm, carry_in, set), // SBCS
                9 => self.regs.r[rdn] = self.add_flags(0, !rrm, 1, set),         // RSBS #0
                10 => _ = self.adc(rrn, !rrm, 1),                                // CMP
                11 => _ = self.adc(rrn, rrm, 0),                                 // CMN
                _ => unreachable!(),
            }
        } else if inst & 0x800 == 0 {
            // special data processing + branch/exchange
            let rm = ((inst >> 3) & 0xf) as usize;
            let rd = (((inst & 0x80) >> 4) | (inst & 7)) as usize;
            let rrm = self.regs.r[rm];
            match (inst >> 8) & 3 {
                0 => {
                    // ADD (register, T2 encoding)
                    self.regs.r[rd] = self.regs.r[rd].wrapping_add(rrm);
                }
                1 => {
                    // CMP (register, T2 encoding)
                    _ = self.adc(self.regs.r[rd], !rrm, 1);
                }
                2 => {
                    // MOV (register, T1 encoding)
                    self.regs.r[rd] = rrm;
                }
                _ => {
                    // BX, BLX (register)
                    if inst & 0x80 != 0 {
                        self.regs.r[14] = self.regs.r[15] | 1;
                    }
                    self.regs.r[15] = rrm & !1;
                }
            }
        } else {
            // LDR (literal, T1 encoding)
            let rt = ((inst >> 8) & 7) as usize;
            let addr = ((self.regs.r[15] + 2) & !3) + ((inst as u32 & 0xff) << 2);
            self.regs.r[rt] = self.load_u32(addr);
        }
    }

    // Load and store (register offset) instructions, see table 3-9
    fn inst_5(&mut self, inst: u16) {
        let opcode = (inst >> 9) & 7;
        let rd = (inst & 7) as usize;
        let rn = ((inst >> 3) & 7) as usize;
        let rm = ((inst >> 6) & 7) as usize;
        let addr = self.regs.r[rn].wrapping_add(self.regs.r[rm]);
        match opcode {
            0 => self.store_u32(addr, self.regs.r[rd]),        // STR
            1 => self.store_u16(addr, self.regs.r[rd] as u16), // STRH
            2 => self.store_u8(addr, self.regs.r[rd] as u8),   // STB
            3 => self.regs.r[rd] = self.load_u8(addr) as i8 as u32, // LDRSB
            4 => self.regs.r[rd] = self.load_u32(addr),        // LDR
            5 => self.regs.r[rd] = self.load_u16(addr) as u32, // LDRH
            6 => self.regs.r[rd] = self.load_u8(addr) as u32,  // LDRB
            7 => self.regs.r[rd] = self.load_u16(addr) as i16 as u32, // LDRSH
            _ => unreachable!(),
        }
    }

    fn inst_7(&mut self, inst: u16) {
        let rd = (inst & 7) as usize;
        let rn = ((inst >> 3) & 7) as usize;
        let imm = ((inst >> 6) & 0x1f) as u32;
        let addr = self.regs.r[rn] + imm;
        if (inst & 0x800) == 0 {
            // STRB (immediate, T1 encoding)
            self.store_u8(addr, self.regs.r[rd] as u8);
        } else {
            // LDRB (immediate, T1 encoding)
            self.regs.r[rd] = self.load_u8(addr) as u32;
        }
    }

    fn inst_8(&mut self, inst: u16) {
        let rd = (inst & 7) as usize;
        let rn = ((inst >> 3) & 7) as usize;
        let imm = ((inst >> 6) & 0x1f) as u32 * 2;
        let addr = self.regs.r[rn] + imm;
        if (inst & 0x800) == 0 {
            // STRH (immediate, T1 encoding)
            self.store_u16(addr, self.regs.r[rd] as u16);
        } else {
            // LDRH (immediate, T1 encoding)
            self.regs.r[rd] = self.load_u16(addr) as u32;
        }
    }

    // Load and store word, SP relative
    fn inst_9(&mut self, inst: u16) {
        let rt = ((inst >> 8) & 7) as usize;
        let addr = self.regs.r[13].wrapping_add((inst & 0xff) as u32 * 4);
        if inst & 0x800 == 0 {
            self.store_u32(addr, self.regs.r[rt]);
        } else {
            self.regs.r[rt] = self.load_u32(addr);
        }
    }

    fn inst_a(&mut self, inst: u16) {
        if inst & 0x800 == 0 {
            // ADR (T1 encoding)
            let imm = (inst & 0xff) << 2;
            let align_pc = (self.regs.r[15] + 2) & !3;
            let rd = (inst & 0x700) >> 8;
            self.regs.r[rd as usize] = align_pc.wrapping_add(imm as u32);
        } else {
            // ADD (SP plus immediate, T1 encoding)
            let imm = (inst & 0xff) << 2;
            let rd = (inst & 0x700) >> 8;
            self.regs.r[rd as usize] = self.regs.r[13].wrapping_add(imm as u32);
        }
    }

    // Miscellaneous instructions, see 3.2.1
    fn inst_b(&mut self, inst: u16) {
        match (inst >> 8) & 0xf {
            0 => {
                // ADD, SUB (SP plus immediate)
                let imm = (inst & 0x7f) as u32 * 4;
                self.regs.r[13] = if inst & 0x80 == 0 {
                    self.regs.r[13].wrapping_add(imm)
                } else {
                    self.regs.r[13].wrapping_sub(imm)
                };
            }
            6 => {
                // CPS: interrupts aren't modeled
            }
            10 => {
                // reverse bytes, table 3-17
                let rm = ((inst >> 3) & 7) as usize;
                let rd = (inst & 7) as usize;
                let rrm = self.regs.r[rm];
                self.regs.r[rd] = match (inst >> 6) & 3 {
                    0 => rrm.swap_bytes(),
                    1 => rev16(rrm),
                    3 => revsh(rrm),
                    _ => todo!("inst {inst:04x} not implemented"),
                };
            }
            2 => {
                // sign/zero extend, see table 3-16
                let opc = (inst >> 6) & 3;
                let rm = ((inst >> 3) & 7) as usize;
                let rrm = self.regs.r[rm];
                let rd = (inst & 7) as usize;
                match opc {
                    0 => self.regs.r[rd] = rrm as i16 as u32, // SXTH
                    1 => self.regs.r[rd] = rrm as i8 as u32,  // SXTB
                    2 => self.regs.r[rd] = rrm as u16 as u32, // UXTH
                    3 => self.regs.r[rd] = rrm as u8 as u32,  // UXTB
                    _ => unreachable!(),
                }
            }
            4 => {
                // PUSH, T1 encoding, LR bit clear
                let regs = inst & 0xff;
                self.push_regs(regs);
            }
            5 => {
                // PUSH, T1 encoding, LR bit set
                let regs = (inst & 0xff) | (1 << 14);
                self.push_regs(regs);
            }
            12 => {
                // POP, T1 encoding, PC bit clear
                let regs = inst & 0xff;
                self.pop_regs(regs);
            }
            13 => {
                // POP, T1 encoding, PC bit set
                let regs = (inst & 0xff) | (1 << 15);
                self.pop_regs(regs);
            }
            14 => {
                let bkpt = inst as u8;
                self.syscall(bkpt);
            }
            1 | 3 | 9 | 11 => {
                // CBZ, CBNZ
                let rn = (inst & 7) as usize;
                let offset = (((inst >> 3) & 0x1f) | ((inst >> 4) & 0x20)) as u32 * 2;
                let nonzero = inst & 0x800 != 0;
                if (self.regs.r[rn] != 0) == nonzero {
                    self.regs.r[15] = self.regs.r[15].wrapping_add(offset + 2);
                }
            }
            15 => {
                if inst & 0xf != 0 {
                    // IT
                    self.it_state = inst as u8;
                }
                // otherwise a hint (nop, wfe, sev, ...), which does nothing here
            }
            _ => todo!("inst {inst:04x} not implemented"),
        }
    }

    fn inst_c(&mut self, inst: u16) {
        let rn = ((inst >> 8) & 7) as usize;
        let mut addr = self.regs.r[rn];
        self.check_aligned(addr);
        if (inst & 0x800) == 0 {
            // STMIA (T1 encoding)
            for i in 0..8 {
                if (inst & (1 << i)) != 0 {
                    self.store_u32(addr, self.regs.r[i]);
                    addr += 4;
                }
            }
            self.regs.r[rn] = addr;
        } else {
            // LDMIA (T1 encoding)
            for i in 0..8 {
                if (inst & (1 << i)) != 0 {
                    self.regs.r[i] = self.load_u32(addr);
                    addr += 4;
                }
            }
            if (inst & (1 << rn)) == 0 {
                self.regs.r[rn] = addr;
            }
        }
    }

    fn inst_d(&mut self, inst: u16) {
        // conditional branch, T1 encoding
        let cond = (inst >> 8) & 0xf;
        if self.eval_cond(cond) {
            let imm8 = inst as i8 as u32;
            self.regs.r[15] = self.regs.r[15].wrapping_add(imm8 << 1).wrapping_add(2);
        }
    }

    fn inst_e(&mut self, inst: u16) {
        if inst & 0x800 == 0 {
            // unconditional branch, T1 encoding
            let imm = (((inst as i32) << 21) >> 20) as u32;
            self.regs.r[15] = self.regs.r[15].wrapping_add(imm).wrapping_add(2);
            return;
        }
        // 32 bit instructions
        let inst_2 = self.load_u16(self.regs.r[15]);
        self.regs.r[15] += 2;
        let rn = (inst & 0xf) as usize;
        if inst & 0xe00 == 0xa00 {
            // data processing, shifted register, table 3-25
            let rd = ((inst_2 >> 8) & 0xf) as usize;
            let rm = (inst_2 & 0xf) as usize;
            let op = (inst >> 5) & 0xf;
            let set = inst & 0x10 != 0;
            let (shift_t, shift_n) = decode_imm_shift(inst_2);
            let (shifted, carry) = self.shift_c(self.regs.r[rm], shift_t, shift_n);
            if op == 6 {
                // PKHBT, PKHTB: the tb bit is the shift type's top bit
                let rrn = self.regs.r[rn];
                self.regs.r[rd] = if inst_2 & 0x20 == 0 {
                    (shifted & 0xffff_0000) | (rrn & 0xffff)
                } else {
                    (rrn & 0xffff_0000) | (shifted & 0xffff)
                };
                return;
            }
            // MOV and MVN are ORR and ORN with rn = pc
            let rrn = if rn == 15 { 0 } else { self.regs.r[rn] };
            self.data_processing(op, rd, rrn, shifted, set, carry);
        } else if inst & 0xe40 == 0x800 {
            self.load_store_multiple(inst, inst_2);
        } else if inst & 0xe40 == 0x840 {
            self.load_store_dual_exclusive(inst, inst_2);
        } else if inst & 0xc00 == 0xc00 {
            self.fp_instr(inst, inst_2);
        } else {
            todo!("inst {inst:04x} {inst_2:04x} not implemented");
        }
    }

    /// LDM, STM, PUSH and POP, table 3-11
    fn load_store_multiple(&mut self, inst: u16, inst_2: u16) {
        let rn = (inst & 0xf) as usize;
        let load = inst & 0x10 != 0;
        let wback = inst & 0x20 != 0;
        let n = inst_2.count_ones();
        let rrn = self.regs.r[rn];
        // increment after, or decrement before
        let (start, end) = match (inst >> 7) & 3 {
            1 => (rrn, rrn.wrapping_add(n * 4)),
            2 => (rrn.wrapping_sub(n * 4), rrn.wrapping_sub(n * 4)),
            _ => todo!("inst {inst:04x} {inst_2:04x} not implemented"),
        };
        let mut addr = start;
        self.check_aligned(addr);
        if load {
            let mut vals = [0; 16];
            for (i, val) in vals.iter_mut().enumerate() {
                if inst_2 & (1 << i) != 0 {
                    *val = self.load_u32(addr);
                    addr = addr.wrapping_add(4);
                }
            }
            if wback && inst_2 & (1 << rn) == 0 {
                self.regs.r[rn] = end;
            }
            for (i, val) in vals.iter().enumerate() {
                if inst_2 & (1 << i) != 0 {
                    self.regs.r[i] = *val;
                }
            }
            self.regs.r[15] &= !1;
        } else {
            for i in 0..16 {
                if inst_2 & (1 << i) != 0 {
                    self.store_u32(addr, self.regs.r[i]);
                    addr = addr.wrapping_add(4);
                }
            }
            if wback {
                self.regs.r[rn] = end;
            }
        }
    }

    /// LDRD and STRD, the exclusives, load-acquire and store-release, and
    /// table branches, table 3-12
    fn load_store_dual_exclusive(&mut self, inst: u16, inst_2: u16) {
        let rn = (inst & 0xf) as usize;
        let rt = ((inst_2 >> 12) & 0xf) as usize;
        let rd = ((inst_2 >> 8) & 0xf) as usize;
        let op1 = (inst >> 7) & 3;
        let op2 = (inst >> 4) & 3;
        let rrn = self.regs.r[rn];
        if op1 >= 2 || op2 >= 2 {
            // LDRD, STRD (immediate); literal when rn is pc
            let base = if rn == 15 { rrn & !3 } else { rrn };
            let imm = (inst_2 & 0xff) as u32 * 4;
            let index = inst & 0x100 != 0;
            let add = inst & 0x80 != 0;
            let wback = inst & 0x20 != 0;
            let offset_addr = if add {
                base.wrapping_add(imm)
            } else {
                base.wrapping_sub(imm)
            };
            let addr = if index { offset_addr } else { base };
            self.check_aligned(addr);
            if inst & 0x10 != 0 {
                self.regs.r[rt] = self.load_u32(addr);
                self.regs.r[rd] = self.load_u32(addr.wrapping_add(4));
            } else {
                self.store_u32(addr, self.regs.r[rt]);
                self.store_u32(addr.wrapping_add(4), self.regs.r[rd]);
            }
            if wback {
                self.regs.r[rn] = offset_addr;
            }
        } else if op1 == 0 {
            // LDREX, STREX
            let addr = rrn.wrapping_add((inst_2 & 0xff) as u32 * 4);
            self.check_aligned(addr);
            if op2 == 1 {
                self.regs.r[rt] = self.load_u32(addr);
                self.exclusive = true;
            } else {
                self.store_exclusive(rd, addr, self.regs.r[rt], 4);
            }
        } else {
            // the byte and halfword exclusives, acquire and release, and
            // table branches, selected by op3
            let op3 = (inst_2 >> 4) & 0xf;
            let size = 1 << (op3 & 3);
            let rd = (inst_2 & 0xf) as usize;
            if op2 == 1 {
                match op3 {
                    0 | 1 => {
                        // TBB, TBH
                        let rm = (inst_2 & 0xf) as usize;
                        let base = if rn == 15 { self.regs.r[15] } else { rrn };
                        let half = if op3 == 0 {
                            self.load_u8(base.wrapping_add(self.regs.r[rm])) as u32
                        } else {
                            self.load_u16(base.wrapping_add(self.regs.r[rm] << 1)) as u32
                        };
                        self.regs.r[15] = self.regs.r[15].wrapping_add(half * 2);
                    }
                    4 | 5 | 8..=10 | 12..=14 => {
                        // LDREXB/H, LDA, LDAEX
                        self.regs.r[rt] = match size {
                            1 => self.load_u8(rrn) as u32,
                            2 => self.load_u16(rrn) as u32,
                            _ => self.load_u32(rrn),
                        };
                        if op3 & 4 != 0 {
                            self.exclusive = true;
                        }
                    }
                    _ => todo!("inst {inst:04x} {inst_2:04x} not implemented"),
                }
            } else {
                let val = self.regs.r[rt];
                match op3 {
                    4 | 5 | 12..=14 => self.store_exclusive(rd, rrn, val, size),
                    8..=10 => self.store_sized(rrn, val, size),
                    _ => todo!("inst {inst:04x} {inst_2:04x} not implemented"),
                }
            }
        }
    }

    fn store_exclusive(&mut self, rd: usize, addr: u32, val: u32, size: u32) {
        if self.exclusive {
            self.store_sized(addr, val, size);
            self.regs.r[rd] = 0;
        } else {
            self.regs.r[rd] = 1;
        }
        self.exclusive = false;
    }

    fn store_sized(&mut self, addr: u32, val: u32, size: u32) {
        match size {
            1 => self.store_u8(addr, val as u8),
            2 => self.store_u16(addr, val as u16),
            _ => self.store_u32(addr, val),
        }
    }

    fn inst_f(&mut self, inst: u16) {
        let inst_2 = self.load_u16(self.regs.r[15]);
        self.regs.r[15] += 2;
        if inst & 0xff00 == 0xfe00 {
            self.fp_unconditional(inst, inst_2);
        } else if inst & 0x800 == 0 && (inst_2 & 0x8000) != 0 {
            self.branch_misc(inst, inst_2);
        } else if inst & 0xe00 == 0x800 {
            self.load_store_single(inst, inst_2);
        } else if inst & 0xa00 == 0 {
            // data processing, modified 12-bit immediate (see 3.3.1)
            let op = (inst >> 5) & 0xf;
            let rn = (inst & 0xf) as usize;
            // MOV and MVN (immediate) are ORR and ORN with rn = pc
            let rrn = if rn == 15 { 0 } else { self.regs.r[rn] };
            let rd = ((inst_2 >> 8) & 0xf) as usize;
            let imm = ((inst & 0x400) << 1) | ((inst_2 & 0x7000) >> 4) | (inst_2 & 0xff);
            let (imm32, carry) = self.thumb_expand_imm_c(imm);
            let set = inst & 0x10 != 0;
            self.data_processing(op, rd, rrn, imm32, set, carry);
        } else if inst & 0xa00 == 0x200 {
            self.plain_immediate(inst, inst_2);
        } else if inst & 0xf00 == 0xa00 {
            self.data_processing_register(inst, inst_2);
        } else if inst & 0xf80 == 0xb00 {
            // multiply and multiply accumulate, table 3-30
            let rn = (inst & 0xf) as usize;
            let ra = ((inst_2 >> 12) & 0xf) as usize;
            let rd = ((inst_2 >> 8) & 0xf) as usize;
            let rm = (inst_2 & 0xf) as usize;
            let op1 = (inst >> 4) & 7;
            let op2 = (inst_2 >> 4) & 3;
            let product = self.regs.r[rn].wrapping_mul(self.regs.r[rm]);
            self.regs.r[rd] = match (op1, op2) {
                // MUL
                (0, 0) if ra == 15 => product,
                // MLA
                (0, 0) => self.regs.r[ra].wrapping_add(product),
                // MLS
                (0, 1) => self.regs.r[ra].wrapping_sub(product),
                _ => todo!("inst {inst:04x} {inst_2:04x} not implemented"),
            };
        } else if inst & 0xf80 == 0xb80 {
            // long multiply and divide, table 3-31
            let rn = self.regs.r[(inst & 0xf) as usize];
            let rlo = ((inst_2 >> 12) & 0xf) as usize;
            let rd = ((inst_2 >> 8) & 0xf) as usize;
            let rm = self.regs.r[(inst_2 & 0xf) as usize];
            let op1 = (inst >> 4) & 7;
            let op2 = (inst_2 >> 4) & 0xf;
            let acc = ((self.regs.r[rd] as u64) << 32) | self.regs.r[rlo] as u64;
            let long = match (op1, op2) {
                // SMULL, UMULL, SMLAL, UMLAL
                (0, 0) => (rn as i32 as i64 * rm as i32 as i64) as u64,
                (2, 0) => rn as u64 * rm as u64,
                (4, 0) => acc.wrapping_add((rn as i32 as i64 * rm as i32 as i64) as u64),
                (6, 0) => acc.wrapping_add(rn as u64 * rm as u64),
                // SDIV, UDIV: divide by zero gives 0, without DIV_0_TRP
                (1, 15) => {
                    self.regs.r[rd] = (rn as i32).checked_div(rm as i32).unwrap_or(
                        // i32::MIN / -1 overflows to i32::MIN
                        if rm == 0 { 0 } else { i32::MIN },
                    ) as u32;
                    return;
                }
                (3, 15) => {
                    self.regs.r[rd] = rn.checked_div(rm).unwrap_or(0);
                    return;
                }
                _ => todo!("inst {inst:04x} {inst_2:04x} not implemented"),
            };
            self.regs.r[rlo] = long as u32;
            self.regs.r[rd] = (long >> 32) as u32;
        } else {
            todo!("inst {inst:04x} {inst_2:04x} not implemented");
        }
    }

    /// Branches and miscellaneous control, table 3-9
    fn branch_misc(&mut self, inst: u16, inst_2: u16) {
        let s = (inst >> 10) as u32 & 1;
        let j1 = (inst_2 >> 13) as u32 & 1;
        let j2 = (inst_2 >> 11) as u32 & 1;
        let imm11 = (inst_2 & 0x7ff) as u32;
        if inst_2 & 0x1000 == 0 {
            let cond = (inst >> 6) & 0xf;
            if cond >= 0xe {
                // hints, barriers, MSR and MRS: nothing to model
                return;
            }
            // B, T3 encoding: conditional
            let imm6 = (inst & 0x3f) as u32;
            let imm = (s << 20) | (j2 << 19) | (j1 << 18) | (imm6 << 12) | (imm11 << 1);
            let imm = ((imm << 11) as i32 >> 11) as u32;
            if self.eval_cond(cond) {
                self.regs.r[15] = self.regs.r[15].wrapping_add(imm);
            }
        } else {
            // B, T4 encoding, and BL
            let i1 = !(j1 ^ s) & 1;
            let i2 = !(j2 ^ s) & 1;
            let imm10 = (inst & 0x3ff) as u32;
            let imm = (s << 24) | (i1 << 23) | (i2 << 22) | (imm10 << 12) | (imm11 << 1);
            let imm = ((imm << 7) as i32 >> 7) as u32;
            if inst_2 & 0x4000 != 0 {
                self.regs.r[14] = self.regs.r[15] | 1;
            }
            self.regs.r[15] = self.regs.r[15].wrapping_add(imm);
        }
    }

    /// Load and store single data item, tables 3-13 to 3-15
    fn load_store_single(&mut self, inst: u16, inst_2: u16) {
        let rn = (inst & 0xf) as usize;
        let rt = ((inst_2 >> 12) & 0xf) as usize;
        let size = (inst >> 5) & 3;
        let load = inst & 0x10 != 0;
        if load && rt == 15 && size != 2 {
            // preload hints
            return;
        }
        let rrn = self.regs.r[rn];
        let imm8 = inst_2 as u32 & 0xff;
        let mut wback = None;
        let addr = if rn == 15 {
            // literal: the U bit is in the first halfword
            let imm12 = inst_2 as u32 & 0xfff;
            let base = rrn & !3;
            if inst & 0x80 != 0 {
                base.wrapping_add(imm12)
            } else {
                base.wrapping_sub(imm12)
            }
        } else if inst & 0x80 != 0 {
            rrn.wrapping_add(inst_2 as u32 & 0xfff)
        } else if inst_2 & 0x800 != 0 {
            // imm8 with P, U and W bits
            let add = inst_2 & 0x200 != 0;
            let offset_addr = if add {
                rrn.wrapping_add(imm8)
            } else {
                rrn.wrapping_sub(imm8)
            };
            let index = inst_2 & 0x400 != 0;
            if inst_2 & 0x100 != 0 {
                wback = Some(offset_addr);
            }
            if index { offset_addr } else { rrn }
        } else if inst_2 & 0xfc0 == 0 {
            let shift = (inst_2 >> 4) & 3;
            let rm = (inst_2 & 0xf) as usize;
            rrn.wrapping_add(self.regs.r[rm] << shift)
        } else {
            todo!("inst {inst:04x} {inst_2:04x} not implemented");
        };
        if load {
            let sign_extend = inst & 0x100 != 0;
            let val = match (size, sign_extend) {
                (0, false) => self.load_u8(addr) as u32,
                (0, true) => self.load_u8(addr) as i8 as u32,
                (1, false) => self.load_u16(addr) as u32,
                (1, true) => self.load_u16(addr) as i16 as u32,
                (2, _) => self.load_u32(addr),
                _ => todo!("inst {inst:04x} {inst_2:04x} not implemented"),
            };
            if let Some(a) = wback {
                self.regs.r[rn] = a;
            }
            self.regs.r[rt] = val;
            if rt == 15 {
                self.regs.r[15] &= !1;
            }
        } else {
            let val = self.regs.r[rt];
            match size {
                0 => self.store_u8(addr, val as u8),
                1 => self.store_u16(addr, val as u16),
                2 => self.store_u32(addr, val),
                _ => todo!("inst {inst:04x} {inst_2:04x} not implemented"),
            }
            if let Some(a) = wback {
                self.regs.r[rn] = a;
            }
        }
    }

    /// Data processing, plain binary immediate, table 3-24
    fn plain_immediate(&mut self, inst: u16, inst_2: u16) {
        let rn = (inst & 0xf) as usize;
        let rd = ((inst_2 >> 8) & 0xf) as usize;
        let rrn = self.regs.r[rn];
        let imm12 = (((inst & 0x400) << 1) | ((inst_2 & 0x7000) >> 4) | (inst_2 & 0xff)) as u32;
        let imm16 = imm12 | ((inst as u32 & 0xf) << 12);
        // for the bit field and saturating ops
        let lsb = ((inst_2 as u32 >> 6) & 3) | ((inst_2 as u32 & 0x7000) >> 10);
        let field = inst_2 as u32 & 0x1f;
        self.regs.r[rd] = match (inst >> 4) & 0x1f {
            // ADDW, SUBW, and ADR when rn is pc
            0 if rn == 15 => (rrn & !3).wrapping_add(imm12),
            0 => rrn.wrapping_add(imm12),
            0xa if rn == 15 => (rrn & !3).wrapping_sub(imm12),
            0xa => rrn.wrapping_sub(imm12),
            // MOVW, MOVT
            0x4 => imm16,
            0xc => (self.regs.r[rd] & 0xffff) | (imm16 << 16),
            // SSAT, USAT: the sh bit selects asr, and Q isn't modeled
            0x10 | 0x12 | 0x18 | 0x1a => {
                let shift_t = if inst & 0x20 != 0 {
                    ShiftType::Asr
                } else {
                    ShiftType::Lsl
                };
                let val = self.shift_c(rrn, shift_t, lsb).0 as i32 as i64;
                if inst & 0x80 == 0 {
                    let bits = field + 1;
                    let max = (1i64 << (bits - 1)) - 1;
                    val.clamp(-max - 1, max) as u32
                } else {
                    let max = (1i64 << field) - 1;
                    val.clamp(0, max) as u32
                }
            }
            // SBFX, UBFX
            0x14 | 0x1c => {
                let width = field + 1;
                let v = (rrn as u64 >> lsb) as u32;
                if width == 32 {
                    v
                } else if inst & 0x80 != 0 {
                    v & ((1 << width) - 1)
                } else {
                    ((v << (32 - width)) as i32 >> (32 - width)) as u32
                }
            }
            // BFI, and BFC when rn is pc
            0x16 => {
                let msb = field;
                let mask = (((2u64 << msb) - 1) as u32) & !((1u32 << lsb) - 1);
                let src = if rn == 15 { 0 } else { rrn << lsb };
                (self.regs.r[rd] & !mask) | (src & mask)
            }
            _ => todo!("inst {inst:04x} {inst_2:04x} not implemented"),
        };
    }

    /// Data processing (register), table 3-26
    fn data_processing_register(&mut self, inst: u16, inst_2: u16) {
        let rn = (inst & 0xf) as usize;
        let rd = ((inst_2 >> 8) & 0xf) as usize;
        let rm = (inst_2 & 0xf) as usize;
        let rrm = self.regs.r[rm];
        if inst & 0x80 == 0 && inst_2 & 0xf0 == 0 {
            // LSL, LSR, ASR, ROR (register)
            let shift_t = match (inst >> 5) & 3 {
                0 => ShiftType::Lsl,
                1 => ShiftType::Lsr,
                2 => ShiftType::Asr,
                _ => ShiftType::Ror,
            };
            let (val, carry) = self.shift_c(self.regs.r[rn], shift_t, rrm & 0xff);
            self.regs.r[rd] = val;
            if inst & 0x10 != 0 {
                self.set_nz(val);
                self.set_c(carry);
            }
        } else if inst & 0x80 == 0 && inst_2 & 0x80 != 0 {
            // sign or zero extend, with optional add, table 3-28
            let rot = ((inst_2 as u32 >> 4) & 3) * 8;
            let v = rrm.rotate_right(rot);
            let ext = match (inst >> 4) & 7 {
                0 => v as i16 as u32,
                1 => v as u16 as u32,
                4 => v as i8 as u32,
                5 => v as u8 as u32,
                _ => todo!("inst {inst:04x} {inst_2:04x} not implemented"),
            };
            let add = if rn == 15 { 0 } else { self.regs.r[rn] };
            self.regs.r[rd] = add.wrapping_add(ext);
        } else if inst & 0x80 != 0 && inst_2 & 0xc0 == 0x80 {
            // miscellaneous operations, table 3-30
            let op = (inst >> 4) & 7;
            let op2 = (inst_2 >> 4) & 7;
            self.regs.r[rd] = match (op, op2) {
                (1, 0) => rrm.swap_bytes(),
                (1, 1) => rev16(rrm),
                (1, 2) => rrm.reverse_bits(),
                (1, 3) => revsh(rrm),
                (3, 0) => rrm.leading_zeros(),
                _ => todo!("inst {inst:04x} {inst_2:04x} not implemented"),
            };
        } else {
            // parallel add and subtract
            todo!("inst {inst:04x} {inst_2:04x} not implemented");
        }
    }

    /// ThumbExpandImm_C: the carry out is the top bit when the value is
    /// rotated, otherwise unchanged.
    fn thumb_expand_imm_c(&self, imm12: u16) -> (u32, u32) {
        if (imm12 >> 10) == 0 {
            const EXP: [u32; 4] = [1, 0x10001, 0x1000100, 0x1010101];
            let carry = (self.regs.psr >> 29) & 1;
            (EXP[imm12 as usize >> 8] * (imm12 as u32 & 0xff), carry)
        } else {
            let unrotated = 0x80 | (imm12 as u32 & 0x7f);
            let v = unrotated.rotate_right(imm12 as u32 >> 7);
            (v, v >> 31)
        }
    }

    /// 32 bit data processing, with an immediate or shifted register
    /// operand (tables 3-21 and 3-25). Rd = 15 with S set is the test form
    /// (TST, TEQ, CMN, CMP). Logical ops with S set C to `carry`, the
    /// carry out of the shift or immediate expansion.
    fn data_processing(
        &mut self,
        op: u16,
        rd: usize,
        rn: u32,
        operand: u32,
        set: bool,
        carry: u32,
    ) {
        let carry_in = (self.regs.psr >> 29) & 1;
        let test = rd == 15 && set;
        let val = match op {
            0 => rn & operand,  // AND, TST
            1 => rn & !operand, // BIC
            2 => rn | operand,  // ORR, MOV when rn is pc
            3 => rn | !operand, // ORN, MVN when rn is pc
            4 => rn ^ operand,  // EOR, TEQ
            8 => {
                // ADD, CMN
                let v = self.add_flags(rn, operand, 0, set);
                if !test {
                    self.regs.r[rd] = v;
                }
                return;
            }
            10 => {
                self.regs.r[rd] = self.add_flags(rn, operand, carry_in, set); // ADC
                return;
            }
            11 => {
                self.regs.r[rd] = self.add_flags(rn, !operand, carry_in, set); // SBC
                return;
            }
            13 => {
                // SUB, CMP
                let v = self.add_flags(rn, !operand, 1, set);
                if !test {
                    self.regs.r[rd] = v;
                }
                return;
            }
            14 => {
                self.regs.r[rd] = self.add_flags(operand, !rn, 1, set); // RSB
                return;
            }
            _ => todo!("data processing op {op}"),
        };
        if set {
            self.set_nz(val);
            self.set_c(carry);
        }
        if !test {
            self.regs.r[rd] = val;
        }
    }
}

fn decode_imm_shift(inst_2: u16) -> (ShiftType, u32) {
    let ty = (inst_2 >> 4) & 3;
    let imm5 = ((inst_2 as u32 >> 6) & 3) | (inst_2 as u32 & 0x7000) >> 10;
    match ty {
        0 => (ShiftType::Lsl, imm5),
        1 => (ShiftType::Lsr, if imm5 == 0 { 32 } else { imm5 }),
        2 => (ShiftType::Asr, if imm5 == 0 { 32 } else { imm5 }),
        3 if imm5 == 0 => (ShiftType::Rrx, 1),
        _ => (ShiftType::Ror, imm5),
    }
}

fn rev16(v: u32) -> u32 {
    ((v & 0x00ff_00ff) << 8) | ((v >> 8) & 0x00ff_00ff)
}

fn revsh(v: u32) -> u32 {
    (v as u16).swap_bytes() as i16 as u32
}
