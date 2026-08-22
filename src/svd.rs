use std::collections::HashMap;

use roxml::{BodyNode, XmlElement, XmlReader};

use crate::{
    lex::Error,
    types::{StructField, Type, TypeHandle, TypePool},
};

#[derive(Default, Debug)]
pub struct Peripherals {
    pub periphs: HashMap<String, Peripheral>,
}

#[derive(Debug)]
pub struct Peripheral {
    name: String,
    pub base_address: u32,
    pub ty: TypeHandle,
}

pub fn parse_svd(svd: &str, types: &mut TypePool) -> Result<Peripherals, Error> {
    eprintln!("parsing {svd}");
    let bytes = std::fs::read_to_string(svd)?;
    let reader = XmlReader::parse(&bytes);
    let root = reader.root_element()?;
    let mut periphs = Peripherals::default();
    for child in root.children() {
        if let BodyNode::Element(e) = child {
            if e.tag_name() == "peripherals" {
                for periph in e.children() {
                    if let BodyNode::Element(p) = periph {
                        parse_peripheral(p, types, &mut periphs)?;
                    }
                }
            }
        }
    }
    eprintln!("{periphs:?}");
    Ok(periphs)
}

fn parse_peripheral(
    e: XmlElement,
    types: &mut TypePool,
    periphs: &mut Peripherals,
) -> Result<(), Error> {
    let mut name = None;
    let mut base_address = 0;
    let mut ty = None;
    for child in e.children() {
        if let BodyNode::Element(e) = child {
            let tag_name = e.tag_name();
            eprintln!("{tag_name}");
            match tag_name {
                "name" => {
                    name = get_text_content(e);
                }
                "baseAddress" => {
                    if let Some(addr) = get_hex_content(e) {
                        base_address = addr;
                    }
                }
                "registers" => {
                    ty = Some(parse_registers(e, types)?);
                }
                _ => (),
            }
        }
    }
    if let Some(name) = name
        && let Some(ty) = ty
    {
        let peripheral = Peripheral {
            name: name.to_string(),
            base_address,
            ty,
        };
        periphs.periphs.insert(name.to_string(), peripheral);
    }
    Ok(())
}

// TODO: figure return value; do we intern struct to TypeHandle, or return fields?
fn parse_registers(e: XmlElement, types: &mut TypePool) -> Result<TypeHandle, Error> {
    let mut fields = vec![];
    for child in e.children() {
        if let BodyNode::Element(e) = child
            && e.tag_name() == "register"
        {
            fields.extend(parse_register(e, types))
        }
    }
    // TODO: we could wire up peripherals/addressBlock/size but not clear it's useful.
    let size = 0;
    let type_handle = types.build_struct(fields, size, 4);
    let ptr_handle = types.get_handle(&Type::Ptr(type_handle));
    Ok(ptr_handle)
}

fn parse_register(e: XmlElement, types: &mut TypePool) -> Option<StructField> {
    let mut name = None;
    let mut offset = 0;
    for child in e.children() {
        if let BodyNode::Element(e) = child {
            let tag_name = e.tag_name();
            match tag_name {
                "name" => name = get_text_content(e),
                "addressOffset" => {
                    if let Some(o) = get_hex_content(e) {
                        offset = o;
                    }
                }
                _ => (),
            }
        }
    }
    name.map(|name| {
        let ty = types.get_handle(&crate::types::Type::U32);
        StructField {
            name: name.to_string(),
            ty,
            offset: offset as usize,
        }
    })
}

fn get_text_content(e: XmlElement<'_>) -> Option<&str> {
    for child in e.children() {
        if let BodyNode::Text(t) = child {
            return Some(t.text());
        }
    }
    None
}

fn get_hex_content(e: XmlElement<'_>) -> Option<u32> {
    get_text_content(e).and_then(|s| {
        let stripped = s.strip_prefix("0x").unwrap_or(s);
        u32::from_str_radix(stripped, 16).ok()
    })
}
