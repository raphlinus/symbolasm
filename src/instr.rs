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
        } else {
            // 32 bit instructions
            let inst_2 = self.load_u16(self.regs.r[15]);
            self.regs.r[15] += 2;
            let rn = (inst & 0xf) as usize;
            if inst & 0xe00 == 0xa00 {
                // Data processing, constant shift
                let rd = ((inst_2 >> 8) & 0xf) as usize;
                let rm = (inst_2 & 0xf) as usize;
                let (shift_t, shift_n) = decode_imm_shift(inst_2);
                let shifted = self.apply_imm_shift(self.regs.r[rm], shift_t, shift_n);
                let op = (inst >> 5) & 0xf;
                let set = inst & 0x10 != 0;
                // TODO: the shifter's carry out, for logical ops with S
                let rrn = if rn == 15 { 0 } else { self.regs.r[rn] };
                self.data_processing(op, rd, rrn, shifted, set);
            } else if inst & 0xe50 == 0x840 {
                // STRD (immediate)
                let rt = ((inst_2 >> 12) & 0xf) as usize;
                let rt2 = ((inst_2 >> 8) & 0xf) as usize;
                let imm32 = (inst_2 & 0xff) as u32 * 4;
                let index = (inst & 0x100) != 0;
                let add = (inst & 0x80) != 0;
                let wback = (inst & 0x20) != 0;
                let rrn = self.regs.r[rn];
                let offset_addr = if add { rrn + imm32 } else { rrn - imm32 };
                let addr = if index { offset_addr } else { rrn };
                self.store_u32(addr, self.regs.r[rt]);
                self.store_u32(addr + 4, self.regs.r[rt2]);
                if wback {
                    self.regs.r[rn] = offset_addr;
                }
            } else if inst == 0xe8bd {
                self.pop_regs(inst_2);
            } else if inst == 0xe92d {
                self.push_regs(inst_2);
            } else {
                todo!("inst {inst:04x} not implemented");
            }
        }
    }

    fn inst_f(&mut self, inst: u16) {
        let inst_2 = self.load_u16(self.regs.r[15]);
        self.regs.r[15] += 2;
        if inst & 0x800 == 0 && (inst_2 & 0x8000) != 0 {
            todo!("branches");
        } else if inst & 0xbf0 == 0x240 {
            // MOV (immediate, T3 encoding)
            let rd = ((inst_2 >> 8) & 0xf) as usize;
            let imm = (inst_2 & 0xff)
                | ((inst_2 & 0x7000) >> 4)
                | ((inst & 0x400) << 1)
                | ((inst & 0xf) << 12);
            self.regs.r[rd] = imm as u32;
        } else if inst & 0xbff == 0x20f {
            // ADR (T3 encoding)
            let imm = ((inst & 0x400) << 1) | ((inst_2 & 0x7000) >> 4) | (inst_2 & 0xff);
            let rd = ((inst_2 >> 8) & 0xf) as usize;
            let align_pc = self.regs.r[15] & !4;
            self.regs.r[rd] = align_pc.wrapping_add(imm as u32);
        } else if inst & 0xe00 == 0x800 {
            // load and store single data item
            let rn = (inst & 0xf) as usize;
            let rrn = self.regs.r[rn];
            let rt = ((inst_2 >> 12) & 0xf) as usize;
            let size = (inst >> 5) & 3;
            let addr = if (inst & 0x80) != 0 {
                let imm12 = inst_2 as u32 & 0xfff;
                rrn + imm12
            } else if rn == 15 {
                let imm12 = inst_2 as u32 & 0xfff;
                rrn - imm12
            } else if inst_2 & 0xf00 == 0xc00 {
                let imm8 = inst_2 as u32 & 0xff;
                rrn - imm8
            } else if inst_2 & 0xd00 == 0x900 {
                // post indexed
                rrn
            } else if inst_2 & 0xd00 == 0xd00 {
                let imm = inst_2 as u32 & 0xff;
                if (inst_2 & 0x200) != 0 {
                    rrn + imm
                } else {
                    rrn - imm
                }
            } else if inst_2 & 0xfc0 == 0 {
                let shift = (inst_2 >> 4) & 3;
                let rm = (inst_2 & 0xf) as usize;
                rrn + (self.regs.r[rm] << shift)
            } else {
                todo!()
            };
            if inst & 0x10 != 0 {
                if size == 2 {
                    self.regs.r[rt] = self.load_u32(addr);
                } else {
                    let sign_extend = inst & 0x100 != 0;
                    if size == 0 && sign_extend {
                        self.regs.r[rt] = self.load_u8(addr) as i8 as u32;
                    } else if size == 0 {
                        self.regs.r[rt] = self.load_u8(addr) as u32;
                    } else if sign_extend {
                        self.regs.r[rt] = self.load_u16(addr) as i16 as u32;
                    } else {
                        self.regs.r[rt] = self.load_u16(addr) as u32;
                    }
                }
            } else {
                let val = self.regs.r[rt];
                match size {
                    0 => self.store_u8(addr, val as u8),
                    1 => self.store_u16(addr, val as u16),
                    2 => self.store_u32(addr, val),
                    _ => todo!(),
                }
            }
            // handle post-indexing
            if inst_2 & 0xd00 == 0x900 {
                let imm = inst_2 as u32 & 0xff;
                self.regs.r[rn] = if (inst_2 & 0x200) != 0 {
                    rrn + imm
                } else {
                    rrn - imm
                };
            }
        } else if inst & 0xa00 == 0 {
            // data processing, modified 12-bit immediate (see 3.3.1)
            let op = (inst >> 5) & 0xf;
            let rn = (inst & 0xf) as usize;
            // MOV and MVN (immediate) are ORR and ORN with rn = pc
            let rrn = if rn == 15 { 0 } else { self.regs.r[rn] };
            let rd = ((inst_2 >> 8) & 0xf) as usize;
            let imm = ((inst & 0x400) << 1) | ((inst_2 & 0x7000) >> 4) | (inst_2 & 0xff);
            let imm32 = thumb_expand_imm(imm);
            let set = inst & 0x10 != 0;
            self.data_processing(op, rd, rrn, imm32, set);
        } else if inst & 0xf10 == 0x300 {
            // bit field operations, saturation with shift (see 3.3.1)
            let op = (inst >> 5) & 7;
            let rn = (inst & 0xf) as usize;
            let rrn = self.regs.r[rn];
            let rd = ((inst_2 >> 8) & 0xf) as usize;
            let imm5 = inst_2 & 0x1f;
            let imm3_2 = ((inst_2 >> 6) & 3) | ((inst_2 & 0x7000) >> 10);
            match op {
                3 => {
                    // BFI
                    let mask = (2u32 << imm5).wrapping_sub(1 << imm3_2);
                    self.regs.r[rd] = (self.regs.r[rd] & !mask) | ((rrn << imm3_2) & mask);
                }
                6 => {
                    // UBFX
                    self.regs.r[rd] = (rrn >> imm3_2) & ((2u32 << imm5).wrapping_sub(1));
                }
                _ => todo!(),
            }
        } else if inst & 0xf00 == 0xa00 {
            // rows 3-7 of figure 3-5
            let rn = (inst & 0xf) as usize;
            let rd = ((inst_2 >> 8) & 0xf) as usize;
            let rm = (inst_2 & 0xf) as usize;
            if inst & 0x80 == 0 {
                if inst_2 & 0x80 == 0 {
                    // register controlled shift, table 3-27
                    todo!();
                } else {
                    // sign or zero extension, with optional addition, table 3-28
                    let op = (inst >> 4) & 7;
                    if rn == 15 {
                        match op {
                            5 => {
                                // UXTB
                                let rot = (inst_2 as u32 >> 4) & 3;
                                self.regs.r[rd] =
                                    self.regs.r[rm].rotate_right(rot * 8) as u8 as u32;
                            }
                            _ => todo!(),
                        }
                    } else {
                        todo!(); // might hoist this
                    }
                }
            } else if inst_2 & 0x80 == 0 {
                // SIMD add or subtract, table 3-29
                todo!();
            } else {
                // other three register data processing, table 3-31
                let op = (inst >> 4) & 7;
                let op2 = (inst_2 >> 4) & 7;
                match (op, op2) {
                    (1, 2) => self.regs.r[rd] = self.regs.r[rm].reverse_bits(),
                    (3, 0) => self.regs.r[rd] = self.regs.r[rm].leading_zeros(),
                    _ => todo!("inst {inst:04x} {inst_2:04x}"),
                }
            }
        } else {
            todo!("inst {inst:04x} not implemented");
        }
    }
}

impl Cpu {
    /// 32 bit data processing, with an immediate or shifted register
    /// operand (tables 3-21 and 3-25). Rd = 15 with S set is the test form
    /// (TST, TEQ, CMN, CMP). The carry out of a shift or rotated immediate
    /// isn't modeled for logical ops.
    fn data_processing(&mut self, op: u16, rd: usize, rn: u32, operand: u32, set: bool) {
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

fn thumb_expand_imm(imm12: u16) -> u32 {
    if (imm12 >> 10) == 0 {
        const EXP: [u32; 4] = [1, 0x10001, 0x1000100, 0x1010101];
        EXP[imm12 as usize >> 8] * (imm12 as u32 & 0xff)
    } else {
        let unrotated = 0x80 | (imm12 as u32 & 0x7f);
        unrotated.rotate_right(imm12 as u32 >> 7)
    }
}
