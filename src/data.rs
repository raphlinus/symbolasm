// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Emission of statics and their initializers.

use std::io::Write;

use crate::{
    compile::section_directive,
    consts,
    error::Error,
    globals::Globals,
    parse::{Expr, Init, StaticDecl},
    types::{Type, TypeHandle, TypePool},
};

/// Emit a static: a `static mut` without an initializer in .bss, with one
/// in .data, and an immutable `static` in .text, where it can be reached
/// with PC-relative loads. `#[link_section]` overrides the section.
pub fn emit_static(
    s: &StaticDecl,
    types: &mut TypePool,
    globals: &Globals,
    w: &mut impl Write,
) -> Result<(), Error> {
    let name = s.name.as_ident().unwrap();
    let ty = globals.statics[name];
    let info = types.info(ty);
    if info.size() == 0 {
        return Err(format!("static {name} has zero size"))?;
    }
    match (&s.link_section, &s.init, s.mutable) {
        (Some(section), _, _) => writeln!(w, "{}", section_directive(section, name, false))?,
        (None, None, _) => writeln!(w, ".section .bss")?,
        (None, Some(_), true) => writeln!(w, ".section .data")?,
        (None, Some(_), false) => writeln!(w, ".section .text")?,
    }
    writeln!(w, ".balign {}", info.align())?;
    writeln!(w, "{name}:")?;
    match &s.init {
        None => writeln!(w, "    .space {}", info.size())?,
        Some(init) => emit_init(ty, init, types, globals, w)?,
    }
    Ok(())
}

fn emit_init(
    ty: TypeHandle,
    init: &Init,
    types: &mut TypePool,
    globals: &Globals,
    w: &mut impl Write,
) -> Result<(), Error> {
    match (types.get(ty).clone(), init) {
        (Type::Array(elem, n), Init::List(items)) => {
            if items.len() != n as usize {
                return Err(format!(
                    "initializer has {} elements, the array has {n}",
                    items.len()
                ))?;
            }
            for item in items {
                emit_init(elem, item, types, globals, w)?;
            }
        }
        (Type::Array(elem, n), Init::Repeat(item, count)) => {
            let count = consts::eval(count, types, &globals.statics)?;
            if count != n {
                return Err(format!(
                    "initializer repeats {count} times, the array has {n} elements"
                ))?;
            }
            // A repeated number is a .fill; anything else is written out.
            if let Init::Expr(e) = &**item
                && scalar_directive(types.get(elem)).is_some()
                && let Value::Num(v) = value(e, types, globals)?
            {
                let size = types.info(elem).size();
                check_fits(v, size)?;
                writeln!(w, "    .fill {n}, {size}, {:#x}", truncate(v, size))?;
            } else {
                for _ in 0..n {
                    emit_init(elem, item, types, globals, w)?;
                }
            }
        }
        (Type::Array(..), Init::Expr(_)) => {
            Err("an array needs a [a, b, c] or [x; n] initializer")?;
        }
        (Type::Struct(_), _) => Err("struct initializers aren't supported yet")?,
        (scalar, Init::Expr(e)) => {
            let directive = scalar_directive(&scalar).ok_or("can't initialize this type")?;
            let size = types.info(ty).size();
            match value(e, types, globals)? {
                Value::Num(v) => {
                    check_fits(v, size)?;
                    writeln!(w, "    {directive} {:#x}", truncate(v, size))?;
                }
                Value::Sym(name, offset) => {
                    if size != 4 {
                        Err("an address needs a 32 bit element")?;
                    }
                    if offset == 0 {
                        writeln!(w, "    .word {name}")?;
                    } else {
                        writeln!(w, "    .word {name} + {offset:#x}")?;
                    }
                }
            }
        }
        (_, _) => Err("a scalar needs an expression initializer, not a list")?,
    }
    Ok(())
}

fn scalar_directive(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::U32 | Type::I32 | Type::Default | Type::Ptr(_) => Some(".word"),
        Type::U16 | Type::I16 => Some(".hword"),
        Type::U8 | Type::I8 => Some(".byte"),
        _ => None,
    }
}

/// A value fits if it's in range either unsigned or sign extended, so
/// both 0xff and -1 fit in a byte.
fn check_fits(v: u32, size: usize) -> Result<(), Error> {
    if size < 4 {
        let bits = size * 8;
        let hi = v >> (bits - 1);
        if hi != 0 && hi != 1 && hi != (u32::MAX >> (bits - 1)) {
            Err(format!("{v:#x} doesn't fit in {size} bytes"))?;
        }
    }
    Ok(())
}

fn truncate(v: u32, size: usize) -> u32 {
    if size < 4 {
        v & ((1 << (size * 8)) - 1)
    } else {
        v
    }
}

enum Value {
    Num(u32),
    /// A symbol or static's address plus an offset, resolved by the linker
    Sym(String, u32),
}

/// The value of an element: a constant expression, or the address of a
/// symbol or static (`sym`, `&static`) plus an optional constant.
fn value(e: &Expr, types: &mut TypePool, globals: &Globals) -> Result<Value, Error> {
    if let Some(name) = address_name(e, globals) {
        return Ok(Value::Sym(name, 0));
    }
    if let Expr::Binop(a, op, b) = e
        && op.tok == crate::lex::TokBody::Plus
        && let Some(name) = address_name(a, globals)
    {
        let offset = consts::eval(b, types, &globals.statics)?;
        return Ok(Value::Sym(name, offset));
    }
    Ok(Value::Num(consts::eval(e, types, &globals.statics)?))
}

fn address_name(e: &Expr, globals: &Globals) -> Option<String> {
    match e {
        Expr::Ident(id) => id
            .as_ident()
            .filter(|id| globals.symbol_ty(id).is_some())
            .map(str::to_owned),
        Expr::Unary(op, inner) if op.tok == crate::lex::TokBody::Ampersand => inner
            .as_ident()
            .filter(|id| globals.static_ty(id).is_some())
            .map(str::to_owned),
        _ => None,
    }
}
