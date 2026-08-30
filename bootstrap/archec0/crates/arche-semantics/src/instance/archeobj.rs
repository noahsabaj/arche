//! ARCHEOBJ v1 64-byte directory envelope container serializer and deserializer (M27-D).

use std::collections::BTreeMap;

pub const ARCHEOBJ_MAGIC: &[u8; 8] = b"ARCHEOBJ";
pub const ARCHEOBJ_VERSION: u32 = 1;
pub const DIRECTORY_ENTRY_SIZE: u64 = 64;
pub const HEADER_SIZE: u32 = 64;

/// An entry in the ARCHEOBJ v1 section directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArcheObjSection {
    pub name: String,
    pub flags: u64,
    pub data: Vec<u8>,
}

/// A serialized ARCHEOBJ v1 package object container.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArcheObjFile {
    pub sections: BTreeMap<String, ArcheObjSection>,
}

impl ArcheObjFile {
    /// Creates an empty ARCHEOBJ container.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a section to the container.
    pub fn add_section(&mut self, name: impl Into<String>, flags: u64, data: Vec<u8>) {
        let name = name.into();
        self.sections
            .insert(name.clone(), ArcheObjSection { name, flags, data });
    }

    /// Serializes the ARCHEOBJ file into bytes conforming to the 64-byte directory envelope specification.
    #[must_use]
    pub fn write_to_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::new();

        // 1. Write placeholder 64-byte header
        buffer.extend_from_slice(ARCHEOBJ_MAGIC);
        buffer.extend_from_slice(&ARCHEOBJ_VERSION.to_le_bytes());
        buffer.extend_from_slice(&HEADER_SIZE.to_le_bytes());
        buffer.extend_from_slice(&0u64.to_le_bytes()); // flags

        // Offsets 24..64 are computed after packing sections
        buffer.resize(64, 0);

        // 2. Write section data payloads
        let mut section_offsets = Vec::new();
        for section in self.sections.values() {
            let offset = buffer.len() as u64;
            buffer.extend_from_slice(&section.data);
            section_offsets.push((section, offset));
        }

        // 3. Write 64-byte section directory table
        let dir_offset = buffer.len() as u64;
        let dir_count = self.sections.len() as u64;

        for (section, data_offset) in section_offsets {
            let mut entry = [0u8; 64];
            // Bytes 0..16: section name (padded with zeros)
            let name_bytes = section.name.as_bytes();
            let copy_len = name_bytes.len().min(16);
            entry[..copy_len].copy_from_slice(&name_bytes[..copy_len]);

            // Bytes 16..24: flags
            entry[16..24].copy_from_slice(&section.flags.to_le_bytes());
            // Bytes 24..32: offset
            entry[24..32].copy_from_slice(&data_offset.to_le_bytes());
            // Bytes 32..40: length
            entry[32..40].copy_from_slice(&(section.data.len() as u64).to_le_bytes());
            // Bytes 40..64: reserved zeros

            buffer.extend_from_slice(&entry);
        }

        let total_length = buffer.len() as u64;

        // 4. Backpatch the 64-byte header
        buffer[24..32].copy_from_slice(&total_length.to_le_bytes());
        buffer[32..40].copy_from_slice(&dir_offset.to_le_bytes());
        buffer[40..48].copy_from_slice(&dir_count.to_le_bytes());
        buffer[48..56].copy_from_slice(&DIRECTORY_ENTRY_SIZE.to_le_bytes());
        buffer[56..64].copy_from_slice(&0u64.to_le_bytes()); // reserved

        buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_archeobj_has_exact_64_byte_envelope() {
        let file = ArcheObjFile::new();
        let bytes = file.write_to_bytes();
        assert_eq!(bytes.len(), 64);
        assert_eq!(&bytes[0..8], ARCHEOBJ_MAGIC);
        assert_eq!(&bytes[8..12], &1u32.to_le_bytes());
        assert_eq!(&bytes[12..16], &64u32.to_le_bytes());
        assert_eq!(&bytes[24..32], &64u64.to_le_bytes()); // total len = 64
        assert_eq!(&bytes[32..40], &64u64.to_le_bytes()); // dir offset = 64
        assert_eq!(&bytes[40..48], &0u64.to_le_bytes()); // dir count = 0
    }

    #[test]
    fn archeobj_with_sections_encodes_directory() {
        let mut file = ArcheObjFile::new();
        file.add_section(".symtab", 0, vec![1, 2, 3, 4]);
        file.add_section(".consts", 0, vec![5, 6, 7, 8, 9]);

        let bytes = file.write_to_bytes();
        assert!(bytes.len() > 64);
        assert_eq!(&bytes[0..8], ARCHEOBJ_MAGIC);
        let dir_count = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
        assert_eq!(dir_count, 2);
    }
}
