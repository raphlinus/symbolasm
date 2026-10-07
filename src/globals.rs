// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Symbols in global scope: peripherals and extern symbols.

use std::collections::HashMap;

use crate::{svd::Peripherals, types::TypeHandle};

#[derive(Default)]
pub struct Globals {
    pub peripherals: Option<Peripherals>,
    /// Extern symbols, with the type of the symbol's value (its address).
    pub externs: HashMap<String, TypeHandle>,
}

impl Globals {
    pub fn extern_ty(&self, name: &str) -> Option<TypeHandle> {
        self.externs.get(name).copied()
    }
}
