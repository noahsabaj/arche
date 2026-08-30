use crate::parser::ComponentDecl;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimitiveType {
    I32,
    F32,
    Bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TypeLayout {
    pub size: u64,
    pub align: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComponentFieldOffset<'a> {
    pub name: &'a str,
    pub type_name: &'a str,
    pub offset: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentLayout<'a> {
    pub fields: Vec<ComponentFieldOffset<'a>>,
    pub size: u64,
    pub align: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayoutError {
    pub message: String,
}

impl PrimitiveType {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "i32" => Some(Self::I32),
            "f32" => Some(Self::F32),
            "bool" => Some(Self::Bool),
            _ => None,
        }
    }

    pub const fn layout(self) -> TypeLayout {
        match self {
            Self::I32 | Self::F32 => TypeLayout { size: 4, align: 4 },
            Self::Bool => TypeLayout { size: 1, align: 1 },
        }
    }
}

pub fn compute_component_layout(
    component: &ComponentDecl,
) -> Result<ComponentLayout<'_>, LayoutError> {
    let mut fields = Vec::new();
    fields
        .try_reserve_exact(component.fields.len())
        .map_err(|_| layout_error("could not allocate component field layout"))?;
    let mut cursor = 0u64;
    let mut component_align = 1u64;

    for field in &component.fields {
        let primitive = PrimitiveType::from_name(&field.type_name.name).ok_or_else(|| {
            layout_error(format!(
                "unknown primitive type `{}` for component field `{}`",
                field.type_name.name, field.name
            ))
        })?;
        let layout = primitive.layout();
        cursor = align_to(cursor, layout.align)?;
        component_align = component_align.max(layout.align);
        fields.push(ComponentFieldOffset {
            name: &field.name,
            type_name: &field.type_name.name,
            offset: cursor,
        });
        cursor = cursor
            .checked_add(layout.size)
            .ok_or_else(|| layout_error("component field layout overflows u64"))?;
    }

    Ok(ComponentLayout {
        fields,
        size: align_to(cursor, component_align)?,
        align: component_align,
    })
}

fn align_to(value: u64, align: u64) -> Result<u64, LayoutError> {
    debug_assert!(align.is_power_of_two());
    value
        .checked_add(align - 1)
        .map(|padded| padded & !(align - 1))
        .ok_or_else(|| layout_error("component alignment overflows u64"))
}

fn layout_error(message: impl Into<String>) -> LayoutError {
    LayoutError {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser};

    #[test]
    fn primitive_type_layouts() {
        assert_eq!(
            PrimitiveType::from_name("i32").map(PrimitiveType::layout),
            Some(TypeLayout { size: 4, align: 4 })
        );
        assert_eq!(
            PrimitiveType::from_name("f32").map(PrimitiveType::layout),
            Some(TypeLayout { size: 4, align: 4 })
        );
        assert_eq!(
            PrimitiveType::from_name("bool").map(PrimitiveType::layout),
            Some(TypeLayout { size: 1, align: 1 })
        );
        assert_eq!(PrimitiveType::from_name("unknown"), None);
    }

    #[test]
    fn computes_position_component_layout_with_u64_offsets() {
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
                        name: "x",
                        type_name: "f32",
                        offset: 0,
                    },
                    ComponentFieldOffset {
                        name: "y",
                        type_name: "f32",
                        offset: 4,
                    },
                ],
                size: 8,
                align: 4,
            }
        );
    }

    #[test]
    fn zero_field_components_have_zero_size_and_alignment_one() {
        let tokens = lexer::lex("world Demo component Empty {} startup { exit 0 }")
            .expect("empty component lexes");
        let program = parser::parse_program(&tokens).expect("empty component parses");
        assert_eq!(
            compute_component_layout(&program.components[0])
                .expect("empty component layout computes"),
            ComponentLayout {
                fields: vec![],
                size: 0,
                align: 1,
            }
        );
    }
}
