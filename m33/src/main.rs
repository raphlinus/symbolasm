use m33::{Cpu, load_elf};

fn main() {
    let filename = std::env::args().nth(1).unwrap();
    let elf = std::fs::read(filename).expect("could not read file");
    let mut mem = vec![0; 1024 * 1024];
    let symbols = load_elf(&elf, &mut mem);
    let start = symbols.get("_start").copied().unwrap_or(0x8000);
    println!("got _start = 0x{start:x}");

    let mut cpu = Cpu::new(mem);
    cpu.trace = true;
    cpu.regs.r[13] = 0x8000;
    cpu.regs.r[15] = start;
    while !cpu.halted {
        cpu.step_instr();
    }
}
