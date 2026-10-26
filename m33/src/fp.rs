//! Single precision arithmetic as the Arm FPU does it, on the bits of an
//! f32.
//!
//! Results are the host's IEEE arithmetic in round to nearest, which is
//! what the FPU does with FPSCR's defaults. What the host doesn't settle is
//! which NaN comes out, so that follows the Arm pseudocode (FPProcessNaNs):
//! a signaling NaN operand, quieted, before a quiet one, the first operand
//! first, and the default NaN for an invalid operation. With FPSCR.DN set,
//! every NaN result is the default NaN.
//!
//! Not modeled: the cumulative exception flags, flush to zero, and rounding
//! modes other than to nearest for arithmetic (the Cpu checks FPSCR).

pub const DEFAULT_NAN: u32 = 0x7fc0_0000;
const SIGN: u32 = 0x8000_0000;
const QUIET: u32 = 0x0040_0000;

pub fn is_nan(x: u32) -> bool {
    x & 0x7fff_ffff > 0x7f80_0000
}

fn is_snan(x: u32) -> bool {
    is_nan(x) && x & QUIET == 0
}

fn is_zero(x: u32) -> bool {
    x & 0x7fff_ffff == 0
}

fn is_inf(x: u32) -> bool {
    x & 0x7fff_ffff == 0x7f80_0000
}

pub fn neg(x: u32) -> u32 {
    x ^ SIGN
}

pub fn abs(x: u32) -> u32 {
    x & !SIGN
}

/// The FPSCR bits that change arithmetic
#[derive(Clone, Copy)]
pub struct Mode {
    /// Default NaN
    pub dn: bool,
}

impl Mode {
    fn nan(self, x: u32) -> u32 {
        if self.dn { DEFAULT_NAN } else { x | QUIET }
    }

    /// The NaN result for NaN operands, in order, if there are any
    fn process_nans(self, ops: &[u32]) -> Option<u32> {
        let x = ops
            .iter()
            .find(|x| is_snan(**x))
            .or_else(|| ops.iter().find(|x| is_nan(**x)))?;
        Some(self.nan(*x))
    }

    /// A result from the host, where a NaN can only be from an invalid
    /// operation, given no NaN operands
    fn result(self, ops: &[u32], f: impl FnOnce() -> f32) -> u32 {
        if let Some(nan) = self.process_nans(ops) {
            return nan;
        }
        let r = f();
        if r.is_nan() { DEFAULT_NAN } else { r.to_bits() }
    }

    pub fn add(self, a: u32, b: u32) -> u32 {
        self.result(&[a, b], || f(a) + f(b))
    }

    pub fn sub(self, a: u32, b: u32) -> u32 {
        self.result(&[a, b], || f(a) - f(b))
    }

    pub fn mul(self, a: u32, b: u32) -> u32 {
        self.result(&[a, b], || f(a) * f(b))
    }

    pub fn div(self, a: u32, b: u32) -> u32 {
        self.result(&[a, b], || f(a) / f(b))
    }

    /// `addend + a * b`, rounded once. Infinity times zero is invalid even
    /// with a quiet NaN addend.
    pub fn mul_add(self, addend: u32, a: u32, b: u32) -> u32 {
        let inf_zero = (is_inf(a) && is_zero(b)) || (is_zero(a) && is_inf(b));
        if is_nan(addend) && !is_snan(addend) && inf_zero {
            return DEFAULT_NAN;
        }
        self.result(&[addend, a, b], || f(a).mul_add(f(b), f(addend)))
    }

    pub fn sqrt(self, a: u32) -> u32 {
        self.result(&[a], || f(a).sqrt())
    }

    /// vmaxnm and vminnm: a quiet NaN loses to a number, and +0 is more
    /// than -0
    pub fn max_min_num(self, a: u32, b: u32, max: bool) -> u32 {
        let quiet = |x| is_nan(x) && !is_snan(x);
        let loser = if max { 0xff80_0000 } else { 0x7f80_0000 };
        let (a, b) = match (quiet(a), quiet(b)) {
            (true, false) => (loser, b),
            (false, true) => (a, loser),
            _ => (a, b),
        };
        if let Some(nan) = self.process_nans(&[a, b]) {
            return nan;
        }
        if is_zero(a) && is_zero(b) {
            let sign = if max { a & b } else { a | b };
            return sign & SIGN;
        }
        let a_wins = (f(a) > f(b)) == max;
        if a_wins { a } else { b }
    }

    /// Round to an integral value, keeping NaNs, infinities and zeros
    pub fn round_int(self, a: u32, rounding: Rounding) -> u32 {
        if is_nan(a) {
            return self.nan(a);
        }
        let r = rounding.apply(f(a));
        // Rounding keeps the sign, so -0.4 goes to -0
        (r.to_bits() & !SIGN) | (a & SIGN)
    }
}

/// How a conversion or vrint rounds
#[derive(Clone, Copy, Debug)]
pub enum Rounding {
    /// Ties away from zero (vrinta, vcvta)
    TiesAway,
    /// Ties to even (vrintn, vcvtn, and FPSCR's round to nearest)
    TiesEven,
    /// Toward +infinity
    Up,
    /// Toward -infinity
    Down,
    /// Toward zero
    Zero,
}

impl Rounding {
    /// The FPSCR.RMode field
    pub fn from_rmode(rmode: u32) -> Self {
        match rmode & 3 {
            0 => Rounding::TiesEven,
            1 => Rounding::Up,
            2 => Rounding::Down,
            _ => Rounding::Zero,
        }
    }

    fn apply(self, x: f32) -> f32 {
        match self {
            Rounding::TiesAway => x.round(),
            Rounding::TiesEven => x.round_ties_even(),
            Rounding::Up => x.ceil(),
            Rounding::Down => x.floor(),
            Rounding::Zero => x.trunc(),
        }
    }
}

/// An f32 to an integer: rounded, saturated, and NaN to 0
pub fn to_int(a: u32, rounding: Rounding, signed: bool) -> u32 {
    let x = rounding.apply(f(a));
    if signed { x as i32 as u32 } else { x as u32 }
}

/// An integer to an f32. An f64 holds the integer exactly, so the nearest
/// f32 is moved a step when it's on the wrong side for the rounding.
pub fn from_int(a: u32, rounding: Rounding, signed: bool) -> u32 {
    let x = if signed { a as i32 as f64 } else { a as f64 };
    let r = x as f32;
    let r = match rounding {
        Rounding::Up if (r as f64) < x => r.next_up(),
        Rounding::Down if (r as f64) > x => r.next_down(),
        Rounding::Zero if (r as f64).abs() > x.abs() => {
            if x > 0.0 {
                r.next_down()
            } else {
                r.next_up()
            }
        }
        _ => r,
    };
    r.to_bits()
}

/// vcmp's NZCV: less 1000, equal 0110, greater 0010, unordered 0011
pub fn compare(a: u32, b: u32) -> u32 {
    let nzcv = if is_nan(a) || is_nan(b) {
        0b0011
    } else if f(a) < f(b) {
        0b1000
    } else if f(a) == f(b) {
        0b0110
    } else {
        0b0010
    };
    nzcv << 28
}

/// VFPExpandImm: the 8 bit immediate of vmov.f32
pub fn expand_imm(imm8: u32) -> u32 {
    let sign = (imm8 >> 7) << 31;
    let b = (imm8 >> 6) & 1;
    let exp = ((b ^ 1) << 7) | (if b == 1 { 0x7c } else { 0 }) | ((imm8 >> 4) & 3);
    sign | (exp << 23) | ((imm8 & 0xf) << 19)
}

fn f(x: u32) -> f32 {
    f32::from_bits(x)
}
