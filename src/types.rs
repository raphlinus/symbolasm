// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::collections::HashMap;

use crate::{
    abi::Abi,
    error::{Error, WithLoc},
};

#[derive(Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TypeHandle(usize);

#[derive(Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StructHandle(usize);

#[derive(Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TupleHandle(usize);

#[derive(Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FnHandle(usize);

/// A function's signature. Default types are stored as u32, so that a
/// declaration with a type and a definition without one match.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct FnSig {
    pub abi: Abi,
    pub args: Vec<TypeHandle>,
    pub rets: Vec<TypeHandle>,
}

#[derive(Default)]
pub struct TypePool {
    types: Vec<Type>,
    inv_map: HashMap<Type, TypeHandle>,
    // info is parallel to types
    info: Vec<TypeInfo>,
    globals: HashMap<String, TypeHandle>,
    structs: Vec<StructLayout>,
    tuples: Vec<Vec<TypeHandle>>,
    tuple_inv: HashMap<Vec<TypeHandle>, TupleHandle>,
    sigs: Vec<FnSig>,
    sig_inv: HashMap<FnSig, FnHandle>,
    /// Values of `const` items. They live here because array lengths in
    /// types can refer to them.
    consts: HashMap<String, u32>,
}

// We can probably derive Copy here too
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Type {
    Default,
    U32,
    U16,
    U8,
    I32,
    I16,
    I8,
    Ptr(TypeHandle),
    Struct(StructHandle),
    Tuple(TupleHandle),
    /// Element type and count
    Array(TypeHandle, u32),
    /// The address of a function with this signature
    Fn(FnHandle),
}

#[derive(Clone, Copy)]
pub struct TypeInfo {
    size: usize,
    align: usize,
}

#[derive(Default)]
pub struct StructLayout(pub Vec<StructField>);

#[derive(Debug)]
pub struct StructField {
    pub name: String,
    pub ty: TypeHandle,
    pub offset: usize,
}

impl TypePool {
    pub fn new() -> Self {
        let mut pool = TypePool::default();
        _ = pool.get_handle(&Type::Default);
        pool
    }

    pub fn get_handle(&mut self, ty: &Type) -> TypeHandle {
        if let Some(handle) = self.inv_map.get(ty) {
            *handle
        } else {
            let handle = TypeHandle(self.types.len());
            self.types.push(ty.clone());
            self.info.push(self.get_info(ty));
            self.inv_map.insert(ty.clone(), handle);
            handle
        }
    }

    /// A type as it would be written in source
    pub fn display(&self, handle: TypeHandle) -> String {
        match self.get(handle) {
            Type::Default | Type::U32 => "u32".into(),
            Type::U16 => "u16".into(),
            Type::U8 => "u8".into(),
            Type::I32 => "i32".into(),
            Type::I16 => "i16".into(),
            Type::I8 => "i8".into(),
            Type::Ptr(t) => format!("*{}", self.display(*t)),
            Type::Struct(_) => self
                .globals
                .iter()
                .find(|(_, h)| **h == handle)
                .map_or_else(|| "struct".into(), |(name, _)| name.clone()),
            Type::Tuple(t) => {
                let els: Vec<String> = self.tuples[t.0].iter().map(|e| self.display(*e)).collect();
                format!("({})", els.join(", "))
            }
            Type::Array(t, n) => format!("[{}; {n}]", self.display(*t)),
            Type::Fn(f) => {
                let sig = &self.sigs[f.0];
                let list = |tys: &[TypeHandle]| -> String {
                    let els: Vec<String> = tys.iter().map(|t| self.display(*t)).collect();
                    els.join(", ")
                };
                let abi = match sig.abi {
                    Abi::Aapcs => String::new(),
                    abi => format!("[abi = {}]", abi.name()),
                };
                let rets = match sig.rets.len() {
                    0 => String::new(),
                    1 => format!(" -> {}", list(&sig.rets)),
                    _ => format!(" -> ({})", list(&sig.rets)),
                };
                format!("fn{abi}({}){rets}", list(&sig.args))
            }
        }
    }

    pub fn get(&self, handle: TypeHandle) -> &Type {
        &self.types[handle.0]
    }

    pub fn intern_from_ast(&mut self, ty: &crate::parse::Type) -> Result<TypeHandle, Error> {
        match ty {
            crate::parse::Type::Ident(id) => {
                let ty_name = id.as_ident().unwrap();
                let prim_ty = match ty_name {
                    "u32" => Type::U32,
                    "u16" => Type::U16,
                    "u8" => Type::U8,
                    "i32" => Type::I32,
                    "i16" => Type::I16,
                    "i8" => Type::I8,
                    _ => {
                        if let Some(ty) = self.globals.get(ty_name) {
                            return Ok(*ty);
                        } else {
                            Err(format!("unknown type {ty_name}"))?
                        }
                    }
                };
                Ok(self.get_handle(&prim_ty))
            }
            crate::parse::Type::Ptr(body) => {
                let body = self.intern_from_ast(body)?;
                let ty = Type::Ptr(body);
                Ok(self.get_handle(&ty))
            }
            crate::parse::Type::Array(elem, count) => {
                let elem = self.intern_from_ast(elem)?;
                if self.info(elem).align() == 0 {
                    Err("array element type must be defined before use")?;
                }
                let count = crate::consts::eval(count, self, &HashMap::new())?;
                Ok(self.get_handle(&Type::Array(elem, count)))
            }
            crate::parse::Type::Fn(f) => {
                let args = f
                    .args
                    .iter()
                    .map(|t| self.intern_from_ast(t))
                    .collect::<Result<Vec<_>, _>>()?;
                let rets = f
                    .rets
                    .iter()
                    .map(|t| self.intern_from_ast(t))
                    .collect::<Result<Vec<_>, _>>()?;
                self.intern_fn(f.abi, args, rets)
            }
        }
    }

    /// The type of a function's address, given its signature. Arguments
    /// and results must fit in a register.
    pub fn intern_fn(
        &mut self,
        abi: Abi,
        args: Vec<TypeHandle>,
        rets: Vec<TypeHandle>,
    ) -> Result<TypeHandle, Error> {
        let u32_ty = self.get_handle(&Type::U32);
        let norm = |tys: Vec<TypeHandle>, what: &str| -> Result<Vec<TypeHandle>, Error> {
            tys.into_iter()
                .map(|t| match self.get(t) {
                    Type::Default => Ok(u32_ty),
                    Type::Struct(_) | Type::Tuple(_) | Type::Array(..) => Err(format!(
                        "{what} must fit in a register, not {}",
                        self.display(t)
                    )
                    .into()),
                    _ => Ok(t),
                })
                .collect()
        };
        let args = norm(args, "arguments")?;
        let rets = norm(rets, "results")?;
        if rets.len() > abi.n_regs() {
            Err(format!(
                "a {} function returns at most {} results",
                abi.name(),
                abi.n_regs()
            ))?;
        }
        let sig = FnSig { abi, args, rets };
        let handle = if let Some(h) = self.sig_inv.get(&sig) {
            *h
        } else {
            let h = FnHandle(self.sigs.len());
            self.sigs.push(sig.clone());
            self.sig_inv.insert(sig, h);
            h
        };
        Ok(self.get_handle(&Type::Fn(handle)))
    }

    /// The signature of a function type, if it is one
    pub fn sig(&self, ty: TypeHandle) -> Option<&FnSig> {
        match self.get(ty) {
            Type::Fn(f) => Some(&self.sigs[f.0]),
            _ => None,
        }
    }

    /// A primitive or struct type by name.
    pub fn named_type(&mut self, name: &str) -> Option<TypeHandle> {
        let prim_ty = match name {
            "u32" => Type::U32,
            "u16" => Type::U16,
            "u8" => Type::U8,
            "i32" => Type::I32,
            "i16" => Type::I16,
            "i8" => Type::I8,
            _ => return self.globals.get(name).copied(),
        };
        Some(self.get_handle(&prim_ty))
    }

    pub fn const_value(&self, name: &str) -> Option<u32> {
        self.consts.get(name).copied()
    }

    pub fn define_const(&mut self, name: &str, value: u32) -> bool {
        self.consts.insert(name.to_owned(), value).is_none()
    }

    /// The type a pointer to `ty` points to, decaying arrays to their
    /// elements as in C, so `&buf` for an array is a pointer to its first
    /// element.
    pub fn decay(&self, ty: TypeHandle) -> TypeHandle {
        match self.get(ty) {
            Type::Array(elem, _) => *elem,
            _ => ty,
        }
    }

    pub fn pointee(&self, handle: TypeHandle) -> Option<&Type> {
        if let Type::Ptr(p) = self.get(handle) {
            Some(self.get(*p))
        } else {
            None
        }
    }

    /// Register a struct into the global symbol table.
    pub fn register_struct(&mut self, s: &crate::parse::Struct) -> Result<(), Error> {
        // TODO: deal with struct already defined
        let ix = self.structs.len();
        let handle = self.get_handle(&Type::Struct(StructHandle(ix)));
        self.structs.push(StructLayout::default());
        let name = s.name.as_ident().unwrap().to_string();
        self.globals.insert(name, handle);
        Ok(())
    }

    pub fn populate_struct(&mut self, s: &crate::parse::Struct) -> Result<(), Error> {
        let mut fields = vec![];
        let mut offset = 0;
        let mut align = 1;
        for field in &s.fields {
            let name = field.name.as_ident().unwrap().to_string();
            let ty = self.intern_from_ast(&field.ty).at(&field.name.loc)?;
            let field_info = self.info[ty.0];
            if field_info.align == 0 {
                return Err("recursive struct (or maybe just not topologically sorted)".into())
                    .at(&field.name.loc);
            }
            let align_mask = field_info.align - 1;
            offset = (offset + align_mask) & !align_mask;
            fields.push(StructField { name, ty, offset });
            offset += field_info.size;
            align = align.max(field_info.align);
        }
        let handle = self
            .globals
            .get(s.name.as_ident().unwrap())
            .ok_or("struct not registered")?;
        // C layout: the size is a multiple of the alignment, so arrays of
        // the struct keep every element aligned.
        let size = (offset + align - 1) & !(align - 1);
        self.info[handle.0] = TypeInfo::new(size, align);
        if let &Type::Struct(sh) = self.get(*handle) {
            self.structs[sh.0] = StructLayout(fields);
        } else {
            unreachable!("struct not registered properly");
        }
        Ok(())
    }

    pub fn build_struct(
        &mut self,
        fields: Vec<StructField>,
        size: usize,
        align: usize,
    ) -> TypeHandle {
        let ix = self.structs.len();
        let handle = self.get_handle(&Type::Struct(StructHandle(ix)));
        self.structs.push(StructLayout(fields));
        self.info[handle.0] = TypeInfo::new(size, align);
        handle
    }

    fn get_info(&self, ty: &Type) -> TypeInfo {
        match ty {
            Type::Default => TypeInfo::new(4, 4),
            Type::U32 => TypeInfo::new(4, 4),
            Type::U16 => TypeInfo::new(2, 2),
            Type::U8 => TypeInfo::new(1, 1),
            Type::I32 => TypeInfo::new(4, 4),
            Type::I16 => TypeInfo::new(2, 2),
            Type::I8 => TypeInfo::new(1, 1),
            Type::Ptr(_type_handle) => TypeInfo::new(4, 4),
            Type::Fn(_) => TypeInfo::new(4, 4),
            // This will get filled in later (in `populate_struct`)
            Type::Struct(_) => TypeInfo::new(0, 0),
            Type::Tuple(_) => TypeInfo::new(0, 0),
            Type::Array(elem, count) => {
                let elem = self.info[elem.0];
                TypeInfo::new(elem.size * *count as usize, elem.align)
            }
        }
    }

    pub fn get_field(&self, s: StructHandle, field: &str) -> Option<&StructField> {
        let layout = &self.structs[s.0].0;
        // This is O(n) in struct size. If we expected huge structs, hashmap would be good.
        layout.iter().find(|f| f.name == field)
    }

    /// A struct's fields, in declaration order.
    pub fn fields(&self, s: StructHandle) -> &[StructField] {
        &self.structs[s.0].0
    }

    pub fn intern_tuple(&mut self, els: &[TypeHandle]) -> TypeHandle {
        if let Some(handle) = self.tuple_inv.get(els) {
            self.get_handle(&Type::Tuple(*handle))
        } else {
            let handle = TupleHandle(self.tuples.len());
            self.tuples.push(els.to_vec());
            self.tuple_inv.insert(els.to_vec(), handle);
            // Layout follows the same rules as structs.
            let mut offset = 0;
            let mut align = 1;
            for el in els {
                let el_info = self.info[el.0];
                let align_mask = el_info.align - 1;
                offset = (offset + align_mask) & !align_mask;
                offset += el_info.size;
                align = align.max(el_info.align);
            }
            let info = TypeInfo::new(offset, align);
            let ty = Type::Tuple(handle);
            let ty_handle = TypeHandle(self.types.len());
            self.types.push(ty.clone());
            self.info.push(info);
            self.inv_map.insert(ty, ty_handle);
            ty_handle
        }
    }

    pub fn info(&self, handle: TypeHandle) -> TypeInfo {
        self.info[handle.0]
    }
}

impl Type {
    pub fn is_signed(&self) -> bool {
        matches!(self, Type::I8 | Type::I16 | Type::I32)
    }
}

impl TypeInfo {
    fn new(size: usize, align: usize) -> Self {
        TypeInfo { size, align }
    }

    pub fn size(self) -> usize {
        self.size
    }

    pub fn align(self) -> usize {
        self.align
    }
}
