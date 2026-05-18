use crate::layout;
use crate::parser::Program;

const MAGIC: &[u8; 8] = b"ARCHECMP";
const VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentMetadataError {
    pub message: String,
}

pub fn encode_component_metadata(program: &Program) -> Result<Vec<u8>, ComponentMetadataError> {
    if program.components.is_empty() {
        return Ok(Vec::new());
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    push_u32(&mut bytes, VERSION);
    push_u32(&mut bytes, program.components.len() as u32);

    for component in &program.components {
        let qualified_name = layout::component_qualified_name(&program.world.name, &component.name);
        let component_id = layout::stable_component_id(&program.world.name, &component.name);
        let component_layout = layout::compute_component_layout(component).map_err(|error| {
            ComponentMetadataError {
                message: error.message,
            }
        })?;

        push_u64(&mut bytes, component_id.0);
        push_string(&mut bytes, &qualified_name);
        push_u32(&mut bytes, component_layout.size);
        push_u32(&mut bytes, component_layout.align);
        push_u32(&mut bytes, component_layout.fields.len() as u32);

        for field in component_layout.fields {
            push_string(&mut bytes, &field.name);
            push_string(&mut bytes, &field.type_name);
            push_u32(&mut bytes, field.offset);
        }
    }

    Ok(bytes)
}

fn push_string(bytes: &mut Vec<u8>, value: &str) {
    push_u32(bytes, value.len() as u32);
    bytes.extend_from_slice(value.as_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::parser;

    #[test]
    fn encodes_position_component_metadata() {
        let source = include_str!("../../../examples/position.arc");
        let tokens = lexer::lex(source).expect("position.arc lexes");
        let program = parser::parse_program(&tokens).expect("position.arc parses");
        let metadata = encode_component_metadata(&program).expect("component metadata encodes");

        assert_eq!(&metadata[0..8], b"ARCHECMP");
        assert_eq!(u32::from_le_bytes(metadata[8..12].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(metadata[12..16].try_into().unwrap()), 1);
        assert_eq!(
            u64::from_le_bytes(metadata[16..24].try_into().unwrap()),
            0x002202c6aeb4f27b
        );
    }
}
