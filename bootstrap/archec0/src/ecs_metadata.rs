#![allow(dead_code)]

use crate::runtime::{
    ComponentDescriptor, ComponentFieldDescriptor, QueryAccess, QueryDescriptor,
    QueryTermDescriptor, ResourceDescriptor, ResourceFieldDescriptor, ScheduleDescriptor,
    ScheduleItemDescriptor, SystemAccess, SystemDescriptor, SystemParamDescriptorKind,
    SystemQueryTermDescriptor,
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
const SYSTEM_PARAM_READ_RESOURCE: u32 = 1;
const SYSTEM_PARAM_QUERY: u32 = 2;
const DESCRIPTOR_ACCESS_READ: u32 = 1;
const DESCRIPTOR_ACCESS_MUT: u32 = 2;
const SCHEDULE_ITEM_RUN: u32 = 1;
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
        SectionPayload {
            kind: SECTION_SYSTEMS,
            bytes: encode_system_descriptors(&assembly.system_descriptors),
            record_count: assembly.system_descriptors.len() as u32,
        },
        SectionPayload {
            kind: SECTION_QUERIES,
            bytes: encode_query_descriptors(&assembly.query_descriptors),
            record_count: assembly.query_descriptors.len() as u32,
        },
        SectionPayload {
            kind: SECTION_SCHEDULES,
            bytes: encode_schedule_descriptors(&assembly.schedule_descriptors),
            record_count: assembly.schedule_descriptors.len() as u32,
        },
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

fn encode_system_descriptors(descriptors: &[SystemDescriptor]) -> Vec<u8> {
    let mut bytes = Vec::new();

    for descriptor in descriptors {
        push_u64(&mut bytes, descriptor.id.0);
        push_string(&mut bytes, &descriptor.name);
        push_u32(&mut bytes, descriptor.params.len() as u32);
        for param in &descriptor.params {
            push_string(&mut bytes, &param.name);
            match &param.kind {
                SystemParamDescriptorKind::ReadResource { resource_id, name } => {
                    push_u32(&mut bytes, SYSTEM_PARAM_READ_RESOURCE);
                    push_u64(&mut bytes, resource_id.0);
                    push_string(&mut bytes, name);
                }
                SystemParamDescriptorKind::Query { terms } => {
                    push_u32(&mut bytes, SYSTEM_PARAM_QUERY);
                    encode_system_query_terms(&mut bytes, terms);
                }
            }
        }
    }

    bytes
}

fn encode_system_query_terms(bytes: &mut Vec<u8>, terms: &[SystemQueryTermDescriptor]) {
    push_u32(bytes, terms.len() as u32);
    for term in terms {
        push_u32(bytes, system_access_code(&term.access));
        push_u64(bytes, term.component_id.0);
        push_string(bytes, &term.name);
    }
}

fn encode_query_descriptors(descriptors: &[QueryDescriptor]) -> Vec<u8> {
    let mut bytes = Vec::new();

    for descriptor in descriptors {
        push_u64(&mut bytes, descriptor.id.0);
        push_string(&mut bytes, &descriptor.name);
        encode_query_terms(&mut bytes, &descriptor.terms);
    }

    bytes
}

fn encode_query_terms(bytes: &mut Vec<u8>, terms: &[QueryTermDescriptor]) {
    push_u32(bytes, terms.len() as u32);
    for term in terms {
        push_u32(bytes, query_access_code(&term.access));
        push_u64(bytes, term.component_id.0);
        push_string(bytes, &term.name);
    }
}

fn encode_schedule_descriptors(descriptors: &[ScheduleDescriptor]) -> Vec<u8> {
    let mut bytes = Vec::new();

    for descriptor in descriptors {
        push_u64(&mut bytes, descriptor.id.0);
        push_string(&mut bytes, &descriptor.name);
        push_u32(&mut bytes, descriptor.items.len() as u32);
        for item in &descriptor.items {
            match item {
                ScheduleItemDescriptor::Run {
                    system_id,
                    system_name,
                } => {
                    push_u32(&mut bytes, SCHEDULE_ITEM_RUN);
                    push_u64(&mut bytes, system_id.0);
                    push_string(&mut bytes, system_name);
                }
            }
        }
    }

    bytes
}

fn system_access_code(access: &SystemAccess) -> u32 {
    match access {
        SystemAccess::Read => DESCRIPTOR_ACCESS_READ,
        SystemAccess::Mut => DESCRIPTOR_ACCESS_MUT,
    }
}

fn query_access_code(access: &QueryAccess) -> u32 {
    match access {
        QueryAccess::Read => DESCRIPTOR_ACCESS_READ,
        QueryAccess::Mut => DESCRIPTOR_ACCESS_MUT,
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

        assert!(metadata.len() >= 303);
        assert_eq!(&metadata[0..8], b"ARCHEECS");
        assert_eq!(read_u32_at(&metadata, 8), 1);
        assert_eq!(read_u32_at(&metadata, 12), 6);

        assert_section(&metadata, 0, 1, 112, 138, 2);
        assert_section(&metadata, 1, 2, 250, 53, 1);

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
        assert_eq!(offset, 303);
    }

    #[test]
    fn encodes_system_query_schedule_descriptors_in_ecs_metadata() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime assembly encodes");
        let metadata = encode_ecs_metadata(&assembly).expect("ECS metadata encodes");

        assert_eq!(metadata.len(), 577);
        assert_eq!(&metadata[0..8], b"ARCHEECS");
        assert_eq!(read_u32_at(&metadata, 8), 1);
        assert_eq!(read_u32_at(&metadata, 12), 6);
        assert_section(&metadata, 0, 1, 112, 138, 2);
        assert_section(&metadata, 1, 2, 250, 53, 1);
        assert_section(&metadata, 2, 3, 303, 134, 1);
        assert_section(&metadata, 3, 4, 437, 90, 1);
        assert_section(&metadata, 4, 5, 527, 50, 1);
        assert_section(&metadata, 5, 6, 577, 0, 0);

        let mut offset = 303;
        assert_eq!(read_u64(&metadata, &mut offset), 0x723b6b52df270ed5);
        assert_eq!(read_string(&metadata, &mut offset), "Demo.Move");
        assert_eq!(read_u32(&metadata, &mut offset), 2);
        assert_eq!(read_string(&metadata, &mut offset), "time");
        assert_eq!(read_u32(&metadata, &mut offset), SYSTEM_PARAM_READ_RESOURCE);
        assert_eq!(read_u64(&metadata, &mut offset), 0x7924ce11db524521);
        assert_eq!(read_string(&metadata, &mut offset), "Demo.Time");
        assert_eq!(read_string(&metadata, &mut offset), "movers");
        assert_eq!(read_u32(&metadata, &mut offset), SYSTEM_PARAM_QUERY);
        assert_query_terms(&metadata, &mut offset);
        assert_eq!(offset, 437);

        assert_eq!(read_u64(&metadata, &mut offset), 0xf4004232b85cef9f);
        assert_eq!(read_string(&metadata, &mut offset), "Demo.Move.movers");
        assert_query_terms(&metadata, &mut offset);
        assert_eq!(offset, 527);

        assert_eq!(read_u64(&metadata, &mut offset), 0xed3d905325519b05);
        assert_eq!(read_string(&metadata, &mut offset), "Demo.Main");
        assert_eq!(read_u32(&metadata, &mut offset), 1);
        assert_eq!(read_u32(&metadata, &mut offset), SCHEDULE_ITEM_RUN);
        assert_eq!(read_u64(&metadata, &mut offset), 0x723b6b52df270ed5);
        assert_eq!(read_string(&metadata, &mut offset), "Demo.Move");
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

    fn assert_query_terms(metadata: &[u8], offset: &mut usize) {
        assert_eq!(read_u32(metadata, offset), 2);
        assert_eq!(read_u32(metadata, offset), DESCRIPTOR_ACCESS_MUT);
        assert_eq!(read_u64(metadata, offset), 0x002202c6aeb4f27b);
        assert_eq!(read_string(metadata, offset), "Demo.Position");
        assert_eq!(read_u32(metadata, offset), DESCRIPTOR_ACCESS_READ);
        assert_eq!(read_u64(metadata, offset), 0x2cf8a68bcb7f913b);
        assert_eq!(read_string(metadata, offset), "Demo.Velocity");
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
