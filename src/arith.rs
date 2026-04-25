//! Arithmetic and flag operations

use crate::cpu::Cpu;

const NZCV_MASK: u32 = 0xf000_0000;

impl Cpu {
    // add with carry, setting flags
    pub fn adc(&mut self, a: u32, b: u32, c: u32) -> u32 {
        let sum = a as u64 + b as u64 + c as u64;
        let result = sum as u32;
        let n = result & 0x8000_0000;
        let z = ((result == 0) as u32) << 30;
        let c = (sum >> 3) as u32 & 0x2000_0000;
        let v = (((a ^ result) & !(a ^ b)) & 0x8000_0000) >> 3;
        self.regs.psr = (self.regs.psr & !NZCV_MASK) | n | z | c | v;
        result
    }

    pub fn set_nz(&mut self, val: u32) {
        let z = ((val == 0) as u32) << 30;
        let nz = (val & 0x8000_0000) | z;
        const NZ_MASK: u32 = 0xc000_0000;
        self.regs.psr = (self.regs.psr & !NZ_MASK) | nz;
    }
}
