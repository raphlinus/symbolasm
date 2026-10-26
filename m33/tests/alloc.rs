//! Randomized test of the linked list allocator in alloc.sy, run on the
//! emulator.
//!
//! Each run does a random sequence of allocs and frees, and after every
//! call checks the result and the hole list against a model: holes are
//! sorted, 8 aligned, at least 8 bytes, fully merged, and don't overlap
//! each other or live blocks, and holes plus live blocks cover the heap
//! exactly. A failing sequence is shrunk before it's reported.
//!
//! alloc.sy is compiled with the symbolasm in this workspace; building
//! also needs arm-none-eabi binutils.

use std::collections::HashMap;

use m33::{Cpu, load_elf};

mod common;

const TEXT: u32 = 0x1000;
const HEAD: u32 = 0x8000;
const HEAP: u32 = 0x10000;
const HEAP_SIZE: u32 = 0x10000;
const MEM_SIZE: usize = 0x40000;

#[derive(Clone, Copy, Debug)]
enum Op {
    Alloc {
        size: u32,
        align: u32,
    },
    /// Free the nth allocation made, if it's still live
    Free(usize),
}

fn rounded(size: u32) -> u32 {
    ((size + 7) & !7).max(8)
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }

    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

fn random_ops(seed: u64, len: usize) -> Vec<Op> {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut n_allocs = 0;
    (0..len)
        .map(|_| {
            if n_allocs == 0 || rng.below(100) < 55 {
                n_allocs += 1;
                let size = match rng.below(4) {
                    0 => 1 + rng.below(16),
                    1 => 1 + rng.below(64),
                    2 => 1 + rng.below(512),
                    _ => 1 + rng.below(4096),
                };
                let align = 1 << rng.below(8);
                Op::Alloc { size, align }
            } else {
                Op::Free(rng.below(n_allocs) as usize)
            }
        })
        .collect()
}

/// Run ops, checking after each. Returns the index of the failing op and
/// what went wrong.
fn run(elf: &[u8], ops: &[Op]) -> Result<(), (usize, String)> {
    let mut mem = vec![0; MEM_SIZE];
    let symbols: HashMap<String, u32> = load_elf(elf, &mut mem);
    let mut cpu = Cpu::new(mem);
    cpu.trace = std::env::var_os("M33_TRACE").is_some();
    cpu.regs.r[13] = HEAD - 0x100;
    // dummy head, then one hole covering the heap
    cpu.store_u32(HEAD, HEAP);
    cpu.store_u32(HEAD + 4, 0);
    cpu.store_u32(HEAP, 0);
    cpu.store_u32(HEAP + 4, HEAP_SIZE);
    let (alloc, free) = (symbols["alloc"], symbols["free"]);
    // allocation index -> (address, requested size), while live
    let mut live: HashMap<usize, (u32, u32)> = HashMap::new();
    let mut n_allocs = 0;
    for (i, op) in ops.iter().enumerate() {
        let fail = |msg: String| Err((i, msg));
        match *op {
            Op::Alloc { size, align } => {
                let holes = holes(&cpu).map_err(|e| (i, e))?;
                let p = cpu.call(alloc, &[HEAD, size, align]);
                let id = n_allocs;
                n_allocs += 1;
                let r = rounded(size);
                if p == 0 {
                    // There must really be no room
                    for (h, hs) in holes {
                        let start = (h + align - 1) & !(align - 1);
                        if start + r <= h + hs {
                            return fail(format!(
                                "alloc({size}, {align}) failed, but hole {h:#x}+{hs:#x} fits it"
                            ));
                        }
                    }
                    continue;
                }
                if p % align != 0 {
                    return fail(format!("alloc({size}, {align}) = {p:#x} is misaligned"));
                }
                if p < HEAP || p + r > HEAP + HEAP_SIZE {
                    return fail(format!(
                        "alloc({size}, {align}) = {p:#x} is outside the heap"
                    ));
                }
                for &(q, qs) in live.values() {
                    if p < q + rounded(qs) && q < p + r {
                        return fail(format!(
                            "alloc({size}, {align}) = {p:#x} overlaps live block {q:#x}"
                        ));
                    }
                }
                live.insert(id, (p, size));
                n_allocs = n_allocs.max(id + 1);
            }
            Op::Free(id) => {
                let Some((p, size)) = live.remove(&id) else {
                    continue;
                };
                cpu.call(free, &[HEAD, p, size]);
            }
        }
        check(&cpu, &live).map_err(|e| (i, e))?;
    }
    Ok(())
}

/// The hole list, checked for order, alignment, size and merging.
fn holes(cpu: &Cpu) -> Result<Vec<(u32, u32)>, String> {
    let mut result = vec![];
    let mut h = cpu.load_u32(HEAD);
    let mut prev_end = 0;
    while h != 0 {
        if result.len() > (HEAP_SIZE / 8) as usize {
            return Err("hole list has a cycle".into());
        }
        if h < HEAP || h >= HEAP + HEAP_SIZE {
            return Err(format!("hole {h:#x} is outside the heap"));
        }
        let size = cpu.load_u32(h + 4);
        if h % 8 != 0 || size % 8 != 0 || size < 8 {
            return Err(format!("hole {h:#x}+{size:#x} is misaligned or too small"));
        }
        if h + size > HEAP + HEAP_SIZE {
            return Err(format!("hole {h:#x}+{size:#x} runs past the heap"));
        }
        if h < prev_end {
            return Err(format!(
                "hole {h:#x} is out of order or overlaps the previous one"
            ));
        }
        if h == prev_end {
            return Err(format!("hole {h:#x} wasn't merged with the previous one"));
        }
        result.push((h, size));
        prev_end = h + size;
        h = cpu.load_u32(h);
    }
    Ok(result)
}

fn check(cpu: &Cpu, live: &HashMap<usize, (u32, u32)>) -> Result<(), String> {
    let holes = holes(cpu)?;
    for &(h, hs) in &holes {
        for &(p, s) in live.values() {
            if h < p + rounded(s) && p < h + hs {
                return Err(format!("hole {h:#x}+{hs:#x} overlaps live block {p:#x}"));
            }
        }
    }
    let total: u32 = holes.iter().map(|(_, s)| s).sum::<u32>()
        + live.values().map(|(_, s)| rounded(*s)).sum::<u32>();
    if total != HEAP_SIZE {
        return Err(format!(
            "holes and live blocks cover {total:#x} bytes, not the heap's {HEAP_SIZE:#x}"
        ));
    }
    Ok(())
}

/// Remove ops one at a time while the sequence still fails.
fn shrink(elf: &[u8], mut ops: Vec<Op>) -> (Vec<Op>, usize, String) {
    let (mut at, mut msg) = run(elf, &ops).unwrap_err();
    ops.truncate(at + 1);
    let mut i = 0;
    while i < ops.len() {
        let mut candidate = ops.clone();
        candidate.remove(i);
        match run(elf, &candidate) {
            Err((a, m)) => {
                candidate.truncate(a + 1);
                ops = candidate;
                at = a;
                msg = m;
            }
            Ok(()) => i += 1,
        }
    }
    (ops, at, msg)
}

#[test]
fn random_alloc_free() {
    let elf = common::build("alloc", TEXT, "alloc");
    for seed in 0..2000 {
        let ops = random_ops(seed, 300);
        if run(&elf, &ops).is_err() {
            let (ops, at, msg) = shrink(&elf, ops);
            panic!("seed {seed}: op {at}: {msg}\nminimal sequence:\n{ops:#?}");
        }
    }
}
