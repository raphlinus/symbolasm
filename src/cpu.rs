pub struct Cpu {
    pub regs: Regs,
    mem: Vec<u8>,
    pub halted: bool,
    /// ITSTATE: the condition in the high 4 bits, the mask below.
    pub it_state: u8,
    /// Print each instruction as it executes.
    pub trace: bool,
}

/// Return address that ends `Cpu::call`.
const CALL_SENTINEL: u32 = 0xffff_fff0;

#[derive(Default, Clone)]
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
    pub fn new(mem: Vec<u8>) -> Self {
        let regs = Regs::default();
        let halted = false;
        Self {
            regs,
            mem,
            halted,
            it_state: 0,
            trace: false,
        }
    }

    /// Call the function at `addr` with up to four arguments in r0-r3, and
    /// return r0 when it returns with bx lr (or pop pc).
    pub fn call(&mut self, addr: u32, args: &[u32]) -> u32 {
        for (i, arg) in args.iter().enumerate() {
            self.regs.r[i] = *arg;
        }
        self.regs.r[14] = CALL_SENTINEL | 1;
        self.regs.r[15] = addr & !1;
        while self.regs.r[15] != CALL_SENTINEL && !self.halted {
            self.step_instr();
        }
        self.regs.r[0]
    }

    /// Whether the current instruction is in an IT block, where 16 bit
    /// data processing instructions don't set flags.
    pub fn in_it(&self) -> bool {
        self.it_state != 0
    }

    fn check(&self, addr: u32, size: u32) -> usize {
        let a = addr as usize;
        if a + size as usize > self.mem.len() {
            panic!(
                "memory access out of range at pc {:#x}: address {addr:#x}, {size} bytes",
                self.regs.r[15]
            );
        }
        a
    }

    pub fn load_u8(&self, addr: u32) -> u8 {
        self.mem[self.check(addr, 1)]
    }

    pub fn load_u16(&self, addr: u32) -> u16 {
        u16::from_le_bytes(self.mem[self.check(addr, 2)..][..2].try_into().unwrap())
    }

    pub fn load_u32(&self, addr: u32) -> u32 {
        u32::from_le_bytes(self.mem[self.check(addr, 4)..][..4].try_into().unwrap())
    }

    pub fn store_u8(&mut self, addr: u32, val: u8) {
        let a = self.check(addr, 1);
        self.mem[a] = val;
    }

    pub fn store_u16(&mut self, addr: u32, val: u16) {
        let a = self.check(addr, 2);
        self.mem[a..][..2].copy_from_slice(&u16::to_le_bytes(val));
    }

    pub fn store_u32(&mut self, addr: u32, val: u32) {
        let a = self.check(addr, 4);
        self.mem[a..][..4].copy_from_slice(&u32::to_le_bytes(val));
    }

    fn pop(&mut self) -> u32 {
        let result = self.load_u32(self.regs.r[13]);
        self.regs.r[13] += 4;
        result
    }

    fn push(&mut self, val: u32) {
        self.regs.r[13] -= 4;
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
}
