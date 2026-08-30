//! Reentrant WorldContext and ECS state machine with Archetype management (M27-F).

use crate::runtime::archetype::ArchetypeStorage;
use crate::runtime::commands::Command;
use crate::runtime::def::{CanonicalValue, EntityHandle};
use arche_foundation::identity::TypeId;
use std::collections::BTreeMap;

/// An entity slot in a WorldContext entity index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntitySlot {
    pub generation: u32,
    pub table_ordinal: Option<u32>,
    pub row_index: u32,
}

/// An isolated, reentrant execution context for an Arche world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldContext {
    pub world_id: u32,
    pub current_tick: u64,
    pub slots: Vec<EntitySlot>,
    pub free_slots: Vec<u32>,
    pub storage: ArchetypeStorage,
    pub resources: BTreeMap<TypeId, CanonicalValue>,
}

impl WorldContext {
    /// Creates a new isolated `WorldContext` with a specific world ID.
    #[must_use]
    pub fn new(world_id: u32) -> Self {
        Self {
            world_id,
            current_tick: 0,
            slots: Vec::new(),
            free_slots: Vec::new(),
            storage: ArchetypeStorage::new(),
            resources: BTreeMap::new(),
        }
    }

    /// Spawns a fresh entity handle (pre-allocating its slot).
    pub fn reserve_entity_handle(&mut self) -> EntityHandle {
        if let Some(slot_index) = self.free_slots.pop() {
            let slot = &mut self.slots[slot_index as usize];
            slot.generation = slot.generation.wrapping_add(1);
            slot.table_ordinal = None;
            slot.row_index = 0;
            EntityHandle {
                slot_index,
                generation: slot.generation,
            }
        } else {
            let slot_index = self.slots.len() as u32;
            self.slots.push(EntitySlot {
                generation: 1,
                table_ordinal: None,
                row_index: 0,
            });
            EntityHandle {
                slot_index,
                generation: 1,
            }
        }
    }

    /// Applies a batch of deferred structural commands at a schedule barrier.
    pub fn apply_commands(&mut self, commands: Vec<Command>) {
        for cmd in commands {
            match cmd {
                Command::Spawn { handle, components } => {
                    let s_idx = handle.slot_index as usize;
                    if s_idx < self.slots.len() {
                        let slot = self.slots[s_idx];
                        if slot.generation == handle.generation && slot.table_ordinal.is_none() {
                            let comp_types: Vec<TypeId> = components.keys().copied().collect();
                            let table_ord = self.storage.get_or_create_table(comp_types);
                            let row_index = self.storage.tables[table_ord as usize]
                                .push_entity(handle, components);

                            self.slots[s_idx].table_ordinal = Some(table_ord);
                            self.slots[s_idx].row_index = row_index;
                        }
                    }
                }
                Command::Despawn(handle) => {
                    let s_idx = handle.slot_index as usize;
                    if s_idx < self.slots.len() {
                        let slot = self.slots[s_idx];
                        if slot.generation == handle.generation {
                            if let Some(table_ord) = slot.table_ordinal {
                                let row_index = slot.row_index;
                                let table = &mut self.storage.tables[table_ord as usize];
                                let (_, _, displaced) = table.swap_remove(row_index);

                                if let Some((displaced_handle, new_row_index)) = displaced {
                                    self.slots[displaced_handle.slot_index as usize].row_index =
                                        new_row_index;
                                }

                                self.slots[s_idx].table_ordinal = None;
                                self.free_slots.push(handle.slot_index);
                            }
                        }
                    }
                }
                Command::AddComponent {
                    handle,
                    component_type_id,
                    value,
                } => {
                    let s_idx = handle.slot_index as usize;
                    if s_idx < self.slots.len() {
                        let slot = self.slots[s_idx];
                        if slot.generation == handle.generation {
                            if let Some(table_ord) = slot.table_ordinal {
                                let row_index = slot.row_index;
                                let table = &mut self.storage.tables[table_ord as usize];
                                let (_, mut comps, displaced) = table.swap_remove(row_index);

                                if let Some((displaced_handle, new_row_index)) = displaced {
                                    self.slots[displaced_handle.slot_index as usize].row_index =
                                        new_row_index;
                                }

                                comps.insert(component_type_id, value);
                                let new_types: Vec<TypeId> = comps.keys().copied().collect();
                                let new_table_ord = self.storage.get_or_create_table(new_types);
                                let new_row_index = self.storage.tables[new_table_ord as usize]
                                    .push_entity(handle, comps);

                                self.slots[s_idx].table_ordinal = Some(new_table_ord);
                                self.slots[s_idx].row_index = new_row_index;
                            }
                        }
                    }
                }
                Command::RemoveComponent {
                    handle,
                    component_type_id,
                } => {
                    let s_idx = handle.slot_index as usize;
                    if s_idx < self.slots.len() {
                        let slot = self.slots[s_idx];
                        if slot.generation == handle.generation {
                            if let Some(table_ord) = slot.table_ordinal {
                                let row_index = slot.row_index;
                                let table = &mut self.storage.tables[table_ord as usize];
                                let (_, mut comps, displaced) = table.swap_remove(row_index);

                                if let Some((displaced_handle, new_row_index)) = displaced {
                                    self.slots[displaced_handle.slot_index as usize].row_index =
                                        new_row_index;
                                }

                                comps.remove(&component_type_id);
                                let new_types: Vec<TypeId> = comps.keys().copied().collect();
                                let new_table_ord = self.storage.get_or_create_table(new_types);
                                let new_row_index = self.storage.tables[new_table_ord as usize]
                                    .push_entity(handle, comps);

                                self.slots[s_idx].table_ordinal = Some(new_table_ord);
                                self.slots[s_idx].row_index = new_row_index;
                            }
                        }
                    }
                }
                Command::InsertResource {
                    resource_type_id,
                    value,
                } => {
                    self.resources.insert(resource_type_id, value);
                }
                Command::RemoveResource { resource_type_id } => {
                    self.resources.remove(&resource_type_id);
                }
            }
        }
    }

    /// Resets the world context state to empty in O(1) bulk operations.
    pub fn reset_world(&mut self) {
        self.current_tick = 0;
        self.slots.clear();
        self.free_slots.clear();
        self.storage = ArchetypeStorage::new();
        self.resources.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_spawn_and_archetype_transition() {
        let mut world = WorldContext::new(1);
        let e1 = world.reserve_entity_handle();

        let c1 = TypeId::from_bytes([1; 16]);
        let c2 = TypeId::from_bytes([2; 16]);

        let mut comps = BTreeMap::new();
        comps.insert(c1, CanonicalValue::String("pos".into()));

        // Barrier 1: Spawn e1 with [c1]
        world.apply_commands(vec![Command::Spawn {
            handle: e1,
            components: comps,
        }]);

        assert_eq!(world.slots[0].table_ordinal, Some(0));
        assert_eq!(world.storage.tables[0].len(), 1);

        // Barrier 2: Add c2 to e1 -> moves to table with [c1, c2]
        world.apply_commands(vec![Command::AddComponent {
            handle: e1,
            component_type_id: c2,
            value: CanonicalValue::String("vel".into()),
        }]);

        assert_eq!(world.slots[0].table_ordinal, Some(1));
        assert_eq!(world.storage.tables[0].len(), 0);
        assert_eq!(world.storage.tables[1].len(), 1);
    }
}
