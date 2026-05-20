#![allow(dead_code)]

use crate::runtime::{
    ComponentDescriptor, ComponentFieldDescriptor, ResourceDescriptor, ResourceFieldDescriptor,
};
use crate::runtime_assembly::RuntimeProgramAssembly;

const MAGIC: &[u8; 8] = b"ARCHEECS";
const VERSION: u32 = 1;
const SECTION_ENTRY_SIZE: usize = 16;
const SECTION_COMPONENTS: u32 = 1;
const SECTION_RESOURCES: u32 = 2;
const SECTION_SYSTEMS: u32 = 3;
const SECTION_QUERIES: u32 = 4;
const SECTION_SCHEDULES: u32 = 5;
const SECTION_STARTUP_OPERATIONS: u32 = 6;
const SECTION_KINDS: [u32; 6] = [
    SECTION_COMPONENTS,
    SECTION_RESOURCES,
    SECTION_SYSTEMS,
    SECTION_QUERIES,
    SECTION_SCHEDULES,
    SECTION_STARTUP_OPERATIONS,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EcsMetadataError {
    pub message: String,
}

pub fn encode_ecs_metadata(assembly: &RuntimeProgramAssembly) -> Result<Vec<u8>, EcsMetadataError> {
    let envelope_size = (MAGIC.len() + 4 + 4 + SECTION_ENTRY_SIZE * SECTION_KINDS.len()) as u32;
    let sections = [
        SectionPayload {
            kind: SECTION_COMPONENTS,
            bytes: encode_component_descriptors(&assembly.component_descriptors),
            record_count: assembly.component_descriptors.len() as u32,
        },
        SectionPayload {
            kind: SECTION_RESOURCES,
            bytes: encode_resource_descriptors(&assembly.resource_descriptors),
            record_count: assembly.resource_descriptors.len() as u32,
        },
        SectionPayload::empty(SECTION_SYSTEMS),
        SectionPayload::empty(SECTION_QUERIES),
        SectionPayload::empty(SECTION_SCHEDULES),
        SectionPayload::empty(SECTION_STARTUP_OPERATIONS),
    ];
    let payload_size = sections
        .iter()
        .map(|section| section.bytes.len())
        .sum::<usize>();
    let mut bytes = Vec::with_capacity(envelope_size as usize + payload_size);

    bytes.extend_from_slice(MAGIC);
    push_u32(&mut bytes, VERSION);
    push_u32(&mut bytes, SECTION_KINDS.len() as u32);

    let mut section_offset = envelope_size;
    for section in &sections {
        push_u32(&mut bytes, section.kind);
        push_u32(&mut bytes, section_offset);
        push_u32(&mut bytes, section.bytes.len() as u32);
        push_u32(&mut bytes, section.record_count);
        section_offset += section.bytes.len() as u32;
    }

    for section in &sections {
        bytes.extend_from_slice(&section.bytes);
    }

    Ok(bytes)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SectionPayload {
    kind: u32,
    bytes: Vec<u8>,
    record_count: u32,
}

impl SectionPayload {
    fn empty(kind: u32) -> Self {
        Self {
            kind,
            bytes: Vec::new(),
            record_count: 0,
        }
    }
}

fn encode_component_descriptors(descriptors: &[ComponentDescriptor]) -> Vec<u8> {
    let mut bytes = Vec::new();

    for descriptor in descriptors {
        push_u64(&mut bytes, descriptor.id.0);
        push_string(&mut bytes, &descriptor.name);
        push_u32(&mut bytes, descriptor.size);
        push_u32(&mut bytes, descriptor.align);
        encode_component_fields(&mut bytes, &descriptor.fields);
    }

    bytes
}

fn encode_resource_descriptors(descriptors: &[ResourceDescriptor]) -> Vec<u8> {
    let mut bytes = Vec::new();

    for descriptor in descriptors {
        push_u64(&mut bytes, descriptor.id.0);
        push_string(&mut bytes, &descriptor.name);
        push_u32(&mut bytes, descriptor.size);
        push_u32(&mut bytes, descriptor.align);
        encode_resource_fields(&mut bytes, &descriptor.fields);
    }

    bytes
}

fn encode_component_fields(bytes: &mut Vec<u8>, fields: &[ComponentFieldDescriptor]) {
    push_u32(bytes, fields.len() as u32);
    for field in fields {
        push_string(bytes, &field.name);
        push_string(bytes, &field.type_name);
        push_u32(bytes, field.offset);
    }
}

fn encode_resource_fields(bytes: &mut Vec<u8>, fields: &[ResourceFieldDescriptor]) {
    push_u32(bytes, fields.len() as u32);
    for field in fields {
        push_string(bytes, &field.name);
        push_string(bytes, &field.type_name);
        push_u32(bytes, field.offset);
    }
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
    use crate::runtime_assembly::assemble_runtime_program_from_source;
    use crate::{lexer, parser};

    #[test]
    fn defines_ecs_metadata_binary_envelope() {
        let assembly = RuntimeProgramAssembly::new("Demo");
        let metadata = encode_ecs_metadata(&assembly).expect("ECS metadata encodes");

        assert_eq!(metadata.len(), 112);
        assert_eq!(&metadata[0..8], b"ARCHEECS");
        assert_eq!(u32::from_le_bytes(metadata[8..12].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(metadata[12..16].try_into().unwrap()), 6);

        let mut offset = 16;
        for expected_kind in [1, 2, 3, 4, 5, 6] {
            assert_eq!(
                u32::from_le_bytes(metadata[offset..offset + 4].try_into().unwrap()),
                expected_kind
            );
            assert_eq!(
                u32::from_le_bytes(metadata[offset + 4..offset + 8].try_into().unwrap()),
                112
            );
            assert_eq!(
                u32::from_le_bytes(metadata[offset + 8..offset + 12].try_into().unwrap()),
                0
            );
            assert_eq!(
                u32::from_le_bytes(metadata[offset + 12..offset + 16].try_into().unwrap()),
                0
            );
            offset += 16;
        }
        assert_eq!(offset, metadata.len());
    }

    #[test]
    fn encodes_component_resource_descriptors_in_ecs_metadata() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime assembly encodes");
        let metadata = encode_ecs_metadata(&assembly).expect("ECS metadata encodes");

        assert_eq!(metadata.len(), 303);
        assert_eq!(&metadata[0..8], b"ARCHEECS");
        assert_eq!(read_u32_at(&metadata, 8), 1);
        assert_eq!(read_u32_at(&metadata, 12), 6);

        assert_section(&metadata, 0, 1, 112, 138, 2);
        assert_section(&metadata, 1, 2, 250, 53, 1);
        for index in 2..6 {
            assert_section(&metadata, index, (index + 1) as u32, 303, 0, 0);
        }

        let mut offset = 112;
        assert_descriptor(
            &metadata,
            &mut offset,
            0x002202c6aeb4f27b,
            "Demo.Position",
            8,
            4,
            &[("x", "f32", 0), ("y", "f32", 4)],
        );
        assert_descriptor(
            &metadata,
            &mut offset,
            0x2cf8a68bcb7f913b,
            "Demo.Velocity",
            8,
            4,
            &[("x", "f32", 0), ("y", "f32", 4)],
        );
        assert_eq!(offset, 250);
        assert_descriptor(
            &metadata,
            &mut offset,
            0x7924ce11db524521,
            "Demo.Time",
            4,
            4,
            &[("delta", "f32", 0)],
        );
        assert_eq!(offset, metadata.len());
    }

    fn assert_section(
        metadata: &[u8],
        index: usize,
        expected_kind: u32,
        expected_offset: u32,
        expected_byte_len: u32,
        expected_record_count: u32,
    ) {
        let offset = 16 + index * 16;
        assert_eq!(read_u32_at(metadata, offset), expected_kind);
        assert_eq!(read_u32_at(metadata, offset + 4), expected_offset);
        assert_eq!(read_u32_at(metadata, offset + 8), expected_byte_len);
        assert_eq!(read_u32_at(metadata, offset + 12), expected_record_count);
    }

    fn assert_descriptor(
        metadata: &[u8],
        offset: &mut usize,
        expected_id: u64,
        expected_name: &str,
        expected_size: u32,
        expected_align: u32,
        expected_fields: &[(&str, &str, u32)],
    ) {
        assert_eq!(read_u64(metadata, offset), expected_id);
        assert_eq!(read_string(metadata, offset), expected_name);
        assert_eq!(read_u32(metadata, offset), expected_size);
        assert_eq!(read_u32(metadata, offset), expected_align);
        assert_eq!(read_u32(metadata, offset), expected_fields.len() as u32);
        for (expected_name, expected_type_name, expected_offset) in expected_fields {
            assert_eq!(read_string(metadata, offset), *expected_name);
            assert_eq!(read_string(metadata, offset), *expected_type_name);
            assert_eq!(read_u32(metadata, offset), *expected_offset);
        }
    }

    fn read_u32_at(metadata: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(metadata[offset..offset + 4].try_into().unwrap())
    }

    fn read_u32(metadata: &[u8], offset: &mut usize) -> u32 {
        let value = read_u32_at(metadata, *offset);
        *offset += 4;
        value
    }

    fn read_u64(metadata: &[u8], offset: &mut usize) -> u64 {
        let value = u64::from_le_bytes(metadata[*offset..*offset + 8].try_into().unwrap());
        *offset += 8;
        value
    }

    fn read_string(metadata: &[u8], offset: &mut usize) -> String {
        let length = read_u32(metadata, offset) as usize;
        let value = String::from_utf8(metadata[*offset..*offset + length].to_vec()).unwrap();
        *offset += length;
        value
    }
}
