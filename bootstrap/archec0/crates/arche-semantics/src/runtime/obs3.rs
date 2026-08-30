//! ARCHEOBS v3 64-byte directory envelope container serializer (M27-E).

use crate::runtime::codec::encode_canonical_value;
use crate::runtime::def::{ArchetypeTable, CanonicalValue, EntityHandle};
use arche_foundation::identity::TypeId;
use std::collections::BTreeMap;

pub const ARCHEOBS_MAGIC: &[u8; 8] = b"ARCHEOBS";
pub const ARCHEOBS_VERSION: u32 = 3;
pub const DIRECTORY_ENTRY_SIZE: u64 = 64;
pub const HEADER_SIZE: u32 = 64;

/// Complete semantic snapshot of an executing Arche ECS simulation world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OBS3Snapshot {
    pub package_id: [u8; 16],
    pub lock_hash: [u8; 32],
    pub profile_name: String,
    pub step_tick: u64,
    pub resources: BTreeMap<TypeId, CanonicalValue>,
    pub tables: Vec<ArchetypeTable>,
    pub entities: Vec<EntityHandle>,
    pub free_list: Vec<u32>,
}

impl OBS3Snapshot {
    /// Creates an empty snapshot.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Serializes this snapshot into an `ARCHEOBS` v3 64-byte directory envelope container.
    #[must_use]
    pub fn write_to_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::new();

        // 1. 64-byte header
        buffer.extend_from_slice(ARCHEOBS_MAGIC);
        buffer.extend_from_slice(&ARCHEOBS_VERSION.to_le_bytes());
        buffer.extend_from_slice(&HEADER_SIZE.to_le_bytes());
        buffer.extend_from_slice(&0u64.to_le_bytes()); // flags
        buffer.resize(64, 0);

        // 2. Sections
        let mut section_entries = Vec::new();

        // Section .meta
        let meta_offset = buffer.len() as u64;
        let mut meta_data = Vec::new();
        meta_data.extend_from_slice(&self.package_id);
        meta_data.extend_from_slice(&self.lock_hash);
        meta_data.extend_from_slice(&(self.profile_name.len() as u64).to_le_bytes());
        meta_data.extend_from_slice(self.profile_name.as_bytes());
        meta_data.extend_from_slice(&self.step_tick.to_le_bytes());
        buffer.extend_from_slice(&meta_data);
        section_entries.push((".meta", meta_offset, meta_data.len() as u64));

        // Section .resources
        let res_offset = buffer.len() as u64;
        let mut res_data = Vec::new();
        res_data.extend_from_slice(&(self.resources.len() as u64).to_le_bytes());
        for (tid, val) in &self.resources {
            res_data.extend_from_slice(tid.as_bytes());
            encode_canonical_value(val, &mut res_data);
        }
        buffer.extend_from_slice(&res_data);
        section_entries.push((".resources", res_offset, res_data.len() as u64));

        // Section .tables
        let tables_offset = buffer.len() as u64;
        let mut tables_data = Vec::new();
        tables_data.extend_from_slice(&(self.tables.len() as u64).to_le_bytes());
        for table in &self.tables {
            tables_data.extend_from_slice(&table.table_ordinal.to_le_bytes());
            tables_data.extend_from_slice(&(table.component_type_ids.len() as u64).to_le_bytes());
            for c in &table.component_type_ids {
                tables_data.extend_from_slice(c.as_bytes());
            }
            tables_data.extend_from_slice(&(table.entity_handles.len() as u64).to_le_bytes());
            for e in &table.entity_handles {
                tables_data.extend_from_slice(&e.slot_index.to_le_bytes());
                tables_data.extend_from_slice(&e.generation.to_le_bytes());
            }
        }
        buffer.extend_from_slice(&tables_data);
        section_entries.push((".tables", tables_offset, tables_data.len() as u64));

        // Section .entities
        let entities_offset = buffer.len() as u64;
        let mut entities_data = Vec::new();
        entities_data.extend_from_slice(&(self.entities.len() as u64).to_le_bytes());
        for e in &self.entities {
            entities_data.extend_from_slice(&e.slot_index.to_le_bytes());
            entities_data.extend_from_slice(&e.generation.to_le_bytes());
        }
        entities_data.extend_from_slice(&(self.free_list.len() as u64).to_le_bytes());
        for f in &self.free_list {
            entities_data.extend_from_slice(&f.to_le_bytes());
        }
        buffer.extend_from_slice(&entities_data);
        section_entries.push((".entities", entities_offset, entities_data.len() as u64));

        // 3. Directory Table
        let dir_offset = buffer.len() as u64;
        let dir_count = section_entries.len() as u64;

        for (name, offset, len) in section_entries {
            let mut entry = [0u8; 64];
            let name_bytes = name.as_bytes();
            let copy_len = name_bytes.len().min(16);
            entry[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
            entry[24..32].copy_from_slice(&offset.to_le_bytes());
            entry[32..40].copy_from_slice(&len.to_le_bytes());
            buffer.extend_from_slice(&entry);
        }

        let total_len = buffer.len() as u64;
        buffer[24..32].copy_from_slice(&total_len.to_le_bytes());
        buffer[32..40].copy_from_slice(&dir_offset.to_le_bytes());
        buffer[40..48].copy_from_slice(&dir_count.to_le_bytes());
        buffer[48..56].copy_from_slice(&DIRECTORY_ENTRY_SIZE.to_le_bytes());

        buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obs3_empty_snapshot_header() {
        let snapshot = OBS3Snapshot::new();
        let bytes = snapshot.write_to_bytes();

        assert_eq!(&bytes[0..8], ARCHEOBS_MAGIC);
        assert_eq!(&bytes[8..12], &3u32.to_le_bytes());
        assert_eq!(&bytes[12..16], &64u32.to_le_bytes());
    }
}
