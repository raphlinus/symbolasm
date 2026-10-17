use crate::cpu::Cpu;

impl Cpu {
    pub fn syscall(&mut self, bkpt: u8) {
        match bkpt {
            0 => self.halted = true,
            1 => {
                println!("bkpt 1, r0 = 0x{:04x}", self.regs.r[0]);
            }
            _ => todo!("unhandled bkpt {bkpt}"),
        }
    }
}
