// Copyright 2026 Raph Levien
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::collections::HashMap;

use crate::lex::{Error, WithLoc};

#[derive(Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TypeHandle(usize);

#[derive(Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StructHandle(usize);

#[derive(Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TupleHandle(usize);

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
        // we should probably align size up; this is needed for arrays, but
        // could conceivably be skipped for just embedding.
        self.info[handle.0] = TypeInfo::new(offset, align);
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
            // This will get filled in later (in `populate_struct`)
            Type::Struct(_) => TypeInfo::new(0, 0),
            Type::Tuple(_) => TypeInfo::new(0, 0),
        }
    }

    pub fn get_field(&self, s: StructHandle, field: &str) -> Option<&StructField> {
        let layout = &self.structs[s.0].0;
        // This is O(n) in struct size. If we expected huge structs, hashmap would be good.
        layout.iter().find(|f| f.name == field)
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

    #[expect(unused)]
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
}
