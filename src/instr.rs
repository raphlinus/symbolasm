use crate::cpu::{Cpu, ShiftType};

impl Cpu {
    pub fn step_instr(&mut self) {
        let inst_16 = self.load_u16(self.regs.r[15]);
        println!("inst: {inst_16:04x}");
        self.regs.r[15] += 2;
        match inst_16 >> 12 {
            0x2 => self.inst_2(inst_16),
            0x3 => self.inst_3(inst_16),
            0x4 => self.inst_4(inst_16),
            0x5 => self.inst_5(inst_16),
            0x7 => self.inst_7(inst_16),
            0x8 => self.inst_8(inst_16),
            0xa => self.inst_a(inst_16),
            0xb => self.inst_b(inst_16),
            0xc => self.inst_c(inst_16),
            0xe => self.inst_e(inst_16),
            0xf => self.inst_f(inst_16),
            _ => todo!("inst {inst_16:04x} not implemented"),
        }
    }

    fn inst_2(&mut self, inst: u16) {
        let imm8 = (inst & 0xff) as u32;
        let rd = ((inst >> 8) & 7) as usize;
        if inst & 0x800 == 0 {
            // MOV (immediate, T1 encoding)
            self.regs.r[rd] = imm8;
        } else {
            // CMP (immediate, T1 encoding)
            _ = self.adc(self.regs.r[rd], !imm8, 1);
        }
    }

    fn inst_3(&mut self, inst: u16) {
        let imm8 = (inst & 0xff) as u32;
        let rd = ((inst >> 8) & 7) as usize;
        if inst & 0x800 == 0 {
            // ADDS (immediate, T2 encoding)
            self.regs.r[rd] = self.adc(self.regs.r[rd], imm8, 0);
        } else {
            // SUBS (immediate, T2 encoding)
            self.regs.r[rd] = self.adc(self.regs.r[rd], !imm8, 1);
        }
    }

    fn inst_4(&mut self, inst: u16) {
        if inst & 0xc00 == 0 {
            let op = (inst >> 6) & 0xf;
            let rm = ((inst >> 3) & 7) as usize;
            let rdn = (inst & 7) as usize;
            let rrm = self.regs.r[rm];
            let rrn = self.regs.r[rdn];
            let val = match op {
                0 => rrn & rrm,  // EORS
                1 => rrn ^ rrm,  // EORS
                12 => rrn | rrm, // ORRS
                _ => todo!(),
            };
            self.regs.r[rdn] = val;
            self.set_nz(val);
        } else if inst & 0x800 == 0 {
            // special data processing + branch/exchange
            if inst & 0xf00 == 0x600 {
                // MOV (register, T1 encoding)
                let rm = ((inst >> 3) & 0xf) as usize;
                let rd = (((inst & 0x80) >> 4) | (inst & 7)) as usize;
                let rrm = self.regs.r[rm];
                self.regs.r[rd] = rrm;
            } else {
                todo!();
            }
        } else {
            // load from literal pool
            todo!()
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
            let align_pc = (self.regs.r[15] + 2) & !4;
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
            _ => todo!(),
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

    fn inst_e(&mut self, inst: u16) {
        if inst & 0x800 == 0 {
            // TODO: unconditional branch
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
                match op {
                    2 => {
                        // ORR (register, T2 encoding)
                        self.regs.r[rd] = self.regs.r[rn] | shifted;
                        // TODO: handle flags when S is set
                    }
                    4 => {
                        // EOR (register, T2 encoding)
                        self.regs.r[rd] = self.regs.r[rn] ^ shifted;
                        // TODO: handle flags when S is set
                    }
                    8 => {
                        // ADD (register, T3 encoding)
                        self.regs.r[rd] = self.regs.r[rn].wrapping_add(shifted);
                        // TODO: handle flags when S is set
                    }
                    _ => todo!("unimplemented data processing op {op}"),
                }
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
        if (inst_2 & 0x8000) != 0 {
            todo!("branches");
        } else if inst & 0xbef == 0x04f {
            // MOV (immediate, T2 encoding)
            todo!();
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
            let rrn = self.regs.r[rn];
            let rd = ((inst_2 >> 8) & 0xf) as usize;
            let imm = ((inst & 0x400) << 1) | ((inst_2 & 0x7000) >> 4) | (inst_2 & 0xff);
            let imm32 = thumb_expand_imm(imm);
            if rd == 15 {
                todo!()
            } else {
                match op {
                    0 => self.regs.r[rd] = rrn & imm32,
                    _ => todo!(),
                }
            }
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
                todo!();
            }
        } else {
            todo!("inst {inst:04x} not implemented");
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
