//! A Cortex-M33 emulator, for running and testing symbolasm code on the
//! host.

use std::collections::HashMap;

use elf::{ElfBytes, abi::PT_LOAD, endian::LittleEndian};

pub mod arith;
pub mod cond;
pub mod cpu;
pub mod fp;
mod fpinstr;
pub mod instr;
pub mod syscall;

pub use cpu::Cpu;

/// Load an ELF file's segments into memory, returning its symbols, with
/// the Thumb bit of function addresses cleared.
pub fn load_elf(elf: &[u8], mem: &mut [u8]) -> HashMap<String, u32> {
    let file = ElfBytes::<LittleEndian>::minimal_parse(elf).expect("error opening elf");
    if let Some(segments) = file.segments() {
        for seg in segments {
            if seg.p_type == PT_LOAD
                && let Ok(data) = file.segment_data(&seg)
            {
                mem[seg.p_vaddr as usize..][..data.len()].copy_from_slice(data);
            }
        }
    }
    let mut symbols = HashMap::new();
    if let Ok(Some((symtab, strtab))) = file.symbol_table() {
        for sym in symtab.iter() {
            if let Ok(name) = strtab.get(sym.st_name as usize)
                && !name.is_empty()
            {
                symbols.insert(name.to_owned(), sym.st_value as u32 & !1);
            }
        }
    }
    symbols
}
