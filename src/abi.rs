// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Calling conventions: `fn[abi = loose]`
//!
//! Floats follow the hard-float variant of the AAPCS (AAPCS-VFP, as in
//! Rust's thumbv8m.main-none-eabihf): s0-s15 are caller-saved and s16-s31
//! callee-saved, and f32 arguments and results go in s registers from s0,
//! separately from the others, which go in core registers from r0.

use crate::{bitset::BitSet, regmap::S0};

/// Where an argument goes
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Place {
    Reg(u8),
    /// Bytes above sp at the call
    Stack(i32),
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub enum Abi {
    /// The standard Arm convention: r0-r3, r12 and lr are caller-saved, and
    /// the stack is 8 byte aligned at calls.
    #[default]
    Aapcs,
    /// Like aapcs, but r4-r7 are caller-saved too, up to 8 arguments and
    /// results go in r0-r7, and the stack is only 4 byte aligned.
    Loose,
    /// Anything goes: all registers may be clobbered, and the stack may be
    /// left unbalanced. Nothing is checked.
    Wild,
}

impl Abi {
    pub fn from_name(name: &str) -> Option<Abi> {
        match name {
            "aapcs" => Some(Abi::Aapcs),
            "loose" => Some(Abi::Loose),
            "wild" => Some(Abi::Wild),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Abi::Aapcs => "aapcs",
            Abi::Loose => "loose",
            Abi::Wild => "wild",
        }
    }

    /// How many integer arguments, and how many results, go in registers,
    /// starting at r0. Further arguments go on the stack.
    pub fn n_regs(self) -> usize {
        match self {
            Abi::Aapcs => 4,
            Abi::Loose | Abi::Wild => 8,
        }
    }

    /// Where each argument goes, given whether each is an f32. Integers go
    /// in r0 up, f32s in s0 up (16 of them), and once a kind runs out of
    /// registers, the rest of it go on the stack, in argument order.
    pub fn arg_places(self, floats: &[bool]) -> Vec<Place> {
        let mut n_core = 0;
        let mut n_fp = 0;
        let mut stack = 0;
        floats
            .iter()
            .map(|&float| {
                let (n, max, first) = if float {
                    (&mut n_fp, 16, S0)
                } else {
                    (&mut n_core, self.n_regs(), 0)
                };
                if *n < max {
                    *n += 1;
                    Place::Reg(first + *n as u8 - 1)
                } else {
                    stack += 4;
                    Place::Stack(stack - 4)
                }
            })
            .collect()
    }

    /// The register of each result: integers in r0 up, f32s in s0 up
    pub fn ret_regs(self, floats: &[bool]) -> Vec<u8> {
        let mut n_core = 0;
        let mut n_fp = 0;
        floats
            .iter()
            .map(|&float| {
                let (n, first) = if float {
                    (&mut n_fp, S0)
                } else {
                    (&mut n_core, 0)
                };
                *n += 1;
                first + *n - 1
            })
            .collect()
    }

    /// The registers a call may change, besides its results
    pub fn clobbers(self) -> BitSet {
        let mut set = BitSet::default();
        let regs: &[usize] = match self {
            Abi::Aapcs => &[0, 1, 2, 3, 12, 14],
            Abi::Loose => &[0, 1, 2, 3, 4, 5, 6, 7, 12, 14],
            Abi::Wild => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14],
        };
        for &r in regs {
            set.insert(r);
        }
        let n_fp = if self == Abi::Wild { 32 } else { 16 };
        for r in 0..n_fp {
            set.insert(S0 as usize + r);
        }
        set
    }

    /// The registers a function must restore before returning, not counting
    /// lr, which holds the return address
    pub fn callee_saved(self) -> Vec<u8> {
        let core: &[u8] = match self {
            Abi::Aapcs => &[4, 5, 6, 7, 8, 9, 10, 11],
            Abi::Loose => &[8, 9, 10, 11],
            Abi::Wild => return vec![],
        };
        // and s16-s31
        let fp = (16..32).map(|n| S0 + n);
        core.iter().copied().chain(fp).collect()
    }
}
