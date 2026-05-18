#![allow(dead_code)]

use crate::parser::ComponentDecl;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimitiveType {
    I32,
    F32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TypeLayout {
    pub size: u32,
    pub align: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentFieldOffset {
    pub name: String,
    pub type_name: String,
    pub offset: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentLayout {
    pub fields: Vec<ComponentFieldOffset>,
    pub size: u32,
    pub align: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComponentId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayoutError {
    pub message: String,
}

impl PrimitiveType {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "i32" => Some(Self::I32),
            "f32" => Some(Self::F32),
            _ => None,
        }
    }

    pub fn layout(self) -> TypeLayout {
        match self {
            Self::I32 => TypeLayout { size: 4, align: 4 },
            Self::F32 => TypeLayout { size: 4, align: 4 },
        }
    }
}

pub fn primitive_type_layout(name: &str) -> Option<TypeLayout> {
    PrimitiveType::from_name(name).map(PrimitiveType::layout)
}

pub fn component_qualified_name(world_name: &str, component_name: &str) -> String {
    format!("{world_name}.{component_name}")
}

pub fn stable_component_id(world_name: &str, component_name: &str) -> ComponentId {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in component_qualified_name(world_name, component_name).bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    ComponentId(hash)
}

pub fn compute_component_field_offsets(
    component: &ComponentDecl,
) -> Result<Vec<ComponentFieldOffset>, LayoutError> {
    compute_component_layout(component).map(|layout| layout.fields)
}

pub fn compute_component_layout(component: &ComponentDecl) -> Result<ComponentLayout, LayoutError> {
    let mut fields = Vec::new();
    let mut cursor = 0;
    let mut component_align = 1;

    for field in &component.fields {
        let layout = primitive_type_layout(&field.type_name.name).ok_or_else(|| LayoutError {
            message: format!(
                "unknown primitive type `{}` for component field `{}`",
                field.type_name.name, field.name
            ),
        })?;

        cursor = align_to(cursor, layout.align);
        component_align = component_align.max(layout.align);
        fields.push(ComponentFieldOffset {
            name: field.name.clone(),
            type_name: field.type_name.name.clone(),
            offset: cursor,
        });
        cursor += layout.size;
    }

    Ok(ComponentLayout {
        fields,
        size: align_to(cursor, component_align),
        align: component_align,
    })
}

fn align_to(value: u32, align: u32) -> u32 {
    debug_assert!(align > 0);
    let remainder = value % align;
    if remainder == 0 {
        value
    } else {
        value + (align - remainder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::parser;

    #[test]
    fn primitive_type_layouts() {
        assert_eq!(
            primitive_type_layout("i32"),
            Some(TypeLayout { size: 4, align: 4 })
        );
        assert_eq!(
            primitive_type_layout("f32"),
            Some(TypeLayout { size: 4, align: 4 })
        );
        assert_eq!(primitive_type_layout("unknown"), None);
    }

    #[test]
    fn computes_position_field_offsets() {
        let source = include_str!("../../../examples/position.arc");
        let tokens = lexer::lex(source).expect("position.arc lexes");
        let program = parser::parse_program(&tokens).expect("position.arc parses");
        let component = program
            .components
            .iter()
            .find(|component| component.name == "Position")
            .expect("Position component exists");

        assert_eq!(
            compute_component_field_offsets(component).expect("Position layout computes"),
            vec![
                ComponentFieldOffset {
                    name: "x".to_string(),
                    type_name: "f32".to_string(),
                    offset: 0,
                },
                ComponentFieldOffset {
                    name: "y".to_string(),
                    type_name: "f32".to_string(),
                    offset: 4,
                },
            ]
        );
    }

    #[test]
    fn computes_position_component_layout() {
        let source = include_str!("../../../examples/position.arc");
        let tokens = lexer::lex(source).expect("position.arc lexes");
        let program = parser::parse_program(&tokens).expect("position.arc parses");
        let component = program
            .components
            .iter()
            .find(|component| component.name == "Position")
            .expect("Position component exists");

        assert_eq!(
            compute_component_layout(component).expect("Position layout computes"),
            ComponentLayout {
                fields: vec![
                    ComponentFieldOffset {
                        name: "x".to_string(),
                        type_name: "f32".to_string(),
                        offset: 0,
                    },
                    ComponentFieldOffset {
                        name: "y".to_string(),
                        type_name: "f32".to_string(),
                        offset: 4,
                    },
                ],
                size: 8,
                align: 4,
            }
        );
    }

    #[test]
    fn stable_component_ids() {
        assert_eq!(
            component_qualified_name("Demo", "Position"),
            "Demo.Position"
        );

        let first = stable_component_id("Demo", "Position");
        let second = stable_component_id("Demo", "Position");

        assert_eq!(first, second);
        assert_eq!(first, ComponentId(0x002202c6aeb4f27b));
        assert_ne!(first, stable_component_id("Demo", "Velocity"));
    }
}
