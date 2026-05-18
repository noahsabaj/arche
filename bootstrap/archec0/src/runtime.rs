#![allow(dead_code)]

use crate::layout::ComponentId;
use std::alloc::{alloc, dealloc, Layout};
use std::ptr::NonNull;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArcheEntity(pub u64);

impl ArcheEntity {
    pub fn new(index: u32, generation: u32) -> Self {
        Self((u64::from(generation) << 32) | u64::from(index))
    }

    pub fn index(self) -> u32 {
        self.0 as u32
    }

    pub fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }

    pub fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EntityTable {
    slots: Vec<EntitySlot>,
    free_indices: Vec<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EntitySlot {
    generation: u32,
    alive: bool,
}

impl EntityTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn alloc(&mut self) -> ArcheEntity {
        if let Some(index) = self.free_indices.pop() {
            let slot = &mut self.slots[index as usize];
            slot.alive = true;
            return ArcheEntity::new(index, slot.generation);
        }

        let index = self.slots.len() as u32;
        self.slots.push(EntitySlot {
            generation: 0,
            alive: true,
        });
        ArcheEntity::new(index, 0)
    }

    pub fn free(&mut self, entity: ArcheEntity) -> bool {
        let index = entity.index();
        let Some(slot) = self.slots.get_mut(index as usize) else {
            return false;
        };

        if !slot.alive || slot.generation != entity.generation() {
            return false;
        }

        slot.alive = false;
        slot.generation = slot.generation.wrapping_add(1);
        self.free_indices.push(index);
        true
    }

    pub fn is_alive(&self, entity: ArcheEntity) -> bool {
        self.slots
            .get(entity.index() as usize)
            .is_some_and(|slot| slot.alive && slot.generation == entity.generation())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentFieldDescriptor {
    pub name: String,
    pub type_name: String,
    pub offset: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentDescriptor {
    pub id: ComponentId,
    pub name: String,
    pub size: u32,
    pub align: u32,
    pub fields: Vec<ComponentFieldDescriptor>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ComponentDescriptorTable {
    descriptors: Vec<ComponentDescriptor>,
}

impl ComponentDescriptorTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    pub fn register(&mut self, descriptor: ComponentDescriptor) -> bool {
        if self.get(descriptor.id).is_some() {
            return false;
        }

        self.descriptors.push(descriptor);
        true
    }

    pub fn get(&self, id: ComponentId) -> Option<&ComponentDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.id == id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchetypeKey {
    component_ids: Vec<ComponentId>,
}

impl ArchetypeKey {
    pub fn new(mut component_ids: Vec<ComponentId>) -> Self {
        component_ids.sort_by_key(|id| id.0);
        component_ids.dedup();
        Self { component_ids }
    }

    pub fn component_ids(&self) -> &[ComponentId] {
        &self.component_ids
    }
}

#[derive(Debug)]
pub struct ArchetypeTable {
    key: ArchetypeKey,
    entities: Vec<ArcheEntity>,
    columns: Vec<ComponentColumn>,
}

impl ArchetypeTable {
    pub fn new(key: ArchetypeKey) -> Self {
        Self {
            key,
            entities: Vec::new(),
            columns: Vec::new(),
        }
    }

    pub fn key(&self) -> &ArchetypeKey {
        &self.key
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn insert_entity(&mut self, entity: ArcheEntity) -> usize {
        let row = self.entities.len();
        self.entities.push(entity);
        row
    }

    pub fn entity(&self, row: usize) -> Option<ArcheEntity> {
        self.entities.get(row).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn allocate_component_column(
        &mut self,
        descriptor: &ComponentDescriptor,
        row_capacity: usize,
    ) -> Result<bool, ComponentColumnError> {
        if self.column(descriptor.id).is_some() {
            return Ok(false);
        }

        let column = ComponentColumn::allocate(descriptor, row_capacity)?;
        self.columns.push(column);
        Ok(true)
    }

    pub fn column(&self, id: ComponentId) -> Option<&ComponentColumn> {
        self.columns.iter().find(|column| column.component_id == id)
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }
}

#[derive(Debug)]
pub struct ComponentColumn {
    component_id: ComponentId,
    element_size: usize,
    element_align: usize,
    row_capacity: usize,
    row_count: usize,
    storage: NonNull<u8>,
    layout: Layout,
}

impl ComponentColumn {
    fn allocate(
        descriptor: &ComponentDescriptor,
        row_capacity: usize,
    ) -> Result<Self, ComponentColumnError> {
        let element_size = descriptor.size as usize;
        let element_align = descriptor.align as usize;

        if element_size == 0 {
            return Err(ComponentColumnError {
                message: "component element size must be greater than 0".to_string(),
            });
        }

        if row_capacity == 0 {
            return Err(ComponentColumnError {
                message: "component column capacity must be greater than 0".to_string(),
            });
        }

        let byte_size =
            element_size
                .checked_mul(row_capacity)
                .ok_or_else(|| ComponentColumnError {
                    message: "component column byte size overflowed".to_string(),
                })?;
        let layout = Layout::from_size_align(byte_size, element_align).map_err(|_| {
            ComponentColumnError {
                message: format!(
                    "invalid component column layout size {byte_size} align {element_align}"
                ),
            }
        })?;
        let storage = {
            let raw = unsafe { alloc(layout) };
            NonNull::new(raw).ok_or_else(|| ComponentColumnError {
                message: "component column allocation failed".to_string(),
            })?
        };

        Ok(Self {
            component_id: descriptor.id,
            element_size,
            element_align,
            row_capacity,
            row_count: 0,
            storage,
            layout,
        })
    }

    pub fn component_id(&self) -> ComponentId {
        self.component_id
    }

    pub fn element_size(&self) -> usize {
        self.element_size
    }

    pub fn element_align(&self) -> usize {
        self.element_align
    }

    pub fn row_capacity(&self) -> usize {
        self.row_capacity
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }

    pub fn storage_byte_size(&self) -> usize {
        self.layout.size()
    }

    pub fn storage_ptr(&self) -> *mut u8 {
        self.storage.as_ptr()
    }
}

impl Drop for ComponentColumn {
    fn drop(&mut self) {
        unsafe {
            dealloc(self.storage.as_ptr(), self.layout);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentColumnError {
    pub message: String,
}

#[derive(Debug)]
pub struct ArcheWorld {
    entities: EntityTable,
    component_descriptors: ComponentDescriptorTable,
    archetypes: Vec<ArchetypeTable>,
}

impl ArcheWorld {
    pub fn create() -> Self {
        Self {
            entities: EntityTable::new(),
            component_descriptors: ComponentDescriptorTable::new(),
            archetypes: Vec::new(),
        }
    }

    pub fn destroy(self) {}

    pub fn entities(&self) -> &EntityTable {
        &self.entities
    }

    pub fn alloc_entity(&mut self) -> ArcheEntity {
        self.entities.alloc()
    }

    pub fn component_descriptors(&self) -> &ComponentDescriptorTable {
        &self.component_descriptors
    }

    pub fn archetype_count(&self) -> usize {
        self.archetypes.len()
    }

    pub fn archetype(&self, key: &ArchetypeKey) -> Option<&ArchetypeTable> {
        self.archetypes.iter().find(|table| table.key() == key)
    }

    pub fn get_or_create_archetype(&mut self, key: ArchetypeKey) -> &mut ArchetypeTable {
        if let Some(index) = self.archetypes.iter().position(|table| table.key() == &key) {
            return &mut self.archetypes[index];
        }

        self.archetypes.push(ArchetypeTable::new(key));
        self.archetypes
            .last_mut()
            .expect("archetype table should exist after push")
    }

    pub fn is_empty(&self) -> bool {
        self.entities.len() == 0
            && self.component_descriptors.len() == 0
            && self.archetypes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arche_entity_packs_index_and_generation() {
        let entity = ArcheEntity::new(0x89abcdef, 0x01234567);

        assert_eq!(entity.raw(), 0x0123456789abcdef);
        assert_eq!(entity.index(), 0x89abcdef);
        assert_eq!(entity.generation(), 0x01234567);
        assert_eq!(ArcheEntity::new(u32::MAX, u32::MAX).raw(), u64::MAX);
    }

    #[test]
    fn entity_table_allocates_and_reuses_generation() {
        let mut entities = EntityTable::new();

        let entity_a = entities.alloc();

        assert_eq!(entity_a.index(), 0);
        assert_eq!(entity_a.generation(), 0);
        assert!(entities.is_alive(entity_a));

        assert!(entities.free(entity_a));
        assert!(!entities.is_alive(entity_a));

        let entity_b = entities.alloc();

        assert_eq!(entity_b.index(), 0);
        assert_eq!(entity_b.generation(), 1);
        assert!(entities.is_alive(entity_b));
        assert!(!entities.is_alive(entity_a));
        assert!(!entities.free(entity_a));
        assert!(entities.free(entity_b));
    }

    #[test]
    fn registers_position_component_descriptor() {
        let mut descriptors = ComponentDescriptorTable::new();
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let position = ComponentDescriptor {
            id: position_id,
            name: "Demo.Position".to_string(),
            size: 8,
            align: 4,
            fields: vec![
                ComponentFieldDescriptor {
                    name: "x".to_string(),
                    type_name: "f32".to_string(),
                    offset: 0,
                },
                ComponentFieldDescriptor {
                    name: "y".to_string(),
                    type_name: "f32".to_string(),
                    offset: 4,
                },
            ],
        };

        assert!(descriptors.register(position.clone()));

        let retrieved = descriptors
            .get(position_id)
            .expect("Position descriptor should be registered");
        assert_eq!(retrieved, &position);
        assert_eq!(retrieved.id, position_id);
        assert_eq!(retrieved.name, "Demo.Position");
        assert_eq!(retrieved.size, 8);
        assert_eq!(retrieved.align, 4);
        assert_eq!(retrieved.fields.len(), 2);
        assert_eq!(retrieved.fields[0].name, "x");
        assert_eq!(retrieved.fields[0].type_name, "f32");
        assert_eq!(retrieved.fields[0].offset, 0);
        assert_eq!(retrieved.fields[1].name, "y");
        assert_eq!(retrieved.fields[1].type_name, "f32");
        assert_eq!(retrieved.fields[1].offset, 4);

        let duplicate = ComponentDescriptor {
            id: position_id,
            name: "Demo.Position.Duplicate".to_string(),
            size: 16,
            align: 8,
            fields: Vec::new(),
        };

        assert!(!descriptors.register(duplicate));
        assert_eq!(descriptors.get(position_id), Some(&position));
    }

    #[test]
    fn creates_archetype_table_for_position() {
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let key = ArchetypeKey::new(vec![position_id]);

        assert_eq!(key.component_ids(), &[position_id]);

        let table = ArchetypeTable::new(key.clone());

        assert_eq!(table.key(), &key);
        assert_eq!(table.key().component_ids(), &[position_id]);
        assert_eq!(table.entity_count(), 0);
        assert!(table.is_empty());

        let duplicate_key = ArchetypeKey::new(vec![position_id, position_id]);

        assert_eq!(duplicate_key.component_ids(), &[position_id]);
    }

    #[test]
    fn allocates_position_component_column() {
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let position = ComponentDescriptor {
            id: position_id,
            name: "Demo.Position".to_string(),
            size: 8,
            align: 4,
            fields: vec![
                ComponentFieldDescriptor {
                    name: "x".to_string(),
                    type_name: "f32".to_string(),
                    offset: 0,
                },
                ComponentFieldDescriptor {
                    name: "y".to_string(),
                    type_name: "f32".to_string(),
                    offset: 4,
                },
            ],
        };
        let mut table = ArchetypeTable::new(ArchetypeKey::new(vec![position_id]));

        assert!(table
            .allocate_component_column(&position, 1)
            .expect("Position column allocation should succeed"));

        assert_eq!(table.column_count(), 1);
        assert_eq!(table.entity_count(), 0);
        assert!(table.is_empty());

        let column = table
            .column(position_id)
            .expect("Position column should be allocated");
        assert_eq!(column.component_id(), position_id);
        assert_eq!(column.element_size(), 8);
        assert_eq!(column.element_align(), 4);
        assert_eq!(column.row_capacity(), 1);
        assert_eq!(column.row_count(), 0);
        assert_eq!(column.storage_byte_size(), 8);
        assert_eq!((column.storage_ptr() as usize) % column.element_align(), 0);

        assert!(!table
            .allocate_component_column(&position, 1)
            .expect("duplicate allocation check should not fail"));
        assert_eq!(table.column_count(), 1);
    }

    #[test]
    fn world_gets_or_creates_position_archetype() {
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let position_key = ArchetypeKey::new(vec![position_id]);
        let mut world = ArcheWorld::create();

        assert_eq!(world.archetype_count(), 0);
        assert!(world.archetype(&position_key).is_none());

        {
            let table = world.get_or_create_archetype(position_key.clone());

            assert_eq!(table.key().component_ids(), &[position_id]);
            assert_eq!(table.entity_count(), 0);
            assert!(table.is_empty());
        }

        assert_eq!(world.archetype_count(), 1);
        assert!(world.archetype(&position_key).is_some());

        {
            let duplicate_key = ArchetypeKey::new(vec![position_id, position_id]);
            let table = world.get_or_create_archetype(duplicate_key);

            assert_eq!(table.key().component_ids(), &[position_id]);
            assert_eq!(table.entity_count(), 0);
            assert!(table.is_empty());
        }

        assert_eq!(world.archetype_count(), 1);
    }

    #[test]
    fn inserts_entity_into_position_archetype() {
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let position_key = ArchetypeKey::new(vec![position_id]);
        let mut world = ArcheWorld::create();
        let entity = world.alloc_entity();

        assert!(world.entities().is_alive(entity));

        {
            let table = world.get_or_create_archetype(position_key.clone());
            let row = table.insert_entity(entity);

            assert_eq!(row, 0);
            assert_eq!(table.entity_count(), 1);
            assert!(!table.is_empty());
            assert_eq!(table.entity(0), Some(entity));
            assert_eq!(table.entity(1), None);
        }

        assert!(world.entities().is_alive(entity));
        assert_eq!(world.entities().len(), 1);

        let table = world
            .archetype(&position_key)
            .expect("Position archetype table should exist");
        assert_eq!(table.entity_count(), 1);
        assert_eq!(table.entity(0), Some(entity));
    }

    #[test]
    fn world_create_destroy_smoke() {
        let world = ArcheWorld::create();

        assert_eq!(world.entities().len(), 0);
        assert_eq!(world.component_descriptors().len(), 0);
        assert_eq!(world.archetype_count(), 0);
        assert!(world.is_empty());

        world.destroy();
    }
}
