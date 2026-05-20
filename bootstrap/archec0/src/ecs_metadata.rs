#![allow(dead_code)]

use crate::runtime_assembly::RuntimeProgramAssembly;

const MAGIC: &[u8; 8] = b"ARCHEECS";
const VERSION: u32 = 1;
const SECTION_ENTRY_SIZE: usize = 16;
const SECTION_KINDS: [u32; 6] = [
    1, // components
    2, // resources
    3, // systems
    4, // queries
    5, // schedules
    6, // startup operations
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EcsMetadataError {
    pub message: String,
}

pub fn encode_ecs_metadata(
    _assembly: &RuntimeProgramAssembly,
) -> Result<Vec<u8>, EcsMetadataError> {
    let envelope_size = (MAGIC.len() + 4 + 4 + SECTION_ENTRY_SIZE * SECTION_KINDS.len()) as u32;
    let mut bytes = Vec::with_capacity(envelope_size as usize);

    bytes.extend_from_slice(MAGIC);
    push_u32(&mut bytes, VERSION);
    push_u32(&mut bytes, SECTION_KINDS.len() as u32);

    for kind in SECTION_KINDS {
        push_u32(&mut bytes, kind);
        push_u32(&mut bytes, envelope_size);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
    }

    debug_assert_eq!(bytes.len(), envelope_size as usize);
    Ok(bytes)
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
