// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Calling conventions: `fn[abi = loose]`

use crate::bitset::BitSet;

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

    /// How many arguments, and how many results, go in registers, starting
    /// at r0. Further arguments go on the stack.
    pub fn n_regs(self) -> usize {
        match self {
            Abi::Aapcs => 4,
            Abi::Loose | Abi::Wild => 8,
        }
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
        set
    }

    /// The registers a function must restore before returning, not counting
    /// lr, which holds the return address
    pub fn callee_saved(self) -> &'static [u8] {
        match self {
            Abi::Aapcs => &[4, 5, 6, 7, 8, 9, 10, 11],
            Abi::Loose => &[8, 9, 10, 11],
            Abi::Wild => &[],
        }
    }
}
