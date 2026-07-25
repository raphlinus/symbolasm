use std::collections::HashMap;

use crate::lex::Error;

#[derive(Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TypeHandle(usize);

#[derive(Default)]
pub struct TypePool {
    types: Vec<Type>,
    inv_map: HashMap<Type, TypeHandle>,
}

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
    // TODO: structs and more
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
            self.inv_map.insert(ty.clone(), handle);
            handle
        }
    }

    pub fn get(&self, handle: TypeHandle) -> &Type {
        &self.types[handle.0]
    }

    pub fn from_ast(&mut self, ty: &crate::parse::Type) -> Result<TypeHandle, Error> {
        match ty {
            crate::parse::Type::Ident(id) => {
                let prim_ty = match id.as_ident().unwrap() {
                    "u32" => Type::U32,
                    "u16" => Type::U16,
                    "u8" => Type::U8,
                    "i32" => Type::I32,
                    "i16" => Type::I16,
                    "i8" => Type::I8,
                    _ => Err("unknown type")?,
                };
                Ok(self.get_handle(&prim_ty))
            }
            crate::parse::Type::Ptr(body) => {
                let body = self.from_ast(body)?;
                let ty = Type::Ptr(body);
                Ok(self.get_handle(&ty))
            }
        }
    }
}
