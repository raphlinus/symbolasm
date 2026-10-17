//! Instruction tests against vectors from a reference emulator.
//!
//! Each line of tests/vectors.txt, made by tests/gen_vectors.py with
//! unicorn's Cortex-M33 model, is a short instruction sequence with a
//! starting state and the expected final registers, flags and memory
//! writes. All cases run, and the failures are reported together.

use m33::Cpu;

const MEM_SIZE: usize = 0x10000;
const CODE: u32 = 0x8000;
const NZCV: u32 = 0xf000_0000;

/// The memory fill, matching pattern() in gen_vectors.py.
fn pattern() -> Vec<u8> {
    let mut x: u32 = 0x1234_5678;
    (0..MEM_SIZE)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn hex_words(s: &str) -> Vec<u32> {
    s.split_whitespace()
        .map(|w| u32::from_str_radix(w, 16).unwrap())
        .collect()
}

/// Run one case, returning a description of what differs.
fn run(line: &str, mem: &[u8]) -> Result<(), String> {
    let f: Vec<&str> = line.split(" | ").map(str::trim).collect();
    let [asm, code, steps, init, want, writes] = f[..] else {
        panic!("bad vector line: {line}");
    };
    let code: Vec<u8> = (0..code.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&code[i..i + 2], 16).unwrap())
        .collect();
    let steps: usize = steps.parse().unwrap();
    let init = hex_words(init);
    let want = hex_words(want);

    let mut mem = mem.to_vec();
    mem[CODE as usize..][..code.len()].copy_from_slice(&code);
    let mut expected_mem = mem.clone();
    for w in writes.split_whitespace() {
        let v: Vec<u32> = w
            .split(':')
            .map(|x| u32::from_str_radix(x, 16).unwrap())
            .collect();
        let (addr, size, val) = (v[0] as usize, v[1] as usize, v[2]);
        expected_mem[addr..][..size].copy_from_slice(&val.to_le_bytes()[..size]);
    }

    let mut cpu = Cpu::new(mem);
    cpu.regs.r[..15].copy_from_slice(&init[..15]);
    cpu.regs.r[15] = CODE;
    cpu.regs.psr = init[15];
    let branch = is_branch(asm);
    let end = CODE + code.len() as u32;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if branch {
            cpu.step_instr();
        } else {
            for _ in 0..steps {
                cpu.step_instr();
                if cpu.regs.r[15] == end {
                    break;
                }
            }
        }
        cpu
    }));
    let cpu = match result {
        Ok(cpu) => cpu,
        Err(e) => {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            return Err(format!("{asm}: panicked: {msg}"));
        }
    };

    let mut diffs = vec![];
    for i in 0..16 {
        if cpu.regs.r[i] != want[i] {
            diffs.push(format!("r{i} {:#x}, want {:#x}", cpu.regs.r[i], want[i]));
        }
    }
    if cpu.regs.psr & NZCV != want[16] {
        diffs.push(format!(
            "nzcv {:x}, want {:x}",
            cpu.regs.psr >> 28,
            want[16] >> 28
        ));
    }
    let mut bad_mem = 0;
    let mut first = None;
    for a in 0..MEM_SIZE {
        if cpu.load_u8(a as u32) != expected_mem[a] {
            bad_mem += 1;
            first.get_or_insert(a);
        }
    }
    if let Some(a) = first {
        diffs.push(format!("{bad_mem} memory bytes differ, first at {a:#x}"));
    }
    if diffs.is_empty() {
        Ok(())
    } else {
        Err(format!("{asm} (from {}): {}", f[3], diffs.join(", ")))
    }
}

/// Matches is_branch() in gen_vectors.py: branches run one step.
fn is_branch(asm: &str) -> bool {
    let op = asm.split_whitespace().next().unwrap();
    (op.starts_with('b') && !["bic", "bfi", "bfc"].iter().any(|p| op.starts_with(p)))
        || op.starts_with("cb")
        || op.starts_with("tb")
}

#[test]
fn vectors() {
    let mem = pattern();
    let text = include_str!("vectors.txt");
    // Quiet the default hook, so unimplemented instructions don't print
    // a panic message each.
    std::panic::set_hook(Box::new(|_| {}));
    let mut n = 0;
    let mut failures = vec![];
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        n += 1;
        if let Err(e) = run(line, &mem) {
            failures.push(e);
        }
    }
    let _ = std::panic::take_hook();
    if !failures.is_empty() {
        // One line per instruction is plenty
        let mut seen = std::collections::HashSet::new();
        let report: Vec<_> = failures
            .iter()
            .filter(|f| {
                seen.insert(
                    f.split(':')
                        .next()
                        .unwrap()
                        .split(" (")
                        .next()
                        .unwrap()
                        .to_owned(),
                )
            })
            .cloned()
            .collect();
        panic!(
            "{} of {n} cases failed, in {} instructions:\n{}",
            failures.len(),
            report.len(),
            report.join("\n")
        );
    }
}
