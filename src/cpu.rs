pub struct Cpu {
    pub regs: Regs,
    mem: Vec<u8>,
}

pub struct Regs {
    pub r: [u32; 16],
    pub psr: u32,
}

pub enum ShiftType {
    Lsl,
    Lsr,
    Asr,
    Rrx,
    Ror,
}

impl Cpu {
    pub fn load_u8(&self, addr: u32) -> u8 {
        self.mem[addr as usize]
    }

    pub fn load_u16(&self, addr: u32) -> u16 {
        u16::from_le_bytes(self.mem[addr as usize..][..2].try_into().unwrap())
    }

    pub fn load_u32(&self, addr: u32) -> u32 {
        u32::from_le_bytes(self.mem[addr as usize..][..4].try_into().unwrap())
    }

    pub fn store_u8(&mut self, addr: u32, val: u8) {
        self.mem[addr as usize] = val;
    }

    pub fn store_u16(&mut self, addr: u32, val: u16) {
        self.mem[addr as usize..][..2].copy_from_slice(&u16::to_le_bytes(val));
    }

    pub fn store_u32(&mut self, addr: u32, val: u32) {
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

    pub fn pop_regs(&mut self, regs: u16) {
        for i in 0..16 {
            if (regs & (1 << i)) != 0 {
                self.regs.r[i as usize] = self.pop();
            }
        }
    }

    pub fn push_regs(&mut self, regs: u16) {
        for i in (0..16).rev() {
            if (regs & (1 << i)) != 0 {
                self.push(self.regs.r[i as usize]);
            }
        }
    }

    pub fn apply_imm_shift(&self, val: u32, shift_t: ShiftType, shift_n: u32) -> u32 {
        match shift_t {
            ShiftType::Lsl => val << shift_n,
            ShiftType::Lsr => ((val as u64) >> shift_n) as u32,
            ShiftType::Asr => (((val as i32) as i64) >> shift_n) as u32,
            ShiftType::Rrx => ((self.regs.psr << 2) & 0x80000000) | (val >> shift_n),
            ShiftType::Ror => val.rotate_right(shift_n),
        }
    }

    pub fn set_nz(&mut self, val: u32) {
        let z = ((val == 0) as u32) << 30;
        let nz = (val & 0x8000_0000) | z;
        const NZ_MASK: u32 = 0xc000_0000;
        self.regs.psr = (self.regs.psr & !NZ_MASK) | nz;
    }
}
