// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Names in global scope: peripherals and linker symbols.

use std::collections::{HashMap, HashSet};

use crate::{svd::Peripherals, types::TypeHandle};

#[derive(Default)]
pub struct Globals {
    pub peripherals: Option<Peripherals>,
    /// Linker symbols, with the type of the symbol's value (its address).
    pub symbols: HashMap<String, TypeHandle>,
    /// Statics defined in this file, with the type of the data.
    pub statics: HashMap<String, TypeHandle>,
    /// The statics declared without `mut`.
    pub immutable: HashSet<String>,
    /// Functions defined in this file.
    pub fns: HashSet<String>,
}

impl Globals {
    pub fn static_ty(&self, name: &str) -> Option<TypeHandle> {
        self.statics.get(name).copied()
    }

    pub fn is_immutable(&self, name: &str) -> bool {
        self.immutable.contains(name)
    }

    /// Whether the name is a symbol, a static or a function.
    pub fn is_defined(&self, name: &str) -> bool {
        self.statics.contains_key(name)
            || self.symbols.contains_key(name)
            || self.fns.contains(name)
    }

    pub fn symbol_ty(&self, name: &str) -> Option<TypeHandle> {
        self.symbols.get(name).copied()
    }
}
