// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Names in global scope: peripherals and linker symbols.

use std::collections::{HashMap, HashSet};

use crate::{
    svd::Peripherals,
    types::{TypeHandle, TypePool},
};

#[derive(Default)]
pub struct Globals {
    pub peripherals: Option<Peripherals>,
    /// Linker symbols, with the type of the symbol's value (its address).
    pub symbols: HashMap<String, TypeHandle>,
    /// Statics defined in this file, with the type of the data.
    pub statics: HashMap<String, TypeHandle>,
    /// The statics declared without `mut`.
    pub immutable: HashSet<String>,
    /// Functions defined in the program, with their types
    pub fns: HashMap<String, TypeHandle>,
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
            || self.fns.contains_key(name)
    }

    /// The type of a function that can be called by name: one defined in
    /// the program, or a symbol with a function type
    pub fn fn_ty(&self, name: &str, types: &TypePool) -> Option<TypeHandle> {
        self.fns
            .get(name)
            .copied()
            .or_else(|| self.symbol_ty(name).filter(|t| types.sig(*t).is_some()))
    }

    pub fn symbol_ty(&self, name: &str) -> Option<TypeHandle> {
        self.symbols.get(name).copied()
    }
}
