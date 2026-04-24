pub struct Cpu {
    regs: Regs,
    mem: Vec<u8>,
}

pub struct Regs {
    r: [u32; 16],
    psr: u32,
}

enum ShiftType {
    Lsl,
    Lsr,
    Asr,
    Rrx,
    Ror,
}

impl Cpu {
    fn load_u8(&self, addr: u32) -> u8 {
        self.mem[addr as usize]
    }

    fn load_u16(&self, addr: u32) -> u16 {
        u16::from_le_bytes(self.mem[addr as usize..][..2].try_into().unwrap())
    }

    fn load_u32(&self, addr: u32) -> u32 {
        u32::from_le_bytes(self.mem[addr as usize..][..4].try_into().unwrap())
    }

    fn store_u8(&mut self, addr: u32, val: u8) {
        self.mem[addr as usize] = val;
    }

    fn store_u16(&mut self, addr: u32, val: u16) {
        self.mem[addr as usize..][..2].copy_from_slice(&u16::to_le_bytes(val));
    }

    fn store_u32(&mut self, addr: u32, val: u32) {
        self.mem[addr as usize..][..4].copy_from_slice(&u32::to_le_bytes(val));
    }

    fn pop(&mut self) -> u32 {
        let result = self.load_u32(self.regs.r[13]);
        self.regs.r[13] += 2;
        result
    }

    fn push(&mut self, val: u32) {
        self.regs.r[13] -= 2;
        self.store_u32(self.regs.r[13], val);
    }

    fn step_instr(&mut self) {
        let inst_16 = self.load_u16(self.regs.r[15]);
        self.regs.r[15] += 2;
        match inst_16 >> 12 {
            0x5 => self.inst_5(inst_16),
            0x7 => self.inst_7(inst_16),
            0x8 => self.inst_8(inst_16),
            0xa => self.inst_a(inst_16),
            0xc => self.inst_c(inst_16),
            0xe => self.inst_e(inst_16),
            0xf => self.inst_f(inst_16),
            _ => todo!("inst {inst_16:04x} not implemented"),
        }
    }

    fn inst_5(&mut self, inst: u16) {
        let opcode = (inst >> 9) & 7;
        let rd = (inst & 7) as usize;
        let rn = ((inst >> 3) & 7) as usize;
        let rm = ((inst >> 6) & 7) as usize;
        todo!()
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
        } else {
            todo!("inst {inst:04x} not implemented");
        }
    }

    fn pop_regs(&mut self, regs: u16) {
        for i in 0..16 {
            if (regs & (1 << i)) != 0 {
                self.regs.r[i as usize] = self.pop();
            }
        }
    }

    fn push_regs(&mut self, regs: u16) {
        for i in (0..16).rev() {
            if (regs & (1 << i)) != 0 {
                self.push(self.regs.r[i as usize]);
            }
        }
    }

    fn apply_imm_shift(&self, val: u32, shift_t: ShiftType, shift_n: u32) -> u32 {
        match shift_t {
            ShiftType::Lsl => val << shift_n,
            ShiftType::Lsr => ((val as u64) >> shift_n) as u32,
            ShiftType::Asr => (((val as i32) as i64) >> shift_n) as u32,
            ShiftType::Rrx => ((self.regs.psr << 2) & 0x80000000) | (val >> shift_n),
            ShiftType::Ror => val.rotate_right(shift_n),
        }
    }

    fn thumb_expand_imm(&self, imm12: u16) -> u32 {
        if (imm12 >> 10) == 0 {
            const EXP: [u32; 4] = [1, 0x10001, 0x1000100, 0x1010101];
            EXP[imm12 as usize >> 8] * (imm12 as u32 & 0xff)
        } else {
            let unrotated = 0x80 | (imm12 as u32 & 0x7f);
            unrotated.rotate_right(imm12 as u32 >> 7)
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
