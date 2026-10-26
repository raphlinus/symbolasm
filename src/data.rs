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
/// with PC-relative loads. `#[link_section]` overrides the section, and
/// `#[align]` raises the alignment.
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
    let align = info.align().max(s.align.unwrap_or(1) as usize);
    writeln!(w, ".balign {align}")?;
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
        (Type::Array(elem, n), Init::Str(bytes)) => {
            if *types.get(elem) != Type::U8 {
                return Err("a string initializer needs a [u8; n]")?;
            }
            if bytes.len() != n as usize {
                return Err(format!(
                    "string has {} bytes, the array has {n}",
                    bytes.len()
                ))?;
            }
            writeln!(w, "    .ascii \"{}\"", ascii_escape(bytes))?;
        }
        (_, Init::Str(_)) => Err("a string initializer needs a [u8; n]")?,
        (Type::Array(..), Init::Expr(_)) => {
            Err("an array needs a [a, b, c] or [x; n] initializer")?;
        }
        (Type::Struct(sh), Init::Struct(name, items)) => {
            let name = name.as_ident().unwrap();
            if types.named_type(name) != Some(ty) {
                return Err(format!("{name} isn't the element's type"))?;
            }
            // Field names, types and offsets, copied out of the pool
            let fields: Vec<_> = types
                .fields(sh)
                .iter()
                .map(|f| (f.name.clone(), f.ty, f.offset))
                .collect();
            for (i, (field, _)) in items.iter().enumerate() {
                let field = field.as_ident().unwrap();
                if !fields.iter().any(|f| f.0 == field) {
                    return Err(format!("{name} has no field {field}"))?;
                }
                if items[..i].iter().any(|(f, _)| f.as_ident() == Some(field)) {
                    return Err(format!("field {field} is given twice"))?;
                }
            }
            // Fields in layout order, whatever order they're written in,
            // with padding between them and at the end
            let mut pos = 0;
            for (field, field_ty, offset) in fields {
                let Some((_, item)) = items.iter().find(|(f, _)| f.as_ident() == Some(&field))
                else {
                    return Err(format!("missing field {field}"))?;
                };
                if offset > pos {
                    writeln!(w, "    .space {}", offset - pos)?;
                }
                emit_init(field_ty, item, types, globals, w)?;
                pos = offset + types.info(field_ty).size();
            }
            let size = types.info(ty).size();
            if size > pos {
                writeln!(w, "    .space {}", size - pos)?;
            }
        }
        (Type::Struct(_), _) => Err("a struct needs a Name { field: value } initializer")?,
        (_, Init::Struct(..)) => Err("a struct initializer needs a struct type")?,
        (Type::F32, Init::Expr(e)) => {
            let bits = float_value(e).ok_or("an f32 needs a constant like 1.0 or -0.5")?;
            writeln!(w, "    .word {bits:#010x}")?;
        }
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
        Type::U32 | Type::I32 | Type::Default | Type::Ptr(_) | Type::Fn(_) => Some(".word"),
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
/// symbol, function or static (`sym`, `f`, `&static`) plus an optional
/// constant. A function's address has the Thumb bit set by the linker.
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

/// The bits of an f32 constant, `1.5` or `-1.5`
fn float_value(e: &Expr) -> Option<u32> {
    match e {
        Expr::Literal(lit) => match lit.tok {
            crate::lex::TokBody::Float(bits) => Some(bits),
            _ => None,
        },
        Expr::Unary(op, inner) if op.tok == crate::lex::TokBody::Minus => {
            float_value(inner).map(|bits| bits ^ 0x8000_0000)
        }
        _ => None,
    }
}

fn address_name(e: &Expr, globals: &Globals) -> Option<String> {
    match e {
        Expr::Ident(id) => id
            .as_ident()
            .filter(|id| globals.symbol_ty(id).is_some() || globals.fns.contains_key(*id))
            .map(str::to_owned),
        Expr::Unary(op, inner) if op.tok == crate::lex::TokBody::Ampersand => inner
            .as_ident()
            .filter(|id| globals.static_ty(id).is_some())
            .map(str::to_owned),
        _ => None,
    }
}

/// A string's bytes as the body of a GNU as `.ascii` directive: printable
/// ASCII as is, apart from `"` and `\\`, and anything else in octal.
fn ascii_escape(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            b'"' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..0x7f => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out
}
