#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::core::{
    CoreComponent, CoreInstruction, CoreProgram, CoreSpawnComponent, CoreSpawnFieldValue, CoreType,
};
use crate::core_verify;
use crate::runtime::ComponentFieldDescriptor;
use crate::runtime_assembly::{
    verified_core_startup_instructions, RuntimeProgramAssembly, StartupOperation,
};

pub(crate) const NATIVE_STORAGE_BASE_OFFSET: u16 = 936;

const QWORD_BYTE_LEN: u16 = 8;
const FRAME_ALIGNMENT: u64 = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeWorldPlanError {
    pub(crate) message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeSlot {
    pub(crate) offset: u16,
    pub(crate) byte_len: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeByteRange {
    pub(crate) offset: u16,
    pub(crate) byte_len: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeWorldStoragePlan {
    pub(crate) frame_size: u16,
    pub(crate) tables: Vec<NativeTableStoragePlan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeTableStoragePlan {
    pub(crate) key: Box<[u64]>,
    pub(crate) rows: Vec<NativePlannedSpawnRow>,
    pub(crate) capacity_steps: Box<[u32]>,
    pub(crate) capacity: u32,
    pub(crate) logical_row_stride: u32,
    pub(crate) storage: NativeTableStorageSlots,
    pub(crate) catalog: NativeCatalogTableSlots,
    pub(crate) columns: Vec<NativeColumnStoragePlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativePlannedSpawnRow {
    pub(crate) spawn_ordinal: u32,
    pub(crate) startup_operation_index: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeColumnStoragePlan {
    pub(crate) schema: NativeComponentSchema,
    pub(crate) payload: NativeByteRange,
    pub(crate) catalog: NativeCatalogColumnSlots,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeComponentSchema {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) size: u32,
    pub(crate) align: u32,
    pub(crate) fields: Vec<ComponentFieldDescriptor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeTableStorageSlots {
    pub(crate) row_count: NativeSlot,
    pub(crate) capacity: NativeSlot,
    pub(crate) row_stride: NativeSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeCatalogTableSlots {
    pub(crate) column_count: NativeSlot,
    pub(crate) row_count_address: NativeSlot,
    pub(crate) capacity: NativeSlot,
    pub(crate) row_stride: NativeSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeCatalogColumnSlots {
    pub(crate) component_id: NativeSlot,
    pub(crate) element_size: NativeSlot,
    pub(crate) element_align: NativeSlot,
    pub(crate) payload_base_address: NativeSlot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeWorldStorageInput {
    schemas: Vec<NativeComponentSchema>,
    spawns: Vec<NativeSpawnInput>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeSpawnInput {
    spawn_ordinal: u32,
    startup_operation_index: u32,
    components: Vec<NativeSpawnComponentInput>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeSpawnComponentInput {
    component_id: u64,
    component_name: String,
    payload_byte_len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TableAccumulator {
    rows: Vec<NativePlannedSpawnRow>,
    capacity_steps: Vec<u32>,
    capacity: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PartialTablePlan {
    key: Vec<u64>,
    rows: Vec<NativePlannedSpawnRow>,
    capacity_steps: Vec<u32>,
    capacity: u32,
    logical_row_stride: u32,
    storage: NativeTableStorageSlots,
    catalog: Option<NativeCatalogTableSlots>,
    columns: Vec<PartialColumnPlan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PartialColumnPlan {
    schema: NativeComponentSchema,
    payload: NativeByteRange,
    catalog: Option<NativeCatalogColumnSlots>,
}

pub(crate) fn derive_native_world_storage_plan(
    core: &CoreProgram,
    assembly: &RuntimeProgramAssembly,
    storage_base_offset: u16,
) -> Result<NativeWorldStoragePlan, NativeWorldPlanError> {
    if storage_base_offset < NATIVE_STORAGE_BASE_OFFSET {
        return Err(plan_error(format!(
            "native world storage base {storage_base_offset} overlaps the fixed prefix ending at {NATIVE_STORAGE_BASE_OFFSET}"
        )));
    }
    core_verify::verify_core_program(core).map_err(|error| {
        plan_error(format!(
            "cannot derive native world storage from invalid Core: {}",
            error.message
        ))
    })?;

    let input = native_world_storage_input(core, assembly)?;
    derive_native_world_storage_plan_from_input(&input, storage_base_offset)
}

fn native_world_storage_input(
    core: &CoreProgram,
    assembly: &RuntimeProgramAssembly,
) -> Result<NativeWorldStorageInput, NativeWorldPlanError> {
    if core.world.name != assembly.world_name {
        return Err(plan_error(format!(
            "Core world `{}` does not match runtime assembly world `{}`",
            core.world.name, assembly.world_name
        )));
    }

    if core.components.len() != assembly.component_descriptors.len() {
        return Err(plan_error(
            "Core and runtime assembly component schema counts do not match",
        ));
    }

    let core_components: HashMap<u64, &CoreComponent> = core
        .components
        .iter()
        .map(|component| (component.id, component))
        .collect();
    let mut schemas = Vec::with_capacity(assembly.component_descriptors.len());
    for descriptor in &assembly.component_descriptors {
        let component = core_components.get(&descriptor.id.0).ok_or_else(|| {
            plan_error(format!(
                "runtime component schema `{}` is absent from verified Core",
                descriptor.name
            ))
        })?;
        verify_runtime_schema_matches_core(component, descriptor)?;
        schemas.push(NativeComponentSchema {
            id: descriptor.id.0,
            name: descriptor.name.clone(),
            size: descriptor.size,
            align: descriptor.align,
            fields: descriptor.fields.clone(),
        });
    }

    let mut spawns = Vec::new();
    let mut spawn_ordinal = 0u32;
    for (operation_index, operation) in assembly.startup_operations.iter().enumerate() {
        let StartupOperation::Spawn { components } = operation else {
            continue;
        };
        let startup_operation_index = u32::try_from(operation_index)
            .map_err(|_| plan_error("native startup operation index exceeds u32"))?;
        spawns.push(NativeSpawnInput {
            spawn_ordinal,
            startup_operation_index,
            components: components
                .iter()
                .map(|component| NativeSpawnComponentInput {
                    component_id: component.component_id.0,
                    component_name: component.component_name.clone(),
                    payload_byte_len: component.payload_bytes.len(),
                })
                .collect(),
        });
        spawn_ordinal = spawn_ordinal
            .checked_add(1)
            .ok_or_else(|| plan_error("native spawn ordinal exceeds u32"))?;
    }

    verify_runtime_spawns_match_core(core, assembly)?;

    Ok(NativeWorldStorageInput { schemas, spawns })
}

fn verify_runtime_spawns_match_core(
    core: &CoreProgram,
    assembly: &RuntimeProgramAssembly,
) -> Result<(), NativeWorldPlanError> {
    let core_spawns = verified_core_startup_instructions(core)
        .map_err(|error| plan_error(error.message))?
        .iter()
        .filter_map(|instruction| match instruction {
            CoreInstruction::Spawn { components } => Some(components),
            _ => None,
        })
        .collect::<Vec<_>>();
    let runtime_spawns = assembly
        .startup_operations
        .iter()
        .filter_map(|operation| match operation {
            StartupOperation::Spawn { components } => Some(components),
            _ => None,
        })
        .collect::<Vec<_>>();

    if core_spawns.len() != runtime_spawns.len() {
        return Err(plan_error(format!(
            "verified Core contains {} startup spawns but runtime assembly contains {}",
            core_spawns.len(),
            runtime_spawns.len()
        )));
    }

    let descriptors = assembly
        .component_descriptors
        .iter()
        .map(|descriptor| (descriptor.id.0, descriptor))
        .collect::<HashMap<_, _>>();
    for (spawn_index, (core_components, runtime_components)) in
        core_spawns.iter().zip(runtime_spawns).enumerate()
    {
        if core_components.len() != runtime_components.len() {
            return Err(plan_error(format!(
                "verified Core startup spawn {spawn_index} contains {} components but runtime assembly contains {}",
                core_components.len(),
                runtime_components.len()
            )));
        }
        for core_component in *core_components {
            let runtime_component = runtime_components
                .iter()
                .find(|component| component.component_id.0 == core_component.component_id)
                .ok_or_else(|| {
                    plan_error(format!(
                        "runtime assembly startup spawn {spawn_index} is missing Core component `{}`",
                        core_component.name
                    ))
                })?;
            if runtime_component.component_name != core_component.name {
                return Err(plan_error(format!(
                    "runtime assembly startup component `{}` does not match Core component `{}`",
                    runtime_component.component_name, core_component.name
                )));
            }
            let descriptor = descriptors
                .get(&core_component.component_id)
                .ok_or_else(|| {
                    plan_error(format!(
                        "runtime component descriptor for Core component `{}` is missing",
                        core_component.name
                    ))
                })?;
            let core_payload = encode_core_spawn_payload(core_component, descriptor)?;
            if core_payload != runtime_component.payload_bytes {
                return Err(plan_error(format!(
                    "runtime assembly startup payload for `{}` does not match verified Core",
                    core_component.name
                )));
            }
        }
    }

    Ok(())
}

fn encode_core_spawn_payload(
    component: &CoreSpawnComponent,
    descriptor: &crate::runtime::ComponentDescriptor,
) -> Result<Vec<u8>, NativeWorldPlanError> {
    let mut payload = vec![0; descriptor.size as usize];
    for field in &component.fields {
        let descriptor_field = descriptor
            .fields
            .iter()
            .find(|candidate| candidate.name == field.name)
            .ok_or_else(|| {
                plan_error(format!(
                    "Core startup field `{}.{}` is absent from the runtime descriptor",
                    component.name, field.name
                ))
            })?;
        let bytes = match field.value {
            CoreSpawnFieldValue::F32Bits(bits) => bits.to_le_bytes(),
            CoreSpawnFieldValue::I32(value) => value.to_le_bytes(),
        };
        let start = descriptor_field.offset as usize;
        let end = start
            .checked_add(bytes.len())
            .ok_or_else(|| plan_error("Core startup field range overflow"))?;
        let target = payload.get_mut(start..end).ok_or_else(|| {
            plan_error(format!(
                "Core startup field `{}.{}` exceeds the runtime descriptor payload",
                component.name, field.name
            ))
        })?;
        target.copy_from_slice(&bytes);
    }
    Ok(payload)
}

fn verify_runtime_schema_matches_core(
    component: &CoreComponent,
    descriptor: &crate::runtime::ComponentDescriptor,
) -> Result<(), NativeWorldPlanError> {
    if component.name != descriptor.name {
        return Err(plan_error(format!(
            "runtime component schema `{}` does not match Core name `{}`",
            descriptor.name, component.name
        )));
    }

    let mut expected_fields = Vec::with_capacity(component.fields.len());
    let mut cursor = 0u32;
    let mut component_align = 1u32;
    for field in &component.fields {
        let (type_name, size, align) = match field.ty {
            CoreType::I32 => ("i32", 4, 4),
            CoreType::F32 => ("f32", 4, 4),
        };
        cursor = checked_align_up_u32(cursor, align, "Core component field offset")?;
        expected_fields.push(ComponentFieldDescriptor {
            name: field.name.clone(),
            type_name: type_name.to_string(),
            offset: cursor,
        });
        cursor = cursor
            .checked_add(size)
            .ok_or_else(|| plan_error("Core component layout exceeds u32"))?;
        component_align = component_align.max(align);
    }
    let expected_size = checked_align_up_u32(cursor, component_align, "Core component size")?;

    if descriptor.fields != expected_fields
        || descriptor.size != expected_size
        || descriptor.align != component_align
    {
        return Err(plan_error(format!(
            "runtime component schema `{}` does not match verified Core layout",
            descriptor.name
        )));
    }

    Ok(())
}

fn derive_native_world_storage_plan_from_input(
    input: &NativeWorldStorageInput,
    storage_base_offset: u16,
) -> Result<NativeWorldStoragePlan, NativeWorldPlanError> {
    if input.schemas.is_empty() {
        return Err(plan_error(
            "native world storage requires at least one component schema",
        ));
    }
    if input.spawns.is_empty() {
        return Err(plan_error(
            "native world storage requires at least one startup spawn",
        ));
    }

    let schemas = validate_and_index_schemas(&input.schemas)?;
    let tables = collect_archetype_tables(&schemas, &input.spawns)?;
    lay_out_tables(&schemas, tables, storage_base_offset)
}

fn validate_and_index_schemas(
    schemas: &[NativeComponentSchema],
) -> Result<BTreeMap<u64, NativeComponentSchema>, NativeWorldPlanError> {
    let mut by_id = BTreeMap::new();
    let mut names = HashSet::new();
    for schema in schemas {
        if schema.name.is_empty() {
            return Err(plan_error("native component schema name must not be empty"));
        }
        if by_id.insert(schema.id, schema.clone()).is_some() {
            return Err(plan_error(format!(
                "duplicate native component schema id 0x{:016x}",
                schema.id
            )));
        }
        if !names.insert(schema.name.as_str()) {
            return Err(plan_error(format!(
                "duplicate native component schema name `{}`",
                schema.name
            )));
        }
        if schema.size == 0 {
            return Err(plan_error(format!(
                "native component schema `{}` has zero size",
                schema.name
            )));
        }
        if schema.align == 0 || !schema.align.is_power_of_two() {
            return Err(plan_error(format!(
                "native component schema `{}` alignment must be a nonzero power of two",
                schema.name
            )));
        }
        if schema.size % schema.align != 0 {
            return Err(plan_error(format!(
                "native component schema `{}` size is not a multiple of its alignment",
                schema.name
            )));
        }
        validate_schema_fields(schema)?;
    }
    Ok(by_id)
}

fn validate_schema_fields(schema: &NativeComponentSchema) -> Result<(), NativeWorldPlanError> {
    let mut names = HashSet::new();
    let mut previous_end = 0u32;
    for field in &schema.fields {
        if !names.insert(field.name.as_str()) {
            return Err(plan_error(format!(
                "duplicate native field `{}.{}`",
                schema.name, field.name
            )));
        }
        let field_size = match field.type_name.as_str() {
            "i32" | "f32" => 4,
            _ => {
                return Err(plan_error(format!(
                    "native field `{}.{}` has unsupported type `{}`",
                    schema.name, field.name, field.type_name
                )))
            }
        };
        if field.offset % field_size != 0 || field.offset < previous_end {
            return Err(plan_error(format!(
                "native field `{}.{}` has an invalid or overlapping offset",
                schema.name, field.name
            )));
        }
        let end = field
            .offset
            .checked_add(field_size)
            .ok_or_else(|| plan_error("native field range exceeds u32"))?;
        if end > schema.size {
            return Err(plan_error(format!(
                "native field `{}.{}` exceeds its component schema",
                schema.name, field.name
            )));
        }
        previous_end = end;
    }
    Ok(())
}

fn collect_archetype_tables(
    schemas: &BTreeMap<u64, NativeComponentSchema>,
    spawns: &[NativeSpawnInput],
) -> Result<BTreeMap<Vec<u64>, TableAccumulator>, NativeWorldPlanError> {
    let mut tables = BTreeMap::new();
    let mut spawn_ordinals = HashSet::new();
    let mut startup_operation_indices = HashSet::new();

    for spawn in spawns {
        if !spawn_ordinals.insert(spawn.spawn_ordinal) {
            return Err(plan_error(format!(
                "duplicate native spawn ordinal {}",
                spawn.spawn_ordinal
            )));
        }
        if !startup_operation_indices.insert(spawn.startup_operation_index) {
            return Err(plan_error(format!(
                "duplicate native startup operation index {}",
                spawn.startup_operation_index
            )));
        }
        if spawn.components.is_empty() {
            return Err(plan_error("native startup spawn has no components"));
        }

        let mut key = Vec::with_capacity(spawn.components.len());
        for component in &spawn.components {
            let schema = schemas.get(&component.component_id).ok_or_else(|| {
                plan_error(format!(
                    "native startup spawn references unknown component id 0x{:016x}",
                    component.component_id
                ))
            })?;
            if component.component_name != schema.name {
                return Err(plan_error(format!(
                    "native startup component name `{}` does not match schema `{}`",
                    component.component_name, schema.name
                )));
            }
            if component.payload_byte_len != schema.size as usize {
                return Err(plan_error(format!(
                    "native startup component `{}` payload has {} bytes, expected {}",
                    schema.name, component.payload_byte_len, schema.size
                )));
            }
            key.push(component.component_id);
        }
        key.sort_unstable();
        if key.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(plan_error(
                "native startup spawn contains a duplicate component id",
            ));
        }

        let table = tables.entry(key).or_insert_with(|| TableAccumulator {
            rows: Vec::new(),
            capacity_steps: Vec::new(),
            capacity: 0,
        });
        let next_row_count = table
            .rows
            .len()
            .checked_add(1)
            .ok_or_else(|| plan_error("native table row count overflow"))?;
        let row_count = u32::try_from(next_row_count)
            .map_err(|_| plan_error("native table row count exceeds u32"))?;
        if row_count > table.capacity {
            table.capacity = grow_capacity(table.capacity)?;
            table.capacity_steps.push(table.capacity);
        }
        table.rows.push(NativePlannedSpawnRow {
            spawn_ordinal: spawn.spawn_ordinal,
            startup_operation_index: spawn.startup_operation_index,
        });
    }

    Ok(tables)
}

fn grow_capacity(capacity: u32) -> Result<u32, NativeWorldPlanError> {
    if capacity == 0 {
        Ok(1)
    } else {
        capacity
            .checked_mul(2)
            .ok_or_else(|| plan_error("native table geometric capacity overflow"))
    }
}

fn lay_out_tables(
    schemas: &BTreeMap<u64, NativeComponentSchema>,
    tables: BTreeMap<Vec<u64>, TableAccumulator>,
    storage_base_offset: u16,
) -> Result<NativeWorldStoragePlan, NativeWorldPlanError> {
    let mut cursor = u64::from(storage_base_offset);
    let mut partial_tables = Vec::with_capacity(tables.len());

    for (key, table) in tables {
        cursor = checked_align_up(cursor, u64::from(QWORD_BYTE_LEN), "native table header")?;
        let storage = NativeTableStorageSlots {
            row_count: allocate_qword(&mut cursor)?,
            capacity: allocate_qword(&mut cursor)?,
            row_stride: allocate_qword(&mut cursor)?,
        };

        let mut logical_row_stride = 0u64;
        let mut columns = Vec::with_capacity(key.len());
        for component_id in &key {
            let schema = schemas
                .get(component_id)
                .expect("canonical table keys are resolved during validation")
                .clone();
            logical_row_stride = logical_row_stride
                .checked_add(u64::from(schema.size))
                .ok_or_else(|| plan_error("native logical row stride overflow"))?;
            cursor = checked_align_up(cursor, u64::from(schema.align), "native component column")?;
            let payload_byte_len = u64::from(schema.size)
                .checked_mul(u64::from(table.capacity))
                .ok_or_else(|| plan_error("native component column size overflow"))?;
            let payload = allocate_range(&mut cursor, payload_byte_len)?;
            columns.push(PartialColumnPlan {
                schema,
                payload,
                catalog: None,
            });
        }

        partial_tables.push(PartialTablePlan {
            key,
            rows: table.rows,
            capacity_steps: table.capacity_steps,
            capacity: table.capacity,
            logical_row_stride: u32::try_from(logical_row_stride)
                .map_err(|_| plan_error("native logical row stride exceeds u32"))?,
            storage,
            catalog: None,
            columns,
        });
    }

    cursor = checked_align_up(cursor, u64::from(QWORD_BYTE_LEN), "native storage catalog")?;
    for table in &mut partial_tables {
        table.catalog = Some(NativeCatalogTableSlots {
            column_count: allocate_qword(&mut cursor)?,
            row_count_address: allocate_qword(&mut cursor)?,
            capacity: allocate_qword(&mut cursor)?,
            row_stride: allocate_qword(&mut cursor)?,
        });
    }
    for table in &mut partial_tables {
        for column in &mut table.columns {
            column.catalog = Some(NativeCatalogColumnSlots {
                component_id: allocate_qword(&mut cursor)?,
                element_size: allocate_qword(&mut cursor)?,
                element_align: allocate_qword(&mut cursor)?,
                payload_base_address: allocate_qword(&mut cursor)?,
            });
        }
    }

    cursor = checked_align_up(cursor, FRAME_ALIGNMENT, "native frame")?;
    let frame_size = u16::try_from(cursor)
        .map_err(|_| plan_error("native world storage frame exceeds the bounded u16 frame"))?;
    let tables = partial_tables
        .into_iter()
        .map(|table| NativeTableStoragePlan {
            key: table.key.into_boxed_slice(),
            rows: table.rows,
            capacity_steps: table.capacity_steps.into_boxed_slice(),
            capacity: table.capacity,
            logical_row_stride: table.logical_row_stride,
            storage: table.storage,
            catalog: table.catalog.expect("catalog table slots were allocated"),
            columns: table
                .columns
                .into_iter()
                .map(|column| NativeColumnStoragePlan {
                    schema: column.schema,
                    payload: column.payload,
                    catalog: column.catalog.expect("catalog column slots were allocated"),
                })
                .collect(),
        })
        .collect();

    Ok(NativeWorldStoragePlan { frame_size, tables })
}

fn allocate_qword(cursor: &mut u64) -> Result<NativeSlot, NativeWorldPlanError> {
    let offset = u16::try_from(*cursor)
        .map_err(|_| plan_error("native qword offset exceeds the bounded u16 frame"))?;
    *cursor = cursor
        .checked_add(u64::from(QWORD_BYTE_LEN))
        .ok_or_else(|| plan_error("native qword allocation overflow"))?;
    if *cursor > u64::from(u16::MAX) {
        return Err(plan_error(
            "native qword allocation exceeds the bounded u16 frame",
        ));
    }
    Ok(NativeSlot {
        offset,
        byte_len: QWORD_BYTE_LEN,
    })
}

fn allocate_range(
    cursor: &mut u64,
    byte_len: u64,
) -> Result<NativeByteRange, NativeWorldPlanError> {
    let offset = u16::try_from(*cursor)
        .map_err(|_| plan_error("native byte range offset exceeds the bounded u16 frame"))?;
    let byte_len = u16::try_from(byte_len)
        .map_err(|_| plan_error("native byte range length exceeds the bounded u16 frame"))?;
    *cursor = cursor
        .checked_add(u64::from(byte_len))
        .ok_or_else(|| plan_error("native byte range allocation overflow"))?;
    if *cursor > u64::from(u16::MAX) {
        return Err(plan_error(
            "native byte range exceeds the bounded u16 frame",
        ));
    }
    Ok(NativeByteRange { offset, byte_len })
}

fn checked_align_up(value: u64, align: u64, label: &str) -> Result<u64, NativeWorldPlanError> {
    if align == 0 || !align.is_power_of_two() {
        return Err(plan_error(format!(
            "{label} alignment must be a nonzero power of two"
        )));
    }
    let mask = align - 1;
    value
        .checked_add(mask)
        .map(|aligned| aligned & !mask)
        .ok_or_else(|| plan_error(format!("{label} alignment overflow")))
}

fn checked_align_up_u32(value: u32, align: u32, label: &str) -> Result<u32, NativeWorldPlanError> {
    let aligned = checked_align_up(u64::from(value), u64::from(align), label)?;
    u32::try_from(aligned).map_err(|_| plan_error(format!("{label} exceeds u32")))
}

fn plan_error(message: impl Into<String>) -> NativeWorldPlanError {
    NativeWorldPlanError {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{checker, core_lower, layout, lexer, parser, runtime_assembly};

    #[test]
    fn materializes_arbitrary_startup_component_lists() {
        let source = include_str!("../../../examples/arena_recovery.arc");
        let tokens = lexer::lex(source).expect("arena_recovery.arc lexes");
        let program = parser::parse_program(&tokens).expect("arena_recovery.arc parses");
        checker::check_program(&program).expect("arena_recovery.arc checks");
        let core = core_lower::lower_program_to_core(&program).expect("Arena Core lowers");
        core_verify::verify_core_program(&core).expect("Arena Core verifies");
        let assembly =
            runtime_assembly::assemble_runtime_program_from_verified_core(&program, &core)
                .expect("Arena runtime assembly builds from verified Core");

        let faction_id = layout::stable_component_id("Arena", "Faction");
        let faction_payloads = assembly
            .startup_operations
            .iter()
            .filter_map(|operation| match operation {
                StartupOperation::Spawn { components } => components
                    .iter()
                    .find(|component| component.component_id == faction_id)
                    .map(|component| component.payload_bytes.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            faction_payloads,
            (1i32..=5)
                .map(|value| value.to_le_bytes().to_vec())
                .collect::<Vec<_>>()
        );

        let source_plan =
            derive_native_world_storage_plan(&core, &assembly, NATIVE_STORAGE_BASE_OFFSET)
                .expect("real Arena native storage plan derives");
        let boundary_plan =
            derive_native_world_storage_plan_from_input(&arena_input(), NATIVE_STORAGE_BASE_OFFSET)
                .expect("Arena planning boundary contract derives");
        assert_eq!(source_plan, boundary_plan);
        assert_eq!(source_plan.frame_size, 1328);
        assert_eq!(source_plan.tables.len(), 2);
        assert_eq!(source_plan.tables[0].capacity_steps.as_ref(), [1, 2, 4]);
        assert_eq!(source_plan.tables[0].rows.len(), 3);
        assert_eq!(source_plan.tables[1].capacity_steps.as_ref(), [1, 2]);
        assert_eq!(source_plan.tables[1].rows.len(), 2);
    }

    #[test]
    fn derives_native_world_storage_plan() {
        let source = include_str!("../../../examples/move_system_two_rows.arc");
        let tokens = lexer::lex(source).expect("move_system_two_rows.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system_two_rows.arc parses");
        checker::check_program(&program).expect("move_system_two_rows.arc checks");
        let core = core_lower::lower_program_to_core(&program).expect("Demo Core lowers");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("Demo runtime assembly builds");

        let demo = derive_native_world_storage_plan(&core, &assembly, NATIVE_STORAGE_BASE_OFFSET)
            .expect("Demo native storage plan derives");
        assert_eq!(demo.frame_size, 1088);
        assert_eq!(demo.tables.len(), 1);
        let demo_table = &demo.tables[0];
        assert_eq!(
            demo_table.key.as_ref(),
            [0x002202c6aeb4f27b, 0x2cf8a68bcb7f913b,]
        );
        assert_eq!(
            demo_table.rows,
            vec![
                NativePlannedSpawnRow {
                    spawn_ordinal: 0,
                    startup_operation_index: 1,
                },
                NativePlannedSpawnRow {
                    spawn_ordinal: 1,
                    startup_operation_index: 2,
                },
            ]
        );
        assert_eq!(demo_table.capacity_steps.as_ref(), [1, 2]);
        assert_eq!(demo_table.capacity, 2);
        assert_eq!(demo_table.logical_row_stride, 16);
        assert_eq!(
            demo_table.storage,
            NativeTableStorageSlots {
                row_count: slot(936),
                capacity: slot(944),
                row_stride: slot(952),
            }
        );
        assert_eq!(demo_table.columns.len(), 2);
        assert_eq!(demo_table.columns[0].payload, range(960, 16));
        assert_eq!(demo_table.columns[1].payload, range(976, 16));
        assert_eq!(demo_table.columns[0].schema.name, "Demo.Position");
        assert_eq!(demo_table.columns[1].schema.name, "Demo.Velocity");
        assert_eq!(demo_table.catalog, catalog_table_slots(992));
        assert_eq!(demo_table.columns[0].catalog, catalog_column_slots(1024));
        assert_eq!(demo_table.columns[1].catalog, catalog_column_slots(1056));

        let mut missing_spawn_assembly = assembly.clone();
        let removed_spawn = missing_spawn_assembly
            .startup_operations
            .iter()
            .rposition(|operation| matches!(operation, StartupOperation::Spawn { .. }))
            .expect("Demo contains a startup spawn");
        missing_spawn_assembly
            .startup_operations
            .remove(removed_spawn);
        assert!(derive_native_world_storage_plan(
            &core,
            &missing_spawn_assembly,
            NATIVE_STORAGE_BASE_OFFSET,
        )
        .expect_err("runtime assembly cannot omit a verified Core spawn")
        .message
        .contains("startup spawns"));

        let mut changed_payload_assembly = assembly.clone();
        let changed_payload = changed_payload_assembly
            .startup_operations
            .iter_mut()
            .find_map(|operation| match operation {
                StartupOperation::Spawn { components } => components
                    .first_mut()
                    .map(|component| &mut component.payload_bytes),
                _ => None,
            })
            .expect("Demo contains a startup component payload");
        changed_payload[0] ^= 0xff;
        assert!(derive_native_world_storage_plan(
            &core,
            &changed_payload_assembly,
            NATIVE_STORAGE_BASE_OFFSET,
        )
        .expect_err("runtime assembly payloads must agree with verified Core")
        .message
        .contains("does not match verified Core"));

        assert!(
            derive_native_world_storage_plan(&core, &assembly, NATIVE_STORAGE_BASE_OFFSET - 1)
                .expect_err("native storage cannot overlap the fixed prefix")
                .message
                .contains("overlaps the fixed prefix")
        );

        let mut reordered_demo_input =
            native_world_storage_input(&core, &assembly).expect("Demo planning input builds");
        reordered_demo_input.schemas.reverse();
        for spawn in &mut reordered_demo_input.spawns {
            spawn.components.reverse();
        }
        assert_eq!(
            derive_native_world_storage_plan_from_input(
                &reordered_demo_input,
                NATIVE_STORAGE_BASE_OFFSET,
            )
            .expect("declaration and payload order do not affect the plan"),
            demo
        );

        let arena = arena_input();
        let arena_plan =
            derive_native_world_storage_plan_from_input(&arena, NATIVE_STORAGE_BASE_OFFSET)
                .expect("Arena-equivalent native storage plan derives");
        assert_eq!(arena_plan.frame_size, 1328);
        assert_eq!(arena_plan.tables.len(), 2);

        let regeneration_id = layout::stable_component_id("Arena", "Regeneration").0;
        let vitality_id = layout::stable_component_id("Arena", "Vitality").0;
        let faction_id = layout::stable_component_id("Arena", "Faction").0;
        assert_eq!(regeneration_id, 0x939d7f182fd22525);
        assert_eq!(vitality_id, 0x98d7339d36f83790);
        assert_eq!(faction_id, 0xc11f1992f249584e);

        let full = &arena_plan.tables[0];
        assert_eq!(
            full.key.as_ref(),
            [regeneration_id, vitality_id, faction_id]
        );
        assert_eq!(
            full.rows
                .iter()
                .map(|row| row.spawn_ordinal)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(full.capacity_steps.as_ref(), [1, 2, 4]);
        assert_eq!(full.capacity, 4);
        assert_eq!(full.logical_row_stride, 24);
        assert_eq!(full.storage.row_count.offset, 936);
        assert_eq!(full.columns[0].payload, range(960, 48));
        assert_eq!(full.columns[1].payload, range(1008, 32));
        assert_eq!(full.columns[2].payload, range(1040, 16));
        assert_eq!(full.catalog, catalog_table_slots(1104));
        assert_eq!(full.columns[0].catalog, catalog_column_slots(1168));
        assert_eq!(full.columns[1].catalog, catalog_column_slots(1200));
        assert_eq!(full.columns[2].catalog, catalog_column_slots(1232));
        assert_eq!(full.columns[0].schema.fields.len(), 3);

        let partial = &arena_plan.tables[1];
        assert_eq!(partial.key.as_ref(), [vitality_id, faction_id]);
        assert_eq!(
            partial
                .rows
                .iter()
                .map(|row| row.spawn_ordinal)
                .collect::<Vec<_>>(),
            [3, 4]
        );
        assert_eq!(partial.capacity_steps.as_ref(), [1, 2]);
        assert_eq!(partial.capacity, 2);
        assert_eq!(partial.logical_row_stride, 12);
        assert_eq!(partial.storage.row_count.offset, 1056);
        assert_eq!(partial.columns[0].payload, range(1080, 16));
        assert_eq!(partial.columns[1].payload, range(1096, 8));
        assert_eq!(partial.catalog, catalog_table_slots(1136));
        assert_eq!(partial.columns[0].catalog, catalog_column_slots(1264));
        assert_eq!(partial.columns[1].catalog, catalog_column_slots(1296));

        let mut reordered_arena = arena.clone();
        reordered_arena.schemas.reverse();
        for spawn in &mut reordered_arena.spawns {
            spawn.components.reverse();
        }
        assert_eq!(
            derive_native_world_storage_plan_from_input(
                &reordered_arena,
                NATIVE_STORAGE_BASE_OFFSET,
            )
            .expect("Arena schema and payload order do not affect the plan"),
            arena_plan
        );

        assert_invalid_inputs();
    }

    fn arena_input() -> NativeWorldStorageInput {
        let vitality_id = layout::stable_component_id("Arena", "Vitality").0;
        let regeneration_id = layout::stable_component_id("Arena", "Regeneration").0;
        let faction_id = layout::stable_component_id("Arena", "Faction").0;
        let schemas = vec![
            schema(faction_id, "Arena.Faction", 4, 4, &[("id", "i32", 0)]),
            schema(
                vitality_id,
                "Arena.Vitality",
                8,
                4,
                &[("current", "f32", 0), ("reserve", "f32", 4)],
            ),
            schema(
                regeneration_id,
                "Arena.Regeneration",
                12,
                4,
                &[
                    ("current_rate", "f32", 0),
                    ("reserve_rate", "f32", 4),
                    ("cap", "f32", 8),
                ],
            ),
        ];
        let full_orders = [
            [vitality_id, regeneration_id, faction_id],
            [faction_id, vitality_id, regeneration_id],
            [regeneration_id, faction_id, vitality_id],
        ];
        let mut spawns = Vec::new();
        for (index, order) in full_orders.into_iter().enumerate() {
            spawns.push(spawn(index as u32, index as u32 + 1, &schemas, &order));
        }
        spawns.push(spawn(3, 4, &schemas, &[faction_id, vitality_id]));
        spawns.push(spawn(4, 5, &schemas, &[vitality_id, faction_id]));
        NativeWorldStorageInput { schemas, spawns }
    }

    fn minimal_input() -> NativeWorldStorageInput {
        let schemas = vec![schema(1, "Test.Value", 4, 4, &[("value", "f32", 0)])];
        let spawns = vec![spawn(0, 0, &schemas, &[1])];
        NativeWorldStorageInput { schemas, spawns }
    }

    fn assert_invalid_inputs() {
        let mut input = minimal_input();
        input.schemas.clear();
        assert_error(input, "at least one component schema");

        let mut input = minimal_input();
        input.spawns.clear();
        assert_error(input, "at least one startup spawn");

        let mut input = minimal_input();
        input.schemas.push(input.schemas[0].clone());
        assert_error(input, "duplicate native component schema id");

        let mut input = minimal_input();
        let mut duplicate_name = input.schemas[0].clone();
        duplicate_name.id = 2;
        input.schemas.push(duplicate_name);
        assert_error(input, "duplicate native component schema name");

        let mut input = minimal_input();
        input.schemas[0].size = 0;
        assert_error(input, "zero size");

        let mut input = minimal_input();
        input.schemas[0].align = 3;
        assert_error(input, "nonzero power of two");

        let mut input = minimal_input();
        input.schemas[0].size = 12;
        input.schemas[0].align = 8;
        assert_error(input, "not a multiple");

        let mut input = minimal_input();
        input.spawns[0].components.clear();
        assert_error(input, "has no components");

        let mut input = minimal_input();
        let duplicate_component = input.spawns[0].components[0].clone();
        input.spawns[0].components.push(duplicate_component);
        assert_error(input, "duplicate component id");

        let mut input = minimal_input();
        input.spawns[0].components[0].component_id = 2;
        assert_error(input, "unknown component id");

        let mut input = minimal_input();
        input.spawns[0].components[0].component_name = "Test.Other".to_string();
        assert_error(input, "does not match schema");

        let mut input = minimal_input();
        input.spawns[0].components[0].payload_byte_len = 8;
        assert_error(input, "expected 4");

        let mut input = minimal_input();
        input.spawns.push(input.spawns[0].clone());
        assert_error(input, "duplicate native spawn ordinal");

        let mut input = minimal_input();
        let mut second = input.spawns[0].clone();
        second.spawn_ordinal = 1;
        input.spawns.push(second);
        assert_error(input, "duplicate native startup operation index");

        let mut input = minimal_input();
        input.schemas[0].size = u32::MAX;
        input.schemas[0].align = 1;
        input.schemas[0].fields.clear();
        input.spawns[0].components[0].payload_byte_len = u32::MAX as usize;
        assert_error(input, "byte range length exceeds");

        let error = derive_native_world_storage_plan_from_input(&minimal_input(), u16::MAX)
            .expect_err("aligned header beyond u16 must fail");
        assert!(error.message.contains("bounded u16 frame"));
        assert!(grow_capacity(u32::MAX)
            .expect_err("capacity doubling must be checked")
            .message
            .contains("capacity overflow"));
        assert!(checked_align_up(u64::MAX, 16, "test")
            .expect_err("alignment addition must be checked")
            .message
            .contains("alignment overflow"));
    }

    fn assert_error(input: NativeWorldStorageInput, expected: &str) {
        let error = derive_native_world_storage_plan_from_input(&input, NATIVE_STORAGE_BASE_OFFSET)
            .expect_err("invalid native planner input must fail");
        assert!(
            error.message.contains(expected),
            "expected `{expected}` in `{}`",
            error.message
        );
    }

    fn schema(
        id: u64,
        name: &str,
        size: u32,
        align: u32,
        fields: &[(&str, &str, u32)],
    ) -> NativeComponentSchema {
        NativeComponentSchema {
            id,
            name: name.to_string(),
            size,
            align,
            fields: fields
                .iter()
                .map(|(name, type_name, offset)| ComponentFieldDescriptor {
                    name: (*name).to_string(),
                    type_name: (*type_name).to_string(),
                    offset: *offset,
                })
                .collect(),
        }
    }

    fn spawn(
        spawn_ordinal: u32,
        startup_operation_index: u32,
        schemas: &[NativeComponentSchema],
        component_ids: &[u64],
    ) -> NativeSpawnInput {
        NativeSpawnInput {
            spawn_ordinal,
            startup_operation_index,
            components: component_ids
                .iter()
                .map(|component_id| {
                    let schema = schemas
                        .iter()
                        .find(|schema| schema.id == *component_id)
                        .expect("test component schema exists");
                    NativeSpawnComponentInput {
                        component_id: *component_id,
                        component_name: schema.name.clone(),
                        payload_byte_len: schema.size as usize,
                    }
                })
                .collect(),
        }
    }

    fn slot(offset: u16) -> NativeSlot {
        NativeSlot {
            offset,
            byte_len: 8,
        }
    }

    fn range(offset: u16, byte_len: u16) -> NativeByteRange {
        NativeByteRange { offset, byte_len }
    }

    fn catalog_table_slots(offset: u16) -> NativeCatalogTableSlots {
        NativeCatalogTableSlots {
            column_count: slot(offset),
            row_count_address: slot(offset + 8),
            capacity: slot(offset + 16),
            row_stride: slot(offset + 24),
        }
    }

    fn catalog_column_slots(offset: u16) -> NativeCatalogColumnSlots {
        NativeCatalogColumnSlots {
            component_id: slot(offset),
            element_size: slot(offset + 8),
            element_align: slot(offset + 16),
            payload_base_address: slot(offset + 24),
        }
    }
}
