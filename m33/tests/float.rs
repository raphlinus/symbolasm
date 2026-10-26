//! The f32 functions in float.sy, run on the emulator with random inputs
//! and checked against the same arithmetic on the host. Results must have
//! the same bits, except that any NaN matches any NaN, since the host
//! doesn't follow Arm's rules for which NaN comes out.

use std::collections::HashMap;

use m33::{Cpu, load_elf};

mod common;

const TEXT: u32 = 0x1000;
const DATA: u32 = 0x8000;
const MEM_SIZE: usize = 0x40000;
const CASES: usize = 2000;

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

    /// Mostly moderate values, with some zeros, infinities, NaNs,
    /// denormals and halves
    fn float(&mut self) -> f32 {
        match self.below(16) {
            0 => [0.0, -0.0, f32::INFINITY, f32::NEG_INFINITY][self.below(4) as usize],
            1 => f32::from_bits(
                [0x7fc0_0000, 0x7f80_0001, 0x0000_0001, 0x807f_ffff][self.below(4) as usize],
            ),
            2..5 => (self.below(80) as f32 - 40.0) / 2.0,
            _ => {
                let bits = (self.next() & 0x807f_ffff) | ((self.below(40) + 107) << 23);
                f32::from_bits(bits)
            }
        }
    }
}

struct Emu {
    cpu: Cpu,
    symbols: HashMap<String, u32>,
}

impl Emu {
    fn new() -> Self {
        let elf = common::build("float", TEXT, "dot");
        let mut mem = vec![0; MEM_SIZE];
        let symbols = load_elf(&elf, &mut mem);
        Emu {
            cpu: Cpu::new(mem),
            symbols,
        }
    }

    /// Call `name` with core and f32 arguments, returning r0 and s0
    fn call(&mut self, name: &str, args: &[u32], fargs: &[f32]) -> (u32, f32) {
        for (i, x) in fargs.iter().enumerate() {
            self.cpu.regs.s[i] = x.to_bits();
        }
        self.cpu.regs.r[13] = MEM_SIZE as u32;
        let r0 = self.cpu.call(self.symbols[name], args);
        (r0, f32::from_bits(self.cpu.regs.s[0]))
    }

    fn store(&mut self, addr: u32, xs: &[f32]) {
        for (i, x) in xs.iter().enumerate() {
            self.cpu.store_u32(addr + 4 * i as u32, x.to_bits());
        }
    }
}

fn check(failures: &mut Vec<String>, what: String, got: f32, want: f32) {
    let same = got.to_bits() == want.to_bits() || (got.is_nan() && want.is_nan());
    if !same {
        failures.push(format!("{what}: got {got:e}, want {want:e}"));
    }
}

fn dot(p: &[f32], q: &[f32]) -> f32 {
    let mut r = 0.0;
    for (a, b) in p.iter().zip(q) {
        r += a * b;
    }
    r
}

#[test]
fn float_functions() {
    let mut emu = Emu::new();
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut failures = vec![];
    for _ in 0..CASES {
        let n = rng.below(8) as usize + 1;
        let p: Vec<f32> = (0..n).map(|_| rng.float()).collect();
        let q: Vec<f32> = (0..n).map(|_| rng.float()).collect();
        let q_addr = DATA + 0x100;
        emu.store(DATA, &p);
        emu.store(q_addr, &q);
        let (_, got) = emu.call("dot", &[DATA, q_addr, n as u32], &[]);
        check(
            &mut failures,
            format!("dot({p:?}, {q:?})"),
            got,
            dot(&p, &q),
        );

        let k = rng.float();
        let saved = [rng.next(), rng.next()];
        emu.cpu.regs.s[16..18].copy_from_slice(&saved);
        let (_, got) = emu.call("scaled_norm", &[DATA, n as u32], &[k]);
        let want = dot(&p, &p).sqrt() * k;
        check(
            &mut failures,
            format!("scaled_norm({p:?}, {k:e})"),
            got,
            want,
        );
        assert_eq!(
            emu.cpu.regs.s[16..18],
            saved,
            "s16 and s17 are callee-saved"
        );
        assert_eq!(emu.cpu.regs.r[13], MEM_SIZE as u32);

        let [x, a, b, c0, c1, c2, c3] = std::array::from_fn(|_| rng.float());
        emu.store(DATA, &[c0, c1, c2, c3]);
        let (_, got) = emu.call("poly", &[DATA], &[x]);
        let t = c3.mul_add(x, c2);
        let u = t.mul_add(x, c1);
        let want = u.mul_add(x, c0);
        check(
            &mut failures,
            format!("poly({x:e}, {c0:e} {c1:e} {c2:e} {c3:e})"),
            got,
            want,
        );

        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        let (_, got) = emu.call("clamp", &[], &[x, lo, hi]);
        let mut want = x;
        if want < lo {
            want = lo;
        }
        if want > hi {
            want = hi;
        }
        check(
            &mut failures,
            format!("clamp({x:e}, {lo:e}, {hi:e})"),
            got,
            want,
        );

        let (r0, _) = emu.call("floor_int", &[], &[x]);
        let want = x.floor() as i32;
        if r0 as i32 != want {
            failures.push(format!("floor_int({x:e}): got {}, want {want}", r0 as i32));
        }

        let i = rng.next() as i32 >> rng.below(32);
        let (_, got) = emu.call("scale", &[i as u32], &[x]);
        check(
            &mut failures,
            format!("scale({i}, {x:e})"),
            got,
            i as f32 * x,
        );

        let (_, got) = emu.call("nmac", &[], &[x, a, b]);
        let y = -x - a * b;
        let want = (a * b - y) / b;
        check(
            &mut failures,
            format!("nmac({x:e}, {a:e}, {b:e})"),
            got,
            want,
        );
    }
    if !failures.is_empty() {
        panic!(
            "{} failures, the first few:\n{}",
            failures.len(),
            failures[..failures.len().min(10)].join("\n")
        );
    }
}
