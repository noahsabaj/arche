//! Columnar Archetype Storage with swap-remove location repair (M27-F).

use crate::runtime::def::{CanonicalValue, EntityHandle};
use arche_foundation::identity::TypeId;
use std::collections::BTreeMap;

/// A single columnar archetype table storing entities with an identical set of component types.
#[derive(Clone, Debug, PartialEq)]
pub struct Archetype {
    pub table_ordinal: u32,
    pub component_type_ids: Vec<TypeId>,
    pub entities: Vec<EntityHandle>,
    pub columns: BTreeMap<TypeId, Vec<CanonicalValue>>,
}

impl Archetype {
    /// Creates a new archetype table for the given sorted component type IDs.
    #[must_use]
    pub fn new(table_ordinal: u32, mut component_type_ids: Vec<TypeId>) -> Self {
        component_type_ids.sort();
        let mut columns = BTreeMap::new();
        for &cid in &component_type_ids {
            columns.insert(cid, Vec::new());
        }
        Self {
            table_ordinal,
            component_type_ids,
            entities: Vec::new(),
            columns,
        }
    }

    /// Number of active entities in this table.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether this table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Appends an entity and its component values to this table.
    /// Returns the allocated row index.
    pub fn push_entity(
        &mut self,
        handle: EntityHandle,
        mut components: BTreeMap<TypeId, CanonicalValue>,
    ) -> u32 {
        let row_index = self.entities.len() as u32;
        self.entities.push(handle);
        for &cid in &self.component_type_ids {
            let val = components.remove(&cid).unwrap_or(CanonicalValue::Unit);
            self.columns.get_mut(&cid).unwrap().push(val);
        }
        row_index
    }

    /// Performs swap-remove at `row_index`.
    /// Returns:
    /// 1. The removed entity handle and its components.
    /// 2. Optional displaced entity handle and its new row index (if a swap occurred).
    pub fn swap_remove(
        &mut self,
        row_index: u32,
    ) -> (
        EntityHandle,
        BTreeMap<TypeId, CanonicalValue>,
        Option<(EntityHandle, u32)>,
    ) {
        let idx = row_index as usize;
        let last_idx = self.entities.len() - 1;

        let removed_handle = self.entities.swap_remove(idx);
        let mut removed_components = BTreeMap::new();

        for (&cid, col) in &mut self.columns {
            let removed_val = col.swap_remove(idx);
            removed_components.insert(cid, removed_val);
        }

        let displaced = if idx < last_idx {
            // The entity that was at `last_idx` is now at `idx`
            Some((self.entities[idx], row_index))
        } else {
            None
        };

        (removed_handle, removed_components, displaced)
    }
}

/// Global storage manager for all archetype tables in a WorldContext.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArchetypeStorage {
    pub tables: Vec<Archetype>,
    pub signature_to_ordinal: BTreeMap<Vec<TypeId>, u32>,
}

impl ArchetypeStorage {
    /// Creates an empty archetype storage.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Finds or creates an archetype table for the given component types.
    pub fn get_or_create_table(&mut self, mut component_types: Vec<TypeId>) -> u32 {
        component_types.sort();
        if let Some(&ordinal) = self.signature_to_ordinal.get(&component_types) {
            return ordinal;
        }
        let ordinal = self.tables.len() as u32;
        let table = Archetype::new(ordinal, component_types.clone());
        self.tables.push(table);
        self.signature_to_ordinal.insert(component_types, ordinal);
        ordinal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archetype_swap_remove_location_repair() {
        let c1 = TypeId::from_bytes([1; 16]);
        let mut table = Archetype::new(0, vec![c1]);

        let e0 = EntityHandle {
            slot_index: 0,
            generation: 1,
        };
        let e1 = EntityHandle {
            slot_index: 1,
            generation: 1,
        };
        let e2 = EntityHandle {
            slot_index: 2,
            generation: 1,
        };

        let mut comps0 = BTreeMap::new();
        comps0.insert(c1, CanonicalValue::String("e0".into()));
        table.push_entity(e0, comps0);

        let mut comps1 = BTreeMap::new();
        comps1.insert(c1, CanonicalValue::String("e1".into()));
        table.push_entity(e1, comps1);

        let mut comps2 = BTreeMap::new();
        comps2.insert(c1, CanonicalValue::String("e2".into()));
        table.push_entity(e2, comps2);

        assert_eq!(table.len(), 3);

        // Remove row 0 (e0). e2 should be swapped into row 0.
        let (removed, _, displaced) = table.swap_remove(0);
        assert_eq!(removed, e0);
        assert_eq!(displaced, Some((e2, 0)));
        assert_eq!(table.len(), 2);
        assert_eq!(table.entities[0], e2);
        assert_eq!(table.entities[1], e1);
    }
}
