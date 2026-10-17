//! Evaluating conditions.

use crate::cpu::Cpu;

const V: u16 = 0xAAAA;
const C: u16 = 0xCCCC;
const Z: u16 = 0xF0F0;
const N: u16 = 0xFF00;

// Table 3-38
const COND_TAB: [u16; 16] = [
    Z,              // EQ
    !Z,             // NE
    C,              // CS, HS
    !C,             // CC, LO
    N,              // MI
    !N,             // PL
    V,              // VS
    !V,             // VC
    C & !Z,         // HI
    !(C & !Z),      // LS
    !(N ^ V),       // GE
    N ^ V,          // LT
    !(Z | (N ^ V)), // GT
    Z | (N ^ V),    // LE
    !0,             // AL
    !0,             // alternative
];

impl Cpu {
    pub fn eval_cond(&self, cond: u16) -> bool {
        let nzcv = self.regs.psr >> 28;
        ((COND_TAB[cond as usize] >> nzcv) & 1) != 0
    }
}
