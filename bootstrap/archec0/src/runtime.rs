#![allow(dead_code)]

use crate::layout::ComponentId;
use std::alloc::{alloc_zeroed, dealloc, Layout};
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

    fn prepare_alloc(&mut self) -> Result<(), SpawnEntityError> {
        if !self.free_indices.is_empty() {
            return Ok(());
        }

        u32::try_from(self.slots.len()).map_err(|_| SpawnEntityError {
            message: "entity index space is exhausted".to_string(),
        })?;
        self.slots.try_reserve(1).map_err(|error| SpawnEntityError {
            message: format!("failed to reserve entity slot: {error}"),
        })
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceFieldDescriptor {
    pub name: String,
    pub type_name: String,
    pub offset: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceDescriptor {
    pub id: ResourceId,
    pub name: String,
    pub size: u32,
    pub align: u32,
    pub fields: Vec<ResourceFieldDescriptor>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResourceDescriptorTable {
    descriptors: Vec<ResourceDescriptor>,
}

impl ResourceDescriptorTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    pub fn register(&mut self, descriptor: ResourceDescriptor) -> bool {
        if self.get(descriptor.id).is_some() {
            return false;
        }

        self.descriptors.push(descriptor);
        true
    }

    pub fn get(&self, id: ResourceId) -> Option<&ResourceDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.id == id)
    }

    pub fn descriptors(&self) -> &[ResourceDescriptor] {
        &self.descriptors
    }
}

fn stable_qualified_id(world_name: &str, item_name: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in format!("{world_name}.{item_name}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

pub fn stable_resource_id(world_name: &str, resource_name: &str) -> ResourceId {
    ResourceId(stable_qualified_id(world_name, resource_name))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SystemId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SystemAccess {
    Read,
    Mut,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemQueryTermDescriptor {
    pub access: SystemAccess,
    pub component_id: ComponentId,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SystemParamDescriptorKind {
    ReadResource {
        resource_id: ResourceId,
        name: String,
    },
    Query {
        terms: Vec<SystemQueryTermDescriptor>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemParamDescriptor {
    pub name: String,
    pub kind: SystemParamDescriptorKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemDescriptor {
    pub id: SystemId,
    pub name: String,
    pub params: Vec<SystemParamDescriptor>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SystemDescriptorTable {
    descriptors: Vec<SystemDescriptor>,
}

impl SystemDescriptorTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    pub fn register(&mut self, descriptor: SystemDescriptor) -> bool {
        if self.get(descriptor.id).is_some() {
            return false;
        }

        self.descriptors.push(descriptor);
        true
    }

    pub fn get(&self, id: SystemId) -> Option<&SystemDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.id == id)
    }
}

pub fn stable_system_id(world_name: &str, system_name: &str) -> SystemId {
    SystemId(stable_qualified_id(world_name, system_name))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScheduleItemDescriptor {
    Run {
        system_id: SystemId,
        system_name: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleDescriptor {
    pub id: ScheduleId,
    pub name: String,
    pub items: Vec<ScheduleItemDescriptor>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScheduleDescriptorTable {
    descriptors: Vec<ScheduleDescriptor>,
}

impl ScheduleDescriptorTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    pub fn register(&mut self, descriptor: ScheduleDescriptor) -> bool {
        if self.get(descriptor.id).is_some() {
            return false;
        }

        self.descriptors.push(descriptor);
        true
    }

    pub fn get(&self, id: ScheduleId) -> Option<&ScheduleDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.id == id)
    }
}

pub fn stable_schedule_id(world_name: &str, schedule_name: &str) -> ScheduleId {
    ScheduleId(stable_qualified_id(world_name, schedule_name))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchedulePlanEntry {
    pub system_id: SystemId,
    pub system_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchedulePlan {
    pub schedule_id: ScheduleId,
    pub schedule_name: String,
    pub entries: Vec<SchedulePlanEntry>,
}

impl SchedulePlan {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[SchedulePlanEntry] {
        &self.entries
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchedulePlanError {
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleExecuteError {
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryAccess {
    Read,
    Mut,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryTermDescriptor {
    pub access: QueryAccess,
    pub component_id: ComponentId,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryDescriptor {
    pub id: QueryId,
    pub name: String,
    pub terms: Vec<QueryTermDescriptor>,
}

impl QueryDescriptor {
    pub fn matches_archetype_key(&self, key: &ArchetypeKey) -> bool {
        self.terms
            .iter()
            .all(|term| key.component_ids().contains(&term.component_id))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryPlanEntry {
    pub archetype_index: usize,
    pub key: ArchetypeKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryPlan {
    pub query_id: QueryId,
    pub query_name: String,
    pub entries: Vec<QueryPlanEntry>,
}

impl QueryPlan {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[QueryPlanEntry] {
        &self.entries
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryRow {
    pub archetype_index: usize,
    pub row: usize,
    pub entity: ArcheEntity,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QueryDescriptorTable {
    descriptors: Vec<QueryDescriptor>,
}

impl QueryDescriptorTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    pub fn register(&mut self, descriptor: QueryDescriptor) -> bool {
        if self.get(descriptor.id).is_some() {
            return false;
        }

        self.descriptors.push(descriptor);
        true
    }

    pub fn get(&self, id: QueryId) -> Option<&QueryDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.id == id)
    }
}

pub fn stable_query_id(world_name: &str, system_name: &str, query_name: &str) -> QueryId {
    QueryId(stable_qualified_id(
        &format!("{world_name}.{system_name}"),
        query_name,
    ))
}

#[derive(Debug)]
struct AlignedByteStorage {
    storage: NonNull<u8>,
    layout: Layout,
}

impl AlignedByteStorage {
    fn allocate_zeroed(
        byte_size: usize,
        byte_align: usize,
        allocation_name: &str,
    ) -> Result<Self, String> {
        if byte_size == 0 {
            return Err(format!("{allocation_name} size must be greater than 0"));
        }

        let layout = Layout::from_size_align(byte_size, byte_align).map_err(|_| {
            format!("invalid {allocation_name} layout size {byte_size} align {byte_align}")
        })?;
        let storage = {
            let raw = unsafe { alloc_zeroed(layout) };
            NonNull::new(raw).ok_or_else(|| format!("{allocation_name} allocation failed"))?
        };

        Ok(Self { storage, layout })
    }

    fn len(&self) -> usize {
        self.layout.size()
    }

    fn align(&self) -> usize {
        self.layout.align()
    }

    fn as_ptr(&self) -> *const u8 {
        self.storage.as_ptr()
    }

    fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.storage.as_ptr(), self.len()) }
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.storage.as_ptr(), self.len()) }
    }

    fn copy_prefix_from(
        &mut self,
        source: &AlignedByteStorage,
        byte_count: usize,
    ) -> Result<(), String> {
        if byte_count > self.len() || byte_count > source.len() {
            return Err(format!(
                "storage prefix copy of {byte_count} bytes exceeds source size {} or destination size {}",
                source.len(),
                self.len()
            ));
        }

        self.as_mut_slice()[..byte_count].copy_from_slice(&source.as_slice()[..byte_count]);
        Ok(())
    }
}

impl Drop for AlignedByteStorage {
    fn drop(&mut self) {
        unsafe {
            dealloc(self.storage.as_ptr(), self.layout);
        }
    }
}

#[derive(Debug)]
pub struct ResourceStorage {
    resource_id: ResourceId,
    byte_size: usize,
    byte_align: usize,
    initialized: bool,
    storage: AlignedByteStorage,
}

impl ResourceStorage {
    fn allocate(descriptor: &ResourceDescriptor) -> Result<Self, ResourceStorageError> {
        let byte_size = descriptor.size as usize;
        let byte_align = descriptor.align as usize;

        let storage =
            AlignedByteStorage::allocate_zeroed(byte_size, byte_align, "resource storage")
                .map_err(|message| ResourceStorageError { message })?;

        Ok(Self {
            resource_id: descriptor.id,
            byte_size,
            byte_align,
            initialized: false,
            storage,
        })
    }

    pub fn resource_id(&self) -> ResourceId {
        self.resource_id
    }

    pub fn byte_size(&self) -> usize {
        self.byte_size
    }

    pub fn byte_align(&self) -> usize {
        self.byte_align
    }

    pub fn storage_byte_size(&self) -> usize {
        self.storage.len()
    }

    fn storage_ptr(&self) -> *const u8 {
        self.storage.as_ptr()
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    fn payload_bytes(&self) -> Result<&[u8], ResourceStorageError> {
        if !self.initialized {
            return Err(ResourceStorageError {
                message: format!(
                    "resource storage 0x{:016x} has not been initialized",
                    self.resource_id.0
                ),
            });
        }

        Ok(&self.storage.as_slice()[..self.byte_size])
    }

    pub fn store_payload(&mut self, payload_bytes: &[u8]) -> Result<(), ResourceStorageError> {
        if payload_bytes.len() != self.byte_size {
            return Err(ResourceStorageError {
                message: format!(
                    "resource payload size {} does not match storage size {}",
                    payload_bytes.len(),
                    self.byte_size
                ),
            });
        }

        self.storage.as_mut_slice()[..self.byte_size].copy_from_slice(payload_bytes);
        self.initialized = true;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceStorageError {
    pub message: String,
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

    fn insert_entity(&mut self, entity: ArcheEntity) -> usize {
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

    fn allocate_component_column(
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

    fn copy_component_payload(
        &mut self,
        component_id: ComponentId,
        row: usize,
        payload_bytes: &[u8],
    ) -> Result<(), ComponentColumnError> {
        if row >= self.entities.len() {
            return Err(ComponentColumnError {
                message: format!("entity row {row} does not exist"),
            });
        }

        let column = self
            .columns
            .iter_mut()
            .find(|column| column.component_id == component_id)
            .ok_or_else(|| ComponentColumnError {
                message: format!("component column 0x{:016x} does not exist", component_id.0),
            })?;

        column.copy_payload(row, payload_bytes)
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
    storage: AlignedByteStorage,
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
        let storage =
            AlignedByteStorage::allocate_zeroed(byte_size, element_align, "component column")
                .map_err(|message| ComponentColumnError { message })?;

        Ok(Self {
            component_id: descriptor.id,
            element_size,
            element_align,
            row_capacity,
            row_count: 0,
            storage,
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
        self.storage.len()
    }

    fn storage_ptr(&self) -> *const u8 {
        self.storage.as_ptr()
    }

    fn prepare_growth(
        &self,
        required_rows: usize,
    ) -> Result<Option<PreparedColumnGrowth>, ComponentColumnError> {
        if required_rows <= self.row_capacity {
            return Ok(None);
        }

        let doubled_capacity = self.row_capacity.checked_mul(2).unwrap_or(required_rows);
        let row_capacity = doubled_capacity.max(required_rows);
        let byte_size =
            self.element_size
                .checked_mul(row_capacity)
                .ok_or_else(|| ComponentColumnError {
                    message: "component column byte size overflowed while growing".to_string(),
                })?;
        let initialized_byte_size =
            self.element_size
                .checked_mul(self.row_count)
                .ok_or_else(|| ComponentColumnError {
                    message: "component column initialized byte size overflowed".to_string(),
                })?;
        let mut storage =
            AlignedByteStorage::allocate_zeroed(byte_size, self.element_align, "component column")
                .map_err(|message| ComponentColumnError { message })?;
        storage
            .copy_prefix_from(&self.storage, initialized_byte_size)
            .map_err(|message| ComponentColumnError { message })?;

        Ok(Some(PreparedColumnGrowth {
            storage,
            row_capacity,
        }))
    }

    fn install_growth(&mut self, growth: PreparedColumnGrowth) {
        self.storage = growth.storage;
        self.row_capacity = growth.row_capacity;
    }

    fn copy_payload(
        &mut self,
        row: usize,
        payload_bytes: &[u8],
    ) -> Result<(), ComponentColumnError> {
        if row > self.row_count {
            return Err(ComponentColumnError {
                message: format!(
                    "component row {row} would leave an uninitialized gap after row {}",
                    self.row_count
                ),
            });
        }

        if row >= self.row_capacity {
            return Err(ComponentColumnError {
                message: format!(
                    "component row {row} exceeds column capacity {}",
                    self.row_capacity
                ),
            });
        }

        if payload_bytes.len() != self.element_size {
            return Err(ComponentColumnError {
                message: format!(
                    "component payload has {} bytes but expected {}",
                    payload_bytes.len(),
                    self.element_size
                ),
            });
        }

        self.commit_payload(row, payload_bytes);

        Ok(())
    }

    fn commit_payload(&mut self, row: usize, payload_bytes: &[u8]) {
        debug_assert!(row <= self.row_count);
        debug_assert!(row < self.row_capacity);
        debug_assert_eq!(payload_bytes.len(), self.element_size);

        let offset = row * self.element_size;
        let end = offset + self.element_size;
        self.storage.as_mut_slice()[offset..end].copy_from_slice(payload_bytes);
        if row == self.row_count {
            self.row_count += 1;
        }
    }

    pub fn row_bytes(&self, row: usize) -> Option<&[u8]> {
        if row >= self.row_count {
            return None;
        }

        let offset = row * self.element_size;
        Some(&self.storage.as_slice()[offset..offset + self.element_size])
    }
}

#[derive(Debug)]
struct PreparedColumnGrowth {
    storage: AlignedByteStorage,
    row_capacity: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentColumnError {
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnEntityError {
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComponentPayload<'a> {
    pub component_id: ComponentId,
    pub payload_bytes: &'a [u8],
}

struct ResolvedComponentPayload<'a> {
    descriptor: ComponentDescriptor,
    payload_bytes: &'a [u8],
}

struct IndexedColumnGrowth {
    column_index: usize,
    growth: PreparedColumnGrowth,
}

struct PreparedTableSpawn {
    growths: Vec<IndexedColumnGrowth>,
    new_columns: Vec<ComponentColumn>,
}

impl ArchetypeTable {
    fn prepare_spawn(
        &mut self,
        components: &[ResolvedComponentPayload<'_>],
    ) -> Result<PreparedTableSpawn, SpawnEntityError> {
        let entity_count = self.entities.len();
        let required_rows = entity_count
            .checked_add(1)
            .ok_or_else(|| SpawnEntityError {
                message: "archetype row count overflowed".to_string(),
            })?;

        for (index, column) in self.columns.iter().enumerate() {
            if self.columns[..index]
                .iter()
                .any(|previous| previous.component_id == column.component_id)
            {
                return Err(SpawnEntityError {
                    message: format!(
                        "archetype has duplicate component column 0x{:016x}",
                        column.component_id.0
                    ),
                });
            }

            let component = components
                .iter()
                .find(|component| component.descriptor.id == column.component_id)
                .ok_or_else(|| SpawnEntityError {
                    message: format!(
                        "archetype has unexpected component column 0x{:016x}",
                        column.component_id.0
                    ),
                })?;
            if column.element_size != component.descriptor.size as usize
                || column.element_align != component.descriptor.align as usize
            {
                return Err(SpawnEntityError {
                    message: format!(
                        "component column `{}` does not match its registered descriptor",
                        component.descriptor.name
                    ),
                });
            }
            if column.row_count != entity_count {
                return Err(SpawnEntityError {
                    message: format!(
                        "component column `{}` has {} committed rows but archetype has {entity_count} entities",
                        component.descriptor.name, column.row_count
                    ),
                });
            }
        }

        let mut growths = Vec::new();
        growths
            .try_reserve(self.columns.len())
            .map_err(|error| SpawnEntityError {
                message: format!("failed to reserve component growth plan: {error}"),
            })?;
        for (column_index, column) in self.columns.iter().enumerate() {
            if let Some(growth) =
                column
                    .prepare_growth(required_rows)
                    .map_err(|error| SpawnEntityError {
                        message: error.message,
                    })?
            {
                growths.push(IndexedColumnGrowth {
                    column_index,
                    growth,
                });
            }
        }

        let missing_count = components
            .iter()
            .filter(|component| self.column(component.descriptor.id).is_none())
            .count();
        if entity_count > 0 && missing_count > 0 {
            return Err(SpawnEntityError {
                message: "established archetype is missing one or more component columns"
                    .to_string(),
            });
        }

        let mut new_columns = Vec::new();
        new_columns
            .try_reserve(missing_count)
            .map_err(|error| SpawnEntityError {
                message: format!("failed to reserve new component columns: {error}"),
            })?;
        for component in components {
            if self.column(component.descriptor.id).is_none() {
                new_columns.push(
                    ComponentColumn::allocate(&component.descriptor, required_rows).map_err(
                        |error| SpawnEntityError {
                            message: error.message,
                        },
                    )?,
                );
            }
        }

        self.entities
            .try_reserve(1)
            .map_err(|error| SpawnEntityError {
                message: format!("failed to reserve archetype entity row: {error}"),
            })?;
        self.columns
            .try_reserve(new_columns.len())
            .map_err(|error| SpawnEntityError {
                message: format!("failed to reserve archetype component columns: {error}"),
            })?;

        Ok(PreparedTableSpawn {
            growths,
            new_columns,
        })
    }

    fn install_spawn_preparation(&mut self, preparation: PreparedTableSpawn) {
        for indexed in preparation.growths {
            self.columns[indexed.column_index].install_growth(indexed.growth);
        }
        self.columns.extend(preparation.new_columns);
    }

    fn commit_spawn(&mut self, entity: ArcheEntity, components: &[ResolvedComponentPayload<'_>]) {
        let row = self.entities.len();
        for component in components {
            let column = self
                .columns
                .iter_mut()
                .find(|column| column.component_id == component.descriptor.id)
                .expect("prepared spawn component column must exist");
            column.commit_payload(row, component.payload_bytes);
        }
        self.entities.push(entity);
    }
}

#[derive(Debug)]
pub struct ArcheWorld {
    entities: EntityTable,
    component_descriptors: ComponentDescriptorTable,
    resource_descriptors: ResourceDescriptorTable,
    system_descriptors: SystemDescriptorTable,
    schedule_descriptors: ScheduleDescriptorTable,
    query_descriptors: QueryDescriptorTable,
    resource_storages: Vec<ResourceStorage>,
    archetypes: Vec<ArchetypeTable>,
}

impl ArcheWorld {
    pub fn create() -> Self {
        Self {
            entities: EntityTable::new(),
            component_descriptors: ComponentDescriptorTable::new(),
            resource_descriptors: ResourceDescriptorTable::new(),
            system_descriptors: SystemDescriptorTable::new(),
            schedule_descriptors: ScheduleDescriptorTable::new(),
            query_descriptors: QueryDescriptorTable::new(),
            resource_storages: Vec::new(),
            archetypes: Vec::new(),
        }
    }

    pub fn destroy(self) {}

    pub fn entities(&self) -> &EntityTable {
        &self.entities
    }

    fn alloc_entity(&mut self) -> ArcheEntity {
        self.entities.alloc()
    }

    pub fn spawn_entity_with_payloads(
        &mut self,
        payloads: &[ComponentPayload<'_>],
    ) -> Result<ArcheEntity, SpawnEntityError> {
        let mut components = Vec::new();
        components
            .try_reserve(payloads.len())
            .map_err(|error| SpawnEntityError {
                message: format!("failed to reserve component payload plan: {error}"),
            })?;

        for payload in payloads {
            if components
                .iter()
                .any(|component: &ResolvedComponentPayload<'_>| {
                    component.descriptor.id == payload.component_id
                })
            {
                return Err(SpawnEntityError {
                    message: format!(
                        "duplicate spawn component 0x{:016x}",
                        payload.component_id.0
                    ),
                });
            }

            let descriptor = self
                .component_descriptors
                .get(payload.component_id)
                .cloned()
                .ok_or_else(|| SpawnEntityError {
                    message: format!(
                        "component descriptor 0x{:016x} is not registered",
                        payload.component_id.0
                    ),
                })?;
            let element_size = descriptor.size as usize;
            let element_align = descriptor.align as usize;
            if element_size == 0 {
                return Err(SpawnEntityError {
                    message: format!("component descriptor `{}` has zero size", descriptor.name),
                });
            }
            Layout::from_size_align(element_size, element_align).map_err(|_| SpawnEntityError {
                message: format!(
                    "component descriptor `{}` has invalid layout size {element_size} align {element_align}",
                    descriptor.name
                ),
            })?;
            if payload.payload_bytes.len() != element_size {
                return Err(SpawnEntityError {
                    message: format!(
                        "component payload size {} does not match descriptor size {} for `{}`",
                        payload.payload_bytes.len(),
                        descriptor.size,
                        descriptor.name
                    ),
                });
            }

            components.push(ResolvedComponentPayload {
                descriptor,
                payload_bytes: payload.payload_bytes,
            });
        }

        let mut component_ids = Vec::new();
        component_ids
            .try_reserve(components.len())
            .map_err(|error| SpawnEntityError {
                message: format!("failed to reserve archetype key: {error}"),
            })?;
        component_ids.extend(components.iter().map(|component| component.descriptor.id));
        let key = ArchetypeKey::new(component_ids);

        if let Some(archetype_index) = self.archetypes.iter().position(|table| table.key() == &key)
        {
            let preparation = self.archetypes[archetype_index].prepare_spawn(&components)?;
            self.entities.prepare_alloc()?;

            let entity = self.alloc_entity();
            let table = &mut self.archetypes[archetype_index];
            table.install_spawn_preparation(preparation);
            table.commit_spawn(entity, &components);
            return Ok(entity);
        }

        let mut table = ArchetypeTable::new(key);
        let preparation = table.prepare_spawn(&components)?;
        table.install_spawn_preparation(preparation);
        self.archetypes
            .try_reserve(1)
            .map_err(|error| SpawnEntityError {
                message: format!("failed to reserve archetype table: {error}"),
            })?;
        self.entities.prepare_alloc()?;

        let entity = self.alloc_entity();
        table.commit_spawn(entity, &components);
        self.archetypes.push(table);
        Ok(entity)
    }

    pub fn register_component_descriptor(&mut self, descriptor: ComponentDescriptor) -> bool {
        self.component_descriptors.register(descriptor)
    }

    pub fn component_descriptors(&self) -> &ComponentDescriptorTable {
        &self.component_descriptors
    }

    pub fn register_resource_descriptor(&mut self, descriptor: ResourceDescriptor) -> bool {
        self.resource_descriptors.register(descriptor)
    }

    pub fn resource_descriptors(&self) -> &ResourceDescriptorTable {
        &self.resource_descriptors
    }

    pub fn register_system_descriptor(&mut self, descriptor: SystemDescriptor) -> bool {
        self.system_descriptors.register(descriptor)
    }

    pub fn system_descriptors(&self) -> &SystemDescriptorTable {
        &self.system_descriptors
    }

    pub fn register_schedule_descriptor(&mut self, descriptor: ScheduleDescriptor) -> bool {
        self.schedule_descriptors.register(descriptor)
    }

    pub fn schedule_descriptors(&self) -> &ScheduleDescriptorTable {
        &self.schedule_descriptors
    }

    pub fn build_schedule_plan(
        &self,
        schedule: &ScheduleDescriptor,
    ) -> Result<SchedulePlan, SchedulePlanError> {
        let mut entries = Vec::new();

        for item in &schedule.items {
            match item {
                ScheduleItemDescriptor::Run {
                    system_id,
                    system_name,
                } => {
                    if self.system_descriptors.get(*system_id).is_none() {
                        return Err(SchedulePlanError {
                            message: format!("unknown system `{system_name}`"),
                        });
                    }

                    entries.push(SchedulePlanEntry {
                        system_id: *system_id,
                        system_name: system_name.clone(),
                    });
                }
            }
        }

        Ok(SchedulePlan {
            schedule_id: schedule.id,
            schedule_name: schedule.name.clone(),
            entries,
        })
    }

    pub fn execute_schedule_plan(
        &mut self,
        plan: &SchedulePlan,
    ) -> Result<(), ScheduleExecuteError> {
        for entry in plan.entries() {
            if entry.system_id == stable_system_id("Demo", "Move")
                && entry.system_name == "Demo.Move"
            {
                self.execute_demo_move_system()?;
                continue;
            }

            return Err(schedule_execute_error(format!(
                "unsupported system `{}`",
                entry.system_name
            )));
        }

        Ok(())
    }

    fn execute_demo_move_system(&mut self) -> Result<(), ScheduleExecuteError> {
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let time_id = stable_resource_id("Demo", "Time");
        let query_id = stable_query_id("Demo", "Move", "movers");
        let query = self
            .query_descriptors
            .get(query_id)
            .cloned()
            .ok_or_else(|| {
                schedule_execute_error("query descriptor `Demo.Move.movers` is not registered")
            })?;
        let query_plan = self.build_query_plan(&query);
        let rows = self.iter_query_rows(&query_plan);

        for row in rows {
            let (position_x, position_y, velocity_x, velocity_y) = {
                let table = self.archetype_at(row.archetype_index).ok_or_else(|| {
                    schedule_execute_error(format!(
                        "query row references missing archetype {}",
                        row.archetype_index
                    ))
                })?;
                let position_bytes = table
                    .column(position_id)
                    .and_then(|column| column.row_bytes(row.row))
                    .ok_or_else(|| {
                        schedule_execute_error(format!(
                            "Demo.Position payload missing for row {}",
                            row.row
                        ))
                    })?;
                let velocity_bytes = table
                    .column(velocity_id)
                    .and_then(|column| column.row_bytes(row.row))
                    .ok_or_else(|| {
                        schedule_execute_error(format!(
                            "Demo.Velocity payload missing for row {}",
                            row.row
                        ))
                    })?;

                (
                    read_schedule_f32(position_bytes, 0, "Demo.Position.x")?,
                    read_schedule_f32(position_bytes, 4, "Demo.Position.y")?,
                    read_schedule_f32(velocity_bytes, 0, "Demo.Velocity.x")?,
                    read_schedule_f32(velocity_bytes, 4, "Demo.Velocity.y")?,
                )
            };
            let delta = self
                .read_resource_f32_field(time_id, "delta")
                .map_err(|error| schedule_execute_error(error.message))?;
            let updated_position = f32_pair_payload(
                position_x + velocity_x * delta,
                position_y + velocity_y * delta,
            );

            self.archetype_at_mut(row.archetype_index)
                .ok_or_else(|| {
                    schedule_execute_error(format!(
                        "query row references missing mutable archetype {}",
                        row.archetype_index
                    ))
                })?
                .copy_component_payload(position_id, row.row, &updated_position)
                .map_err(|error| schedule_execute_error(error.message))?;
        }

        Ok(())
    }

    pub fn register_query_descriptor(&mut self, descriptor: QueryDescriptor) -> bool {
        self.query_descriptors.register(descriptor)
    }

    pub fn query_descriptors(&self) -> &QueryDescriptorTable {
        &self.query_descriptors
    }

    pub fn build_query_plan(&self, query: &QueryDescriptor) -> QueryPlan {
        let entries = self
            .archetypes
            .iter()
            .enumerate()
            .filter_map(|(archetype_index, table)| {
                query
                    .matches_archetype_key(table.key())
                    .then(|| QueryPlanEntry {
                        archetype_index,
                        key: table.key().clone(),
                    })
            })
            .collect();

        QueryPlan {
            query_id: query.id,
            query_name: query.name.clone(),
            entries,
        }
    }

    pub fn iter_query_rows(&self, plan: &QueryPlan) -> Vec<QueryRow> {
        let mut rows = Vec::new();

        for entry in plan.entries() {
            if let Some(table) = self.archetypes.get(entry.archetype_index) {
                for row in 0..table.entity_count() {
                    if let Some(entity) = table.entity(row) {
                        rows.push(QueryRow {
                            archetype_index: entry.archetype_index,
                            row,
                            entity,
                        });
                    }
                }
            }
        }

        rows
    }

    pub fn allocate_resource_storage(
        &mut self,
        descriptor: &ResourceDescriptor,
    ) -> Result<bool, ResourceStorageError> {
        if self.resource_storage(descriptor.id).is_some() {
            return Ok(false);
        }

        let storage = ResourceStorage::allocate(descriptor)?;
        self.resource_storages.push(storage);
        Ok(true)
    }

    pub fn resource_storage(&self, id: ResourceId) -> Option<&ResourceStorage> {
        self.resource_storages
            .iter()
            .find(|storage| storage.resource_id == id)
    }

    pub fn resource_payload(&self, id: ResourceId) -> Result<&[u8], ResourceStorageError> {
        let storage = self
            .resource_storage(id)
            .ok_or_else(|| ResourceStorageError {
                message: format!("resource storage 0x{:016x} is not allocated", id.0),
            })?;

        storage.payload_bytes()
    }

    pub fn read_resource_f32_field(
        &self,
        id: ResourceId,
        field_name: &str,
    ) -> Result<f32, ResourceStorageError> {
        let descriptor = self
            .resource_descriptors
            .get(id)
            .ok_or_else(|| ResourceStorageError {
                message: format!("resource descriptor 0x{:016x} is not registered", id.0),
            })?;
        let field = descriptor
            .fields
            .iter()
            .find(|field| field.name == field_name)
            .ok_or_else(|| ResourceStorageError {
                message: format!("resource field `{field_name}` is not registered"),
            })?;

        if field.type_name != "f32" {
            return Err(ResourceStorageError {
                message: format!(
                    "resource field `{field_name}` has unsupported type `{}`",
                    field.type_name
                ),
            });
        }

        let payload = self.resource_payload(id)?;
        let offset = field.offset as usize;
        let bytes = payload
            .get(offset..offset + 4)
            .ok_or_else(|| ResourceStorageError {
                message: format!("resource field `{field_name}` extends beyond payload"),
            })?;

        Ok(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn store_resource_payload(
        &mut self,
        id: ResourceId,
        payload_bytes: &[u8],
    ) -> Result<(), ResourceStorageError> {
        let storage = self
            .resource_storages
            .iter_mut()
            .find(|storage| storage.resource_id == id)
            .ok_or_else(|| ResourceStorageError {
                message: format!("resource storage 0x{:016x} is not allocated", id.0),
            })?;

        storage.store_payload(payload_bytes)
    }

    pub fn resource_storage_count(&self) -> usize {
        self.resource_storages.len()
    }

    pub fn archetype_count(&self) -> usize {
        self.archetypes.len()
    }

    pub fn archetype(&self, key: &ArchetypeKey) -> Option<&ArchetypeTable> {
        self.archetypes.iter().find(|table| table.key() == key)
    }

    pub fn archetype_at(&self, index: usize) -> Option<&ArchetypeTable> {
        self.archetypes.get(index)
    }

    pub fn archetype_at_mut(&mut self, index: usize) -> Option<&mut ArchetypeTable> {
        self.archetypes.get_mut(index)
    }

    fn get_or_create_archetype(&mut self, key: ArchetypeKey) -> &mut ArchetypeTable {
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
            && self.resource_descriptors.len() == 0
            && self.system_descriptors.len() == 0
            && self.schedule_descriptors.len() == 0
            && self.query_descriptors.len() == 0
            && self.resource_storages.is_empty()
            && self.archetypes.is_empty()
    }
}

fn schedule_execute_error(message: impl Into<String>) -> ScheduleExecuteError {
    ScheduleExecuteError {
        message: message.into(),
    }
}

fn f32_pair_payload(x: f32, y: f32) -> [u8; 8] {
    let x = x.to_le_bytes();
    let y = y.to_le_bytes();
    [x[0], x[1], x[2], x[3], y[0], y[1], y[2], y[3]]
}

fn read_schedule_f32(
    bytes: &[u8],
    offset: usize,
    field_name: &str,
) -> Result<f32, ScheduleExecuteError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| schedule_execute_error(format!("{field_name} offset overflows")))?;
    let bytes = bytes.get(offset..end).ok_or_else(|| {
        schedule_execute_error(format!("{field_name} extends beyond component payload"))
    })?;

    Ok(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

pub fn debug_inspect_world(world: &ArcheWorld) -> String {
    let mut lines = Vec::new();

    lines.push("world".to_string());
    lines.push(format!("  entities {}", world.entities.len()));
    lines.push(format!("  archetypes {}", world.archetypes.len()));

    for table in &world.archetypes {
        lines.push(format!(
            "  archetype {}",
            debug_archetype_name(world, table.key())
        ));

        for row in 0..table.entity_count() {
            if let Some(entity) = table.entity(row) {
                lines.push(format!(
                    "    row {row} entity index {} generation {}",
                    entity.index(),
                    entity.generation()
                ));

                for component_id in table.key().component_ids() {
                    debug_inspect_component(world, table, *component_id, row, &mut lines);
                }
            }
        }
    }

    if world.resource_descriptors.len() > 0 {
        lines.push(format!("  resources {}", world.resource_descriptors.len()));

        for descriptor in world.resource_descriptors.descriptors() {
            debug_inspect_resource(world, descriptor, &mut lines);
        }
    }

    lines.join("\n")
}

fn debug_archetype_name(world: &ArcheWorld, key: &ArchetypeKey) -> String {
    key.component_ids()
        .iter()
        .map(|component_id| {
            world
                .component_descriptors
                .get(*component_id)
                .map(|descriptor| descriptor.name.clone())
                .unwrap_or_else(|| format!("component 0x{:016x}", component_id.0))
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

fn debug_inspect_component(
    world: &ArcheWorld,
    table: &ArchetypeTable,
    component_id: ComponentId,
    row: usize,
    lines: &mut Vec<String>,
) {
    let Some(descriptor) = world.component_descriptors.get(component_id) else {
        lines.push(format!("      component 0x{:016x}", component_id.0));
        lines.push("        descriptor missing".to_string());
        return;
    };

    lines.push(format!("      component {}", descriptor.name));

    let Some(column) = table.column(component_id) else {
        lines.push("        column missing".to_string());
        return;
    };

    let Some(row_bytes) = column.row_bytes(row) else {
        lines.push("        payload missing".to_string());
        return;
    };

    for field in &descriptor.fields {
        lines.push(format!(
            "        {}: {} = {}",
            field.name,
            field.type_name,
            debug_format_field_value(field, row_bytes)
        ));
    }
}

fn debug_format_field_value(field: &ComponentFieldDescriptor, row_bytes: &[u8]) -> String {
    let offset = field.offset as usize;

    if field.type_name == "f32" && offset + 4 <= row_bytes.len() {
        let value = f32::from_le_bytes([
            row_bytes[offset],
            row_bytes[offset + 1],
            row_bytes[offset + 2],
            row_bytes[offset + 3],
        ]);

        if value.fract() == 0.0 {
            return format!("{value:.1}");
        }

        return value.to_string();
    }

    "unsupported".to_string()
}

fn debug_inspect_resource(
    world: &ArcheWorld,
    descriptor: &ResourceDescriptor,
    lines: &mut Vec<String>,
) {
    lines.push(format!("  resource {}", descriptor.name));

    for field in &descriptor.fields {
        lines.push(format!(
            "    {}: {} = {}",
            field.name,
            field.type_name,
            debug_format_resource_field_value(world, descriptor.id, field)
        ));
    }
}

fn debug_format_resource_field_value(
    world: &ArcheWorld,
    resource_id: ResourceId,
    field: &ResourceFieldDescriptor,
) -> String {
    if field.type_name == "f32" {
        if let Ok(value) = world.read_resource_f32_field(resource_id, &field.name) {
            if value.fract() == 0.0 {
                return format!("{value:.1}");
            }

            return value.to_string();
        }
    }

    "unsupported".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_resource_descriptor() -> ResourceDescriptor {
        ResourceDescriptor {
            id: stable_resource_id("Test", "Time"),
            name: "Test.Time".to_string(),
            size: 4,
            align: 4,
            fields: vec![ResourceFieldDescriptor {
                name: "delta".to_string(),
                type_name: "f32".to_string(),
                offset: 0,
            }],
        }
    }

    fn test_xy_component_descriptor(id: ComponentId, name: &str) -> ComponentDescriptor {
        ComponentDescriptor {
            id,
            name: name.to_string(),
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
        }
    }

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
    fn defines_time_delta_resource_descriptor() {
        let time_id = stable_resource_id("Demo", "Time");
        let time = ResourceDescriptor {
            id: time_id,
            name: "Demo.Time".to_string(),
            size: 4,
            align: 4,
            fields: vec![ResourceFieldDescriptor {
                name: "delta".to_string(),
                type_name: "f32".to_string(),
                offset: 0,
            }],
        };
        let mut world = ArcheWorld::create();

        assert_eq!(time_id, ResourceId(0x7924ce11db524521));
        assert!(world.register_resource_descriptor(time.clone()));
        assert_eq!(world.resource_descriptors().len(), 1);
        assert_eq!(world.resource_descriptors().get(time_id), Some(&time));

        let descriptor = world
            .resource_descriptors()
            .get(time_id)
            .expect("Demo.Time resource descriptor should be registered");
        assert_eq!(descriptor.id, ResourceId(0x7924ce11db524521));
        assert_eq!(descriptor.name, "Demo.Time");
        assert_eq!(descriptor.size, 4);
        assert_eq!(descriptor.align, 4);
        assert_eq!(
            descriptor.fields,
            vec![ResourceFieldDescriptor {
                name: "delta".to_string(),
                type_name: "f32".to_string(),
                offset: 0,
            }]
        );

        let duplicate = ResourceDescriptor {
            id: time_id,
            name: "Demo.Time.Duplicate".to_string(),
            size: 8,
            align: 4,
            fields: Vec::new(),
        };

        assert!(!world.register_resource_descriptor(duplicate));
        assert_eq!(world.resource_descriptors().get(time_id), Some(&time));
    }

    #[test]
    fn registers_move_system_descriptor() {
        let move_id = stable_system_id("Demo", "Move");
        let move_system = SystemDescriptor {
            id: move_id,
            name: "Demo.Move".to_string(),
            params: vec![
                SystemParamDescriptor {
                    name: "time".to_string(),
                    kind: SystemParamDescriptorKind::ReadResource {
                        resource_id: ResourceId(0x7924ce11db524521),
                        name: "Demo.Time".to_string(),
                    },
                },
                SystemParamDescriptor {
                    name: "movers".to_string(),
                    kind: SystemParamDescriptorKind::Query {
                        terms: vec![
                            SystemQueryTermDescriptor {
                                access: SystemAccess::Mut,
                                component_id: ComponentId(0x002202c6aeb4f27b),
                                name: "Demo.Position".to_string(),
                            },
                            SystemQueryTermDescriptor {
                                access: SystemAccess::Read,
                                component_id: ComponentId(0x2cf8a68bcb7f913b),
                                name: "Demo.Velocity".to_string(),
                            },
                        ],
                    },
                },
            ],
        };
        let mut world = ArcheWorld::create();

        assert_eq!(move_id, SystemId(0x723b6b52df270ed5));
        assert!(world.register_system_descriptor(move_system.clone()));
        assert_eq!(world.system_descriptors().len(), 1);
        assert_eq!(world.system_descriptors().get(move_id), Some(&move_system));

        let descriptor = world
            .system_descriptors()
            .get(move_id)
            .expect("Demo.Move system descriptor should be registered");
        assert_eq!(descriptor.id, SystemId(0x723b6b52df270ed5));
        assert_eq!(descriptor.name, "Demo.Move");
        assert_eq!(descriptor.params.len(), 2);
        assert_eq!(descriptor.params[0].name, "time");
        assert_eq!(
            descriptor.params[0].kind,
            SystemParamDescriptorKind::ReadResource {
                resource_id: ResourceId(0x7924ce11db524521),
                name: "Demo.Time".to_string(),
            }
        );
        assert_eq!(descriptor.params[1].name, "movers");
        assert_eq!(
            descriptor.params[1].kind,
            SystemParamDescriptorKind::Query {
                terms: vec![
                    SystemQueryTermDescriptor {
                        access: SystemAccess::Mut,
                        component_id: ComponentId(0x002202c6aeb4f27b),
                        name: "Demo.Position".to_string(),
                    },
                    SystemQueryTermDescriptor {
                        access: SystemAccess::Read,
                        component_id: ComponentId(0x2cf8a68bcb7f913b),
                        name: "Demo.Velocity".to_string(),
                    },
                ],
            }
        );

        let duplicate = SystemDescriptor {
            id: move_id,
            name: "Demo.Move.Duplicate".to_string(),
            params: Vec::new(),
        };

        assert!(!world.register_system_descriptor(duplicate));
        assert_eq!(world.system_descriptors().get(move_id), Some(&move_system));
    }

    #[test]
    fn registers_main_schedule_descriptor() {
        let main_id = stable_schedule_id("Demo", "Main");
        let main_schedule = ScheduleDescriptor {
            id: main_id,
            name: "Demo.Main".to_string(),
            items: vec![ScheduleItemDescriptor::Run {
                system_id: SystemId(0x723b6b52df270ed5),
                system_name: "Demo.Move".to_string(),
            }],
        };
        let mut world = ArcheWorld::create();

        assert_eq!(main_id, ScheduleId(0xed3d905325519b05));
        assert!(world.register_schedule_descriptor(main_schedule.clone()));
        assert_eq!(world.schedule_descriptors().len(), 1);
        assert_eq!(
            world.schedule_descriptors().get(main_id),
            Some(&main_schedule)
        );

        let descriptor = world
            .schedule_descriptors()
            .get(main_id)
            .expect("Demo.Main schedule descriptor should be registered");
        assert_eq!(descriptor.id, ScheduleId(0xed3d905325519b05));
        assert_eq!(descriptor.name, "Demo.Main");
        assert_eq!(descriptor.items.len(), 1);
        assert_eq!(
            descriptor.items[0],
            ScheduleItemDescriptor::Run {
                system_id: SystemId(0x723b6b52df270ed5),
                system_name: "Demo.Move".to_string(),
            }
        );

        let duplicate = ScheduleDescriptor {
            id: main_id,
            name: "Demo.Main.Duplicate".to_string(),
            items: Vec::new(),
        };

        assert!(!world.register_schedule_descriptor(duplicate));
        assert_eq!(
            world.schedule_descriptors().get(main_id),
            Some(&main_schedule)
        );
    }

    #[test]
    fn builds_sequential_schedule_plan() {
        let move_id = SystemId(0x723b6b52df270ed5);
        let main_id = ScheduleId(0xed3d905325519b05);
        let move_system = SystemDescriptor {
            id: move_id,
            name: "Demo.Move".to_string(),
            params: Vec::new(),
        };
        let main_schedule = ScheduleDescriptor {
            id: main_id,
            name: "Demo.Main".to_string(),
            items: vec![ScheduleItemDescriptor::Run {
                system_id: move_id,
                system_name: "Demo.Move".to_string(),
            }],
        };
        let mut world = ArcheWorld::create();

        assert!(world.register_system_descriptor(move_system));

        let plan = world
            .build_schedule_plan(&main_schedule)
            .expect("Demo.Main schedule should build a plan");

        assert_eq!(plan.schedule_id, main_id);
        assert_eq!(plan.schedule_name, "Demo.Main");
        assert_eq!(plan.len(), 1);
        assert!(!plan.is_empty());
        assert_eq!(
            plan.entries(),
            &[SchedulePlanEntry {
                system_id: move_id,
                system_name: "Demo.Move".to_string(),
            }]
        );

        let missing_schedule = ScheduleDescriptor {
            id: main_id,
            name: "Demo.Main".to_string(),
            items: vec![ScheduleItemDescriptor::Run {
                system_id: SystemId(0xffff000000000003),
                system_name: "Demo.Missing".to_string(),
            }],
        };
        let error = world
            .build_schedule_plan(&missing_schedule)
            .expect_err("missing systems should fail schedule planning");

        assert!(
            error.message.contains("unknown system"),
            "unexpected schedule plan error: {}",
            error.message
        );
    }

    #[test]
    fn executes_runtime_schedule_plan() {
        fn xy_descriptor(id: ComponentId, name: &str) -> ComponentDescriptor {
            ComponentDescriptor {
                id,
                name: name.to_string(),
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
            }
        }

        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let time_id = stable_resource_id("Demo", "Time");
        let move_id = stable_system_id("Demo", "Move");
        let main_id = stable_schedule_id("Demo", "Main");
        let position = xy_descriptor(position_id, "Demo.Position");
        let velocity = xy_descriptor(velocity_id, "Demo.Velocity");
        let time = ResourceDescriptor {
            id: time_id,
            name: "Demo.Time".to_string(),
            size: 4,
            align: 4,
            fields: vec![ResourceFieldDescriptor {
                name: "delta".to_string(),
                type_name: "f32".to_string(),
                offset: 0,
            }],
        };
        let move_system = SystemDescriptor {
            id: move_id,
            name: "Demo.Move".to_string(),
            params: vec![
                SystemParamDescriptor {
                    name: "time".to_string(),
                    kind: SystemParamDescriptorKind::ReadResource {
                        resource_id: time_id,
                        name: "Demo.Time".to_string(),
                    },
                },
                SystemParamDescriptor {
                    name: "movers".to_string(),
                    kind: SystemParamDescriptorKind::Query {
                        terms: vec![
                            SystemQueryTermDescriptor {
                                access: SystemAccess::Mut,
                                component_id: position_id,
                                name: "Demo.Position".to_string(),
                            },
                            SystemQueryTermDescriptor {
                                access: SystemAccess::Read,
                                component_id: velocity_id,
                                name: "Demo.Velocity".to_string(),
                            },
                        ],
                    },
                },
            ],
        };
        let movers_query = QueryDescriptor {
            id: stable_query_id("Demo", "Move", "movers"),
            name: "Demo.Move.movers".to_string(),
            terms: vec![
                QueryTermDescriptor {
                    access: QueryAccess::Mut,
                    component_id: position_id,
                    name: "Demo.Position".to_string(),
                },
                QueryTermDescriptor {
                    access: QueryAccess::Read,
                    component_id: velocity_id,
                    name: "Demo.Velocity".to_string(),
                },
            ],
        };
        let main_schedule = ScheduleDescriptor {
            id: main_id,
            name: "Demo.Main".to_string(),
            items: vec![ScheduleItemDescriptor::Run {
                system_id: move_id,
                system_name: "Demo.Move".to_string(),
            }],
        };
        let initial_position = f32_pair_payload(1.0, 2.0);
        let initial_velocity = f32_pair_payload(3.0, 4.0);
        let expected_position = f32_pair_payload(4.0, 6.0);
        let mut world = ArcheWorld::create();
        let entity = world.alloc_entity();

        assert!(world.register_component_descriptor(position.clone()));
        assert!(world.register_component_descriptor(velocity.clone()));
        assert!(world.register_resource_descriptor(time.clone()));
        assert!(world.register_system_descriptor(move_system));
        assert!(world.register_query_descriptor(movers_query));
        assert!(world.register_schedule_descriptor(main_schedule.clone()));
        assert!(world
            .allocate_resource_storage(&time)
            .expect("Demo.Time resource storage allocation should succeed"));
        world
            .store_resource_payload(time_id, &1.0f32.to_le_bytes())
            .expect("Demo.Time payload store should succeed");

        {
            let table =
                world.get_or_create_archetype(ArchetypeKey::new(vec![position_id, velocity_id]));
            assert!(table
                .allocate_component_column(&position, 1)
                .expect("Demo.Position column allocation should succeed"));
            assert!(table
                .allocate_component_column(&velocity, 1)
                .expect("Demo.Velocity column allocation should succeed"));
            assert_eq!(table.insert_entity(entity), 0);
            table
                .copy_component_payload(position_id, 0, &initial_position)
                .expect("initial Demo.Position payload copy should succeed");
            table
                .copy_component_payload(velocity_id, 0, &initial_velocity)
                .expect("initial Demo.Velocity payload copy should succeed");
        }

        let plan = world
            .build_schedule_plan(&main_schedule)
            .expect("Demo.Main schedule should build a plan");

        world
            .execute_schedule_plan(&plan)
            .expect("Demo.Main schedule plan should execute");

        let table = world
            .archetype_at(0)
            .expect("Demo.Position + Demo.Velocity archetype should exist");
        let position_column = table
            .column(position_id)
            .expect("Demo.Position column should exist");
        let velocity_column = table
            .column(velocity_id)
            .expect("Demo.Velocity column should exist");

        assert_eq!(
            position_column.row_bytes(0),
            Some(expected_position.as_slice())
        );
        assert_eq!(
            velocity_column.row_bytes(0),
            Some(initial_velocity.as_slice())
        );
        assert_eq!(position_column.row_count(), 1);
        assert_eq!(velocity_column.row_count(), 1);
        assert_eq!(table.entity_count(), 1);
        assert_eq!(table.entity(0), Some(entity));
        assert!(world.entities().is_alive(entity));

        let unsupported_plan = SchedulePlan {
            schedule_id: main_id,
            schedule_name: "Demo.Main".to_string(),
            entries: vec![SchedulePlanEntry {
                system_id: SystemId(0xffff000000000004),
                system_name: "Demo.Unsupported".to_string(),
            }],
        };
        let error = world
            .execute_schedule_plan(&unsupported_plan)
            .expect_err("unsupported systems should fail schedule execution");

        assert!(
            error.message.contains("unsupported system"),
            "unexpected schedule execution error: {}",
            error.message
        );
    }

    #[test]
    fn defines_position_velocity_query_descriptor() {
        let query_id = stable_query_id("Demo", "Move", "movers");
        let query = QueryDescriptor {
            id: query_id,
            name: "Demo.Move.movers".to_string(),
            terms: vec![
                QueryTermDescriptor {
                    access: QueryAccess::Mut,
                    component_id: ComponentId(0x002202c6aeb4f27b),
                    name: "Demo.Position".to_string(),
                },
                QueryTermDescriptor {
                    access: QueryAccess::Read,
                    component_id: ComponentId(0x2cf8a68bcb7f913b),
                    name: "Demo.Velocity".to_string(),
                },
            ],
        };
        let mut world = ArcheWorld::create();

        assert_eq!(query_id, QueryId(0xf4004232b85cef9f));
        assert!(world.register_query_descriptor(query.clone()));
        assert_eq!(world.query_descriptors().len(), 1);
        assert_eq!(world.query_descriptors().get(query_id), Some(&query));

        let descriptor = world
            .query_descriptors()
            .get(query_id)
            .expect("Demo.Move.movers query descriptor should be registered");
        assert_eq!(descriptor.id, QueryId(0xf4004232b85cef9f));
        assert_eq!(descriptor.name, "Demo.Move.movers");
        assert_eq!(
            descriptor.terms,
            vec![
                QueryTermDescriptor {
                    access: QueryAccess::Mut,
                    component_id: ComponentId(0x002202c6aeb4f27b),
                    name: "Demo.Position".to_string(),
                },
                QueryTermDescriptor {
                    access: QueryAccess::Read,
                    component_id: ComponentId(0x2cf8a68bcb7f913b),
                    name: "Demo.Velocity".to_string(),
                },
            ]
        );

        let duplicate = QueryDescriptor {
            id: query_id,
            name: "Demo.Move.duplicate".to_string(),
            terms: Vec::new(),
        };

        assert!(!world.register_query_descriptor(duplicate));
        assert_eq!(world.query_descriptors().get(query_id), Some(&query));
    }

    #[test]
    fn matches_position_velocity_query_to_archetype() {
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let extra_id = ComponentId(0xffff000000000001);
        let query = QueryDescriptor {
            id: stable_query_id("Demo", "Move", "movers"),
            name: "Demo.Move.movers".to_string(),
            terms: vec![
                QueryTermDescriptor {
                    access: QueryAccess::Mut,
                    component_id: position_id,
                    name: "Demo.Position".to_string(),
                },
                QueryTermDescriptor {
                    access: QueryAccess::Read,
                    component_id: velocity_id,
                    name: "Demo.Velocity".to_string(),
                },
            ],
        };

        assert!(query.matches_archetype_key(&ArchetypeKey::new(vec![position_id, velocity_id])));
        assert!(query.matches_archetype_key(&ArchetypeKey::new(vec![
            velocity_id,
            position_id,
            position_id
        ])));
        assert!(query.matches_archetype_key(&ArchetypeKey::new(vec![
            extra_id,
            position_id,
            velocity_id
        ])));
        assert!(!query.matches_archetype_key(&ArchetypeKey::new(vec![velocity_id])));
        assert!(!query.matches_archetype_key(&ArchetypeKey::new(vec![position_id])));
        assert!(!query.matches_archetype_key(&ArchetypeKey::new(Vec::new())));
    }

    #[test]
    fn builds_position_velocity_query_plan() {
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let missing_id = ComponentId(0xffff000000000002);
        let query = QueryDescriptor {
            id: stable_query_id("Demo", "Move", "movers"),
            name: "Demo.Move.movers".to_string(),
            terms: vec![
                QueryTermDescriptor {
                    access: QueryAccess::Mut,
                    component_id: position_id,
                    name: "Demo.Position".to_string(),
                },
                QueryTermDescriptor {
                    access: QueryAccess::Read,
                    component_id: velocity_id,
                    name: "Demo.Velocity".to_string(),
                },
            ],
        };
        let mut world = ArcheWorld::create();

        world.get_or_create_archetype(ArchetypeKey::new(vec![position_id]));
        world.get_or_create_archetype(ArchetypeKey::new(vec![position_id, velocity_id]));
        world.get_or_create_archetype(ArchetypeKey::new(vec![velocity_id]));

        let plan = world.build_query_plan(&query);

        assert_eq!(plan.query_id, QueryId(0xf4004232b85cef9f));
        assert_eq!(plan.query_name, "Demo.Move.movers");
        assert_eq!(plan.len(), 1);
        assert!(!plan.is_empty());
        assert_eq!(
            plan.entries(),
            &[QueryPlanEntry {
                archetype_index: 1,
                key: ArchetypeKey::new(vec![position_id, velocity_id]),
            }]
        );

        let missing_query = QueryDescriptor {
            id: stable_query_id("Demo", "Move", "missing"),
            name: "Demo.Move.missing".to_string(),
            terms: vec![QueryTermDescriptor {
                access: QueryAccess::Read,
                component_id: missing_id,
                name: "Demo.Missing".to_string(),
            }],
        };
        let empty_plan = world.build_query_plan(&missing_query);

        assert_eq!(
            empty_plan.query_id,
            stable_query_id("Demo", "Move", "missing")
        );
        assert_eq!(empty_plan.query_name, "Demo.Move.missing");
        assert_eq!(empty_plan.len(), 0);
        assert!(empty_plan.is_empty());
        assert_eq!(empty_plan.entries(), &[]);
    }

    #[test]
    fn iterates_position_velocity_query_rows() {
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let query = QueryDescriptor {
            id: stable_query_id("Demo", "Move", "movers"),
            name: "Demo.Move.movers".to_string(),
            terms: vec![
                QueryTermDescriptor {
                    access: QueryAccess::Mut,
                    component_id: position_id,
                    name: "Demo.Position".to_string(),
                },
                QueryTermDescriptor {
                    access: QueryAccess::Read,
                    component_id: velocity_id,
                    name: "Demo.Velocity".to_string(),
                },
            ],
        };
        let mut world = ArcheWorld::create();
        let entity = world.alloc_entity();

        world.get_or_create_archetype(ArchetypeKey::new(vec![position_id]));
        {
            let table =
                world.get_or_create_archetype(ArchetypeKey::new(vec![position_id, velocity_id]));
            assert_eq!(table.insert_entity(entity), 0);
        }

        let plan = world.build_query_plan(&query);
        let rows = world.iter_query_rows(&plan);

        assert_eq!(
            rows,
            vec![QueryRow {
                archetype_index: 1,
                row: 0,
                entity,
            }]
        );
        assert_eq!(rows[0].entity.index(), 0);
        assert_eq!(rows[0].entity.generation(), 0);
        assert!(world.entities().is_alive(entity));

        let empty_query = QueryDescriptor {
            id: stable_query_id("Demo", "Move", "missing"),
            name: "Demo.Move.missing".to_string(),
            terms: vec![QueryTermDescriptor {
                access: QueryAccess::Read,
                component_id: ComponentId(0xffff000000000002),
                name: "Demo.Missing".to_string(),
            }],
        };
        let empty_plan = world.build_query_plan(&empty_query);

        assert!(world.iter_query_rows(&empty_plan).is_empty());
    }

    #[test]
    fn reads_time_delta_during_query_iteration() {
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let time_id = stable_resource_id("Demo", "Time");
        let time = ResourceDescriptor {
            id: time_id,
            name: "Demo.Time".to_string(),
            size: 4,
            align: 4,
            fields: vec![ResourceFieldDescriptor {
                name: "delta".to_string(),
                type_name: "f32".to_string(),
                offset: 0,
            }],
        };
        let query = QueryDescriptor {
            id: stable_query_id("Demo", "Move", "movers"),
            name: "Demo.Move.movers".to_string(),
            terms: vec![
                QueryTermDescriptor {
                    access: QueryAccess::Mut,
                    component_id: position_id,
                    name: "Demo.Position".to_string(),
                },
                QueryTermDescriptor {
                    access: QueryAccess::Read,
                    component_id: velocity_id,
                    name: "Demo.Velocity".to_string(),
                },
            ],
        };
        let mut world = ArcheWorld::create();
        let entity = world.alloc_entity();
        let time_payload = [0x00, 0x00, 0x80, 0x3f];

        assert!(world.register_resource_descriptor(time.clone()));
        assert!(world
            .allocate_resource_storage(&time)
            .expect("Demo.Time resource storage allocation should succeed"));
        world
            .store_resource_payload(time_id, &time_payload)
            .expect("Demo.Time payload store should succeed");

        {
            let table =
                world.get_or_create_archetype(ArchetypeKey::new(vec![position_id, velocity_id]));
            assert_eq!(table.insert_entity(entity), 0);
        }

        let plan = world.build_query_plan(&query);
        let rows = world.iter_query_rows(&plan);

        assert_eq!(
            rows,
            vec![QueryRow {
                archetype_index: 0,
                row: 0,
                entity,
            }]
        );

        let row_deltas: Vec<(QueryRow, f32)> = rows
            .iter()
            .map(|row| {
                (
                    *row,
                    world
                        .read_resource_f32_field(time_id, "delta")
                        .expect("Demo.Time.delta decode should succeed"),
                )
            })
            .collect();

        assert_eq!(row_deltas, vec![(rows[0], 1.0)]);
        assert!(world.entities().is_alive(entity));
    }

    #[test]
    fn applies_move_system_to_position_rows() {
        fn f32_pair_payload(x: f32, y: f32) -> [u8; 8] {
            let x = x.to_le_bytes();
            let y = y.to_le_bytes();
            [x[0], x[1], x[2], x[3], y[0], y[1], y[2], y[3]]
        }

        fn read_f32(bytes: &[u8], offset: usize) -> f32 {
            f32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        }

        fn xy_descriptor(id: ComponentId, name: &str) -> ComponentDescriptor {
            ComponentDescriptor {
                id,
                name: name.to_string(),
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
            }
        }

        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let time_id = stable_resource_id("Demo", "Time");
        let position = xy_descriptor(position_id, "Demo.Position");
        let velocity = xy_descriptor(velocity_id, "Demo.Velocity");
        let time = ResourceDescriptor {
            id: time_id,
            name: "Demo.Time".to_string(),
            size: 4,
            align: 4,
            fields: vec![ResourceFieldDescriptor {
                name: "delta".to_string(),
                type_name: "f32".to_string(),
                offset: 0,
            }],
        };
        let query = QueryDescriptor {
            id: stable_query_id("Demo", "Move", "movers"),
            name: "Demo.Move.movers".to_string(),
            terms: vec![
                QueryTermDescriptor {
                    access: QueryAccess::Mut,
                    component_id: position_id,
                    name: "Demo.Position".to_string(),
                },
                QueryTermDescriptor {
                    access: QueryAccess::Read,
                    component_id: velocity_id,
                    name: "Demo.Velocity".to_string(),
                },
            ],
        };
        let initial_position = f32_pair_payload(1.0, 2.0);
        let initial_velocity = f32_pair_payload(3.0, 4.0);
        let expected_position = f32_pair_payload(4.0, 6.0);
        let mut world = ArcheWorld::create();
        let entity = world.alloc_entity();

        assert!(world.register_component_descriptor(position.clone()));
        assert!(world.register_component_descriptor(velocity.clone()));
        assert!(world.register_resource_descriptor(time.clone()));
        assert!(world
            .allocate_resource_storage(&time)
            .expect("Demo.Time resource storage allocation should succeed"));
        world
            .store_resource_payload(time_id, &1.0f32.to_le_bytes())
            .expect("Demo.Time payload store should succeed");

        {
            let table =
                world.get_or_create_archetype(ArchetypeKey::new(vec![position_id, velocity_id]));
            assert!(table
                .allocate_component_column(&position, 1)
                .expect("Demo.Position column allocation should succeed"));
            assert!(table
                .allocate_component_column(&velocity, 1)
                .expect("Demo.Velocity column allocation should succeed"));
            assert_eq!(table.insert_entity(entity), 0);
            table
                .copy_component_payload(position_id, 0, &initial_position)
                .expect("initial Demo.Position payload copy should succeed");
            table
                .copy_component_payload(velocity_id, 0, &initial_velocity)
                .expect("initial Demo.Velocity payload copy should succeed");
        }

        let plan = world.build_query_plan(&query);
        let rows = world.iter_query_rows(&plan);
        assert_eq!(
            rows,
            vec![QueryRow {
                archetype_index: 0,
                row: 0,
                entity,
            }]
        );

        for row in &rows {
            let (position_x, position_y, velocity_x, velocity_y) = {
                let table = world
                    .archetype_at(row.archetype_index)
                    .expect("query row should reference an existing archetype");
                let position_bytes = table
                    .column(position_id)
                    .and_then(|column| column.row_bytes(row.row))
                    .expect("Demo.Position payload should exist for query row");
                let velocity_bytes = table
                    .column(velocity_id)
                    .and_then(|column| column.row_bytes(row.row))
                    .expect("Demo.Velocity payload should exist for query row");

                (
                    read_f32(position_bytes, 0),
                    read_f32(position_bytes, 4),
                    read_f32(velocity_bytes, 0),
                    read_f32(velocity_bytes, 4),
                )
            };
            let delta = world
                .read_resource_f32_field(time_id, "delta")
                .expect("Demo.Time.delta decode should succeed");
            let updated_position = f32_pair_payload(
                position_x + velocity_x * delta,
                position_y + velocity_y * delta,
            );

            world
                .archetype_at_mut(row.archetype_index)
                .expect("query row should reference an existing mutable archetype")
                .copy_component_payload(position_id, row.row, &updated_position)
                .expect("updated Demo.Position payload copy should succeed");
        }

        let table = world
            .archetype_at(0)
            .expect("Demo.Position + Demo.Velocity archetype should exist");
        let position_column = table
            .column(position_id)
            .expect("Demo.Position column should exist");
        let velocity_column = table
            .column(velocity_id)
            .expect("Demo.Velocity column should exist");

        assert_eq!(
            position_column.row_bytes(0),
            Some(expected_position.as_slice())
        );
        assert_eq!(
            velocity_column.row_bytes(0),
            Some(initial_velocity.as_slice())
        );
        assert_eq!(position_column.row_count(), 1);
        assert_eq!(velocity_column.row_count(), 1);
        assert_eq!(table.entity_count(), 1);
        assert_eq!(table.entity(0), Some(entity));
        assert!(world.entities().is_alive(entity));
    }

    #[test]
    fn allocates_time_delta_resource_storage() {
        let time_id = stable_resource_id("Demo", "Time");
        let time = ResourceDescriptor {
            id: time_id,
            name: "Demo.Time".to_string(),
            size: 4,
            align: 4,
            fields: vec![ResourceFieldDescriptor {
                name: "delta".to_string(),
                type_name: "f32".to_string(),
                offset: 0,
            }],
        };
        let mut world = ArcheWorld::create();

        assert!(world.register_resource_descriptor(time.clone()));
        assert!(world
            .allocate_resource_storage(&time)
            .expect("Demo.Time resource storage allocation should succeed"));

        assert_eq!(world.resource_storage_count(), 1);
        assert!(!world.is_empty());

        let storage = world
            .resource_storage(time_id)
            .expect("Demo.Time resource storage should be allocated");
        assert_eq!(storage.resource_id(), time_id);
        assert_eq!(storage.byte_size(), 4);
        assert_eq!(storage.byte_align(), 4);
        assert_eq!(storage.storage_byte_size(), 4);
        assert_eq!((storage.storage_ptr() as usize) % storage.byte_align(), 0);

        assert!(!world
            .allocate_resource_storage(&time)
            .expect("duplicate resource storage allocation check should not fail"));
        assert_eq!(world.resource_storage_count(), 1);
    }

    #[test]
    fn rejects_reads_until_resource_storage_is_initialized() {
        let descriptor = test_resource_descriptor();
        let resource_id = descriptor.id;
        let mut world = ArcheWorld::create();

        assert!(world.register_resource_descriptor(descriptor.clone()));
        assert!(world
            .allocate_resource_storage(&descriptor)
            .expect("resource allocation succeeds"));
        let storage = world
            .resource_storage(resource_id)
            .expect("resource storage exists");
        assert!(!storage.is_initialized());
        assert!(storage.storage.as_slice().iter().all(|byte| *byte == 0));

        let error = world
            .resource_payload(resource_id)
            .expect_err("uninitialized resource read must fail");
        assert!(error.message.contains("has not been initialized"));
        let error = world
            .read_resource_f32_field(resource_id, "delta")
            .expect_err("uninitialized resource field read must fail");
        assert!(error.message.contains("has not been initialized"));

        world
            .store_resource_payload(resource_id, &1.5_f32.to_le_bytes())
            .expect("exact-size resource store succeeds");
        assert_eq!(
            world
                .read_resource_f32_field(resource_id, "delta")
                .expect("initialized resource field reads"),
            1.5
        );
    }

    #[test]
    fn failed_resource_stores_preserve_payload_and_initialization_state() {
        let descriptor = test_resource_descriptor();
        let resource_id = descriptor.id;
        let mut world = ArcheWorld::create();

        assert!(world.register_resource_descriptor(descriptor.clone()));
        assert!(world
            .allocate_resource_storage(&descriptor)
            .expect("resource allocation succeeds"));
        assert!(world.store_resource_payload(resource_id, &[0, 1]).is_err());
        assert!(!world
            .resource_storage(resource_id)
            .expect("resource storage exists")
            .is_initialized());
        assert!(world.resource_payload(resource_id).is_err());

        let original = 2.5_f32.to_le_bytes();
        world
            .store_resource_payload(resource_id, &original)
            .expect("exact-size resource store succeeds");
        assert!(world.store_resource_payload(resource_id, &[9, 9]).is_err());
        assert_eq!(
            world
                .resource_payload(resource_id)
                .expect("previous payload remains readable"),
            original.as_slice()
        );
        assert!(world
            .resource_storage(resource_id)
            .expect("resource storage exists")
            .is_initialized());
    }

    #[test]
    fn stores_time_delta_resource_payload() {
        let time_id = stable_resource_id("Demo", "Time");
        let time = ResourceDescriptor {
            id: time_id,
            name: "Demo.Time".to_string(),
            size: 4,
            align: 4,
            fields: vec![ResourceFieldDescriptor {
                name: "delta".to_string(),
                type_name: "f32".to_string(),
                offset: 0,
            }],
        };
        let mut world = ArcheWorld::create();
        let payload = [0x00, 0x00, 0x80, 0x3f];

        assert!(world.register_resource_descriptor(time.clone()));
        assert!(world
            .allocate_resource_storage(&time)
            .expect("Demo.Time resource storage allocation should succeed"));

        world
            .store_resource_payload(time_id, &payload)
            .expect("Demo.Time payload store should succeed");

        let storage = world
            .resource_storage(time_id)
            .expect("Demo.Time resource storage should be allocated");
        let stored_bytes =
            unsafe { std::slice::from_raw_parts(storage.storage_ptr(), storage.byte_size()) };

        assert_eq!(stored_bytes, &payload);
        assert_eq!(world.resource_storage_count(), 1);
        assert_eq!(world.resource_descriptors().get(time_id), Some(&time));

        let wrong_size = world.store_resource_payload(time_id, &[0x00, 0x00]);
        assert!(wrong_size.is_err());

        let missing = world.store_resource_payload(ResourceId(0xffffffffffffffff), &payload);
        assert!(missing.is_err());
    }

    #[test]
    fn retrieves_time_delta_resource_payload() {
        let time_id = stable_resource_id("Demo", "Time");
        let time = ResourceDescriptor {
            id: time_id,
            name: "Demo.Time".to_string(),
            size: 4,
            align: 4,
            fields: vec![ResourceFieldDescriptor {
                name: "delta".to_string(),
                type_name: "f32".to_string(),
                offset: 0,
            }],
        };
        let mut world = ArcheWorld::create();
        let payload = [0x00, 0x00, 0x80, 0x3f];

        assert!(world.register_resource_descriptor(time.clone()));
        assert!(world
            .allocate_resource_storage(&time)
            .expect("Demo.Time resource storage allocation should succeed"));
        world
            .store_resource_payload(time_id, &payload)
            .expect("Demo.Time payload store should succeed");

        let stored_payload = world
            .resource_payload(time_id)
            .expect("Demo.Time payload read should succeed");
        assert_eq!(stored_payload, &payload);
        assert_eq!(
            world
                .read_resource_f32_field(time_id, "delta")
                .expect("Demo.Time.delta decode should succeed"),
            1.0
        );

        let missing_storage = world.resource_payload(ResourceId(0xffffffffffffffff));
        assert!(missing_storage.is_err());

        let missing_field = world.read_resource_f32_field(time_id, "missing");
        assert!(missing_field.is_err());
    }

    #[test]
    fn debug_inspects_time_delta_resource() {
        let time_id = stable_resource_id("Demo", "Time");
        let time = ResourceDescriptor {
            id: time_id,
            name: "Demo.Time".to_string(),
            size: 4,
            align: 4,
            fields: vec![ResourceFieldDescriptor {
                name: "delta".to_string(),
                type_name: "f32".to_string(),
                offset: 0,
            }],
        };
        let mut world = ArcheWorld::create();
        let payload = [0x00, 0x00, 0x80, 0x3f];

        assert!(world.register_resource_descriptor(time.clone()));
        assert!(world
            .allocate_resource_storage(&time)
            .expect("Demo.Time resource storage allocation should succeed"));
        world
            .store_resource_payload(time_id, &payload)
            .expect("Demo.Time payload store should succeed");

        let expected = [
            "world",
            "  entities 0",
            "  archetypes 0",
            "  resources 1",
            "  resource Demo.Time",
            "    delta: f32 = 1.0",
        ]
        .join("\n");

        assert_eq!(debug_inspect_world(&world), expected);
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
    fn component_column_rejects_row_gaps_and_tracks_a_contiguous_prefix() {
        let descriptor = test_xy_component_descriptor(ComponentId(0x10), "Test.Position");
        let first = f32_pair_payload(1.0, 2.0);
        let replacement = f32_pair_payload(3.0, 4.0);
        let second = f32_pair_payload(5.0, 6.0);
        let mut column =
            ComponentColumn::allocate(&descriptor, 2).expect("component column allocates");

        assert!(column.storage.as_slice().iter().all(|byte| *byte == 0));
        let error = column
            .copy_payload(1, &second)
            .expect_err("row one cannot be initialized before row zero");
        assert!(error.message.contains("uninitialized gap"));
        assert_eq!(column.row_count(), 0);
        assert_eq!(column.row_bytes(0), None);

        column
            .copy_payload(0, &first)
            .expect("first contiguous row appends");
        column
            .copy_payload(0, &replacement)
            .expect("existing initialized row updates");
        column
            .copy_payload(1, &second)
            .expect("next contiguous row appends");

        assert_eq!(column.row_count(), 2);
        assert_eq!(column.row_bytes(0), Some(replacement.as_slice()));
        assert_eq!(column.row_bytes(1), Some(second.as_slice()));
    }

    #[test]
    fn component_growth_failure_preserves_the_live_column() {
        let descriptor = test_xy_component_descriptor(ComponentId(0x11), "Test.Position");
        let payload = f32_pair_payload(7.0, 8.0);
        let mut column =
            ComponentColumn::allocate(&descriptor, 1).expect("component column allocates");
        column
            .copy_payload(0, &payload)
            .expect("first payload stores");
        let original_ptr = column.storage_ptr();

        let error = column
            .prepare_growth(usize::MAX)
            .expect_err("impossible byte size must fail");

        assert!(error.message.contains("overflowed"));
        assert_eq!(column.storage_ptr(), original_ptr);
        assert_eq!(column.row_capacity(), 1);
        assert_eq!(column.row_count(), 1);
        assert_eq!(column.row_bytes(0), Some(payload.as_slice()));
    }

    #[test]
    fn world_spawn_grows_columns_geometrically_and_preserves_rows() {
        let position_id = ComponentId(0x20);
        let position = ComponentDescriptor {
            id: position_id,
            name: "Test.HighAlignmentPosition".to_string(),
            size: 64,
            align: 64,
            fields: Vec::new(),
        };
        let mut payloads = [[0_u8; 64]; 4];
        for (index, payload) in payloads.iter_mut().enumerate() {
            payload[..8].copy_from_slice(&f32_pair_payload(index as f32 + 1.0, index as f32 + 2.0));
            payload[63] = index as u8;
        }
        let expected_capacities = [1, 2, 4, 4];
        let key = ArchetypeKey::new(vec![position_id]);
        let mut world = ArcheWorld::create();
        assert!(world.register_component_descriptor(position));

        for (row, payload) in payloads.iter().enumerate() {
            let entity = world
                .spawn_entity_with_payloads(&[ComponentPayload {
                    component_id: position_id,
                    payload_bytes: payload,
                }])
                .expect("transactional spawn succeeds");
            assert_eq!(entity.index(), row as u32);
            assert!(world.entities().is_alive(entity));

            let table = world.archetype(&key).expect("position archetype exists");
            let column = table.column(position_id).expect("position column exists");
            assert_eq!(table.entity_count(), row + 1);
            assert_eq!(column.row_count(), row + 1);
            assert_eq!(column.row_capacity(), expected_capacities[row]);
            assert_eq!(column.element_align(), 64);
            assert_eq!((column.storage_ptr() as usize) % 64, 0);
            for (stored_row, stored_payload) in payloads[..=row].iter().enumerate() {
                assert_eq!(
                    column.row_bytes(stored_row),
                    Some(stored_payload.as_slice())
                );
            }
        }
    }

    #[test]
    fn invalid_second_spawn_leaves_world_logically_unchanged() {
        let position_id = ComponentId(0x30);
        let velocity_id = ComponentId(0x31);
        let position = test_xy_component_descriptor(position_id, "Test.Position");
        let velocity = test_xy_component_descriptor(velocity_id, "Test.Velocity");
        let position_payload = f32_pair_payload(1.0, 2.0);
        let velocity_payload = f32_pair_payload(3.0, 4.0);
        let key = ArchetypeKey::new(vec![position_id, velocity_id]);
        let mut world = ArcheWorld::create();
        assert!(world.register_component_descriptor(position));
        assert!(world.register_component_descriptor(velocity));

        let first_entity = world
            .spawn_entity_with_payloads(&[
                ComponentPayload {
                    component_id: position_id,
                    payload_bytes: &position_payload,
                },
                ComponentPayload {
                    component_id: velocity_id,
                    payload_bytes: &velocity_payload,
                },
            ])
            .expect("first spawn succeeds");
        let error = world
            .spawn_entity_with_payloads(&[
                ComponentPayload {
                    component_id: position_id,
                    payload_bytes: &position_payload,
                },
                ComponentPayload {
                    component_id: velocity_id,
                    payload_bytes: &[0, 1],
                },
            ])
            .expect_err("invalid second payload must fail before commit");

        assert!(error.message.contains("payload size"));
        assert_eq!(world.entities().len(), 1);
        assert!(world.entities().is_alive(first_entity));
        assert_eq!(world.archetype_count(), 1);
        let table = world.archetype(&key).expect("original archetype remains");
        assert_eq!(table.entity_count(), 1);
        assert_eq!(table.entity(0), Some(first_entity));
        let position_column = table.column(position_id).expect("position column exists");
        let velocity_column = table.column(velocity_id).expect("velocity column exists");
        assert_eq!(position_column.row_capacity(), 1);
        assert_eq!(velocity_column.row_capacity(), 1);
        assert_eq!(position_column.row_count(), 1);
        assert_eq!(velocity_column.row_count(), 1);
        assert_eq!(
            position_column.row_bytes(0),
            Some(position_payload.as_slice())
        );
        assert_eq!(
            velocity_column.row_bytes(0),
            Some(velocity_payload.as_slice())
        );
    }

    #[test]
    fn duplicate_spawn_components_are_rejected_without_world_mutation() {
        let position_id = ComponentId(0x40);
        let position = test_xy_component_descriptor(position_id, "Test.Position");
        let payload = f32_pair_payload(1.0, 2.0);
        let mut world = ArcheWorld::create();
        assert!(world.register_component_descriptor(position));

        let error = world
            .spawn_entity_with_payloads(&[
                ComponentPayload {
                    component_id: position_id,
                    payload_bytes: &payload,
                },
                ComponentPayload {
                    component_id: position_id,
                    payload_bytes: &payload,
                },
            ])
            .expect_err("duplicate component must fail");

        assert!(error.message.contains("duplicate spawn component"));
        assert_eq!(world.entities().len(), 0);
        assert_eq!(world.archetype_count(), 0);
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
    fn copies_position_payload_into_column() {
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let position_key = ArchetypeKey::new(vec![position_id]);
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
        let position_payload = [0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40];
        let mut world = ArcheWorld::create();
        let entity = world.alloc_entity();

        {
            let table = world.get_or_create_archetype(position_key.clone());
            assert!(table
                .allocate_component_column(&position, 1)
                .expect("Position column allocation should succeed"));
            let row = table.insert_entity(entity);

            table
                .copy_component_payload(position_id, row, &position_payload)
                .expect("Position payload copy should succeed");

            assert_eq!(row, 0);
            assert_eq!(table.entity_count(), 1);
            assert_eq!(table.entity(0), Some(entity));

            let column = table
                .column(position_id)
                .expect("Position column should be allocated");
            assert_eq!(column.row_count(), 1);
            assert_eq!(column.row_bytes(0), Some(position_payload.as_slice()));
            assert_eq!(column.row_bytes(1), None);
        }

        assert!(world.entities().is_alive(entity));

        let table = world
            .archetype(&position_key)
            .expect("Position archetype table should exist");
        let column = table
            .column(position_id)
            .expect("Position column should remain allocated");
        assert_eq!(column.row_count(), 1);
        assert_eq!(column.row_bytes(0), Some(position_payload.as_slice()));
    }

    #[test]
    fn debug_inspects_spawned_position_world() {
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let position_key = ArchetypeKey::new(vec![position_id]);
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
        let position_payload = [0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40];
        let mut world = ArcheWorld::create();

        assert!(world.register_component_descriptor(position.clone()));

        let entity = world.alloc_entity();
        {
            let table = world.get_or_create_archetype(position_key);
            assert!(table
                .allocate_component_column(&position, 1)
                .expect("Position column allocation should succeed"));
            let row = table.insert_entity(entity);

            table
                .copy_component_payload(position_id, row, &position_payload)
                .expect("Position payload copy should succeed");
        }

        let expected = [
            "world",
            "  entities 1",
            "  archetypes 1",
            "  archetype Demo.Position",
            "    row 0 entity index 0 generation 0",
            "      component Demo.Position",
            "        x: f32 = 1.0",
            "        y: f32 = 2.0",
        ]
        .join("\n");

        assert_eq!(debug_inspect_world(&world), expected);
    }

    #[test]
    fn world_create_destroy_smoke() {
        let world = ArcheWorld::create();

        assert_eq!(world.entities().len(), 0);
        assert_eq!(world.component_descriptors().len(), 0);
        assert_eq!(world.resource_descriptors().len(), 0);
        assert_eq!(world.system_descriptors().len(), 0);
        assert_eq!(world.schedule_descriptors().len(), 0);
        assert_eq!(world.query_descriptors().len(), 0);
        assert_eq!(world.resource_storage_count(), 0);
        assert_eq!(world.archetype_count(), 0);
        assert!(world.is_empty());

        world.destroy();
    }
}
