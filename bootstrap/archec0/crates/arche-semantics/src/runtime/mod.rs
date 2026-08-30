//! Values, reentrant runtime, entity lifecycle, and ARCHEOBS v3 for M27-E & M27-F.

pub mod archetype;
pub mod codec;
pub mod commands;
pub mod def;
pub mod obs3;
pub mod world;

pub use archetype::{Archetype, ArchetypeStorage};
pub use codec::{
    encode_canonical_value, serialize_archeval_container, validate_canonical_value,
    ValueValidationError, ARCHEVAL_MAGIC, ARCHEVAL_VERSION,
};
pub use commands::{Command, CommandBuffer};
pub use def::{ArchetypeTable, CanonicalScalar, CanonicalValue, EntityHandle};
pub use obs3::{OBS3Snapshot, ARCHEOBS_MAGIC, ARCHEOBS_VERSION};
pub use world::{EntitySlot, WorldContext};

#[cfg(test)]
mod tests {
    use super::*;
    use arche_foundation::identity::TypeId;
    use std::collections::BTreeMap;

    #[test]
    fn complete_entity_lifecycle_with_command_buffer_and_location_repair() {
        let mut world = WorldContext::new(10);
        let mut commands = CommandBuffer::new();

        let e1 = world.reserve_entity_handle();
        let e2 = world.reserve_entity_handle();

        let tid = TypeId::from_bytes([99; 16]);

        let mut comps1 = BTreeMap::new();
        comps1.insert(tid, CanonicalValue::String("e1".into()));

        let mut comps2 = BTreeMap::new();
        comps2.insert(tid, CanonicalValue::String("e2".into()));

        // Flush 1: Spawn both entities into table 0
        commands.spawn(e1, comps1);
        commands.spawn(e2, comps2);
        world.apply_commands(commands.drain_staged());

        assert_eq!(world.storage.tables[0].len(), 2);
        assert_eq!(world.slots[0].row_index, 0);
        assert_eq!(world.slots[1].row_index, 1);

        // Flush 2: Despawn e1 (row 0). e2 (row 1) must be moved to row 0 with location repair!
        commands.despawn(e1);
        world.apply_commands(commands.drain_staged());

        assert_eq!(world.storage.tables[0].len(), 1);
        assert_eq!(world.slots[0].table_ordinal, None); // e1 is dead
        assert_eq!(world.slots[1].table_ordinal, Some(0)); // e2 is in table 0
        assert_eq!(world.slots[1].row_index, 0); // e2 repaired to row 0!
    }
}
