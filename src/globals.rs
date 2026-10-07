// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Names in global scope: peripherals and linker symbols.

use std::collections::HashMap;

use crate::{svd::Peripherals, types::TypeHandle};

#[derive(Default)]
pub struct Globals {
    pub peripherals: Option<Peripherals>,
    /// Linker symbols, with the type of the symbol's value (its address).
    pub symbols: HashMap<String, TypeHandle>,
}

impl Globals {
    pub fn symbol_ty(&self, name: &str) -> Option<TypeHandle> {
        self.symbols.get(name).copied()
    }
}
