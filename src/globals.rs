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
    /// Statics defined in this file, with the type of the data.
    pub statics: HashMap<String, TypeHandle>,
}

impl Globals {
    pub fn static_ty(&self, name: &str) -> Option<TypeHandle> {
        self.statics.get(name).copied()
    }

    pub fn symbol_ty(&self, name: &str) -> Option<TypeHandle> {
        self.symbols.get(name).copied()
    }
}
