use elf::{ElfBytes, abi::PT_LOAD, endian::LittleEndian, segment::ProgramHeader};

use crate::cpu::Cpu;

mod arith;
mod cond;
mod cpu;
mod instr;
mod syscall;

fn load_segment(mem: &mut [u8], file: &ElfBytes<LittleEndian>, header: &ProgramHeader) {
    if let Ok(data) = file.segment_data(header) {
        mem[header.p_vaddr as usize..][..data.len()].copy_from_slice(data);
    }
}

fn main() {
    let filename = std::env::args().nth(1).unwrap();
    let elf = std::fs::read(filename).expect("could not read file");
    let file = ElfBytes::<LittleEndian>::minimal_parse(&elf).expect("error opening elf");
    let (headers, strtab) = file.section_headers_with_strtab().unwrap();
    for header in headers.unwrap() {
        println!(
            "{header:?}, {}",
            strtab
                .unwrap()
                .get(header.sh_name as usize)
                .unwrap_or_default()
        );
    }
    let mut start = 0x8000;
    if let Ok(Some((symtab, strtab))) = file.symbol_table() {
        for x in symtab.iter() {
            if let Ok(name) = strtab.get(x.st_name as usize)
                && name == "_start"
            {
                start = x.st_value & !1;
                println!("got _start = 0x{start:x}");
            }
        }
    }

    let mut mem = vec![0; 1024 * 1024];

    if let Some(segments) = file.segments() {
        for seg in segments {
            if seg.p_type == PT_LOAD {
                load_segment(&mut mem, &file, &seg);
            }
        }
    }

    let mut cpu = Cpu::new(mem);
    cpu.regs.r[13] = 0x8000;
    cpu.regs.r[15] = start as u32;
    while !cpu.halted {
        cpu.step_instr();
    }
}
