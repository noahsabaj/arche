use crate::core::{
    CoreProgram, CoreQueryAccess, CoreScheduleItem, CoreSystemBinaryOp, CoreSystemExpression,
    CoreSystemParamKind, CoreSystemPlace, CoreSystemStatement,
};
use crate::core_lower;
use crate::core_verify;
use crate::native_world_plan::{
    derive_native_world_storage_plan, NativeCatalogColumnSlots, NativeSlot, NativeWorldStoragePlan,
    NATIVE_STORAGE_BASE_OFFSET,
};
use crate::parser::{BinaryOperator, Expression, Program, Statement};
use crate::runtime_assembly;

const NATIVE_ECS_QWORD_BYTE_LEN: u16 = 8;
const NATIVE_ECS_DWORD_BYTE_LEN: u16 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeEcsSlot {
    offset: u16,
    byte_len: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeDescriptorCountSlots {
    components: NativeEcsSlot,
    resources: NativeEcsSlot,
    systems: NativeEcsSlot,
    queries: NativeEcsSlot,
    schedules: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeDescriptorSectionSlots {
    payload_offset: NativeEcsSlot,
    byte_len: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeDescriptorRecordStateSlots {
    components: NativeDescriptorSectionSlots,
    resources: NativeDescriptorSectionSlots,
    systems: NativeDescriptorSectionSlots,
    queries: NativeDescriptorSectionSlots,
    schedules: NativeDescriptorSectionSlots,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeSpawnPayloadStorageSlots {
    position_payload: NativeEcsSlot,
    velocity_payload: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeStartupStateSlots {
    time_payload: NativeEcsSlot,
    row_count: NativeEcsSlot,
    spawn_payload_rows: [NativeSpawnPayloadStorageSlots; 2],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeStartupDispatchSlots {
    operation_count: NativeEcsSlot,
    resource_dispatch_count: NativeEcsSlot,
    spawn_dispatch_count: NativeEcsSlot,
    run_schedule_dispatch_count: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeQueryPlanSlots {
    matched_row_count: NativeEcsSlot,
    position_payload_address: NativeEcsSlot,
    velocity_payload_address: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeCompiledScheduleSlots {
    schedule_id: NativeEcsSlot,
    scheduled_system_id: NativeEcsSlot,
    scheduled_system_count: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeXyDescriptorSlots {
    id: NativeEcsSlot,
    size: NativeEcsSlot,
    align: NativeEcsSlot,
    field_count: NativeEcsSlot,
    x_field_offset: NativeEcsSlot,
    y_field_offset: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeTimeDescriptorSlots {
    id: NativeEcsSlot,
    size: NativeEcsSlot,
    align: NativeEcsSlot,
    field_count: NativeEcsSlot,
    delta_field_offset: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeComponentResourceDescriptorTableSlots {
    position: NativeXyDescriptorSlots,
    velocity: NativeXyDescriptorSlots,
    time: NativeTimeDescriptorSlots,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeMoveSystemDescriptorSlots {
    id: NativeEcsSlot,
    param_count: NativeEcsSlot,
    resource_param_kind: NativeEcsSlot,
    resource_param_resource_id: NativeEcsSlot,
    query_param_kind: NativeEcsSlot,
    query_param_term_count: NativeEcsSlot,
    query_term0_access: NativeEcsSlot,
    query_term0_component_id: NativeEcsSlot,
    query_term1_access: NativeEcsSlot,
    query_term1_component_id: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeMoversQueryDescriptorSlots {
    id: NativeEcsSlot,
    term_count: NativeEcsSlot,
    term0_access: NativeEcsSlot,
    term0_component_id: NativeEcsSlot,
    term1_access: NativeEcsSlot,
    term1_component_id: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeMainScheduleDescriptorSlots {
    id: NativeEcsSlot,
    item_count: NativeEcsSlot,
    run_item_kind: NativeEcsSlot,
    run_system_id: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeSystemQueryScheduleDescriptorTableSlots {
    move_system: NativeMoveSystemDescriptorSlots,
    movers_query: NativeMoversQueryDescriptorSlots,
    main_schedule: NativeMainScheduleDescriptorSlots,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeNameReferenceSlots {
    byte_offset: NativeEcsSlot,
    byte_len: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeDescriptorNameTableSlots {
    position: NativeNameReferenceSlots,
    velocity: NativeNameReferenceSlots,
    time: NativeNameReferenceSlots,
    move_system: NativeNameReferenceSlots,
    movers_query: NativeNameReferenceSlots,
    main_schedule: NativeNameReferenceSlots,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeResourceStartupOperationSlots {
    kind: NativeEcsSlot,
    resource_id: NativeEcsSlot,
    payload_offset: NativeEcsSlot,
    payload_len: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeSpawnStartupOperationSlots {
    kind: NativeEcsSlot,
    component_count: NativeEcsSlot,
    position_component_id: NativeEcsSlot,
    position_payload_offset: NativeEcsSlot,
    position_payload_len: NativeEcsSlot,
    velocity_component_id: NativeEcsSlot,
    velocity_payload_offset: NativeEcsSlot,
    velocity_payload_len: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeRunScheduleStartupOperationSlots {
    kind: NativeEcsSlot,
    schedule_id: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeStartupOperationTableSlots {
    resource: NativeResourceStartupOperationSlots,
    spawn_rows: [NativeSpawnStartupOperationSlots; 2],
    run_schedule: NativeRunScheduleStartupOperationSlots,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativePlannedComponentDescriptorSlots {
    access: NativeEcsSlot,
    component_id: NativeEcsSlot,
    size: NativeEcsSlot,
    x_field_offset: NativeEcsSlot,
    y_field_offset: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeDescriptorBackedQueryPlanSlots {
    query_id: NativeEcsSlot,
    term_count: NativeEcsSlot,
    position: NativePlannedComponentDescriptorSlots,
    velocity: NativePlannedComponentDescriptorSlots,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeCompiledMoveSlots {
    target_position_payload: NativeEcsSlot,
    scanned_row_count: NativeEcsSlot,
    field_product_payload: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeComponentColumnPayloadSlots {
    payload_rows: [NativeEcsSlot; 2],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeArchetypeTableStorageSlots {
    row_count: NativeEcsSlot,
    capacity: NativeEcsSlot,
    row_stride: NativeEcsSlot,
    position_column: NativeComponentColumnPayloadSlots,
    velocity_column: NativeComponentColumnPayloadSlots,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeArchetypeTableStorageRowSlots {
    position_payload: NativeEcsSlot,
    velocity_payload: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeStorageCatalogTableRowSlots {
    column_count: NativeEcsSlot,
    row_count_address: NativeEcsSlot,
    capacity: NativeEcsSlot,
    row_stride: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeStorageCatalogColumnRowSlots {
    component_id: NativeEcsSlot,
    element_size: NativeEcsSlot,
    element_align: NativeEcsSlot,
    payload_base_address: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeStorageCatalogSlots {
    table_rows: [NativeStorageCatalogTableRowSlots; 1],
    column_rows: [NativeStorageCatalogColumnRowSlots; 2],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeEcsExecutionStateLayout {
    frame_size: u16,
    zeroed_qword_offsets: [u16; 136],
    descriptor_counts: NativeDescriptorCountSlots,
    descriptor_records: NativeDescriptorRecordStateSlots,
    startup_state: NativeStartupStateSlots,
    startup_dispatch: NativeStartupDispatchSlots,
    query_plan: NativeQueryPlanSlots,
    compiled_schedule: NativeCompiledScheduleSlots,
    component_resource_descriptors: NativeComponentResourceDescriptorTableSlots,
    system_query_schedule_descriptors: NativeSystemQueryScheduleDescriptorTableSlots,
    startup_operations: NativeStartupOperationTableSlots,
    descriptor_backed_query_plan: NativeDescriptorBackedQueryPlanSlots,
    descriptor_names: NativeDescriptorNameTableSlots,
    compiled_move: NativeCompiledMoveSlots,
    archetype_storage: NativeArchetypeTableStorageSlots,
    storage_catalog: NativeStorageCatalogSlots,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeXyDescriptorTableRow {
    slots: NativeXyDescriptorSlots,
    name: NativeNameReferenceSlots,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeTimeDescriptorTableRow {
    slots: NativeTimeDescriptorSlots,
    name: NativeNameReferenceSlots,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeMoveSystemDescriptorTableRow {
    slots: NativeMoveSystemDescriptorSlots,
    name: NativeNameReferenceSlots,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeMoversQueryDescriptorTableRow {
    slots: NativeMoversQueryDescriptorSlots,
    name: NativeNameReferenceSlots,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeMainScheduleDescriptorTableRow {
    slots: NativeMainScheduleDescriptorSlots,
    name: NativeNameReferenceSlots,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeDescriptorTableModel {
    component_rows: [NativeXyDescriptorTableRow; 2],
    resource_rows: [NativeTimeDescriptorTableRow; 1],
    system_rows: [NativeMoveSystemDescriptorTableRow; 1],
    query_rows: [NativeMoversQueryDescriptorTableRow; 1],
    schedule_rows: [NativeMainScheduleDescriptorTableRow; 1],
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeStartupOperationTableModel {
    resource_payload_rows: [NativeResourceStartupOperationSlots; 1],
    spawn_rows: [NativeSpawnStartupOperationSlots; 2],
    run_schedule_rows: [NativeRunScheduleStartupOperationSlots; 1],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeStartupOperationHandler {
    ResourcePayload,
    Spawn,
    RunSchedule,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeStartupOperationDispatchRow {
    handler: NativeStartupOperationHandler,
    expected_kind: u32,
    kind_slot: u16,
    dispatch_count_slot: u16,
    dispatch_count_after_row: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeQueryPlanTermRole {
    Position,
    Velocity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeQueryPlanTermBuildRow {
    role: NativeQueryPlanTermRole,
    query_access_slot: u16,
    query_component_id_slot: u16,
    system_access_slot: u16,
    system_component_id_slot: u16,
    component_descriptor_id_slot: u16,
    component_size_slot: u16,
    component_x_field_offset_slot: u16,
    component_y_field_offset_slot: u16,
    catalog_component_id_slot: u16,
    catalog_element_size_slot: u16,
    catalog_payload_base_address_slot: u16,
    plan_access_slot: u16,
    plan_component_id_slot: u16,
    plan_size_slot: u16,
    plan_x_field_offset_slot: u16,
    plan_y_field_offset_slot: u16,
    planned_payload_address_slot: u16,
    expected_access: u64,
    expected_size: u64,
    expected_x_field_offset: u64,
    expected_y_field_offset: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeQueryPlanBuildRow {
    query_id_slot: u16,
    query_term_count_slot: u16,
    system_query_term_count_slot: u16,
    catalog_column_count_slot: u16,
    catalog_row_count_address_slot: u16,
    plan_query_id_slot: u16,
    plan_term_count_slot: u16,
    matched_row_count_slot: u16,
    terms: [NativeQueryPlanTermBuildRow; 2],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeQueryPlanTableIterationRow {
    cursor_table: NativeTableIterationKind,
    cursor_row_index: usize,
    primary_slot: NativeEcsSlot,
    build_row: NativeQueryPlanBuildRow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeCompiledScheduleBuildRow {
    startup_schedule_id_slot: u16,
    descriptor_schedule_id_slot: u16,
    descriptor_item_count_slot: u16,
    descriptor_run_system_id_slot: u16,
    system_descriptor_id_slot: u16,
    compiled_schedule_id_slot: u16,
    compiled_scheduled_system_id_slot: u16,
    compiled_scheduled_system_count_slot: u16,
    expected_scheduled_system_count: u64,
    expected_scheduled_system_id: u64,
    query_plan_row_index: usize,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeCompiledScheduleTableModel {
    rows: [NativeCompiledScheduleSlots; 1],
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeQueryPlanTableModel {
    rows: [NativeDescriptorBackedQueryPlanSlots; 1],
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeStorageCatalogTableRow {
    slots: NativeStorageCatalogTableRowSlots,
    storage: NativeArchetypeTableStorageSlots,
    columns: [NativeStorageCatalogColumnRow; 2],
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeStorageCatalogColumnRow {
    slots: NativeStorageCatalogColumnRowSlots,
    descriptor: NativeXyDescriptorSlots,
    payload_column: NativeComponentColumnPayloadSlots,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeStorageCatalogModel {
    table_rows: [NativeStorageCatalogTableRow; 1],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeStorageCompatibilityModel {
    catalog_table: NativeStorageCatalogTableRow,
    capacity: u64,
    row_stride: u64,
}

#[derive(Clone, Copy, Debug)]
struct NativeCompiledQueryExecution<'a> {
    observable: &'a NativeMoveQueryLoopObservable,
    storage_compatibility: NativeStorageCompatibilityModel,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeEcsTableModel {
    descriptors: NativeDescriptorTableModel,
    startup_operations: NativeStartupOperationTableModel,
    compiled_schedules: NativeCompiledScheduleTableModel,
    query_plans: NativeQueryPlanTableModel,
    archetype_storage: NativeArchetypeTableStorageSlots,
    storage_catalog: NativeStorageCatalogModel,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeTableIterationKind {
    ComponentDescriptors,
    ResourceDescriptors,
    SystemDescriptors,
    QueryDescriptors,
    ScheduleDescriptors,
    StartupOperations,
    CompiledSchedules,
    QueryPlans,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeTableIterationRowKind {
    ComponentDescriptor,
    ResourceDescriptor,
    SystemDescriptor,
    QueryDescriptor,
    ScheduleDescriptor,
    StartupResourcePayload,
    StartupSpawn,
    StartupRunSchedule,
    CompiledSchedule,
    QueryPlan,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeTableIterationRow {
    row_kind: NativeTableIterationRowKind,
    row_index: usize,
    primary_slot: NativeEcsSlot,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeTableIterationCursor<const N: usize> {
    table: NativeTableIterationKind,
    expected_row_count: usize,
    count_slot: Option<NativeEcsSlot>,
    rows: [NativeTableIterationRow; N],
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeEcsTableIterationCursorModel {
    component_descriptors: NativeTableIterationCursor<2>,
    resource_descriptors: NativeTableIterationCursor<1>,
    system_descriptors: NativeTableIterationCursor<1>,
    query_descriptors: NativeTableIterationCursor<1>,
    schedule_descriptors: NativeTableIterationCursor<1>,
    startup_operations: NativeTableIterationCursor<4>,
    compiled_schedules: NativeTableIterationCursor<1>,
    query_plans: NativeTableIterationCursor<1>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeDescriptorDecodeFamily {
    ComponentResource,
    SystemQuerySchedule,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeDescriptorTableIterationRow {
    cursor_table: NativeTableIterationKind,
    cursor_row_index: usize,
    expected_table_count: u64,
    count_slot: NativeEcsSlot,
    primary_slot: NativeEcsSlot,
    decode_family: NativeDescriptorDecodeFamily,
    qword_load_start: usize,
    qword_load_len: usize,
    dword_load_start: usize,
    dword_load_len: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeStartupOperationTableIterationRow {
    cursor_table: NativeTableIterationKind,
    cursor_row_index: usize,
    expected_table_count: u64,
    count_slot: NativeEcsSlot,
    primary_slot: NativeEcsSlot,
    dispatch_row: NativeStartupOperationDispatchRow,
}

const NATIVE_ECS_EXECUTION_STATE_LAYOUT: NativeEcsExecutionStateLayout =
    NativeEcsExecutionStateLayout {
        frame_size: 1088,
        zeroed_qword_offsets: [
            0, 8, 16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96, 104, 112, 120, 128, 136, 144, 152,
            160, 168, 176, 184, 192, 200, 208, 216, 224, 232, 240, 248, 256, 264, 272, 280, 288,
            296, 304, 312, 320, 328, 336, 344, 352, 360, 368, 376, 384, 392, 400, 408, 416, 424,
            432, 440, 448, 456, 464, 472, 480, 488, 496, 504, 512, 520, 528, 536, 544, 552, 560,
            568, 576, 584, 592, 600, 608, 616, 624, 632, 640, 648, 656, 664, 672, 680, 688, 696,
            704, 712, 720, 728, 736, 744, 752, 760, 768, 776, 784, 792, 800, 808, 816, 824, 832,
            840, 848, 856, 864, 872, 880, 888, 896, 904, 912, 920, 928, 936, 944, 952, 960, 968,
            976, 984, 992, 1000, 1008, 1016, 1024, 1032, 1040, 1048, 1056, 1064, 1072, 1080,
        ],
        descriptor_counts: NativeDescriptorCountSlots {
            components: NativeEcsSlot {
                offset: 0,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            resources: NativeEcsSlot {
                offset: 8,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            systems: NativeEcsSlot {
                offset: 16,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            queries: NativeEcsSlot {
                offset: 24,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            schedules: NativeEcsSlot {
                offset: 32,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
        },
        descriptor_records: NativeDescriptorRecordStateSlots {
            components: NativeDescriptorSectionSlots {
                payload_offset: NativeEcsSlot {
                    offset: 96,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                byte_len: NativeEcsSlot {
                    offset: 104,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            resources: NativeDescriptorSectionSlots {
                payload_offset: NativeEcsSlot {
                    offset: 112,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                byte_len: NativeEcsSlot {
                    offset: 120,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            systems: NativeDescriptorSectionSlots {
                payload_offset: NativeEcsSlot {
                    offset: 128,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                byte_len: NativeEcsSlot {
                    offset: 136,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            queries: NativeDescriptorSectionSlots {
                payload_offset: NativeEcsSlot {
                    offset: 144,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                byte_len: NativeEcsSlot {
                    offset: 152,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            schedules: NativeDescriptorSectionSlots {
                payload_offset: NativeEcsSlot {
                    offset: 160,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                byte_len: NativeEcsSlot {
                    offset: 168,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
        },
        startup_state: NativeStartupStateSlots {
            time_payload: NativeEcsSlot {
                offset: 40,
                byte_len: NATIVE_ECS_DWORD_BYTE_LEN,
            },
            row_count: NativeEcsSlot {
                offset: 48,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            spawn_payload_rows: [
                NativeSpawnPayloadStorageSlots {
                    position_payload: NativeEcsSlot {
                        offset: 56,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    velocity_payload: NativeEcsSlot {
                        offset: 64,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                },
                NativeSpawnPayloadStorageSlots {
                    position_payload: NativeEcsSlot {
                        offset: 920,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    velocity_payload: NativeEcsSlot {
                        offset: 928,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                },
            ],
        },
        startup_dispatch: NativeStartupDispatchSlots {
            operation_count: NativeEcsSlot {
                offset: 176,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            resource_dispatch_count: NativeEcsSlot {
                offset: 184,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            spawn_dispatch_count: NativeEcsSlot {
                offset: 192,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            run_schedule_dispatch_count: NativeEcsSlot {
                offset: 200,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
        },
        query_plan: NativeQueryPlanSlots {
            matched_row_count: NativeEcsSlot {
                offset: 208,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            position_payload_address: NativeEcsSlot {
                offset: 216,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            velocity_payload_address: NativeEcsSlot {
                offset: 224,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
        },
        compiled_schedule: NativeCompiledScheduleSlots {
            schedule_id: NativeEcsSlot {
                offset: 232,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            scheduled_system_id: NativeEcsSlot {
                offset: 240,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            scheduled_system_count: NativeEcsSlot {
                offset: 248,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
        },
        component_resource_descriptors: NativeComponentResourceDescriptorTableSlots {
            position: NativeXyDescriptorSlots {
                id: NativeEcsSlot {
                    offset: 256,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                size: NativeEcsSlot {
                    offset: 264,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                align: NativeEcsSlot {
                    offset: 272,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                field_count: NativeEcsSlot {
                    offset: 280,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                x_field_offset: NativeEcsSlot {
                    offset: 288,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                y_field_offset: NativeEcsSlot {
                    offset: 296,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            velocity: NativeXyDescriptorSlots {
                id: NativeEcsSlot {
                    offset: 304,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                size: NativeEcsSlot {
                    offset: 312,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                align: NativeEcsSlot {
                    offset: 320,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                field_count: NativeEcsSlot {
                    offset: 328,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                x_field_offset: NativeEcsSlot {
                    offset: 336,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                y_field_offset: NativeEcsSlot {
                    offset: 344,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            time: NativeTimeDescriptorSlots {
                id: NativeEcsSlot {
                    offset: 352,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                size: NativeEcsSlot {
                    offset: 360,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                align: NativeEcsSlot {
                    offset: 368,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                field_count: NativeEcsSlot {
                    offset: 376,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                delta_field_offset: NativeEcsSlot {
                    offset: 384,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
        },
        system_query_schedule_descriptors: NativeSystemQueryScheduleDescriptorTableSlots {
            move_system: NativeMoveSystemDescriptorSlots {
                id: NativeEcsSlot {
                    offset: 392,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                param_count: NativeEcsSlot {
                    offset: 400,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                resource_param_kind: NativeEcsSlot {
                    offset: 408,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                resource_param_resource_id: NativeEcsSlot {
                    offset: 416,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                query_param_kind: NativeEcsSlot {
                    offset: 424,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                query_param_term_count: NativeEcsSlot {
                    offset: 432,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                query_term0_access: NativeEcsSlot {
                    offset: 440,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                query_term0_component_id: NativeEcsSlot {
                    offset: 448,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                query_term1_access: NativeEcsSlot {
                    offset: 456,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                query_term1_component_id: NativeEcsSlot {
                    offset: 464,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            movers_query: NativeMoversQueryDescriptorSlots {
                id: NativeEcsSlot {
                    offset: 472,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                term_count: NativeEcsSlot {
                    offset: 480,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                term0_access: NativeEcsSlot {
                    offset: 488,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                term0_component_id: NativeEcsSlot {
                    offset: 496,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                term1_access: NativeEcsSlot {
                    offset: 504,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                term1_component_id: NativeEcsSlot {
                    offset: 512,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            main_schedule: NativeMainScheduleDescriptorSlots {
                id: NativeEcsSlot {
                    offset: 520,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                item_count: NativeEcsSlot {
                    offset: 528,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                run_item_kind: NativeEcsSlot {
                    offset: 536,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                run_system_id: NativeEcsSlot {
                    offset: 544,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
        },
        startup_operations: NativeStartupOperationTableSlots {
            resource: NativeResourceStartupOperationSlots {
                kind: NativeEcsSlot {
                    offset: 552,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                resource_id: NativeEcsSlot {
                    offset: 560,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                payload_offset: NativeEcsSlot {
                    offset: 568,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                payload_len: NativeEcsSlot {
                    offset: 576,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            spawn_rows: [
                NativeSpawnStartupOperationSlots {
                    kind: NativeEcsSlot {
                        offset: 584,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    component_count: NativeEcsSlot {
                        offset: 592,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    position_component_id: NativeEcsSlot {
                        offset: 600,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    position_payload_offset: NativeEcsSlot {
                        offset: 608,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    position_payload_len: NativeEcsSlot {
                        offset: 616,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    velocity_component_id: NativeEcsSlot {
                        offset: 624,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    velocity_payload_offset: NativeEcsSlot {
                        offset: 632,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    velocity_payload_len: NativeEcsSlot {
                        offset: 640,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                },
                NativeSpawnStartupOperationSlots {
                    kind: NativeEcsSlot {
                        offset: 856,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    component_count: NativeEcsSlot {
                        offset: 864,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    position_component_id: NativeEcsSlot {
                        offset: 872,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    position_payload_offset: NativeEcsSlot {
                        offset: 880,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    position_payload_len: NativeEcsSlot {
                        offset: 888,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    velocity_component_id: NativeEcsSlot {
                        offset: 896,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    velocity_payload_offset: NativeEcsSlot {
                        offset: 904,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    velocity_payload_len: NativeEcsSlot {
                        offset: 912,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                },
            ],
            run_schedule: NativeRunScheduleStartupOperationSlots {
                kind: NativeEcsSlot {
                    offset: 648,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                schedule_id: NativeEcsSlot {
                    offset: 656,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
        },
        descriptor_backed_query_plan: NativeDescriptorBackedQueryPlanSlots {
            query_id: NativeEcsSlot {
                offset: 664,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            term_count: NativeEcsSlot {
                offset: 672,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            position: NativePlannedComponentDescriptorSlots {
                access: NativeEcsSlot {
                    offset: 680,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                component_id: NativeEcsSlot {
                    offset: 688,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                size: NativeEcsSlot {
                    offset: 696,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                x_field_offset: NativeEcsSlot {
                    offset: 704,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                y_field_offset: NativeEcsSlot {
                    offset: 712,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            velocity: NativePlannedComponentDescriptorSlots {
                access: NativeEcsSlot {
                    offset: 720,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                component_id: NativeEcsSlot {
                    offset: 728,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                size: NativeEcsSlot {
                    offset: 736,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                x_field_offset: NativeEcsSlot {
                    offset: 744,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                y_field_offset: NativeEcsSlot {
                    offset: 752,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
        },
        descriptor_names: NativeDescriptorNameTableSlots {
            position: NativeNameReferenceSlots {
                byte_offset: NativeEcsSlot {
                    offset: 760,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                byte_len: NativeEcsSlot {
                    offset: 768,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            velocity: NativeNameReferenceSlots {
                byte_offset: NativeEcsSlot {
                    offset: 776,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                byte_len: NativeEcsSlot {
                    offset: 784,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            time: NativeNameReferenceSlots {
                byte_offset: NativeEcsSlot {
                    offset: 792,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                byte_len: NativeEcsSlot {
                    offset: 800,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            move_system: NativeNameReferenceSlots {
                byte_offset: NativeEcsSlot {
                    offset: 808,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                byte_len: NativeEcsSlot {
                    offset: 816,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            movers_query: NativeNameReferenceSlots {
                byte_offset: NativeEcsSlot {
                    offset: 824,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                byte_len: NativeEcsSlot {
                    offset: 832,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
            main_schedule: NativeNameReferenceSlots {
                byte_offset: NativeEcsSlot {
                    offset: 840,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                byte_len: NativeEcsSlot {
                    offset: 848,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            },
        },
        compiled_move: NativeCompiledMoveSlots {
            target_position_payload: NativeEcsSlot {
                offset: 72,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            scanned_row_count: NativeEcsSlot {
                offset: 80,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            field_product_payload: NativeEcsSlot {
                offset: 88,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
        },
        archetype_storage: NativeArchetypeTableStorageSlots {
            row_count: NativeEcsSlot {
                offset: 936,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            capacity: NativeEcsSlot {
                offset: 944,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            row_stride: NativeEcsSlot {
                offset: 952,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            position_column: NativeComponentColumnPayloadSlots {
                payload_rows: [
                    NativeEcsSlot {
                        offset: 960,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    NativeEcsSlot {
                        offset: 968,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                ],
            },
            velocity_column: NativeComponentColumnPayloadSlots {
                payload_rows: [
                    NativeEcsSlot {
                        offset: 976,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    NativeEcsSlot {
                        offset: 984,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                ],
            },
        },
        storage_catalog: NativeStorageCatalogSlots {
            table_rows: [NativeStorageCatalogTableRowSlots {
                column_count: NativeEcsSlot {
                    offset: 992,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                row_count_address: NativeEcsSlot {
                    offset: 1000,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                capacity: NativeEcsSlot {
                    offset: 1008,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
                row_stride: NativeEcsSlot {
                    offset: 1016,
                    byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                },
            }],
            column_rows: [
                NativeStorageCatalogColumnRowSlots {
                    component_id: NativeEcsSlot {
                        offset: 1024,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    element_size: NativeEcsSlot {
                        offset: 1032,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    element_align: NativeEcsSlot {
                        offset: 1040,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    payload_base_address: NativeEcsSlot {
                        offset: 1048,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                },
                NativeStorageCatalogColumnRowSlots {
                    component_id: NativeEcsSlot {
                        offset: 1056,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    element_size: NativeEcsSlot {
                        offset: 1064,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    element_align: NativeEcsSlot {
                        offset: 1072,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                    payload_base_address: NativeEcsSlot {
                        offset: 1080,
                        byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
                    },
                },
            ],
        },
    };

#[allow(dead_code)]
const NATIVE_ECS_TABLE_MODEL: NativeEcsTableModel = NativeEcsTableModel {
    descriptors: NativeDescriptorTableModel {
        component_rows: [
            NativeXyDescriptorTableRow {
                slots: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                    .component_resource_descriptors
                    .position,
                name: NATIVE_ECS_EXECUTION_STATE_LAYOUT.descriptor_names.position,
            },
            NativeXyDescriptorTableRow {
                slots: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                    .component_resource_descriptors
                    .velocity,
                name: NATIVE_ECS_EXECUTION_STATE_LAYOUT.descriptor_names.velocity,
            },
        ],
        resource_rows: [NativeTimeDescriptorTableRow {
            slots: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                .component_resource_descriptors
                .time,
            name: NATIVE_ECS_EXECUTION_STATE_LAYOUT.descriptor_names.time,
        }],
        system_rows: [NativeMoveSystemDescriptorTableRow {
            slots: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                .system_query_schedule_descriptors
                .move_system,
            name: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                .descriptor_names
                .move_system,
        }],
        query_rows: [NativeMoversQueryDescriptorTableRow {
            slots: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                .system_query_schedule_descriptors
                .movers_query,
            name: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                .descriptor_names
                .movers_query,
        }],
        schedule_rows: [NativeMainScheduleDescriptorTableRow {
            slots: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                .system_query_schedule_descriptors
                .main_schedule,
            name: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                .descriptor_names
                .main_schedule,
        }],
    },
    startup_operations: NativeStartupOperationTableModel {
        resource_payload_rows: [NATIVE_ECS_EXECUTION_STATE_LAYOUT
            .startup_operations
            .resource],
        spawn_rows: NATIVE_ECS_EXECUTION_STATE_LAYOUT
            .startup_operations
            .spawn_rows,
        run_schedule_rows: [NATIVE_ECS_EXECUTION_STATE_LAYOUT
            .startup_operations
            .run_schedule],
    },
    compiled_schedules: NativeCompiledScheduleTableModel {
        rows: [NATIVE_ECS_EXECUTION_STATE_LAYOUT.compiled_schedule],
    },
    query_plans: NativeQueryPlanTableModel {
        rows: [NATIVE_ECS_EXECUTION_STATE_LAYOUT.descriptor_backed_query_plan],
    },
    archetype_storage: NATIVE_ECS_EXECUTION_STATE_LAYOUT.archetype_storage,
    storage_catalog: NativeStorageCatalogModel {
        table_rows: [NativeStorageCatalogTableRow {
            slots: NATIVE_ECS_EXECUTION_STATE_LAYOUT.storage_catalog.table_rows[0],
            storage: NATIVE_ECS_EXECUTION_STATE_LAYOUT.archetype_storage,
            columns: [
                NativeStorageCatalogColumnRow {
                    slots: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                        .storage_catalog
                        .column_rows[0],
                    descriptor: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                        .component_resource_descriptors
                        .position,
                    payload_column: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                        .archetype_storage
                        .position_column,
                },
                NativeStorageCatalogColumnRow {
                    slots: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                        .storage_catalog
                        .column_rows[1],
                    descriptor: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                        .component_resource_descriptors
                        .velocity,
                    payload_column: NATIVE_ECS_EXECUTION_STATE_LAYOUT
                        .archetype_storage
                        .velocity_column,
                },
            ],
        }],
    },
};

#[allow(dead_code)]
const NATIVE_ECS_TABLE_ITERATION_CURSORS: NativeEcsTableIterationCursorModel =
    NativeEcsTableIterationCursorModel {
        component_descriptors: NativeTableIterationCursor {
            table: NativeTableIterationKind::ComponentDescriptors,
            expected_row_count: 2,
            count_slot: Some(
                NATIVE_ECS_EXECUTION_STATE_LAYOUT
                    .descriptor_counts
                    .components,
            ),
            rows: [
                NativeTableIterationRow {
                    row_kind: NativeTableIterationRowKind::ComponentDescriptor,
                    row_index: 0,
                    primary_slot: NATIVE_ECS_TABLE_MODEL.descriptors.component_rows[0]
                        .slots
                        .id,
                },
                NativeTableIterationRow {
                    row_kind: NativeTableIterationRowKind::ComponentDescriptor,
                    row_index: 1,
                    primary_slot: NATIVE_ECS_TABLE_MODEL.descriptors.component_rows[1]
                        .slots
                        .id,
                },
            ],
        },
        resource_descriptors: NativeTableIterationCursor {
            table: NativeTableIterationKind::ResourceDescriptors,
            expected_row_count: 1,
            count_slot: Some(
                NATIVE_ECS_EXECUTION_STATE_LAYOUT
                    .descriptor_counts
                    .resources,
            ),
            rows: [NativeTableIterationRow {
                row_kind: NativeTableIterationRowKind::ResourceDescriptor,
                row_index: 0,
                primary_slot: NATIVE_ECS_TABLE_MODEL.descriptors.resource_rows[0].slots.id,
            }],
        },
        system_descriptors: NativeTableIterationCursor {
            table: NativeTableIterationKind::SystemDescriptors,
            expected_row_count: 1,
            count_slot: Some(NATIVE_ECS_EXECUTION_STATE_LAYOUT.descriptor_counts.systems),
            rows: [NativeTableIterationRow {
                row_kind: NativeTableIterationRowKind::SystemDescriptor,
                row_index: 0,
                primary_slot: NATIVE_ECS_TABLE_MODEL.descriptors.system_rows[0].slots.id,
            }],
        },
        query_descriptors: NativeTableIterationCursor {
            table: NativeTableIterationKind::QueryDescriptors,
            expected_row_count: 1,
            count_slot: Some(NATIVE_ECS_EXECUTION_STATE_LAYOUT.descriptor_counts.queries),
            rows: [NativeTableIterationRow {
                row_kind: NativeTableIterationRowKind::QueryDescriptor,
                row_index: 0,
                primary_slot: NATIVE_ECS_TABLE_MODEL.descriptors.query_rows[0].slots.id,
            }],
        },
        schedule_descriptors: NativeTableIterationCursor {
            table: NativeTableIterationKind::ScheduleDescriptors,
            expected_row_count: 1,
            count_slot: Some(
                NATIVE_ECS_EXECUTION_STATE_LAYOUT
                    .descriptor_counts
                    .schedules,
            ),
            rows: [NativeTableIterationRow {
                row_kind: NativeTableIterationRowKind::ScheduleDescriptor,
                row_index: 0,
                primary_slot: NATIVE_ECS_TABLE_MODEL.descriptors.schedule_rows[0].slots.id,
            }],
        },
        startup_operations: NativeTableIterationCursor {
            table: NativeTableIterationKind::StartupOperations,
            expected_row_count: 4,
            count_slot: Some(
                NATIVE_ECS_EXECUTION_STATE_LAYOUT
                    .startup_dispatch
                    .operation_count,
            ),
            rows: [
                NativeTableIterationRow {
                    row_kind: NativeTableIterationRowKind::StartupResourcePayload,
                    row_index: 0,
                    primary_slot: NATIVE_ECS_TABLE_MODEL
                        .startup_operations
                        .resource_payload_rows[0]
                        .kind,
                },
                NativeTableIterationRow {
                    row_kind: NativeTableIterationRowKind::StartupSpawn,
                    row_index: 1,
                    primary_slot: NATIVE_ECS_TABLE_MODEL.startup_operations.spawn_rows[0].kind,
                },
                NativeTableIterationRow {
                    row_kind: NativeTableIterationRowKind::StartupRunSchedule,
                    row_index: 2,
                    primary_slot: NATIVE_ECS_TABLE_MODEL.startup_operations.run_schedule_rows[0]
                        .kind,
                },
                NativeTableIterationRow {
                    row_kind: NativeTableIterationRowKind::StartupSpawn,
                    row_index: 3,
                    primary_slot: NATIVE_ECS_TABLE_MODEL.startup_operations.spawn_rows[1].kind,
                },
            ],
        },
        compiled_schedules: NativeTableIterationCursor {
            table: NativeTableIterationKind::CompiledSchedules,
            expected_row_count: 1,
            count_slot: None,
            rows: [NativeTableIterationRow {
                row_kind: NativeTableIterationRowKind::CompiledSchedule,
                row_index: 0,
                primary_slot: NATIVE_ECS_TABLE_MODEL.compiled_schedules.rows[0].schedule_id,
            }],
        },
        query_plans: NativeTableIterationCursor {
            table: NativeTableIterationKind::QueryPlans,
            expected_row_count: 1,
            count_slot: None,
            rows: [NativeTableIterationRow {
                row_kind: NativeTableIterationRowKind::QueryPlan,
                row_index: 0,
                primary_slot: NATIVE_ECS_TABLE_MODEL.query_plans.rows[0].query_id,
            }],
        },
    };

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodegenError {
    pub message: String,
}

const ECS_METADATA_ENVELOPE_SIZE: usize = 112;
const ECS_METADATA_FAILURE_EXIT_CODE: u8 = 16;
const ECS_STARTUP_STATE_FAILURE_EXIT_CODE: u8 = 17;
const ECS_QUERY_LOOP_SCAN_FAILURE_EXIT_CODE: u8 = 18;
const ECS_QUERY_LOOP_FIELD_MATH_FAILURE_EXIT_CODE: u8 = 19;
const ECS_QUERY_LOOP_POSITION_STORE_FAILURE_EXIT_CODE: u8 = 20;
const ECS_RUN_SCHEDULE_DISPATCH_FAILURE_EXIT_CODE: u8 = 21;
const ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE: u8 = 47;
const ECS_STARTUP_SECTION_DIRECTORY_OFFSET: usize = 16 + 5 * 16;
const ECS_SECTION_OFFSET_FIELD_OFFSET: usize = 4;
const ECS_SECTION_RECORD_COUNT_FIELD_OFFSET: usize = 12;
const ECS_STARTUP_OP_RESOURCE_PAYLOAD: u32 = 1;
const ECS_STARTUP_OP_SPAWN: u32 = 2;
const ECS_STARTUP_OP_RUN_SCHEDULE: u32 = 3;
const ECS_STARTUP_RECORD_COUNT_OFFSET: u8 =
    (ECS_STARTUP_SECTION_DIRECTORY_OFFSET + ECS_SECTION_RECORD_COUNT_FIELD_OFFSET) as u8;
const ECS_EXPECTED_DESCRIPTOR_COUNTS: [u64; 5] = [2, 1, 1, 1, 1];
const ECS_DESCRIPTOR_RECORD_COUNT_OFFSETS: [u8; 5] = [28, 44, 60, 76, 92];
const ECS_DESCRIPTOR_RECORD_OFFSET_FIELD_OFFSETS: [u8; 5] = [20, 36, 52, 68, 84];
const ECS_DESCRIPTOR_RECORD_BYTE_LEN_FIELD_OFFSETS: [u8; 5] = [24, 40, 56, 72, 88];
const ECS_EXPECTED_DESCRIPTOR_RECORD_OFFSETS: [u64; 5] = [112, 250, 303, 437, 527];
const ECS_EXPECTED_DESCRIPTOR_RECORD_BYTE_LENS: [u64; 5] = [138, 53, 134, 90, 50];
const ECS_DESCRIPTOR_REGISTRY_SLOTS: [u16; 5] = [
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_counts
        .components
        .offset,
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_counts
        .resources
        .offset,
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_counts
        .systems
        .offset,
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_counts
        .queries
        .offset,
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_counts
        .schedules
        .offset,
];
const ECS_DESCRIPTOR_RECORD_OFFSET_SLOTS: [u16; 5] = [
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_records
        .components
        .payload_offset
        .offset,
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_records
        .resources
        .payload_offset
        .offset,
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_records
        .systems
        .payload_offset
        .offset,
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_records
        .queries
        .payload_offset
        .offset,
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_records
        .schedules
        .payload_offset
        .offset,
];
const ECS_DESCRIPTOR_RECORD_BYTE_LEN_SLOTS: [u16; 5] = [
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_records
        .components
        .byte_len
        .offset,
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_records
        .resources
        .byte_len
        .offset,
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_records
        .systems
        .byte_len
        .offset,
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_records
        .queries
        .byte_len
        .offset,
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_records
        .schedules
        .byte_len
        .offset,
];
const ECS_RESOURCE_PAYLOAD_STORAGE_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_state
    .time_payload
    .offset;
const ECS_SPAWN_ROW_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_state
    .row_count
    .offset;
#[cfg(test)]
const ECS_POSITION_PAYLOAD_STORAGE_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_state
    .spawn_payload_rows[0]
    .position_payload
    .offset;
#[cfg(test)]
const ECS_VELOCITY_PAYLOAD_STORAGE_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_state
    .spawn_payload_rows[0]
    .velocity_payload
    .offset;
#[cfg(test)]
const ECS_SECOND_POSITION_PAYLOAD_STORAGE_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_state
    .spawn_payload_rows[1]
    .position_payload
    .offset;
#[cfg(test)]
const ECS_SECOND_VELOCITY_PAYLOAD_STORAGE_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_state
    .spawn_payload_rows[1]
    .velocity_payload
    .offset;
#[cfg(test)]
const ECS_ARCHETYPE_STORAGE_ROW_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .archetype_storage
    .row_count
    .offset;
#[cfg(test)]
const ECS_ARCHETYPE_STORAGE_CAPACITY_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .archetype_storage
    .capacity
    .offset;
#[cfg(test)]
const ECS_ARCHETYPE_STORAGE_ROW_STRIDE_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .archetype_storage
    .row_stride
    .offset;
#[cfg(test)]
const ECS_ARCHETYPE_STORAGE_POSITION_ROW0_PAYLOAD_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .archetype_storage
    .position_column
    .payload_rows[0]
    .offset;
#[cfg(test)]
const ECS_ARCHETYPE_STORAGE_POSITION_ROW1_PAYLOAD_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .archetype_storage
    .position_column
    .payload_rows[1]
    .offset;
#[cfg(test)]
const ECS_ARCHETYPE_STORAGE_VELOCITY_ROW0_PAYLOAD_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .archetype_storage
    .velocity_column
    .payload_rows[0]
    .offset;
#[cfg(test)]
const ECS_ARCHETYPE_STORAGE_VELOCITY_ROW1_PAYLOAD_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .archetype_storage
    .velocity_column
    .payload_rows[1]
    .offset;
#[cfg(test)]
const ECS_ARCHETYPE_STORAGE_CAPACITY: u64 = 2;
#[cfg(test)]
const ECS_ARCHETYPE_STORAGE_ROW_STRIDE: u64 = 16;
const ECS_QUERY_LOOP_TARGET_POSITION_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .compiled_move
    .target_position_payload
    .offset;
const ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .compiled_move
    .scanned_row_count
    .offset;
const ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .compiled_move
    .field_product_payload
    .offset;
const ECS_STARTUP_OPERATION_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_dispatch
    .operation_count
    .offset;
const ECS_STARTUP_RESOURCE_DISPATCH_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_dispatch
    .resource_dispatch_count
    .offset;
const ECS_STARTUP_SPAWN_DISPATCH_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_dispatch
    .spawn_dispatch_count
    .offset;
const ECS_STARTUP_RUN_SCHEDULE_DISPATCH_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_dispatch
    .run_schedule_dispatch_count
    .offset;
const ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .query_plan
    .matched_row_count
    .offset;
const ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .query_plan
    .position_payload_address
    .offset;
const ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .query_plan
    .velocity_payload_address
    .offset;
const ECS_COMPILED_SCHEDULE_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .compiled_schedule
    .schedule_id
    .offset;
const ECS_COMPILED_SCHEDULED_SYSTEM_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .compiled_schedule
    .scheduled_system_id
    .offset;
const ECS_COMPILED_SCHEDULED_SYSTEM_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .compiled_schedule
    .scheduled_system_count
    .offset;
const ECS_POSITION_DESCRIPTOR_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .position
    .id
    .offset;
const ECS_POSITION_DESCRIPTOR_SIZE_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .position
    .size
    .offset;
const ECS_POSITION_DESCRIPTOR_ALIGN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .position
    .align
    .offset;
const ECS_POSITION_DESCRIPTOR_FIELD_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .position
    .field_count
    .offset;
const ECS_POSITION_DESCRIPTOR_X_FIELD_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .position
    .x_field_offset
    .offset;
const ECS_POSITION_DESCRIPTOR_Y_FIELD_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .position
    .y_field_offset
    .offset;
const ECS_VELOCITY_DESCRIPTOR_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .velocity
    .id
    .offset;
const ECS_VELOCITY_DESCRIPTOR_SIZE_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .velocity
    .size
    .offset;
const ECS_VELOCITY_DESCRIPTOR_ALIGN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .velocity
    .align
    .offset;
const ECS_VELOCITY_DESCRIPTOR_FIELD_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .velocity
    .field_count
    .offset;
const ECS_VELOCITY_DESCRIPTOR_X_FIELD_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .velocity
    .x_field_offset
    .offset;
const ECS_VELOCITY_DESCRIPTOR_Y_FIELD_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .velocity
    .y_field_offset
    .offset;
const ECS_TIME_DESCRIPTOR_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .time
    .id
    .offset;
const ECS_TIME_DESCRIPTOR_SIZE_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .time
    .size
    .offset;
const ECS_TIME_DESCRIPTOR_ALIGN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .time
    .align
    .offset;
const ECS_TIME_DESCRIPTOR_FIELD_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .time
    .field_count
    .offset;
const ECS_TIME_DESCRIPTOR_DELTA_FIELD_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .component_resource_descriptors
    .time
    .delta_field_offset
    .offset;
const ECS_MOVE_SYSTEM_DESCRIPTOR_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .move_system
    .id
    .offset;
const ECS_MOVE_SYSTEM_DESCRIPTOR_PARAM_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .move_system
    .param_count
    .offset;
const ECS_MOVE_SYSTEM_RESOURCE_PARAM_KIND_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .move_system
    .resource_param_kind
    .offset;
const ECS_MOVE_SYSTEM_RESOURCE_PARAM_RESOURCE_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .move_system
    .resource_param_resource_id
    .offset;
const ECS_MOVE_SYSTEM_QUERY_PARAM_KIND_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .move_system
    .query_param_kind
    .offset;
const ECS_MOVE_SYSTEM_QUERY_PARAM_TERM_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .move_system
    .query_param_term_count
    .offset;
const ECS_MOVE_SYSTEM_QUERY_TERM0_ACCESS_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .move_system
    .query_term0_access
    .offset;
const ECS_MOVE_SYSTEM_QUERY_TERM0_COMPONENT_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .move_system
    .query_term0_component_id
    .offset;
const ECS_MOVE_SYSTEM_QUERY_TERM1_ACCESS_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .move_system
    .query_term1_access
    .offset;
const ECS_MOVE_SYSTEM_QUERY_TERM1_COMPONENT_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .move_system
    .query_term1_component_id
    .offset;
const ECS_MOVERS_QUERY_DESCRIPTOR_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .movers_query
    .id
    .offset;
const ECS_MOVERS_QUERY_DESCRIPTOR_TERM_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .movers_query
    .term_count
    .offset;
const ECS_MOVERS_QUERY_TERM0_ACCESS_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .movers_query
    .term0_access
    .offset;
const ECS_MOVERS_QUERY_TERM0_COMPONENT_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .movers_query
    .term0_component_id
    .offset;
const ECS_MOVERS_QUERY_TERM1_ACCESS_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .movers_query
    .term1_access
    .offset;
const ECS_MOVERS_QUERY_TERM1_COMPONENT_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .movers_query
    .term1_component_id
    .offset;
const ECS_MAIN_SCHEDULE_DESCRIPTOR_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .main_schedule
    .id
    .offset;
const ECS_MAIN_SCHEDULE_DESCRIPTOR_ITEM_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .main_schedule
    .item_count
    .offset;
const ECS_MAIN_SCHEDULE_RUN_ITEM_KIND_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .main_schedule
    .run_item_kind
    .offset;
const ECS_MAIN_SCHEDULE_RUN_SYSTEM_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .system_query_schedule_descriptors
    .main_schedule
    .run_system_id
    .offset;
const ECS_STARTUP_TABLE_RESOURCE_KIND_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .resource
    .kind
    .offset;
const ECS_STARTUP_TABLE_RESOURCE_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .resource
    .resource_id
    .offset;
const ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .resource
    .payload_offset
    .offset;
const ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_LEN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .resource
    .payload_len
    .offset;
const ECS_STARTUP_TABLE_SPAWN_KIND_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[0]
    .kind
    .offset;
#[cfg(test)]
const ECS_STARTUP_TABLE_SPAWN_COMPONENT_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[0]
    .component_count
    .offset;
#[cfg(test)]
const ECS_STARTUP_TABLE_POSITION_COMPONENT_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[0]
    .position_component_id
    .offset;
#[cfg(test)]
const ECS_STARTUP_TABLE_POSITION_PAYLOAD_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[0]
    .position_payload_offset
    .offset;
#[cfg(test)]
const ECS_STARTUP_TABLE_POSITION_PAYLOAD_LEN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[0]
    .position_payload_len
    .offset;
#[cfg(test)]
const ECS_STARTUP_TABLE_VELOCITY_COMPONENT_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[0]
    .velocity_component_id
    .offset;
#[cfg(test)]
const ECS_STARTUP_TABLE_VELOCITY_PAYLOAD_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[0]
    .velocity_payload_offset
    .offset;
#[cfg(test)]
const ECS_STARTUP_TABLE_VELOCITY_PAYLOAD_LEN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[0]
    .velocity_payload_len
    .offset;
const ECS_SECOND_STARTUP_TABLE_SPAWN_KIND_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[1]
    .kind
    .offset;
#[cfg(test)]
const ECS_SECOND_STARTUP_TABLE_SPAWN_COMPONENT_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[1]
    .component_count
    .offset;
#[cfg(test)]
const ECS_SECOND_STARTUP_TABLE_POSITION_COMPONENT_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[1]
    .position_component_id
    .offset;
#[cfg(test)]
const ECS_SECOND_STARTUP_TABLE_POSITION_PAYLOAD_OFFSET_SLOT: u16 =
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .startup_operations
        .spawn_rows[1]
        .position_payload_offset
        .offset;
#[cfg(test)]
const ECS_SECOND_STARTUP_TABLE_POSITION_PAYLOAD_LEN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[1]
    .position_payload_len
    .offset;
#[cfg(test)]
const ECS_SECOND_STARTUP_TABLE_VELOCITY_COMPONENT_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[1]
    .velocity_component_id
    .offset;
#[cfg(test)]
const ECS_SECOND_STARTUP_TABLE_VELOCITY_PAYLOAD_OFFSET_SLOT: u16 =
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .startup_operations
        .spawn_rows[1]
        .velocity_payload_offset
        .offset;
#[cfg(test)]
const ECS_SECOND_STARTUP_TABLE_VELOCITY_PAYLOAD_LEN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .spawn_rows[1]
    .velocity_payload_len
    .offset;
const ECS_STARTUP_TABLE_RUN_SCHEDULE_KIND_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .run_schedule
    .kind
    .offset;
const ECS_STARTUP_TABLE_RUN_SCHEDULE_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_operations
    .run_schedule
    .schedule_id
    .offset;
const ECS_DESCRIPTOR_QUERY_PLAN_QUERY_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_backed_query_plan
    .query_id
    .offset;
const ECS_DESCRIPTOR_QUERY_PLAN_TERM_COUNT_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_backed_query_plan
    .term_count
    .offset;
const ECS_DESCRIPTOR_QUERY_PLAN_POSITION_ACCESS_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_backed_query_plan
    .position
    .access
    .offset;
const ECS_DESCRIPTOR_QUERY_PLAN_POSITION_COMPONENT_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_backed_query_plan
    .position
    .component_id
    .offset;
const ECS_DESCRIPTOR_QUERY_PLAN_POSITION_SIZE_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_backed_query_plan
    .position
    .size
    .offset;
const ECS_DESCRIPTOR_QUERY_PLAN_POSITION_X_FIELD_OFFSET_SLOT: u16 =
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_backed_query_plan
        .position
        .x_field_offset
        .offset;
const ECS_DESCRIPTOR_QUERY_PLAN_POSITION_Y_FIELD_OFFSET_SLOT: u16 =
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_backed_query_plan
        .position
        .y_field_offset
        .offset;
const ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_ACCESS_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_backed_query_plan
    .velocity
    .access
    .offset;
const ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_COMPONENT_ID_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_backed_query_plan
    .velocity
    .component_id
    .offset;
const ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_SIZE_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_backed_query_plan
    .velocity
    .size
    .offset;
const ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_X_FIELD_OFFSET_SLOT: u16 =
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_backed_query_plan
        .velocity
        .x_field_offset
        .offset;
const ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_Y_FIELD_OFFSET_SLOT: u16 =
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .descriptor_backed_query_plan
        .velocity
        .y_field_offset
        .offset;
const ECS_POSITION_DESCRIPTOR_NAME_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_names
    .position
    .byte_offset
    .offset;
const ECS_POSITION_DESCRIPTOR_NAME_LEN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_names
    .position
    .byte_len
    .offset;
const ECS_VELOCITY_DESCRIPTOR_NAME_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_names
    .velocity
    .byte_offset
    .offset;
const ECS_VELOCITY_DESCRIPTOR_NAME_LEN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_names
    .velocity
    .byte_len
    .offset;
const ECS_TIME_DESCRIPTOR_NAME_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_names
    .time
    .byte_offset
    .offset;
const ECS_TIME_DESCRIPTOR_NAME_LEN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_names
    .time
    .byte_len
    .offset;
const ECS_MOVE_SYSTEM_DESCRIPTOR_NAME_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_names
    .move_system
    .byte_offset
    .offset;
const ECS_MOVE_SYSTEM_DESCRIPTOR_NAME_LEN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_names
    .move_system
    .byte_len
    .offset;
const ECS_MOVERS_QUERY_DESCRIPTOR_NAME_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_names
    .movers_query
    .byte_offset
    .offset;
const ECS_MOVERS_QUERY_DESCRIPTOR_NAME_LEN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_names
    .movers_query
    .byte_len
    .offset;
const ECS_MAIN_SCHEDULE_DESCRIPTOR_NAME_OFFSET_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_names
    .main_schedule
    .byte_offset
    .offset;
const ECS_MAIN_SCHEDULE_DESCRIPTOR_NAME_LEN_SLOT: u16 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .descriptor_names
    .main_schedule
    .byte_len
    .offset;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeDescriptorNameReference {
    name: &'static str,
    byte_len_offset: i32,
    byte_offset: u64,
    byte_offset_slot: u16,
    byte_len_slot: u16,
}

const ECS_DESCRIPTOR_NAME_REFERENCES: [NativeDescriptorNameReference; 6] = [
    NativeDescriptorNameReference {
        name: "Demo.Position",
        byte_len_offset: 120,
        byte_offset: 124,
        byte_offset_slot: ECS_POSITION_DESCRIPTOR_NAME_OFFSET_SLOT,
        byte_len_slot: ECS_POSITION_DESCRIPTOR_NAME_LEN_SLOT,
    },
    NativeDescriptorNameReference {
        name: "Demo.Velocity",
        byte_len_offset: 189,
        byte_offset: 193,
        byte_offset_slot: ECS_VELOCITY_DESCRIPTOR_NAME_OFFSET_SLOT,
        byte_len_slot: ECS_VELOCITY_DESCRIPTOR_NAME_LEN_SLOT,
    },
    NativeDescriptorNameReference {
        name: "Demo.Time",
        byte_len_offset: 258,
        byte_offset: 262,
        byte_offset_slot: ECS_TIME_DESCRIPTOR_NAME_OFFSET_SLOT,
        byte_len_slot: ECS_TIME_DESCRIPTOR_NAME_LEN_SLOT,
    },
    NativeDescriptorNameReference {
        name: "Demo.Move",
        byte_len_offset: 311,
        byte_offset: 315,
        byte_offset_slot: ECS_MOVE_SYSTEM_DESCRIPTOR_NAME_OFFSET_SLOT,
        byte_len_slot: ECS_MOVE_SYSTEM_DESCRIPTOR_NAME_LEN_SLOT,
    },
    NativeDescriptorNameReference {
        name: "Demo.Move.movers",
        byte_len_offset: 445,
        byte_offset: 449,
        byte_offset_slot: ECS_MOVERS_QUERY_DESCRIPTOR_NAME_OFFSET_SLOT,
        byte_len_slot: ECS_MOVERS_QUERY_DESCRIPTOR_NAME_LEN_SLOT,
    },
    NativeDescriptorNameReference {
        name: "Demo.Main",
        byte_len_offset: 535,
        byte_offset: 539,
        byte_offset_slot: ECS_MAIN_SCHEDULE_DESCRIPTOR_NAME_OFFSET_SLOT,
        byte_len_slot: ECS_MAIN_SCHEDULE_DESCRIPTOR_NAME_LEN_SLOT,
    },
];
const ECS_COMPONENT_RESOURCE_DESCRIPTOR_QWORD_LOADS: [(i32, u16); 3] = [
    (112, ECS_POSITION_DESCRIPTOR_ID_SLOT),
    (181, ECS_VELOCITY_DESCRIPTOR_ID_SLOT),
    (250, ECS_TIME_DESCRIPTOR_ID_SLOT),
];
const ECS_COMPONENT_RESOURCE_DESCRIPTOR_DWORD_LOADS: [(i32, u16); 14] = [
    (137, ECS_POSITION_DESCRIPTOR_SIZE_SLOT),
    (141, ECS_POSITION_DESCRIPTOR_ALIGN_SLOT),
    (145, ECS_POSITION_DESCRIPTOR_FIELD_COUNT_SLOT),
    (161, ECS_POSITION_DESCRIPTOR_X_FIELD_OFFSET_SLOT),
    (177, ECS_POSITION_DESCRIPTOR_Y_FIELD_OFFSET_SLOT),
    (206, ECS_VELOCITY_DESCRIPTOR_SIZE_SLOT),
    (210, ECS_VELOCITY_DESCRIPTOR_ALIGN_SLOT),
    (214, ECS_VELOCITY_DESCRIPTOR_FIELD_COUNT_SLOT),
    (230, ECS_VELOCITY_DESCRIPTOR_X_FIELD_OFFSET_SLOT),
    (246, ECS_VELOCITY_DESCRIPTOR_Y_FIELD_OFFSET_SLOT),
    (271, ECS_TIME_DESCRIPTOR_SIZE_SLOT),
    (275, ECS_TIME_DESCRIPTOR_ALIGN_SLOT),
    (279, ECS_TIME_DESCRIPTOR_FIELD_COUNT_SLOT),
    (299, ECS_TIME_DESCRIPTOR_DELTA_FIELD_OFFSET_SLOT),
];
const ECS_COMPONENT_RESOURCE_DESCRIPTOR_EXPECTED: [(u16, u64); 17] = [
    (ECS_POSITION_DESCRIPTOR_ID_SLOT, DEMO_POSITION_COMPONENT_ID),
    (ECS_POSITION_DESCRIPTOR_SIZE_SLOT, 8),
    (ECS_POSITION_DESCRIPTOR_ALIGN_SLOT, 4),
    (ECS_POSITION_DESCRIPTOR_FIELD_COUNT_SLOT, 2),
    (ECS_POSITION_DESCRIPTOR_X_FIELD_OFFSET_SLOT, 0),
    (ECS_POSITION_DESCRIPTOR_Y_FIELD_OFFSET_SLOT, 4),
    (ECS_VELOCITY_DESCRIPTOR_ID_SLOT, DEMO_VELOCITY_COMPONENT_ID),
    (ECS_VELOCITY_DESCRIPTOR_SIZE_SLOT, 8),
    (ECS_VELOCITY_DESCRIPTOR_ALIGN_SLOT, 4),
    (ECS_VELOCITY_DESCRIPTOR_FIELD_COUNT_SLOT, 2),
    (ECS_VELOCITY_DESCRIPTOR_X_FIELD_OFFSET_SLOT, 0),
    (ECS_VELOCITY_DESCRIPTOR_Y_FIELD_OFFSET_SLOT, 4),
    (ECS_TIME_DESCRIPTOR_ID_SLOT, DEMO_TIME_RESOURCE_ID),
    (ECS_TIME_DESCRIPTOR_SIZE_SLOT, 4),
    (ECS_TIME_DESCRIPTOR_ALIGN_SLOT, 4),
    (ECS_TIME_DESCRIPTOR_FIELD_COUNT_SLOT, 1),
    (ECS_TIME_DESCRIPTOR_DELTA_FIELD_OFFSET_SLOT, 0),
];
const ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_QWORD_LOADS: [(i32, u16); 9] = [
    (303, ECS_MOVE_SYSTEM_DESCRIPTOR_ID_SLOT),
    (340, ECS_MOVE_SYSTEM_RESOURCE_PARAM_RESOURCE_ID_SLOT),
    (383, ECS_MOVE_SYSTEM_QUERY_TERM0_COMPONENT_ID_SLOT),
    (412, ECS_MOVE_SYSTEM_QUERY_TERM1_COMPONENT_ID_SLOT),
    (437, ECS_MOVERS_QUERY_DESCRIPTOR_ID_SLOT),
    (473, ECS_MOVERS_QUERY_TERM0_COMPONENT_ID_SLOT),
    (502, ECS_MOVERS_QUERY_TERM1_COMPONENT_ID_SLOT),
    (527, ECS_MAIN_SCHEDULE_DESCRIPTOR_ID_SLOT),
    (556, ECS_MAIN_SCHEDULE_RUN_SYSTEM_ID_SLOT),
];
const ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_DWORD_LOADS: [(i32, u16); 11] = [
    (324, ECS_MOVE_SYSTEM_DESCRIPTOR_PARAM_COUNT_SLOT),
    (336, ECS_MOVE_SYSTEM_RESOURCE_PARAM_KIND_SLOT),
    (371, ECS_MOVE_SYSTEM_QUERY_PARAM_KIND_SLOT),
    (375, ECS_MOVE_SYSTEM_QUERY_PARAM_TERM_COUNT_SLOT),
    (379, ECS_MOVE_SYSTEM_QUERY_TERM0_ACCESS_SLOT),
    (408, ECS_MOVE_SYSTEM_QUERY_TERM1_ACCESS_SLOT),
    (465, ECS_MOVERS_QUERY_DESCRIPTOR_TERM_COUNT_SLOT),
    (469, ECS_MOVERS_QUERY_TERM0_ACCESS_SLOT),
    (498, ECS_MOVERS_QUERY_TERM1_ACCESS_SLOT),
    (548, ECS_MAIN_SCHEDULE_DESCRIPTOR_ITEM_COUNT_SLOT),
    (552, ECS_MAIN_SCHEDULE_RUN_ITEM_KIND_SLOT),
];
const ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_EXPECTED: [(u16, u64); 20] = [
    (ECS_MOVE_SYSTEM_DESCRIPTOR_ID_SLOT, DEMO_MOVE_SYSTEM_ID),
    (ECS_MOVE_SYSTEM_DESCRIPTOR_PARAM_COUNT_SLOT, 2),
    (ECS_MOVE_SYSTEM_RESOURCE_PARAM_KIND_SLOT, 1),
    (
        ECS_MOVE_SYSTEM_RESOURCE_PARAM_RESOURCE_ID_SLOT,
        DEMO_TIME_RESOURCE_ID,
    ),
    (ECS_MOVE_SYSTEM_QUERY_PARAM_KIND_SLOT, 2),
    (ECS_MOVE_SYSTEM_QUERY_PARAM_TERM_COUNT_SLOT, 2),
    (ECS_MOVE_SYSTEM_QUERY_TERM0_ACCESS_SLOT, 2),
    (
        ECS_MOVE_SYSTEM_QUERY_TERM0_COMPONENT_ID_SLOT,
        DEMO_POSITION_COMPONENT_ID,
    ),
    (ECS_MOVE_SYSTEM_QUERY_TERM1_ACCESS_SLOT, 1),
    (
        ECS_MOVE_SYSTEM_QUERY_TERM1_COMPONENT_ID_SLOT,
        DEMO_VELOCITY_COMPONENT_ID,
    ),
    (ECS_MOVERS_QUERY_DESCRIPTOR_ID_SLOT, DEMO_MOVERS_QUERY_ID),
    (ECS_MOVERS_QUERY_DESCRIPTOR_TERM_COUNT_SLOT, 2),
    (ECS_MOVERS_QUERY_TERM0_ACCESS_SLOT, 2),
    (
        ECS_MOVERS_QUERY_TERM0_COMPONENT_ID_SLOT,
        DEMO_POSITION_COMPONENT_ID,
    ),
    (ECS_MOVERS_QUERY_TERM1_ACCESS_SLOT, 1),
    (
        ECS_MOVERS_QUERY_TERM1_COMPONENT_ID_SLOT,
        DEMO_VELOCITY_COMPONENT_ID,
    ),
    (ECS_MAIN_SCHEDULE_DESCRIPTOR_ID_SLOT, DEMO_MAIN_SCHEDULE_ID),
    (ECS_MAIN_SCHEDULE_DESCRIPTOR_ITEM_COUNT_SLOT, 1),
    (ECS_MAIN_SCHEDULE_RUN_ITEM_KIND_SLOT, 1),
    (ECS_MAIN_SCHEDULE_RUN_SYSTEM_ID_SLOT, DEMO_MOVE_SYSTEM_ID),
];
const ECS_DESCRIPTOR_TABLE_ITERATION_ROWS: [NativeDescriptorTableIterationRow; 6] = [
    NativeDescriptorTableIterationRow {
        cursor_table: NativeTableIterationKind::ComponentDescriptors,
        cursor_row_index: 0,
        expected_table_count: 2,
        count_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .component_descriptors
            .count_slot
            .unwrap(),
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .component_descriptors
            .rows[0]
            .primary_slot,
        decode_family: NativeDescriptorDecodeFamily::ComponentResource,
        qword_load_start: 0,
        qword_load_len: 1,
        dword_load_start: 0,
        dword_load_len: 5,
    },
    NativeDescriptorTableIterationRow {
        cursor_table: NativeTableIterationKind::ComponentDescriptors,
        cursor_row_index: 1,
        expected_table_count: 2,
        count_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .component_descriptors
            .count_slot
            .unwrap(),
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .component_descriptors
            .rows[1]
            .primary_slot,
        decode_family: NativeDescriptorDecodeFamily::ComponentResource,
        qword_load_start: 1,
        qword_load_len: 1,
        dword_load_start: 5,
        dword_load_len: 5,
    },
    NativeDescriptorTableIterationRow {
        cursor_table: NativeTableIterationKind::ResourceDescriptors,
        cursor_row_index: 0,
        expected_table_count: 1,
        count_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .resource_descriptors
            .count_slot
            .unwrap(),
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS.resource_descriptors.rows[0].primary_slot,
        decode_family: NativeDescriptorDecodeFamily::ComponentResource,
        qword_load_start: 2,
        qword_load_len: 1,
        dword_load_start: 10,
        dword_load_len: 4,
    },
    NativeDescriptorTableIterationRow {
        cursor_table: NativeTableIterationKind::SystemDescriptors,
        cursor_row_index: 0,
        expected_table_count: 1,
        count_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .system_descriptors
            .count_slot
            .unwrap(),
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS.system_descriptors.rows[0].primary_slot,
        decode_family: NativeDescriptorDecodeFamily::SystemQuerySchedule,
        qword_load_start: 0,
        qword_load_len: 4,
        dword_load_start: 0,
        dword_load_len: 6,
    },
    NativeDescriptorTableIterationRow {
        cursor_table: NativeTableIterationKind::QueryDescriptors,
        cursor_row_index: 0,
        expected_table_count: 1,
        count_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .query_descriptors
            .count_slot
            .unwrap(),
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS.query_descriptors.rows[0].primary_slot,
        decode_family: NativeDescriptorDecodeFamily::SystemQuerySchedule,
        qword_load_start: 4,
        qword_load_len: 3,
        dword_load_start: 6,
        dword_load_len: 3,
    },
    NativeDescriptorTableIterationRow {
        cursor_table: NativeTableIterationKind::ScheduleDescriptors,
        cursor_row_index: 0,
        expected_table_count: 1,
        count_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .schedule_descriptors
            .count_slot
            .unwrap(),
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS.schedule_descriptors.rows[0].primary_slot,
        decode_family: NativeDescriptorDecodeFamily::SystemQuerySchedule,
        qword_load_start: 7,
        qword_load_len: 2,
        dword_load_start: 9,
        dword_load_len: 2,
    },
];
#[cfg(test)]
const ECS_STARTUP_OPERATION_TABLE_QWORD_LOADS: [(i32, u16); 4] = [
    (581, ECS_STARTUP_TABLE_RESOURCE_ID_SLOT),
    (618, ECS_STARTUP_TABLE_POSITION_COMPONENT_ID_SLOT),
    (655, ECS_STARTUP_TABLE_VELOCITY_COMPONENT_ID_SLOT),
    (696, ECS_STARTUP_TABLE_RUN_SCHEDULE_ID_SLOT),
];
#[cfg(test)]
const ECS_STARTUP_OPERATION_TABLE_DWORD_LOADS: [(i32, u16); 7] = [
    (577, ECS_STARTUP_TABLE_RESOURCE_KIND_SLOT),
    (602, ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_LEN_SLOT),
    (610, ECS_STARTUP_TABLE_SPAWN_KIND_SLOT),
    (614, ECS_STARTUP_TABLE_SPAWN_COMPONENT_COUNT_SLOT),
    (643, ECS_STARTUP_TABLE_POSITION_PAYLOAD_LEN_SLOT),
    (680, ECS_STARTUP_TABLE_VELOCITY_PAYLOAD_LEN_SLOT),
    (692, ECS_STARTUP_TABLE_RUN_SCHEDULE_KIND_SLOT),
];
#[cfg(test)]
const ECS_STARTUP_OPERATION_TABLE_PAYLOAD_OFFSETS: [(u64, u16); 3] = [
    (606, ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_OFFSET_SLOT),
    (647, ECS_STARTUP_TABLE_POSITION_PAYLOAD_OFFSET_SLOT),
    (684, ECS_STARTUP_TABLE_VELOCITY_PAYLOAD_OFFSET_SLOT),
];
#[cfg(test)]
const ECS_STARTUP_OPERATION_TABLE_EXPECTED: [(u16, u64); 10] = [
    (ECS_STARTUP_TABLE_RESOURCE_ID_SLOT, DEMO_TIME_RESOURCE_ID),
    (ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_OFFSET_SLOT, 606),
    (ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_LEN_SLOT, 4),
    (ECS_STARTUP_TABLE_SPAWN_COMPONENT_COUNT_SLOT, 2),
    (
        ECS_STARTUP_TABLE_POSITION_COMPONENT_ID_SLOT,
        DEMO_POSITION_COMPONENT_ID,
    ),
    (ECS_STARTUP_TABLE_POSITION_PAYLOAD_OFFSET_SLOT, 647),
    (ECS_STARTUP_TABLE_POSITION_PAYLOAD_LEN_SLOT, 8),
    (
        ECS_STARTUP_TABLE_VELOCITY_COMPONENT_ID_SLOT,
        DEMO_VELOCITY_COMPONENT_ID,
    ),
    (ECS_STARTUP_TABLE_VELOCITY_PAYLOAD_OFFSET_SLOT, 684),
    (ECS_STARTUP_TABLE_VELOCITY_PAYLOAD_LEN_SLOT, 8),
];
const ECS_STARTUP_OPERATION_DISPATCH_ROWS: [NativeStartupOperationDispatchRow; 3] = [
    NativeStartupOperationDispatchRow {
        handler: NativeStartupOperationHandler::ResourcePayload,
        expected_kind: ECS_STARTUP_OP_RESOURCE_PAYLOAD,
        kind_slot: ECS_STARTUP_TABLE_RESOURCE_KIND_SLOT,
        dispatch_count_slot: ECS_STARTUP_RESOURCE_DISPATCH_COUNT_SLOT,
        dispatch_count_after_row: 1,
    },
    NativeStartupOperationDispatchRow {
        handler: NativeStartupOperationHandler::Spawn,
        expected_kind: ECS_STARTUP_OP_SPAWN,
        kind_slot: ECS_STARTUP_TABLE_SPAWN_KIND_SLOT,
        dispatch_count_slot: ECS_STARTUP_SPAWN_DISPATCH_COUNT_SLOT,
        dispatch_count_after_row: 1,
    },
    NativeStartupOperationDispatchRow {
        handler: NativeStartupOperationHandler::RunSchedule,
        expected_kind: ECS_STARTUP_OP_RUN_SCHEDULE,
        kind_slot: ECS_STARTUP_TABLE_RUN_SCHEDULE_KIND_SLOT,
        dispatch_count_slot: ECS_STARTUP_RUN_SCHEDULE_DISPATCH_COUNT_SLOT,
        dispatch_count_after_row: 1,
    },
];
const ECS_SECOND_SPAWN_STARTUP_OPERATION_DISPATCH_ROW: NativeStartupOperationDispatchRow =
    NativeStartupOperationDispatchRow {
        handler: NativeStartupOperationHandler::Spawn,
        expected_kind: ECS_STARTUP_OP_SPAWN,
        kind_slot: ECS_SECOND_STARTUP_TABLE_SPAWN_KIND_SLOT,
        dispatch_count_slot: ECS_STARTUP_SPAWN_DISPATCH_COUNT_SLOT,
        dispatch_count_after_row: 2,
    };
const ECS_STARTUP_OPERATION_TABLE_ITERATION_ROWS: [NativeStartupOperationTableIterationRow; 3] = [
    NativeStartupOperationTableIterationRow {
        cursor_table: NativeTableIterationKind::StartupOperations,
        cursor_row_index: 0,
        expected_table_count: 3,
        count_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .startup_operations
            .count_slot
            .unwrap(),
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS.startup_operations.rows[0].primary_slot,
        dispatch_row: ECS_STARTUP_OPERATION_DISPATCH_ROWS[0],
    },
    NativeStartupOperationTableIterationRow {
        cursor_table: NativeTableIterationKind::StartupOperations,
        cursor_row_index: 1,
        expected_table_count: 3,
        count_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .startup_operations
            .count_slot
            .unwrap(),
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS.startup_operations.rows[1].primary_slot,
        dispatch_row: ECS_STARTUP_OPERATION_DISPATCH_ROWS[1],
    },
    NativeStartupOperationTableIterationRow {
        cursor_table: NativeTableIterationKind::StartupOperations,
        cursor_row_index: 2,
        expected_table_count: 3,
        count_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .startup_operations
            .count_slot
            .unwrap(),
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS.startup_operations.rows[2].primary_slot,
        dispatch_row: ECS_STARTUP_OPERATION_DISPATCH_ROWS[2],
    },
];
const ECS_TWO_SPAWN_STARTUP_OPERATION_TABLE_ITERATION_ROWS:
    [NativeStartupOperationTableIterationRow; 4] = [
    NativeStartupOperationTableIterationRow {
        cursor_table: NativeTableIterationKind::StartupOperations,
        cursor_row_index: 0,
        expected_table_count: 4,
        count_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .startup_operations
            .count_slot
            .unwrap(),
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS.startup_operations.rows[0].primary_slot,
        dispatch_row: ECS_STARTUP_OPERATION_DISPATCH_ROWS[0],
    },
    NativeStartupOperationTableIterationRow {
        cursor_table: NativeTableIterationKind::StartupOperations,
        cursor_row_index: 1,
        expected_table_count: 4,
        count_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .startup_operations
            .count_slot
            .unwrap(),
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS.startup_operations.rows[1].primary_slot,
        dispatch_row: ECS_STARTUP_OPERATION_DISPATCH_ROWS[1],
    },
    NativeStartupOperationTableIterationRow {
        cursor_table: NativeTableIterationKind::StartupOperations,
        cursor_row_index: 3,
        expected_table_count: 4,
        count_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .startup_operations
            .count_slot
            .unwrap(),
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS.startup_operations.rows[3].primary_slot,
        dispatch_row: ECS_SECOND_SPAWN_STARTUP_OPERATION_DISPATCH_ROW,
    },
    NativeStartupOperationTableIterationRow {
        cursor_table: NativeTableIterationKind::StartupOperations,
        cursor_row_index: 2,
        expected_table_count: 4,
        count_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS
            .startup_operations
            .count_slot
            .unwrap(),
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS.startup_operations.rows[2].primary_slot,
        dispatch_row: ECS_STARTUP_OPERATION_DISPATCH_ROWS[2],
    },
];
const ECS_RESOURCE_STARTUP_DESCRIPTOR_RELATIONS: [(u16, u16); 2] = [
    (
        ECS_STARTUP_TABLE_RESOURCE_ID_SLOT,
        ECS_TIME_DESCRIPTOR_ID_SLOT,
    ),
    (
        ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_LEN_SLOT,
        ECS_TIME_DESCRIPTOR_SIZE_SLOT,
    ),
];
const ECS_QUERY_PLAN_BUILD_ROWS: [NativeQueryPlanBuildRow; 1] = [NativeQueryPlanBuildRow {
    query_id_slot: ECS_MOVERS_QUERY_DESCRIPTOR_ID_SLOT,
    query_term_count_slot: ECS_MOVERS_QUERY_DESCRIPTOR_TERM_COUNT_SLOT,
    system_query_term_count_slot: ECS_MOVE_SYSTEM_QUERY_PARAM_TERM_COUNT_SLOT,
    catalog_column_count_slot: NATIVE_ECS_TABLE_MODEL.storage_catalog.table_rows[0]
        .slots
        .column_count
        .offset,
    catalog_row_count_address_slot: NATIVE_ECS_TABLE_MODEL.storage_catalog.table_rows[0]
        .slots
        .row_count_address
        .offset,
    plan_query_id_slot: ECS_DESCRIPTOR_QUERY_PLAN_QUERY_ID_SLOT,
    plan_term_count_slot: ECS_DESCRIPTOR_QUERY_PLAN_TERM_COUNT_SLOT,
    matched_row_count_slot: ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT,
    terms: [
        NativeQueryPlanTermBuildRow {
            role: NativeQueryPlanTermRole::Position,
            query_access_slot: ECS_MOVERS_QUERY_TERM0_ACCESS_SLOT,
            query_component_id_slot: ECS_MOVERS_QUERY_TERM0_COMPONENT_ID_SLOT,
            system_access_slot: ECS_MOVE_SYSTEM_QUERY_TERM0_ACCESS_SLOT,
            system_component_id_slot: ECS_MOVE_SYSTEM_QUERY_TERM0_COMPONENT_ID_SLOT,
            component_descriptor_id_slot: ECS_POSITION_DESCRIPTOR_ID_SLOT,
            component_size_slot: ECS_POSITION_DESCRIPTOR_SIZE_SLOT,
            component_x_field_offset_slot: ECS_POSITION_DESCRIPTOR_X_FIELD_OFFSET_SLOT,
            component_y_field_offset_slot: ECS_POSITION_DESCRIPTOR_Y_FIELD_OFFSET_SLOT,
            catalog_component_id_slot: NATIVE_ECS_TABLE_MODEL.storage_catalog.table_rows[0].columns
                [0]
            .slots
            .component_id
            .offset,
            catalog_element_size_slot: NATIVE_ECS_TABLE_MODEL.storage_catalog.table_rows[0].columns
                [0]
            .slots
            .element_size
            .offset,
            catalog_payload_base_address_slot: NATIVE_ECS_TABLE_MODEL.storage_catalog.table_rows[0]
                .columns[0]
                .slots
                .payload_base_address
                .offset,
            plan_access_slot: ECS_DESCRIPTOR_QUERY_PLAN_POSITION_ACCESS_SLOT,
            plan_component_id_slot: ECS_DESCRIPTOR_QUERY_PLAN_POSITION_COMPONENT_ID_SLOT,
            plan_size_slot: ECS_DESCRIPTOR_QUERY_PLAN_POSITION_SIZE_SLOT,
            plan_x_field_offset_slot: ECS_DESCRIPTOR_QUERY_PLAN_POSITION_X_FIELD_OFFSET_SLOT,
            plan_y_field_offset_slot: ECS_DESCRIPTOR_QUERY_PLAN_POSITION_Y_FIELD_OFFSET_SLOT,
            planned_payload_address_slot: ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
            expected_access: 2,
            expected_size: NATIVE_ECS_QWORD_BYTE_LEN as u64,
            expected_x_field_offset: 0,
            expected_y_field_offset: 4,
        },
        NativeQueryPlanTermBuildRow {
            role: NativeQueryPlanTermRole::Velocity,
            query_access_slot: ECS_MOVERS_QUERY_TERM1_ACCESS_SLOT,
            query_component_id_slot: ECS_MOVERS_QUERY_TERM1_COMPONENT_ID_SLOT,
            system_access_slot: ECS_MOVE_SYSTEM_QUERY_TERM1_ACCESS_SLOT,
            system_component_id_slot: ECS_MOVE_SYSTEM_QUERY_TERM1_COMPONENT_ID_SLOT,
            component_descriptor_id_slot: ECS_VELOCITY_DESCRIPTOR_ID_SLOT,
            component_size_slot: ECS_VELOCITY_DESCRIPTOR_SIZE_SLOT,
            component_x_field_offset_slot: ECS_VELOCITY_DESCRIPTOR_X_FIELD_OFFSET_SLOT,
            component_y_field_offset_slot: ECS_VELOCITY_DESCRIPTOR_Y_FIELD_OFFSET_SLOT,
            catalog_component_id_slot: NATIVE_ECS_TABLE_MODEL.storage_catalog.table_rows[0].columns
                [1]
            .slots
            .component_id
            .offset,
            catalog_element_size_slot: NATIVE_ECS_TABLE_MODEL.storage_catalog.table_rows[0].columns
                [1]
            .slots
            .element_size
            .offset,
            catalog_payload_base_address_slot: NATIVE_ECS_TABLE_MODEL.storage_catalog.table_rows[0]
                .columns[1]
                .slots
                .payload_base_address
                .offset,
            plan_access_slot: ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_ACCESS_SLOT,
            plan_component_id_slot: ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_COMPONENT_ID_SLOT,
            plan_size_slot: ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_SIZE_SLOT,
            plan_x_field_offset_slot: ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_X_FIELD_OFFSET_SLOT,
            plan_y_field_offset_slot: ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_Y_FIELD_OFFSET_SLOT,
            planned_payload_address_slot: ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
            expected_access: 1,
            expected_size: NATIVE_ECS_QWORD_BYTE_LEN as u64,
            expected_x_field_offset: 0,
            expected_y_field_offset: 4,
        },
    ],
}];
const ECS_QUERY_PLAN_TABLE_ITERATION_ROWS: [NativeQueryPlanTableIterationRow; 1] =
    [NativeQueryPlanTableIterationRow {
        cursor_table: NativeTableIterationKind::QueryPlans,
        cursor_row_index: 0,
        primary_slot: NATIVE_ECS_TABLE_ITERATION_CURSORS.query_plans.rows[0].primary_slot,
        build_row: ECS_QUERY_PLAN_BUILD_ROWS[0],
    }];
const ECS_COMPILED_SCHEDULE_BUILD_ROWS: [NativeCompiledScheduleBuildRow; 1] =
    [NativeCompiledScheduleBuildRow {
        startup_schedule_id_slot: ECS_STARTUP_TABLE_RUN_SCHEDULE_ID_SLOT,
        descriptor_schedule_id_slot: ECS_MAIN_SCHEDULE_DESCRIPTOR_ID_SLOT,
        descriptor_item_count_slot: ECS_MAIN_SCHEDULE_DESCRIPTOR_ITEM_COUNT_SLOT,
        descriptor_run_system_id_slot: ECS_MAIN_SCHEDULE_RUN_SYSTEM_ID_SLOT,
        system_descriptor_id_slot: ECS_MOVE_SYSTEM_DESCRIPTOR_ID_SLOT,
        compiled_schedule_id_slot: ECS_COMPILED_SCHEDULE_ID_SLOT,
        compiled_scheduled_system_id_slot: ECS_COMPILED_SCHEDULED_SYSTEM_ID_SLOT,
        compiled_scheduled_system_count_slot: ECS_COMPILED_SCHEDULED_SYSTEM_COUNT_SLOT,
        expected_scheduled_system_count: 1,
        expected_scheduled_system_id: DEMO_MOVE_SYSTEM_ID,
        query_plan_row_index: 0,
    }];

const DEMO_POSITION_COMPONENT_ID: u64 = 0x002202c6aeb4f27b;
const DEMO_VELOCITY_COMPONENT_ID: u64 = 0x2cf8a68bcb7f913b;
const DEMO_TIME_RESOURCE_ID: u64 = 0x7924ce11db524521;
const DEMO_MOVE_SYSTEM_ID: u64 = 0x723b6b52df270ed5;
const DEMO_MOVERS_QUERY_ID: u64 = 0xf4004232b85cef9f;
const DEMO_MAIN_SCHEDULE_ID: u64 = 0xed3d905325519b05;

#[derive(Clone, Debug, Eq, PartialEq)]
struct EcsStartupPayloads {
    startup_record_count: u32,
    resource_operation_kind_offset: i32,
    resource_id_offset: i32,
    resource_id: u64,
    resource_payload_len_offset: i32,
    resource_payload_offset: i32,
    resource_payload: [u8; 4],
    spawn_operation_kind_offset: i32,
    spawn_component_count_offset: i32,
    spawn_component_count: u32,
    position_component_id_offset: i32,
    position_component_id: u64,
    position_payload_len_offset: i32,
    position_payload_offset: i32,
    position_payload: [u8; 8],
    velocity_component_id_offset: i32,
    velocity_component_id: u64,
    velocity_payload_len_offset: i32,
    velocity_payload_offset: i32,
    velocity_payload: [u8; 8],
    spawn_operations: Vec<ParsedSpawnOperation>,
    run_schedule_operation_kind_offset: i32,
    run_schedule_id_offset: i32,
    run_schedule_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParsedResourcePayloadOperation {
    operation_kind_offset: i32,
    resource_id_offset: i32,
    resource_id: u64,
    payload_len_offset: i32,
    payload_offset: i32,
    payload: [u8; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParsedSpawnOperation {
    operation_kind_offset: i32,
    component_count_offset: i32,
    component_count: u32,
    position_component_id_offset: i32,
    position_component_id: u64,
    position_payload_len_offset: i32,
    position_payload_offset: i32,
    position_payload: [u8; 8],
    velocity_component_id_offset: i32,
    velocity_component_id: u64,
    velocity_payload_len_offset: i32,
    velocity_payload_offset: i32,
    velocity_payload: [u8; 8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParsedRunScheduleOperation {
    operation_kind_offset: i32,
    schedule_id_offset: i32,
    schedule_id: u64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeMoveQueryLoopObservable {
    schedule_name: String,
    schedule_id: u64,
    schedule_run_system_id: u64,
    schedule_run_system_name: String,
    system_name: String,
    query_param: String,
    position_binding: String,
    velocity_binding: String,
    position_component_id: u64,
    position_component_name: String,
    velocity_component_id: u64,
    velocity_component_name: String,
    time_param: String,
    time_resource_id: u64,
    time_resource_name: String,
    updates: Vec<NativeMoveQueryLoopUpdate>,
    target_position_payload: [u8; 8],
    field_product_payload: [u8; 8],
    rows: Vec<NativeMoveQueryLoopRowObservable>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeMoveQueryLoopRowObservable {
    row_index: usize,
    target_position_payload: [u8; 8],
    field_product_payload: [u8; 8],
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeMoveQueryLoopUpdate {
    target_field: String,
    velocity_field: String,
    time_field: String,
}

pub fn startup_text_payload(program: &Program) -> Result<Vec<u8>, CodegenError> {
    let startup = program.startup.as_ref().ok_or_else(unsupported_shape)?;

    let startup_body = match startup.statements.as_slice() {
        [Statement::Exit(exit)] => immediate_exit_body(&exit.expression),
        [Statement::Let(let_statement), Statement::Exit(exit)] => {
            if let Expression::Identifier { name, .. } = &exit.expression {
                if name == &let_statement.name && let_statement.type_name.name == "i32" {
                    local_arithmetic_exit_body(&let_statement.initializer)
                } else {
                    Err(unsupported_shape())
                }
            } else {
                Err(unsupported_shape())
            }
        }
        _ => Err(unsupported_shape()),
    }?;

    Ok(runtime_wrapped_payload(
        &startup_body,
        NATIVE_ECS_EXECUTION_STATE_LAYOUT.frame_size,
    ))
}

pub fn ecs_metadata_decoder_text_payload(
    program: &Program,
    metadata_payload: &[u8],
) -> Result<Vec<u8>, CodegenError> {
    require_metadata_decoder_exit(program)?;

    if metadata_payload.len() < ECS_METADATA_ENVELOPE_SIZE {
        return Err(CodegenError {
            message: format!(
                "ECS metadata payload must contain at least {ECS_METADATA_ENVELOPE_SIZE} envelope bytes"
            ),
        });
    }

    let core = lower_verified_core(program)?;
    let assembly =
        runtime_assembly::assemble_runtime_program_from_source(program).map_err(|error| {
            CodegenError {
                message: format!(
                    "could not assemble native world storage input: {}",
                    error.message
                ),
            }
        })?;
    let storage_plan =
        derive_native_world_storage_plan(&core, &assembly, NATIVE_STORAGE_BASE_OFFSET).map_err(
            |error| CodegenError {
                message: format!(
                    "could not derive native world storage plan: {}",
                    error.message
                ),
            },
        )?;
    let startup_payloads = startup_payloads(metadata_payload)?;
    let query_loop_observable =
        native_move_query_loop_observable_from_core(&core, &startup_payloads)?;
    let storage_compatibility =
        native_storage_compatibility_model(&storage_plan, &query_loop_observable)?;
    let startup_body = ecs_metadata_decoder_body(
        &metadata_payload[..ECS_METADATA_ENVELOPE_SIZE],
        startup_payloads,
        query_loop_observable,
        &storage_plan,
        storage_compatibility,
    );
    Ok(runtime_wrapped_payload(
        &startup_body,
        storage_plan.frame_size,
    ))
}

fn lower_verified_core(program: &Program) -> Result<CoreProgram, CodegenError> {
    let core = core_lower::lower_program_to_core(program).map_err(|error| CodegenError {
        message: format!(
            "could not lower Core for native query-loop observable: {}",
            error.message
        ),
    })?;
    core_verify::verify_core_program(&core).map_err(|error| CodegenError {
        message: format!(
            "invalid Core for native query-loop observable: {}",
            error.message
        ),
    })?;
    Ok(core)
}

#[cfg(test)]
fn native_move_query_loop_observable(
    program: &Program,
    startup_payloads: &EcsStartupPayloads,
) -> Result<NativeMoveQueryLoopObservable, CodegenError> {
    let core = lower_verified_core(program)?;

    native_move_query_loop_observable_from_core(&core, startup_payloads)
}

fn native_move_query_loop_observable_from_core(
    core: &CoreProgram,
    startup_payloads: &EcsStartupPayloads,
) -> Result<NativeMoveQueryLoopObservable, CodegenError> {
    let system = core
        .systems
        .iter()
        .find(|system| system.name == "Move")
        .ok_or_else(native_move_query_loop_observable_error)?;
    let CoreSystemParamKind::ReadResource {
        resource_id,
        name: resource_name,
    } = &system
        .params
        .iter()
        .find(|param| param.name == "time")
        .ok_or_else(native_move_query_loop_observable_error)?
        .kind
    else {
        return Err(native_move_query_loop_observable_error());
    };
    if *resource_id != DEMO_TIME_RESOURCE_ID || resource_name != "Demo.Time" {
        return Err(native_move_query_loop_observable_error());
    }

    let CoreSystemParamKind::Query { terms } = &system
        .params
        .iter()
        .find(|param| param.name == "movers")
        .ok_or_else(native_move_query_loop_observable_error)?
        .kind
    else {
        return Err(native_move_query_loop_observable_error());
    };
    if terms.len() != 2
        || terms[0].access != CoreQueryAccess::Mut
        || terms[0].component_id != DEMO_POSITION_COMPONENT_ID
        || terms[0].name != "Demo.Position"
        || terms[1].access != CoreQueryAccess::Read
        || terms[1].component_id != DEMO_VELOCITY_COMPONENT_ID
        || terms[1].name != "Demo.Velocity"
    {
        return Err(native_move_query_loop_observable_error());
    }

    if system.body.statements.len() != 1 {
        return Err(native_move_query_loop_observable_error());
    }
    let CoreSystemStatement::QueryLoop(query_loop) = &system.body.statements[0] else {
        return Err(native_move_query_loop_observable_error());
    };
    if query_loop.query_param != "movers"
        || query_loop.bindings.len() != 2
        || query_loop.bindings[0].name != "pos"
        || query_loop.bindings[0].access != CoreQueryAccess::Mut
        || query_loop.bindings[0].component_id != DEMO_POSITION_COMPONENT_ID
        || query_loop.bindings[0].component_name != "Demo.Position"
        || query_loop.bindings[1].name != "vel"
        || query_loop.bindings[1].access != CoreQueryAccess::Read
        || query_loop.bindings[1].component_id != DEMO_VELOCITY_COMPONENT_ID
        || query_loop.bindings[1].component_name != "Demo.Velocity"
        || query_loop.body.len() != 2
    {
        return Err(native_move_query_loop_observable_error());
    }

    let updates = vec![
        require_move_add_assign(&query_loop.body[0], "x", "x")?,
        require_move_add_assign(&query_loop.body[1], "y", "y")?,
    ];

    let schedule = core
        .schedules
        .iter()
        .find(|schedule| schedule.name == "Main")
        .ok_or_else(native_move_query_loop_observable_error)?;
    if startup_payloads.run_schedule_id != DEMO_MAIN_SCHEDULE_ID || schedule.items.len() != 1 {
        return Err(native_move_query_loop_observable_error());
    }
    let CoreScheduleItem::Run {
        system_id,
        system_name,
    } = &schedule.items[0];
    if *system_id != DEMO_MOVE_SYSTEM_ID || system_name != "Demo.Move" {
        return Err(native_move_query_loop_observable_error());
    }

    let rows = native_move_query_loop_rows(startup_payloads)?;
    let target_position_payload = rows[0].target_position_payload;
    let field_product_payload = rows[0].field_product_payload;

    Ok(NativeMoveQueryLoopObservable {
        schedule_name: "Demo.Main".to_string(),
        schedule_id: startup_payloads.run_schedule_id,
        schedule_run_system_id: *system_id,
        schedule_run_system_name: system_name.clone(),
        system_name: system.name.clone(),
        query_param: query_loop.query_param.clone(),
        position_binding: query_loop.bindings[0].name.clone(),
        velocity_binding: query_loop.bindings[1].name.clone(),
        position_component_id: query_loop.bindings[0].component_id,
        position_component_name: query_loop.bindings[0].component_name.clone(),
        velocity_component_id: query_loop.bindings[1].component_id,
        velocity_component_name: query_loop.bindings[1].component_name.clone(),
        time_param: "time".to_string(),
        time_resource_id: *resource_id,
        time_resource_name: resource_name.clone(),
        updates,
        target_position_payload,
        field_product_payload,
        rows,
    })
}

fn require_move_add_assign(
    statement: &CoreSystemStatement,
    position_field: &str,
    velocity_field: &str,
) -> Result<NativeMoveQueryLoopUpdate, CodegenError> {
    let CoreSystemStatement::AddAssign { target, value } = statement else {
        return Err(native_move_query_loop_observable_error());
    };
    let CoreSystemPlace::ComponentField {
        binding,
        component_id,
        component_name,
        field_name,
    } = target;
    if binding != "pos"
        || *component_id != DEMO_POSITION_COMPONENT_ID
        || component_name != "Demo.Position"
        || field_name != position_field
    {
        return Err(native_move_query_loop_observable_error());
    }

    require_velocity_delta_expression(value, velocity_field)?;
    Ok(NativeMoveQueryLoopUpdate {
        target_field: position_field.to_string(),
        velocity_field: velocity_field.to_string(),
        time_field: "delta".to_string(),
    })
}

fn require_velocity_delta_expression(
    expression: &CoreSystemExpression,
    velocity_field: &str,
) -> Result<(), CodegenError> {
    let CoreSystemExpression::Binary { op, left, right } = expression else {
        return Err(native_move_query_loop_observable_error());
    };
    if *op != CoreSystemBinaryOp::F32Multiply {
        return Err(native_move_query_loop_observable_error());
    }

    let CoreSystemExpression::ComponentField {
        binding,
        component_id,
        component_name,
        field_name,
    } = &**left
    else {
        return Err(native_move_query_loop_observable_error());
    };
    if binding != "vel"
        || *component_id != DEMO_VELOCITY_COMPONENT_ID
        || component_name != "Demo.Velocity"
        || field_name != velocity_field
    {
        return Err(native_move_query_loop_observable_error());
    }

    let CoreSystemExpression::ResourceField {
        param,
        resource_id,
        resource_name,
        field_name,
    } = &**right
    else {
        return Err(native_move_query_loop_observable_error());
    };
    if param != "time"
        || *resource_id != DEMO_TIME_RESOURCE_ID
        || resource_name != "Demo.Time"
        || field_name != "delta"
    {
        return Err(native_move_query_loop_observable_error());
    }

    Ok(())
}

fn native_move_query_loop_rows(
    startup_payloads: &EcsStartupPayloads,
) -> Result<Vec<NativeMoveQueryLoopRowObservable>, CodegenError> {
    startup_payloads
        .spawn_operations
        .iter()
        .enumerate()
        .map(|(row_index, spawn)| {
            Ok(NativeMoveQueryLoopRowObservable {
                row_index,
                target_position_payload: target_position_payload(
                    &spawn.position_payload,
                    &spawn.velocity_payload,
                    &startup_payloads.resource_payload,
                ),
                field_product_payload: field_product_payload(
                    &spawn.velocity_payload,
                    &startup_payloads.resource_payload,
                ),
            })
        })
        .collect()
}

fn target_position_payload(
    position_payload: &[u8; 8],
    velocity_payload: &[u8; 8],
    resource_payload: &[u8; 4],
) -> [u8; 8] {
    let position_x = f32_from_le_bytes(&position_payload[0..4]);
    let position_y = f32_from_le_bytes(&position_payload[4..8]);
    let velocity_x = f32_from_le_bytes(&velocity_payload[0..4]);
    let velocity_y = f32_from_le_bytes(&velocity_payload[4..8]);
    let delta = f32_from_le_bytes(resource_payload);

    let mut payload = [0; 8];
    payload[0..4].copy_from_slice(&(position_x + velocity_x * delta).to_le_bytes());
    payload[4..8].copy_from_slice(&(position_y + velocity_y * delta).to_le_bytes());
    payload
}

fn field_product_payload(velocity_payload: &[u8; 8], resource_payload: &[u8; 4]) -> [u8; 8] {
    let velocity_x = f32_from_le_bytes(&velocity_payload[0..4]);
    let velocity_y = f32_from_le_bytes(&velocity_payload[4..8]);
    let delta = f32_from_le_bytes(resource_payload);

    let mut payload = [0; 8];
    payload[0..4].copy_from_slice(&(velocity_x * delta).to_le_bytes());
    payload[4..8].copy_from_slice(&(velocity_y * delta).to_le_bytes());
    payload
}

fn f32_from_le_bytes(bytes: &[u8]) -> f32 {
    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn startup_spawn_slots(row_index: usize) -> Result<NativeSpawnStartupOperationSlots, CodegenError> {
    NATIVE_ECS_TABLE_MODEL
        .startup_operations
        .spawn_rows
        .get(row_index)
        .copied()
        .ok_or_else(metadata_startup_payload_error)
}

fn startup_payload_storage_slots(
    row_index: usize,
) -> Result<NativeSpawnPayloadStorageSlots, CodegenError> {
    NATIVE_ECS_EXECUTION_STATE_LAYOUT
        .startup_state
        .spawn_payload_rows
        .get(row_index)
        .copied()
        .ok_or_else(metadata_startup_payload_error)
}

#[cfg(test)]
fn archetype_storage_row_slots(
    row_index: usize,
) -> Result<NativeArchetypeTableStorageRowSlots, CodegenError> {
    let storage = NATIVE_ECS_TABLE_MODEL.archetype_storage;
    let position_payload = storage
        .position_column
        .payload_rows
        .get(row_index)
        .copied()
        .ok_or_else(metadata_startup_payload_error)?;
    let velocity_payload = storage
        .velocity_column
        .payload_rows
        .get(row_index)
        .copied()
        .ok_or_else(metadata_startup_payload_error)?;

    Ok(NativeArchetypeTableStorageRowSlots {
        position_payload,
        velocity_payload,
    })
}

fn startup_operation_iteration_rows(
    startup_payloads: &EcsStartupPayloads,
) -> &'static [NativeStartupOperationTableIterationRow] {
    match startup_payloads.spawn_operations.len() {
        1 => &ECS_STARTUP_OPERATION_TABLE_ITERATION_ROWS,
        2 => &ECS_TWO_SPAWN_STARTUP_OPERATION_TABLE_ITERATION_ROWS,
        _ => unreachable!("startup payload parser only accepts one or two spawn rows"),
    }
}

fn startup_operation_table_expected(startup_payloads: &EcsStartupPayloads) -> Vec<(u16, u64)> {
    let mut expected = vec![
        (
            ECS_STARTUP_TABLE_RESOURCE_ID_SLOT,
            startup_payloads.resource_id,
        ),
        (
            ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_OFFSET_SLOT,
            startup_payloads.resource_payload_offset as u64,
        ),
        (ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_LEN_SLOT, 4),
    ];

    for (row_index, spawn) in startup_payloads.spawn_operations.iter().enumerate() {
        let slots = startup_spawn_slots(row_index)
            .expect("startup payload parser bounds match spawn slots");
        expected.extend_from_slice(&[
            (slots.component_count.offset, spawn.component_count as u64),
            (
                slots.position_component_id.offset,
                spawn.position_component_id,
            ),
            (
                slots.position_payload_offset.offset,
                spawn.position_payload_offset as u64,
            ),
            (slots.position_payload_len.offset, 8),
            (
                slots.velocity_component_id.offset,
                spawn.velocity_component_id,
            ),
            (
                slots.velocity_payload_offset.offset,
                spawn.velocity_payload_offset as u64,
            ),
            (slots.velocity_payload_len.offset, 8),
        ]);
    }

    expected
}

fn require_metadata_decoder_exit(program: &Program) -> Result<(), CodegenError> {
    let startup = program.startup.as_ref().ok_or_else(unsupported_shape)?;
    let Some(Statement::Exit(exit)) = startup.statements.last() else {
        return Err(metadata_decoder_error());
    };
    let Expression::Integer(integer) = &exit.expression else {
        return Err(metadata_decoder_error());
    };

    if integer.value != 0 {
        return Err(metadata_decoder_error());
    }

    Ok(())
}

fn startup_payloads(metadata_payload: &[u8]) -> Result<EcsStartupPayloads, CodegenError> {
    let startup_section_offset = read_metadata_u32(
        metadata_payload,
        ECS_STARTUP_SECTION_DIRECTORY_OFFSET + ECS_SECTION_OFFSET_FIELD_OFFSET,
    )? as usize;
    let startup_record_count = read_metadata_u32(
        metadata_payload,
        ECS_STARTUP_SECTION_DIRECTORY_OFFSET + ECS_SECTION_RECORD_COUNT_FIELD_OFFSET,
    )?;

    if !matches!(startup_record_count, 3 | 4) {
        return Err(metadata_startup_payload_error());
    }

    let mut offset = startup_section_offset;
    let resource_payload = parse_resource_payload_operation(metadata_payload, &mut offset)?;
    let spawn_count = startup_record_count as usize - 2;
    let mut spawn_operations = Vec::with_capacity(spawn_count);
    for _ in 0..spawn_count {
        spawn_operations.push(parse_spawn_operation(metadata_payload, &mut offset)?);
    }
    let spawn_operation = spawn_operations[0];
    let run_schedule = parse_run_schedule_operation(metadata_payload, &mut offset)?;

    Ok(EcsStartupPayloads {
        startup_record_count,
        resource_operation_kind_offset: resource_payload.operation_kind_offset,
        resource_id_offset: resource_payload.resource_id_offset,
        resource_id: resource_payload.resource_id,
        resource_payload_len_offset: resource_payload.payload_len_offset,
        resource_payload_offset: resource_payload.payload_offset,
        resource_payload: resource_payload.payload,
        spawn_operation_kind_offset: spawn_operation.operation_kind_offset,
        spawn_component_count_offset: spawn_operation.component_count_offset,
        spawn_component_count: spawn_operation.component_count,
        position_component_id_offset: spawn_operation.position_component_id_offset,
        position_component_id: spawn_operation.position_component_id,
        position_payload_len_offset: spawn_operation.position_payload_len_offset,
        position_payload_offset: spawn_operation.position_payload_offset,
        position_payload: spawn_operation.position_payload,
        velocity_component_id_offset: spawn_operation.velocity_component_id_offset,
        velocity_component_id: spawn_operation.velocity_component_id,
        velocity_payload_len_offset: spawn_operation.velocity_payload_len_offset,
        velocity_payload_offset: spawn_operation.velocity_payload_offset,
        velocity_payload: spawn_operation.velocity_payload,
        spawn_operations,
        run_schedule_operation_kind_offset: run_schedule.operation_kind_offset,
        run_schedule_id_offset: run_schedule.schedule_id_offset,
        run_schedule_id: run_schedule.schedule_id,
    })
}

fn parse_resource_payload_operation(
    metadata_payload: &[u8],
    offset: &mut usize,
) -> Result<ParsedResourcePayloadOperation, CodegenError> {
    let operation_kind_offset = metadata_i32_offset(
        *offset,
        "ECS metadata startup resource operation kind offset must fit in signed 32-bit displacement",
    )?;
    let operation_kind = read_metadata_u32(metadata_payload, *offset)?;
    *offset += 4;

    if operation_kind != ECS_STARTUP_OP_RESOURCE_PAYLOAD {
        return Err(metadata_startup_payload_error());
    }

    checked_metadata_range(metadata_payload, *offset, 8)?;
    let resource_id_offset = metadata_i32_offset(
        *offset,
        "ECS metadata startup resource id offset must fit in signed 32-bit displacement",
    )?;
    let resource_id = read_metadata_u64(metadata_payload, *offset)?;
    *offset += 8;
    skip_metadata_string(metadata_payload, offset)?;

    let payload = parse_payload_offset_and_bytes(metadata_payload, offset)?;
    Ok(ParsedResourcePayloadOperation {
        operation_kind_offset,
        resource_id_offset,
        resource_id,
        payload_len_offset: payload.0,
        payload_offset: payload.1,
        payload: payload.2,
    })
}

fn parse_spawn_operation(
    metadata_payload: &[u8],
    offset: &mut usize,
) -> Result<ParsedSpawnOperation, CodegenError> {
    let operation_kind_offset = metadata_i32_offset(
        *offset,
        "ECS metadata startup spawn operation kind offset must fit in signed 32-bit displacement",
    )?;
    let operation_kind = read_metadata_u32(metadata_payload, *offset)?;
    *offset += 4;

    if operation_kind != ECS_STARTUP_OP_SPAWN {
        return Err(metadata_startup_payload_error());
    }

    let component_count_offset = metadata_i32_offset(
        *offset,
        "ECS metadata startup spawn component count offset must fit in signed 32-bit displacement",
    )?;
    let component_count = read_metadata_u32(metadata_payload, *offset)?;
    *offset += 4;

    if component_count != 2 {
        return Err(metadata_startup_payload_error());
    }

    let position_payload = parse_spawn_component_payload(metadata_payload, offset)?;
    let velocity_payload = parse_spawn_component_payload(metadata_payload, offset)?;

    Ok(ParsedSpawnOperation {
        operation_kind_offset,
        component_count_offset,
        component_count,
        position_component_id_offset: position_payload.0,
        position_component_id: position_payload.1,
        position_payload_len_offset: position_payload.2,
        position_payload_offset: position_payload.3,
        position_payload: position_payload.4,
        velocity_component_id_offset: velocity_payload.0,
        velocity_component_id: velocity_payload.1,
        velocity_payload_len_offset: velocity_payload.2,
        velocity_payload_offset: velocity_payload.3,
        velocity_payload: velocity_payload.4,
    })
}

fn parse_spawn_component_payload(
    metadata_payload: &[u8],
    offset: &mut usize,
) -> Result<(i32, u64, i32, i32, [u8; 8]), CodegenError> {
    checked_metadata_range(metadata_payload, *offset, 8)?;
    let component_id_offset = metadata_i32_offset(
        *offset,
        "ECS metadata startup spawn component id offset must fit in signed 32-bit displacement",
    )?;
    let component_id = read_metadata_u64(metadata_payload, *offset)?;
    *offset += 8;
    skip_metadata_string(metadata_payload, offset)?;
    let payload = parse_payload_offset_and_bytes(metadata_payload, offset)?;
    Ok((
        component_id_offset,
        component_id,
        payload.0,
        payload.1,
        payload.2,
    ))
}

fn parse_run_schedule_operation(
    metadata_payload: &[u8],
    offset: &mut usize,
) -> Result<ParsedRunScheduleOperation, CodegenError> {
    let operation_kind_offset = metadata_i32_offset(
        *offset,
        "ECS metadata startup run schedule operation kind offset must fit in signed 32-bit displacement",
    )?;
    let operation_kind = read_metadata_u32(metadata_payload, *offset)?;
    *offset += 4;

    if operation_kind != ECS_STARTUP_OP_RUN_SCHEDULE {
        return Err(metadata_startup_payload_error());
    }

    checked_metadata_range(metadata_payload, *offset, 8)?;
    let schedule_id_offset = *offset;
    let schedule_id = u64::from_le_bytes([
        metadata_payload[*offset],
        metadata_payload[*offset + 1],
        metadata_payload[*offset + 2],
        metadata_payload[*offset + 3],
        metadata_payload[*offset + 4],
        metadata_payload[*offset + 5],
        metadata_payload[*offset + 6],
        metadata_payload[*offset + 7],
    ]);
    *offset += 8;

    let schedule_name = read_metadata_string(metadata_payload, offset)?;
    if schedule_id != DEMO_MAIN_SCHEDULE_ID || schedule_name != "Demo.Main" {
        return Err(metadata_startup_payload_error());
    }

    Ok(ParsedRunScheduleOperation {
        operation_kind_offset,
        schedule_id_offset: metadata_i32_offset(
            schedule_id_offset,
            "ECS metadata startup run schedule offset must fit in signed 32-bit displacement",
        )?,
        schedule_id,
    })
}

fn parse_payload_offset_and_bytes<const N: usize>(
    metadata_payload: &[u8],
    offset: &mut usize,
) -> Result<(i32, i32, [u8; N]), CodegenError> {
    let payload_len_offset = metadata_i32_offset(
        *offset,
        "ECS metadata startup payload length offset must fit in signed 32-bit displacement",
    )?;
    let payload_len = read_metadata_u32(metadata_payload, *offset)? as usize;
    *offset += 4;

    if payload_len != N {
        return Err(metadata_startup_payload_error());
    }

    checked_metadata_range(metadata_payload, *offset, payload_len)?;
    let payload_offset = *offset;
    *offset += payload_len;

    if payload_offset > i32::MAX as usize {
        return Err(CodegenError {
            message: "ECS metadata startup payload offset must fit in signed 32-bit displacement"
                .to_string(),
        });
    }

    let mut payload = [0; N];
    payload.copy_from_slice(&metadata_payload[payload_offset..*offset]);

    Ok((payload_len_offset, payload_offset as i32, payload))
}

fn read_metadata_u32(metadata_payload: &[u8], offset: usize) -> Result<u32, CodegenError> {
    checked_metadata_range(metadata_payload, offset, 4)?;
    Ok(u32::from_le_bytes([
        metadata_payload[offset],
        metadata_payload[offset + 1],
        metadata_payload[offset + 2],
        metadata_payload[offset + 3],
    ]))
}

fn read_metadata_u64(metadata_payload: &[u8], offset: usize) -> Result<u64, CodegenError> {
    checked_metadata_range(metadata_payload, offset, 8)?;
    Ok(u64::from_le_bytes([
        metadata_payload[offset],
        metadata_payload[offset + 1],
        metadata_payload[offset + 2],
        metadata_payload[offset + 3],
        metadata_payload[offset + 4],
        metadata_payload[offset + 5],
        metadata_payload[offset + 6],
        metadata_payload[offset + 7],
    ]))
}

fn skip_metadata_string(metadata_payload: &[u8], offset: &mut usize) -> Result<(), CodegenError> {
    let byte_len = read_metadata_u32(metadata_payload, *offset)? as usize;
    *offset += 4;
    checked_metadata_range(metadata_payload, *offset, byte_len)?;
    *offset += byte_len;
    Ok(())
}

fn read_metadata_string(
    metadata_payload: &[u8],
    offset: &mut usize,
) -> Result<String, CodegenError> {
    let byte_len = read_metadata_u32(metadata_payload, *offset)? as usize;
    *offset += 4;
    checked_metadata_range(metadata_payload, *offset, byte_len)?;
    let value = std::str::from_utf8(&metadata_payload[*offset..*offset + byte_len])
        .map_err(|_| metadata_startup_payload_error())?
        .to_string();
    *offset += byte_len;
    Ok(value)
}

fn checked_metadata_range(
    metadata_payload: &[u8],
    offset: usize,
    byte_len: usize,
) -> Result<(), CodegenError> {
    let Some(end) = offset.checked_add(byte_len) else {
        return Err(metadata_startup_payload_error());
    };

    if end > metadata_payload.len() {
        return Err(metadata_startup_payload_error());
    }

    Ok(())
}

fn metadata_i32_offset(offset: usize, message: &str) -> Result<i32, CodegenError> {
    if offset > i32::MAX as usize {
        return Err(CodegenError {
            message: message.to_string(),
        });
    }

    Ok(offset as i32)
}

fn ecs_metadata_decoder_body(
    envelope: &[u8],
    startup_payloads: EcsStartupPayloads,
    query_loop_observable: NativeMoveQueryLoopObservable,
    storage_plan: &NativeWorldStoragePlan,
    storage_compatibility: NativeStorageCompatibilityModel,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut jump_to_metadata_failure_offsets = Vec::new();
    let mut jump_to_startup_state_failure_offsets = Vec::new();
    let mut jump_to_query_loop_scan_failure_offsets = Vec::new();
    let mut jump_to_query_loop_field_math_failure_offsets = Vec::new();
    let mut jump_to_query_loop_position_store_failure_offsets = Vec::new();
    let mut jump_to_run_schedule_dispatch_failure_offsets = Vec::new();

    bytes.extend_from_slice(&[0x48, 0x8d, 0x35, 0x00, 0x00, 0x00, 0x00]); // lea rsi, [rip + metadata]

    for (index, chunk) in envelope.chunks_exact(8).enumerate() {
        bytes.extend_from_slice(&[0x48, 0xb8]); // mov rax, imm64
        bytes.extend_from_slice(chunk);
        bytes.extend_from_slice(&[0x48, 0x39, 0x46, (index * 8) as u8]); // cmp [rsi + offset], rax

        let jump_offset = bytes.len();
        bytes.extend_from_slice(&[0x0f, 0x85, 0x00, 0x00, 0x00, 0x00]); // jne failure
        jump_to_metadata_failure_offsets.push(jump_offset);
    }

    for (count_offset, stack_slot) in ECS_DESCRIPTOR_RECORD_COUNT_OFFSETS
        .iter()
        .zip(ECS_DESCRIPTOR_REGISTRY_SLOTS)
    {
        bytes.extend_from_slice(&[0x8b, 0x46, *count_offset]); // mov eax, dword ptr [rsi + offset]
        store_rax_to_stack_slot(&mut bytes, stack_slot);
    }
    for (record_offset, stack_slot) in ECS_DESCRIPTOR_RECORD_OFFSET_FIELD_OFFSETS
        .iter()
        .zip(ECS_DESCRIPTOR_RECORD_OFFSET_SLOTS)
    {
        bytes.extend_from_slice(&[0x8b, 0x46, *record_offset]); // mov eax, dword ptr [rsi + offset]
        store_rax_to_stack_slot(&mut bytes, stack_slot);
    }
    for (byte_len_offset, stack_slot) in ECS_DESCRIPTOR_RECORD_BYTE_LEN_FIELD_OFFSETS
        .iter()
        .zip(ECS_DESCRIPTOR_RECORD_BYTE_LEN_SLOTS)
    {
        bytes.extend_from_slice(&[0x8b, 0x46, *byte_len_offset]); // mov eax, dword ptr [rsi + offset]
        store_rax_to_stack_slot(&mut bytes, stack_slot);
    }
    emit_native_descriptor_table_row_iteration(
        &mut bytes,
        &mut jump_to_startup_state_failure_offsets,
    );
    emit_descriptor_name_table_decodes(&mut bytes, &mut jump_to_startup_state_failure_offsets);
    emit_startup_operation_table_decodes(&mut bytes, &startup_payloads);
    emit_native_storage_catalog_materialization(&mut bytes, storage_plan);

    emit_native_startup_operation_table_iteration(
        &mut bytes,
        &startup_payloads,
        &query_loop_observable,
        storage_compatibility,
        &mut jump_to_startup_state_failure_offsets,
        &mut jump_to_query_loop_scan_failure_offsets,
        &mut jump_to_query_loop_field_math_failure_offsets,
        &mut jump_to_query_loop_position_store_failure_offsets,
        &mut jump_to_run_schedule_dispatch_failure_offsets,
    );

    move_edi_exit_code(&mut bytes, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE);

    let jump_to_done_offset = bytes.len();
    bytes.extend_from_slice(&[0xe9, 0x00, 0x00, 0x00, 0x00]); // jmp done

    let metadata_failure_offset = bytes.len();
    move_edi_exit_code(&mut bytes, ECS_METADATA_FAILURE_EXIT_CODE);
    let jump_from_metadata_failure_to_done_offset = bytes.len();
    bytes.extend_from_slice(&[0xe9, 0x00, 0x00, 0x00, 0x00]); // jmp done

    let startup_state_failure_offset = bytes.len();
    move_edi_exit_code(&mut bytes, ECS_STARTUP_STATE_FAILURE_EXIT_CODE);
    let jump_from_startup_state_failure_to_done_offset = bytes.len();
    bytes.extend_from_slice(&[0xe9, 0x00, 0x00, 0x00, 0x00]); // jmp done

    let query_loop_scan_failure_offset = bytes.len();
    move_edi_exit_code(&mut bytes, ECS_QUERY_LOOP_SCAN_FAILURE_EXIT_CODE);
    let jump_from_query_loop_scan_failure_to_done_offset = bytes.len();
    bytes.extend_from_slice(&[0xe9, 0x00, 0x00, 0x00, 0x00]); // jmp done

    let query_loop_field_math_failure_offset = bytes.len();
    move_edi_exit_code(&mut bytes, ECS_QUERY_LOOP_FIELD_MATH_FAILURE_EXIT_CODE);
    let jump_from_query_loop_field_math_failure_to_done_offset = bytes.len();
    bytes.extend_from_slice(&[0xe9, 0x00, 0x00, 0x00, 0x00]); // jmp done

    let query_loop_position_store_failure_offset = bytes.len();
    move_edi_exit_code(&mut bytes, ECS_QUERY_LOOP_POSITION_STORE_FAILURE_EXIT_CODE);
    let jump_from_query_loop_position_store_failure_to_done_offset = bytes.len();
    bytes.extend_from_slice(&[0xe9, 0x00, 0x00, 0x00, 0x00]); // jmp done

    let run_schedule_dispatch_failure_offset = bytes.len();
    move_edi_exit_code(&mut bytes, ECS_RUN_SCHEDULE_DISPATCH_FAILURE_EXIT_CODE);
    let done_offset = bytes.len();

    for jump_offset in jump_to_metadata_failure_offsets {
        patch_rel32(
            &mut bytes,
            jump_offset + 2,
            metadata_failure_offset,
            jump_offset + 6,
        );
    }
    for jump_offset in jump_to_startup_state_failure_offsets {
        patch_rel32(
            &mut bytes,
            jump_offset + 2,
            startup_state_failure_offset,
            jump_offset + 6,
        );
    }
    for jump_offset in jump_to_query_loop_scan_failure_offsets {
        patch_rel32(
            &mut bytes,
            jump_offset + 2,
            query_loop_scan_failure_offset,
            jump_offset + 6,
        );
    }
    for jump_offset in jump_to_query_loop_field_math_failure_offsets {
        patch_rel32(
            &mut bytes,
            jump_offset + 2,
            query_loop_field_math_failure_offset,
            jump_offset + 6,
        );
    }
    for jump_offset in jump_to_query_loop_position_store_failure_offsets {
        patch_rel32(
            &mut bytes,
            jump_offset + 2,
            query_loop_position_store_failure_offset,
            jump_offset + 6,
        );
    }
    for jump_offset in jump_to_run_schedule_dispatch_failure_offsets {
        patch_rel32(
            &mut bytes,
            jump_offset + 2,
            run_schedule_dispatch_failure_offset,
            jump_offset + 6,
        );
    }
    patch_rel32(
        &mut bytes,
        jump_to_done_offset + 1,
        done_offset,
        jump_to_done_offset + 5,
    );
    patch_rel32(
        &mut bytes,
        jump_from_metadata_failure_to_done_offset + 1,
        done_offset,
        jump_from_metadata_failure_to_done_offset + 5,
    );
    patch_rel32(
        &mut bytes,
        jump_from_startup_state_failure_to_done_offset + 1,
        done_offset,
        jump_from_startup_state_failure_to_done_offset + 5,
    );
    patch_rel32(
        &mut bytes,
        jump_from_query_loop_scan_failure_to_done_offset + 1,
        done_offset,
        jump_from_query_loop_scan_failure_to_done_offset + 5,
    );
    patch_rel32(
        &mut bytes,
        jump_from_query_loop_field_math_failure_to_done_offset + 1,
        done_offset,
        jump_from_query_loop_field_math_failure_to_done_offset + 5,
    );
    patch_rel32(
        &mut bytes,
        jump_from_query_loop_position_store_failure_to_done_offset + 1,
        done_offset,
        jump_from_query_loop_position_store_failure_to_done_offset + 5,
    );

    let metadata_displacement =
        (bytes.len() + runtime_destroy_suffix(storage_plan.frame_size).len() - 7) as i32;
    patch_i32(&mut bytes, 3, metadata_displacement);

    bytes
}

fn compare_stack_slot_to_u64(
    bytes: &mut Vec<u8>,
    stack_slot: u16,
    expected: u64,
    jump_offsets: &mut Vec<usize>,
) {
    bytes.extend_from_slice(&[0x48, 0xb8]); // mov rax, imm64
    bytes.extend_from_slice(&expected.to_le_bytes());
    if stack_slot == 0 {
        bytes.extend_from_slice(&[0x48, 0x39, 0x04, 0x24]); // cmp qword ptr [rsp], rax
    } else if stack_slot <= 127 {
        bytes.extend_from_slice(&[0x48, 0x39, 0x44, 0x24, stack_slot as u8]); // cmp qword ptr [rsp + slot], rax
    } else {
        bytes.extend_from_slice(&[0x48, 0x39, 0x84, 0x24]); // cmp qword ptr [rsp + slot], rax
        bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
    }

    let jump_offset = bytes.len();
    bytes.extend_from_slice(&[0x0f, 0x85, 0x00, 0x00, 0x00, 0x00]); // jne failure
    jump_offsets.push(jump_offset);
}

fn compare_stack_slots_equal(
    bytes: &mut Vec<u8>,
    left_slot: u16,
    right_slot: u16,
    jump_offsets: &mut Vec<usize>,
) {
    load_stack_slot_to_rax(bytes, left_slot);
    if right_slot == 0 {
        bytes.extend_from_slice(&[0x48, 0x39, 0x04, 0x24]); // cmp qword ptr [rsp], rax
    } else if right_slot <= 127 {
        bytes.extend_from_slice(&[0x48, 0x39, 0x44, 0x24, right_slot as u8]); // cmp qword ptr [rsp + slot], rax
    } else {
        bytes.extend_from_slice(&[0x48, 0x39, 0x84, 0x24]); // cmp qword ptr [rsp + slot], rax
        bytes.extend_from_slice(&(right_slot as u32).to_le_bytes());
    }

    let jump_offset = bytes.len();
    bytes.extend_from_slice(&[0x0f, 0x85, 0x00, 0x00, 0x00, 0x00]); // jne failure
    jump_offsets.push(jump_offset);
}

fn compare_qword_at_stack_address_to_u64(
    bytes: &mut Vec<u8>,
    address_slot: u16,
    expected: u64,
    jump_offsets: &mut Vec<usize>,
) {
    load_stack_slot_to_rax(bytes, address_slot);
    bytes.extend_from_slice(&[0x48, 0x8b, 0x00]); // mov rax, qword ptr [rax]
    bytes.extend_from_slice(&[0x48, 0xba]); // mov rdx, imm64
    bytes.extend_from_slice(&expected.to_le_bytes());
    bytes.extend_from_slice(&[0x48, 0x39, 0xd0]); // cmp rax, rdx

    let jump_offset = bytes.len();
    bytes.extend_from_slice(&[0x0f, 0x85, 0x00, 0x00, 0x00, 0x00]); // jne failure
    jump_offsets.push(jump_offset);
}

fn emit_startup_operation_dispatch(
    bytes: &mut Vec<u8>,
    operation_kind_slot: u16,
    expected_kind: u32,
    dispatch_count_slot: u16,
    dispatch_count_after_row: u64,
    jump_offsets: &mut Vec<usize>,
) {
    compare_stack_slot_to_u64(
        bytes,
        operation_kind_slot,
        expected_kind as u64,
        jump_offsets,
    );
    bytes.push(0xb8); // mov eax, dispatch count
    bytes.extend_from_slice(&(dispatch_count_after_row as u32).to_le_bytes());
    store_rax_to_stack_slot(bytes, dispatch_count_slot);
}

fn emit_native_startup_operation_table_iteration(
    bytes: &mut Vec<u8>,
    startup_payloads: &EcsStartupPayloads,
    query_loop_observable: &NativeMoveQueryLoopObservable,
    storage_compatibility: NativeStorageCompatibilityModel,
    startup_state_failure_offsets: &mut Vec<usize>,
    scan_failure_offsets: &mut Vec<usize>,
    field_math_failure_offsets: &mut Vec<usize>,
    position_store_failure_offsets: &mut Vec<usize>,
    dispatch_failure_offsets: &mut Vec<usize>,
) {
    bytes.extend_from_slice(&[0x8b, 0x46, ECS_STARTUP_RECORD_COUNT_OFFSET]); // mov eax, dword ptr [rsi + offset]
    store_rax_to_stack_slot(bytes, ECS_STARTUP_OPERATION_COUNT_SLOT);

    for row in startup_operation_iteration_rows(startup_payloads) {
        compare_stack_slot_to_u64(
            bytes,
            row.count_slot.offset,
            row.expected_table_count,
            dispatch_failure_offsets,
        );
        emit_startup_operation_dispatch_row(bytes, row.dispatch_row, dispatch_failure_offsets);
        match row.dispatch_row.handler {
            NativeStartupOperationHandler::ResourcePayload => {
                emit_resource_payload_startup_operation_handler(
                    bytes,
                    startup_state_failure_offsets,
                )
            }
            NativeStartupOperationHandler::Spawn => {
                let spawn_row_index = (row.dispatch_row.dispatch_count_after_row - 1) as usize;
                emit_spawn_startup_operation_handler(
                    bytes,
                    spawn_row_index,
                    row.dispatch_row.dispatch_count_after_row,
                    storage_compatibility,
                    startup_state_failure_offsets,
                )
            }
            NativeStartupOperationHandler::RunSchedule => {
                emit_run_schedule_startup_operation_handler(
                    bytes,
                    startup_payloads,
                    query_loop_observable,
                    storage_compatibility,
                    startup_state_failure_offsets,
                    scan_failure_offsets,
                    field_math_failure_offsets,
                    position_store_failure_offsets,
                    dispatch_failure_offsets,
                )
            }
        }
    }
}

fn emit_startup_operation_dispatch_row(
    bytes: &mut Vec<u8>,
    row: NativeStartupOperationDispatchRow,
    jump_offsets: &mut Vec<usize>,
) {
    emit_startup_operation_dispatch(
        bytes,
        row.kind_slot,
        row.expected_kind,
        row.dispatch_count_slot,
        row.dispatch_count_after_row,
        jump_offsets,
    );
}

fn emit_resource_payload_startup_operation_handler(
    bytes: &mut Vec<u8>,
    jump_offsets: &mut Vec<usize>,
) {
    for (startup_table_slot, descriptor_slot) in ECS_RESOURCE_STARTUP_DESCRIPTOR_RELATIONS {
        compare_stack_slots_equal(bytes, startup_table_slot, descriptor_slot, jump_offsets);
    }
    load_metadata_dword_via_offset_slot(
        bytes,
        ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_OFFSET_SLOT,
        ECS_RESOURCE_PAYLOAD_STORAGE_SLOT,
    );
}

fn emit_spawn_startup_operation_handler(
    bytes: &mut Vec<u8>,
    spawn_row_index: usize,
    row_count_after_spawn: u64,
    storage_compatibility: NativeStorageCompatibilityModel,
    jump_offsets: &mut Vec<usize>,
) {
    let spawn_slots =
        startup_spawn_slots(spawn_row_index).expect("startup iteration row matches spawn table");
    let payload_slots = startup_payload_storage_slots(spawn_row_index)
        .expect("startup iteration row matches spawn payload storage");
    let catalog_table = storage_compatibility.catalog_table;

    compare_stack_slots_equal(
        bytes,
        spawn_slots.component_count.offset,
        catalog_table.slots.column_count.offset,
        jump_offsets,
    );
    compare_stack_slot_to_u64(
        bytes,
        catalog_table.slots.column_count.offset,
        catalog_table.columns.len() as u64,
        jump_offsets,
    );
    compare_stack_slot_to_u64(
        bytes,
        ECS_SPAWN_ROW_COUNT_SLOT,
        spawn_row_index as u64,
        jump_offsets,
    );
    compare_qword_at_stack_address_to_u64(
        bytes,
        catalog_table.slots.row_count_address.offset,
        spawn_row_index as u64,
        jump_offsets,
    );
    compare_stack_slot_to_u64(
        bytes,
        catalog_table.slots.capacity.offset,
        storage_compatibility.capacity,
        jump_offsets,
    );
    for (startup_table_slot, catalog_slot) in
        startup_spawn_catalog_relations(spawn_slots, catalog_table)
    {
        compare_stack_slots_equal(bytes, startup_table_slot, catalog_slot, jump_offsets);
    }
    for column in catalog_table.columns {
        compare_stack_slot_to_u64(
            bytes,
            column.slots.element_size.offset,
            u64::from(NATIVE_ECS_QWORD_BYTE_LEN),
            jump_offsets,
        );
    }

    load_metadata_qword_via_offset_slot(
        bytes,
        spawn_slots.position_payload_offset.offset,
        payload_slots.position_payload.offset,
    );
    load_metadata_qword_via_offset_slot(
        bytes,
        spawn_slots.velocity_payload_offset.offset,
        payload_slots.velocity_payload.offset,
    );
    emit_catalog_payload_store(
        bytes,
        catalog_table.columns[0],
        spawn_row_index,
        payload_slots.position_payload.offset,
    );
    emit_catalog_payload_store(
        bytes,
        catalog_table.columns[1],
        spawn_row_index,
        payload_slots.velocity_payload.offset,
    );

    load_stack_slot_to_rax(bytes, catalog_table.slots.row_count_address.offset);
    bytes.push(0xba); // mov edx, committed native archetype storage row count
    bytes.extend_from_slice(&(row_count_after_spawn as u32).to_le_bytes());
    bytes.extend_from_slice(&[0x48, 0x89, 0x10]); // mov qword ptr [rax], rdx
    bytes.push(0xb8); // mov eax, committed materialized spawn row count
    bytes.extend_from_slice(&(row_count_after_spawn as u32).to_le_bytes());
    store_rax_to_stack_slot(bytes, ECS_SPAWN_ROW_COUNT_SLOT);
}

fn startup_spawn_catalog_relations(
    spawn_slots: NativeSpawnStartupOperationSlots,
    catalog_table: NativeStorageCatalogTableRow,
) -> [(u16, u16); 4] {
    [
        (
            spawn_slots.position_component_id.offset,
            catalog_table.columns[0].slots.component_id.offset,
        ),
        (
            spawn_slots.position_payload_len.offset,
            catalog_table.columns[0].slots.element_size.offset,
        ),
        (
            spawn_slots.velocity_component_id.offset,
            catalog_table.columns[1].slots.component_id.offset,
        ),
        (
            spawn_slots.velocity_payload_len.offset,
            catalog_table.columns[1].slots.element_size.offset,
        ),
    ]
}

fn emit_catalog_payload_store(
    bytes: &mut Vec<u8>,
    column: NativeStorageCatalogColumnRow,
    row_index: usize,
    payload_slot: u16,
) {
    load_stack_slot_to_rax(bytes, column.slots.payload_base_address.offset);
    for _ in 0..row_index {
        add_stack_slot_to_rax(bytes, column.slots.element_size.offset);
    }
    load_stack_slot_to_rdx(bytes, payload_slot);
    bytes.extend_from_slice(&[0x48, 0x89, 0x10]); // mov qword ptr [rax], rdx
}

fn compare_catalog_payload_to_u64(
    bytes: &mut Vec<u8>,
    column: NativeStorageCatalogColumnRow,
    row_index: usize,
    expected: u64,
    jump_offsets: &mut Vec<usize>,
) {
    load_stack_slot_to_rax(bytes, column.slots.payload_base_address.offset);
    for _ in 0..row_index {
        add_stack_slot_to_rax(bytes, column.slots.element_size.offset);
    }
    bytes.extend_from_slice(&[0x48, 0x8b, 0x00]); // mov rax, qword ptr [rax]
    bytes.extend_from_slice(&[0x48, 0xba]); // mov rdx, imm64
    bytes.extend_from_slice(&expected.to_le_bytes());
    bytes.extend_from_slice(&[0x48, 0x39, 0xd0]); // cmp rax, rdx

    let jump_offset = bytes.len();
    bytes.extend_from_slice(&[0x0f, 0x85, 0x00, 0x00, 0x00, 0x00]); // jne failure
    jump_offsets.push(jump_offset);
}

fn emit_run_schedule_startup_operation_handler(
    bytes: &mut Vec<u8>,
    startup_payloads: &EcsStartupPayloads,
    query_loop_observable: &NativeMoveQueryLoopObservable,
    storage_compatibility: NativeStorageCompatibilityModel,
    startup_state_failure_offsets: &mut Vec<usize>,
    scan_failure_offsets: &mut Vec<usize>,
    field_math_failure_offsets: &mut Vec<usize>,
    position_store_failure_offsets: &mut Vec<usize>,
    dispatch_failure_offsets: &mut Vec<usize>,
) {
    emit_startup_operation_state_validations(
        bytes,
        startup_payloads,
        query_loop_observable,
        storage_compatibility,
        startup_state_failure_offsets,
        dispatch_failure_offsets,
    );
    emit_compiled_demo_main_schedule(
        bytes,
        query_loop_observable,
        storage_compatibility,
        scan_failure_offsets,
        field_math_failure_offsets,
        position_store_failure_offsets,
        dispatch_failure_offsets,
    );
}

fn emit_startup_operation_state_validations(
    bytes: &mut Vec<u8>,
    startup_payloads: &EcsStartupPayloads,
    query_loop_observable: &NativeMoveQueryLoopObservable,
    storage_compatibility: NativeStorageCompatibilityModel,
    startup_state_failure_offsets: &mut Vec<usize>,
    dispatch_failure_offsets: &mut Vec<usize>,
) {
    for (expected_count, stack_slot) in ECS_EXPECTED_DESCRIPTOR_COUNTS
        .iter()
        .zip(ECS_DESCRIPTOR_REGISTRY_SLOTS)
    {
        compare_stack_slot_to_u64(
            bytes,
            stack_slot,
            *expected_count,
            startup_state_failure_offsets,
        );
    }
    for (expected_offset, stack_slot) in ECS_EXPECTED_DESCRIPTOR_RECORD_OFFSETS
        .iter()
        .zip(ECS_DESCRIPTOR_RECORD_OFFSET_SLOTS)
    {
        compare_stack_slot_to_u64(
            bytes,
            stack_slot,
            *expected_offset,
            startup_state_failure_offsets,
        );
    }
    for (expected_byte_len, stack_slot) in ECS_EXPECTED_DESCRIPTOR_RECORD_BYTE_LENS
        .iter()
        .zip(ECS_DESCRIPTOR_RECORD_BYTE_LEN_SLOTS)
    {
        compare_stack_slot_to_u64(
            bytes,
            stack_slot,
            *expected_byte_len,
            startup_state_failure_offsets,
        );
    }
    for (stack_slot, expected) in ECS_COMPONENT_RESOURCE_DESCRIPTOR_EXPECTED {
        compare_stack_slot_to_u64(bytes, stack_slot, expected, startup_state_failure_offsets);
    }
    for (stack_slot, expected) in ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_EXPECTED {
        compare_stack_slot_to_u64(bytes, stack_slot, expected, startup_state_failure_offsets);
    }
    for (stack_slot, expected) in startup_operation_table_expected(startup_payloads) {
        compare_stack_slot_to_u64(bytes, stack_slot, expected, startup_state_failure_offsets);
    }
    compare_stack_slot_to_u64(
        bytes,
        ECS_RESOURCE_PAYLOAD_STORAGE_SLOT,
        u64::from(u32::from_le_bytes(startup_payloads.resource_payload)),
        startup_state_failure_offsets,
    );
    compare_stack_slot_to_u64(
        bytes,
        ECS_SPAWN_ROW_COUNT_SLOT,
        startup_payloads.spawn_operations.len() as u64,
        startup_state_failure_offsets,
    );
    let catalog_table = storage_compatibility.catalog_table;
    compare_qword_at_stack_address_to_u64(
        bytes,
        catalog_table.slots.row_count_address.offset,
        startup_payloads.spawn_operations.len() as u64,
        startup_state_failure_offsets,
    );
    compare_stack_slot_to_u64(
        bytes,
        catalog_table.slots.capacity.offset,
        storage_compatibility.capacity,
        startup_state_failure_offsets,
    );
    compare_stack_slot_to_u64(
        bytes,
        catalog_table.slots.row_stride.offset,
        storage_compatibility.row_stride,
        startup_state_failure_offsets,
    );
    for (row_index, spawn) in startup_payloads.spawn_operations.iter().enumerate() {
        let payload_slots = startup_payload_storage_slots(row_index)
            .expect("startup payload parser bounds match native payload slots");
        compare_stack_slot_to_u64(
            bytes,
            payload_slots.position_payload.offset,
            u64::from_le_bytes(spawn.position_payload),
            startup_state_failure_offsets,
        );
        compare_stack_slot_to_u64(
            bytes,
            payload_slots.velocity_payload.offset,
            u64::from_le_bytes(spawn.velocity_payload),
            startup_state_failure_offsets,
        );
        compare_catalog_payload_to_u64(
            bytes,
            catalog_table.columns[0],
            row_index,
            u64::from_le_bytes(spawn.position_payload),
            startup_state_failure_offsets,
        );
        compare_catalog_payload_to_u64(
            bytes,
            catalog_table.columns[1],
            row_index,
            u64::from_le_bytes(spawn.velocity_payload),
            startup_state_failure_offsets,
        );
    }

    for (dispatch_count_slot, expected_count) in [
        (ECS_STARTUP_RESOURCE_DISPATCH_COUNT_SLOT, 1),
        (
            ECS_STARTUP_SPAWN_DISPATCH_COUNT_SLOT,
            startup_payloads.spawn_operations.len() as u64,
        ),
        (ECS_STARTUP_RUN_SCHEDULE_DISPATCH_COUNT_SLOT, 1),
    ] {
        compare_stack_slot_to_u64(
            bytes,
            dispatch_count_slot,
            expected_count,
            dispatch_failure_offsets,
        );
    }

    bytes.extend_from_slice(&[0x48, 0xb8]); // mov rax, target Position payload
    bytes.extend_from_slice(&query_loop_observable.target_position_payload);
    store_rax_to_stack_slot(bytes, ECS_QUERY_LOOP_TARGET_POSITION_SLOT);
    compare_stack_slot_to_u64(
        bytes,
        ECS_QUERY_LOOP_TARGET_POSITION_SLOT,
        u64::from_le_bytes(query_loop_observable.target_position_payload),
        startup_state_failure_offsets,
    );
}

fn emit_native_descriptor_table_row_iteration(bytes: &mut Vec<u8>, jump_offsets: &mut Vec<usize>) {
    for row in ECS_DESCRIPTOR_TABLE_ITERATION_ROWS {
        compare_stack_slot_to_u64(
            bytes,
            row.count_slot.offset,
            row.expected_table_count,
            jump_offsets,
        );

        let (qword_loads, dword_loads): (&[(i32, u16)], &[(i32, u16)]) = match row.decode_family {
            NativeDescriptorDecodeFamily::ComponentResource => (
                &ECS_COMPONENT_RESOURCE_DESCRIPTOR_QWORD_LOADS,
                &ECS_COMPONENT_RESOURCE_DESCRIPTOR_DWORD_LOADS,
            ),
            NativeDescriptorDecodeFamily::SystemQuerySchedule => (
                &ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_QWORD_LOADS,
                &ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_DWORD_LOADS,
            ),
        };

        for (metadata_offset, stack_slot) in qword_loads
            .iter()
            .copied()
            .skip(row.qword_load_start)
            .take(row.qword_load_len)
        {
            emit_descriptor_qword_load(bytes, metadata_offset, stack_slot);
        }
        for (metadata_offset, stack_slot) in dword_loads
            .iter()
            .copied()
            .skip(row.dword_load_start)
            .take(row.dword_load_len)
        {
            emit_descriptor_dword_load(bytes, metadata_offset, stack_slot);
        }
    }
}

fn emit_descriptor_qword_load(bytes: &mut Vec<u8>, metadata_offset: i32, stack_slot: u16) {
    bytes.extend_from_slice(&[0x48, 0x8b, 0x86]); // mov rax, qword ptr [rsi + offset]
    bytes.extend_from_slice(&metadata_offset.to_le_bytes());
    store_rax_to_stack_slot(bytes, stack_slot);
}

fn emit_descriptor_dword_load(bytes: &mut Vec<u8>, metadata_offset: i32, stack_slot: u16) {
    bytes.extend_from_slice(&[0x8b, 0x86]); // mov eax, dword ptr [rsi + offset]
    bytes.extend_from_slice(&metadata_offset.to_le_bytes());
    store_rax_to_stack_slot(bytes, stack_slot);
}

fn emit_descriptor_name_table_decodes(bytes: &mut Vec<u8>, jump_offsets: &mut Vec<usize>) {
    for reference in ECS_DESCRIPTOR_NAME_REFERENCES {
        bytes.extend_from_slice(&[0x48, 0xb8]); // mov rax, descriptor name byte offset
        bytes.extend_from_slice(&reference.byte_offset.to_le_bytes());
        store_rax_to_stack_slot(bytes, reference.byte_offset_slot);

        bytes.extend_from_slice(&[0x8b, 0x86]); // mov eax, dword ptr [rsi + offset]
        bytes.extend_from_slice(&reference.byte_len_offset.to_le_bytes());
        store_rax_to_stack_slot(bytes, reference.byte_len_slot);

        compare_stack_slot_to_u64(
            bytes,
            reference.byte_offset_slot,
            reference.byte_offset,
            jump_offsets,
        );
        compare_stack_slot_to_u64(
            bytes,
            reference.byte_len_slot,
            reference.name.len() as u64,
            jump_offsets,
        );
        compare_metadata_ascii_bytes(
            bytes,
            reference.byte_offset as i32,
            reference.name.as_bytes(),
            jump_offsets,
        );
    }
}

fn compare_metadata_ascii_bytes(
    bytes: &mut Vec<u8>,
    metadata_offset: i32,
    expected: &[u8],
    jump_offsets: &mut Vec<usize>,
) {
    let mut offset = 0usize;
    while expected.len() - offset >= 8 {
        let mut chunk = [0u8; 8];
        chunk.copy_from_slice(&expected[offset..offset + 8]);
        compare_metadata_qword_to_u64(
            bytes,
            metadata_offset + offset as i32,
            u64::from_le_bytes(chunk),
            jump_offsets,
        );
        offset += 8;
    }
    if expected.len() - offset >= 4 {
        let mut chunk = [0u8; 4];
        chunk.copy_from_slice(&expected[offset..offset + 4]);
        compare_metadata_dword_to_u32(
            bytes,
            metadata_offset + offset as i32,
            u32::from_le_bytes(chunk),
            jump_offsets,
        );
        offset += 4;
    }
    while offset < expected.len() {
        compare_metadata_byte_to_u8(
            bytes,
            metadata_offset + offset as i32,
            expected[offset],
            jump_offsets,
        );
        offset += 1;
    }
}

fn compare_metadata_qword_to_u64(
    bytes: &mut Vec<u8>,
    metadata_offset: i32,
    expected: u64,
    jump_offsets: &mut Vec<usize>,
) {
    bytes.extend_from_slice(&[0x48, 0xb8]); // mov rax, imm64
    bytes.extend_from_slice(&expected.to_le_bytes());
    bytes.extend_from_slice(&[0x48, 0x39, 0x86]); // cmp qword ptr [rsi + offset], rax
    bytes.extend_from_slice(&metadata_offset.to_le_bytes());

    let jump_offset = bytes.len();
    bytes.extend_from_slice(&[0x0f, 0x85, 0x00, 0x00, 0x00, 0x00]); // jne failure
    jump_offsets.push(jump_offset);
}

fn compare_metadata_dword_to_u32(
    bytes: &mut Vec<u8>,
    metadata_offset: i32,
    expected: u32,
    jump_offsets: &mut Vec<usize>,
) {
    bytes.push(0xb8); // mov eax, imm32
    bytes.extend_from_slice(&expected.to_le_bytes());
    bytes.extend_from_slice(&[0x39, 0x86]); // cmp dword ptr [rsi + offset], eax
    bytes.extend_from_slice(&metadata_offset.to_le_bytes());

    let jump_offset = bytes.len();
    bytes.extend_from_slice(&[0x0f, 0x85, 0x00, 0x00, 0x00, 0x00]); // jne failure
    jump_offsets.push(jump_offset);
}

fn compare_metadata_byte_to_u8(
    bytes: &mut Vec<u8>,
    metadata_offset: i32,
    expected: u8,
    jump_offsets: &mut Vec<usize>,
) {
    bytes.extend_from_slice(&[0x80, 0xbe]); // cmp byte ptr [rsi + offset], imm8
    bytes.extend_from_slice(&metadata_offset.to_le_bytes());
    bytes.push(expected);

    let jump_offset = bytes.len();
    bytes.extend_from_slice(&[0x0f, 0x85, 0x00, 0x00, 0x00, 0x00]); // jne failure
    jump_offsets.push(jump_offset);
}

fn emit_startup_operation_table_decodes(
    bytes: &mut Vec<u8>,
    startup_payloads: &EcsStartupPayloads,
) {
    emit_startup_table_dword_load(
        bytes,
        startup_payloads.resource_operation_kind_offset,
        ECS_STARTUP_TABLE_RESOURCE_KIND_SLOT,
    );
    emit_startup_table_qword_load(
        bytes,
        startup_payloads.resource_id_offset,
        ECS_STARTUP_TABLE_RESOURCE_ID_SLOT,
    );
    emit_startup_table_payload_offset(
        bytes,
        startup_payloads.resource_payload_offset as u64,
        ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_OFFSET_SLOT,
    );
    emit_startup_table_dword_load(
        bytes,
        startup_payloads.resource_payload_len_offset,
        ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_LEN_SLOT,
    );

    for (row_index, spawn) in startup_payloads.spawn_operations.iter().enumerate() {
        let slots = startup_spawn_slots(row_index)
            .expect("startup payload parser bounds match native spawn slots");
        emit_startup_table_dword_load(bytes, spawn.operation_kind_offset, slots.kind.offset);
        emit_startup_table_dword_load(
            bytes,
            spawn.component_count_offset,
            slots.component_count.offset,
        );
        emit_startup_table_qword_load(
            bytes,
            spawn.position_component_id_offset,
            slots.position_component_id.offset,
        );
        emit_startup_table_payload_offset(
            bytes,
            spawn.position_payload_offset as u64,
            slots.position_payload_offset.offset,
        );
        emit_startup_table_dword_load(
            bytes,
            spawn.position_payload_len_offset,
            slots.position_payload_len.offset,
        );
        emit_startup_table_qword_load(
            bytes,
            spawn.velocity_component_id_offset,
            slots.velocity_component_id.offset,
        );
        emit_startup_table_payload_offset(
            bytes,
            spawn.velocity_payload_offset as u64,
            slots.velocity_payload_offset.offset,
        );
        emit_startup_table_dword_load(
            bytes,
            spawn.velocity_payload_len_offset,
            slots.velocity_payload_len.offset,
        );
    }

    emit_startup_table_dword_load(
        bytes,
        startup_payloads.run_schedule_operation_kind_offset,
        ECS_STARTUP_TABLE_RUN_SCHEDULE_KIND_SLOT,
    );
    emit_startup_table_qword_load(
        bytes,
        startup_payloads.run_schedule_id_offset,
        ECS_STARTUP_TABLE_RUN_SCHEDULE_ID_SLOT,
    );
}

fn emit_native_storage_catalog_materialization(
    bytes: &mut Vec<u8>,
    storage_plan: &NativeWorldStoragePlan,
) {
    for table in &storage_plan.tables {
        emit_u64_to_stack_slot(
            bytes,
            table.columns.len() as u64,
            table.catalog.column_count.offset,
        );

        emit_lea_stack_address_to_rax(bytes, table.storage.row_count.offset);
        store_rax_to_stack_slot(bytes, table.catalog.row_count_address.offset);

        emit_u64_to_stack_slot(
            bytes,
            u64::from(table.capacity),
            table.storage.capacity.offset,
        );
        emit_u64_to_stack_slot(
            bytes,
            u64::from(table.capacity),
            table.catalog.capacity.offset,
        );
        emit_u64_to_stack_slot(
            bytes,
            u64::from(table.logical_row_stride),
            table.storage.row_stride.offset,
        );
        emit_u64_to_stack_slot(
            bytes,
            u64::from(table.logical_row_stride),
            table.catalog.row_stride.offset,
        );

        for column in &table.columns {
            emit_u64_to_stack_slot(bytes, column.schema.id, column.catalog.component_id.offset);
            emit_u64_to_stack_slot(
                bytes,
                u64::from(column.schema.size),
                column.catalog.element_size.offset,
            );
            emit_u64_to_stack_slot(
                bytes,
                u64::from(column.schema.align),
                column.catalog.element_align.offset,
            );
            emit_lea_stack_address_to_rax(bytes, column.payload.offset);
            store_rax_to_stack_slot(bytes, column.catalog.payload_base_address.offset);
        }
    }
}

fn emit_u64_to_stack_slot(bytes: &mut Vec<u8>, value: u64, stack_slot: u16) {
    bytes.extend_from_slice(&[0x48, 0xb8]); // mov rax, immediate
    bytes.extend_from_slice(&value.to_le_bytes());
    store_rax_to_stack_slot(bytes, stack_slot);
}

fn emit_startup_table_qword_load(bytes: &mut Vec<u8>, metadata_offset: i32, stack_slot: u16) {
    bytes.extend_from_slice(&[0x48, 0x8b, 0x86]); // mov rax, qword ptr [rsi + offset]
    bytes.extend_from_slice(&metadata_offset.to_le_bytes());
    store_rax_to_stack_slot(bytes, stack_slot);
}

fn emit_startup_table_dword_load(bytes: &mut Vec<u8>, metadata_offset: i32, stack_slot: u16) {
    bytes.extend_from_slice(&[0x8b, 0x86]); // mov eax, dword ptr [rsi + offset]
    bytes.extend_from_slice(&metadata_offset.to_le_bytes());
    store_rax_to_stack_slot(bytes, stack_slot);
}

fn emit_startup_table_payload_offset(bytes: &mut Vec<u8>, payload_offset: u64, stack_slot: u16) {
    bytes.extend_from_slice(&[0x48, 0xb8]); // mov rax, payload offset
    bytes.extend_from_slice(&payload_offset.to_le_bytes());
    store_rax_to_stack_slot(bytes, stack_slot);
}

fn load_metadata_dword_via_offset_slot(bytes: &mut Vec<u8>, offset_slot: u16, target_slot: u16) {
    load_stack_slot_to_rax(bytes, offset_slot);
    bytes.extend_from_slice(&[0x8b, 0x04, 0x06]); // mov eax, dword ptr [rsi + rax]
    store_eax_to_stack_dword_slot(bytes, target_slot);
}

fn load_metadata_qword_via_offset_slot(bytes: &mut Vec<u8>, offset_slot: u16, target_slot: u16) {
    load_stack_slot_to_rax(bytes, offset_slot);
    bytes.extend_from_slice(&[0x48, 0x8b, 0x04, 0x06]); // mov rax, qword ptr [rsi + rax]
    store_rax_to_stack_slot(bytes, target_slot);
}

fn emit_native_query_plan_builder(
    bytes: &mut Vec<u8>,
    row: NativeQueryPlanTableIterationRow,
    expected_matched_row_count: u64,
    scan_failure_offsets: &mut Vec<usize>,
) {
    emit_native_query_plan_build_row(
        bytes,
        row.build_row,
        expected_matched_row_count,
        scan_failure_offsets,
    );
}

fn emit_native_query_plan_build_row(
    bytes: &mut Vec<u8>,
    row: NativeQueryPlanBuildRow,
    expected_matched_row_count: u64,
    scan_failure_offsets: &mut Vec<usize>,
) {
    load_stack_slot_to_rax(bytes, row.query_id_slot);
    store_rax_to_stack_slot(bytes, row.plan_query_id_slot);
    load_stack_slot_to_rax(bytes, row.query_term_count_slot);
    store_rax_to_stack_slot(bytes, row.plan_term_count_slot);
    compare_stack_slots_equal(
        bytes,
        row.plan_term_count_slot,
        row.system_query_term_count_slot,
        scan_failure_offsets,
    );
    compare_stack_slot_to_u64(bytes, row.plan_term_count_slot, 2, scan_failure_offsets);
    compare_stack_slots_equal(
        bytes,
        row.plan_term_count_slot,
        row.catalog_column_count_slot,
        scan_failure_offsets,
    );

    for term in row.terms {
        emit_native_query_plan_term_row(bytes, term, scan_failure_offsets);
    }

    load_stack_slot_to_rax(bytes, row.catalog_row_count_address_slot);
    bytes.extend_from_slice(&[0x48, 0x8b, 0x00]); // mov rax, qword ptr [rax]
    store_rax_to_stack_slot(bytes, row.matched_row_count_slot);
    compare_stack_slot_to_u64(
        bytes,
        row.matched_row_count_slot,
        expected_matched_row_count,
        scan_failure_offsets,
    );

    for term in row.terms {
        load_stack_slot_to_rax(bytes, term.catalog_payload_base_address_slot);
        store_rax_to_stack_slot(bytes, term.planned_payload_address_slot);
    }
}

fn emit_native_query_plan_term_row(
    bytes: &mut Vec<u8>,
    term: NativeQueryPlanTermBuildRow,
    scan_failure_offsets: &mut Vec<usize>,
) {
    load_stack_slot_to_rax(bytes, term.query_access_slot);
    store_rax_to_stack_slot(bytes, term.plan_access_slot);
    load_stack_slot_to_rax(bytes, term.catalog_component_id_slot);
    store_rax_to_stack_slot(bytes, term.plan_component_id_slot);
    load_stack_slot_to_rax(bytes, term.catalog_element_size_slot);
    store_rax_to_stack_slot(bytes, term.plan_size_slot);
    load_stack_slot_to_rax(bytes, term.component_x_field_offset_slot);
    store_rax_to_stack_slot(bytes, term.plan_x_field_offset_slot);
    load_stack_slot_to_rax(bytes, term.component_y_field_offset_slot);
    store_rax_to_stack_slot(bytes, term.plan_y_field_offset_slot);

    compare_stack_slot_to_u64(
        bytes,
        term.plan_access_slot,
        term.expected_access,
        scan_failure_offsets,
    );
    compare_stack_slot_to_u64(
        bytes,
        term.plan_size_slot,
        term.expected_size,
        scan_failure_offsets,
    );
    compare_stack_slot_to_u64(
        bytes,
        term.plan_x_field_offset_slot,
        term.expected_x_field_offset,
        scan_failure_offsets,
    );
    compare_stack_slot_to_u64(
        bytes,
        term.plan_y_field_offset_slot,
        term.expected_y_field_offset,
        scan_failure_offsets,
    );
    compare_stack_slots_equal(
        bytes,
        term.plan_access_slot,
        term.system_access_slot,
        scan_failure_offsets,
    );
    compare_stack_slots_equal(
        bytes,
        term.plan_component_id_slot,
        term.query_component_id_slot,
        scan_failure_offsets,
    );
    compare_stack_slots_equal(
        bytes,
        term.plan_component_id_slot,
        term.system_component_id_slot,
        scan_failure_offsets,
    );
    compare_stack_slots_equal(
        bytes,
        term.plan_component_id_slot,
        term.component_descriptor_id_slot,
        scan_failure_offsets,
    );
    compare_stack_slots_equal(
        bytes,
        term.plan_size_slot,
        term.component_size_slot,
        scan_failure_offsets,
    );
}

fn emit_compiled_demo_main_schedule(
    bytes: &mut Vec<u8>,
    query_loop_observable: &NativeMoveQueryLoopObservable,
    storage_compatibility: NativeStorageCompatibilityModel,
    scan_failure_offsets: &mut Vec<usize>,
    field_math_failure_offsets: &mut Vec<usize>,
    position_store_failure_offsets: &mut Vec<usize>,
    dispatch_failure_offsets: &mut Vec<usize>,
) {
    let query_execution = NativeCompiledQueryExecution {
        observable: query_loop_observable,
        storage_compatibility,
    };
    for row in ECS_COMPILED_SCHEDULE_BUILD_ROWS {
        emit_compiled_schedule_build_row(
            bytes,
            row,
            query_execution,
            scan_failure_offsets,
            field_math_failure_offsets,
            position_store_failure_offsets,
            dispatch_failure_offsets,
        );
    }
}

fn emit_compiled_schedule_build_row(
    bytes: &mut Vec<u8>,
    row: NativeCompiledScheduleBuildRow,
    query_execution: NativeCompiledQueryExecution<'_>,
    scan_failure_offsets: &mut Vec<usize>,
    field_math_failure_offsets: &mut Vec<usize>,
    position_store_failure_offsets: &mut Vec<usize>,
    dispatch_failure_offsets: &mut Vec<usize>,
) {
    load_stack_slot_to_rax(bytes, row.startup_schedule_id_slot);
    store_rax_to_stack_slot(bytes, row.compiled_schedule_id_slot);
    load_stack_slot_to_rax(bytes, row.descriptor_run_system_id_slot);
    store_rax_to_stack_slot(bytes, row.compiled_scheduled_system_id_slot);
    load_stack_slot_to_rax(bytes, row.descriptor_item_count_slot);
    store_rax_to_stack_slot(bytes, row.compiled_scheduled_system_count_slot);

    compare_stack_slots_equal(
        bytes,
        row.compiled_schedule_id_slot,
        row.descriptor_schedule_id_slot,
        dispatch_failure_offsets,
    );
    compare_stack_slots_equal(
        bytes,
        row.compiled_scheduled_system_id_slot,
        row.system_descriptor_id_slot,
        dispatch_failure_offsets,
    );
    compare_stack_slot_to_u64(
        bytes,
        row.compiled_scheduled_system_count_slot,
        row.expected_scheduled_system_count,
        dispatch_failure_offsets,
    );
    compare_stack_slot_to_u64(
        bytes,
        row.compiled_scheduled_system_id_slot,
        row.expected_scheduled_system_id,
        dispatch_failure_offsets,
    );

    emit_native_query_plan_builder(
        bytes,
        native_query_plan_iteration_row(query_execution.storage_compatibility),
        query_execution.observable.rows.len() as u64,
        scan_failure_offsets,
    );
    emit_compiled_demo_move_query_loop(
        bytes,
        query_execution.observable,
        scan_failure_offsets,
        field_math_failure_offsets,
        position_store_failure_offsets,
    );
}

fn emit_compiled_demo_move_query_loop(
    bytes: &mut Vec<u8>,
    query_loop_observable: &NativeMoveQueryLoopObservable,
    scan_failure_offsets: &mut Vec<usize>,
    field_math_failure_offsets: &mut Vec<usize>,
    position_store_failure_offsets: &mut Vec<usize>,
) {
    load_stack_slot_to_rax(bytes, ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT);
    store_rax_to_stack_slot(bytes, ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT);
    compare_stack_slot_to_u64(
        bytes,
        ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT,
        query_loop_observable.rows.len() as u64,
        scan_failure_offsets,
    );

    for row in &query_loop_observable.rows {
        emit_query_loop_payload_address_row(bytes, row);

        emit_query_loop_field_multiply(bytes);
        compare_stack_slot_to_u64(
            bytes,
            ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
            u64::from_le_bytes(row.field_product_payload),
            field_math_failure_offsets,
        );

        emit_query_loop_position_stores(bytes);
        compare_qword_at_stack_address_to_u64(
            bytes,
            ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
            u64::from_le_bytes(row.target_position_payload),
            position_store_failure_offsets,
        );
    }
}

fn emit_query_loop_payload_address_row(
    bytes: &mut Vec<u8>,
    row: &NativeMoveQueryLoopRowObservable,
) {
    if row.row_index == 0 {
        return;
    }

    for (planned_address_slot, plan_size_slot) in [
        (
            ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
            ECS_DESCRIPTOR_QUERY_PLAN_POSITION_SIZE_SLOT,
        ),
        (
            ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
            ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_SIZE_SLOT,
        ),
    ] {
        load_stack_slot_to_rax(bytes, planned_address_slot);
        for _ in 0..row.row_index {
            add_stack_slot_to_rax(bytes, plan_size_slot);
        }
        store_rax_to_stack_slot(bytes, planned_address_slot);
    }
}

fn emit_query_loop_field_multiply(bytes: &mut Vec<u8>) {
    load_stack_slot_to_rax(bytes, ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT);
    emit_movss_xmm_from_rax(bytes, 0, 0);
    emit_movss_xmm_from_stack(bytes, 1, ECS_RESOURCE_PAYLOAD_STORAGE_SLOT);
    emit_mulss_xmm(bytes, 0, 1);
    emit_movss_stack_from_xmm(bytes, ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT, 0);

    load_stack_slot_to_rax(bytes, ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT);
    emit_movss_xmm_from_rax(bytes, 0, 4);
    emit_movss_xmm_from_stack(bytes, 1, ECS_RESOURCE_PAYLOAD_STORAGE_SLOT);
    emit_mulss_xmm(bytes, 0, 1);
    emit_movss_stack_from_xmm(bytes, ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT + 4, 0);
}

fn emit_query_loop_position_stores(bytes: &mut Vec<u8>) {
    load_stack_slot_to_rax(bytes, ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT);
    emit_movss_xmm_from_rax(bytes, 0, 0);
    emit_movss_xmm_from_stack(bytes, 1, ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT);
    emit_addss_xmm(bytes, 0, 1);
    emit_movss_rax_from_xmm(bytes, 0, 0);

    load_stack_slot_to_rax(bytes, ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT);
    emit_movss_xmm_from_rax(bytes, 0, 4);
    emit_movss_xmm_from_stack(bytes, 1, ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT + 4);
    emit_addss_xmm(bytes, 0, 1);
    emit_movss_rax_from_xmm(bytes, 4, 0);
}

fn emit_movss_xmm_from_stack(bytes: &mut Vec<u8>, xmm_register: u8, stack_slot: u16) {
    bytes.extend_from_slice(&[0xf3, 0x0f, 0x10]);
    if stack_slot <= 127 {
        bytes.push(0x44 | (xmm_register << 3));
        bytes.extend_from_slice(&[0x24, stack_slot as u8]);
    } else {
        bytes.push(0x84 | (xmm_register << 3));
        bytes.push(0x24);
        bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
    }
}

fn emit_movss_stack_from_xmm(bytes: &mut Vec<u8>, stack_slot: u16, xmm_register: u8) {
    bytes.extend_from_slice(&[0xf3, 0x0f, 0x11]);
    if stack_slot <= 127 {
        bytes.push(0x44 | (xmm_register << 3));
        bytes.extend_from_slice(&[0x24, stack_slot as u8]);
    } else {
        bytes.push(0x84 | (xmm_register << 3));
        bytes.push(0x24);
        bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
    }
}

fn emit_movss_xmm_from_rax(bytes: &mut Vec<u8>, xmm_register: u8, field_offset: u8) {
    bytes.extend_from_slice(&[0xf3, 0x0f, 0x10]);
    bytes.push(0x40 | (xmm_register << 3));
    bytes.push(field_offset);
}

fn emit_movss_rax_from_xmm(bytes: &mut Vec<u8>, field_offset: u8, xmm_register: u8) {
    bytes.extend_from_slice(&[0xf3, 0x0f, 0x11]);
    bytes.push(0x40 | (xmm_register << 3));
    bytes.push(field_offset);
}

fn emit_mulss_xmm(bytes: &mut Vec<u8>, destination_xmm_register: u8, source_xmm_register: u8) {
    bytes.extend_from_slice(&[0xf3, 0x0f, 0x59]);
    bytes.push(0xc0 | (destination_xmm_register << 3) | source_xmm_register);
}

fn emit_addss_xmm(bytes: &mut Vec<u8>, destination_xmm_register: u8, source_xmm_register: u8) {
    bytes.extend_from_slice(&[0xf3, 0x0f, 0x58]);
    bytes.push(0xc0 | (destination_xmm_register << 3) | source_xmm_register);
}

fn move_edi_exit_code(bytes: &mut Vec<u8>, exit_code: u8) {
    bytes.extend_from_slice(&[0xbf, exit_code, 0x00, 0x00, 0x00]); // mov edi, exit_code
}

fn store_rax_to_stack_slot(bytes: &mut Vec<u8>, stack_slot: u16) {
    if stack_slot == 0 {
        bytes.extend_from_slice(&[0x48, 0x89, 0x04, 0x24]); // mov qword ptr [rsp], rax
    } else if stack_slot <= 127 {
        bytes.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, stack_slot as u8]); // mov qword ptr [rsp + slot], rax
    } else {
        bytes.extend_from_slice(&[0x48, 0x89, 0x84, 0x24]); // mov qword ptr [rsp + slot], rax
        bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
    }
}

fn store_eax_to_stack_dword_slot(bytes: &mut Vec<u8>, stack_slot: u16) {
    if stack_slot == 0 {
        bytes.extend_from_slice(&[0x89, 0x04, 0x24]); // mov dword ptr [rsp], eax
    } else if stack_slot <= 127 {
        bytes.extend_from_slice(&[0x89, 0x44, 0x24, stack_slot as u8]); // mov dword ptr [rsp + slot], eax
    } else {
        bytes.extend_from_slice(&[0x89, 0x84, 0x24]); // mov dword ptr [rsp + slot], eax
        bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
    }
}

fn load_stack_slot_to_rax(bytes: &mut Vec<u8>, stack_slot: u16) {
    if stack_slot == 0 {
        bytes.extend_from_slice(&[0x48, 0x8b, 0x04, 0x24]); // mov rax, qword ptr [rsp]
    } else if stack_slot <= 127 {
        bytes.extend_from_slice(&[0x48, 0x8b, 0x44, 0x24, stack_slot as u8]); // mov rax, qword ptr [rsp + slot]
    } else {
        bytes.extend_from_slice(&[0x48, 0x8b, 0x84, 0x24]); // mov rax, qword ptr [rsp + slot]
        bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
    }
}

fn load_stack_slot_to_rdx(bytes: &mut Vec<u8>, stack_slot: u16) {
    if stack_slot == 0 {
        bytes.extend_from_slice(&[0x48, 0x8b, 0x14, 0x24]); // mov rdx, qword ptr [rsp]
    } else if stack_slot <= 127 {
        bytes.extend_from_slice(&[0x48, 0x8b, 0x54, 0x24, stack_slot as u8]); // mov rdx, qword ptr [rsp + slot]
    } else {
        bytes.extend_from_slice(&[0x48, 0x8b, 0x94, 0x24]); // mov rdx, qword ptr [rsp + slot]
        bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
    }
}

fn add_stack_slot_to_rax(bytes: &mut Vec<u8>, stack_slot: u16) {
    if stack_slot == 0 {
        bytes.extend_from_slice(&[0x48, 0x03, 0x04, 0x24]); // add rax, qword ptr [rsp]
    } else if stack_slot <= 127 {
        bytes.extend_from_slice(&[0x48, 0x03, 0x44, 0x24, stack_slot as u8]); // add rax, qword ptr [rsp + slot]
    } else {
        bytes.extend_from_slice(&[0x48, 0x03, 0x84, 0x24]); // add rax, qword ptr [rsp + slot]
        bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
    }
}

fn emit_lea_stack_address_to_rax(bytes: &mut Vec<u8>, stack_slot: u16) {
    if stack_slot <= 127 {
        bytes.extend_from_slice(&[0x48, 0x8d, 0x44, 0x24, stack_slot as u8]); // lea rax, [rsp + slot]
    } else {
        bytes.extend_from_slice(&[0x48, 0x8d, 0x84, 0x24]); // lea rax, [rsp + slot]
        bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
    }
}

fn patch_rel32(bytes: &mut [u8], patch_offset: usize, target_offset: usize, next_offset: usize) {
    let displacement = target_offset as i32 - next_offset as i32;
    patch_i32(bytes, patch_offset, displacement);
}

fn patch_i32(bytes: &mut [u8], offset: usize, value: i32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn runtime_wrapped_payload(startup_body: &[u8], frame_size: u16) -> Vec<u8> {
    let create_prefix = runtime_create_prefix(frame_size);
    let destroy_suffix = runtime_destroy_suffix(frame_size);
    let mut bytes =
        Vec::with_capacity(create_prefix.len() + startup_body.len() + destroy_suffix.len());

    bytes.extend_from_slice(&create_prefix);
    bytes.extend_from_slice(startup_body);
    bytes.extend_from_slice(&destroy_suffix);

    bytes
}

fn runtime_create_prefix(frame_size: u16) -> Vec<u8> {
    let mut bytes = Vec::new();
    emit_stack_frame_adjust(&mut bytes, 0xec, frame_size);
    bytes.extend_from_slice(&[0x31, 0xc0]); // xor eax, eax
    for offset in (0..frame_size).step_by(usize::from(NATIVE_ECS_QWORD_BYTE_LEN)) {
        store_rax_to_stack_slot(&mut bytes, offset);
    }
    bytes
}

fn runtime_destroy_suffix(frame_size: u16) -> Vec<u8> {
    let mut bytes = vec![0x31, 0xc0]; // xor eax, eax
    for offset in (0..frame_size).step_by(usize::from(NATIVE_ECS_QWORD_BYTE_LEN)) {
        store_rax_to_stack_slot(&mut bytes, offset);
    }
    emit_stack_frame_adjust(&mut bytes, 0xc4, frame_size);
    bytes.extend_from_slice(&[
        0xb8, 0x3c, 0x00, 0x00, 0x00, // mov eax, 60
        0x0f, 0x05, // syscall
    ]);
    bytes
}

fn emit_stack_frame_adjust(bytes: &mut Vec<u8>, opcode: u8, frame_size: u16) {
    if frame_size <= 127 {
        bytes.extend_from_slice(&[0x48, 0x83, opcode, frame_size as u8]);
    } else {
        bytes.extend_from_slice(&[0x48, 0x81, opcode]);
        bytes.extend_from_slice(&(frame_size as u32).to_le_bytes());
    }
}

fn immediate_exit_body(expression: &Expression) -> Result<Vec<u8>, CodegenError> {
    let Expression::Integer(integer) = expression else {
        return Err(unsupported_shape());
    };

    if integer.value <= 255 {
        let exit_code = (integer.value as u32).to_le_bytes();
        Ok(vec![
            0xbf,
            exit_code[0],
            exit_code[1],
            exit_code[2],
            exit_code[3], // mov edi, exit_code
        ])
    } else {
        Err(CodegenError {
            message: format!(
                "exit code must be an integer process status in 0..=255: {}",
                integer.value
            ),
        })
    }
}

fn local_arithmetic_exit_body(expression: &Expression) -> Result<Vec<u8>, CodegenError> {
    let Expression::Binary(binary) = expression else {
        return Err(unsupported_shape());
    };

    let left = integer_expression(&binary.left)?;
    let right = integer_expression(&binary.right)?;

    match binary.operator {
        BinaryOperator::Add => add_stack_slot_exit_body(left, right),
        BinaryOperator::Subtract => sub_stack_slot_exit_body(left, right),
        BinaryOperator::Multiply => mul_stack_slot_exit_body(left, right),
    }
}

fn integer_expression(expression: &Expression) -> Result<u64, CodegenError> {
    match expression {
        Expression::Integer(integer) => Ok(integer.value),
        Expression::Identifier { .. } | Expression::FieldAccess { .. } | Expression::Binary(_) => {
            Err(unsupported_shape())
        }
    }
}

fn add_stack_slot_exit_body(left: u64, right: u64) -> Result<Vec<u8>, CodegenError> {
    let left = i32_immediate(left, "left operand")?.to_le_bytes();
    let right = i32_immediate(right, "right operand")?.to_le_bytes();
    let mut bytes = Vec::with_capacity(25);

    bytes.extend_from_slice(&[0x48, 0x83, 0xec, 0x08]); // sub rsp, 8
    bytes.extend_from_slice(&[0xc7, 0x04, 0x24]); // mov dword ptr [rsp], imm32
    bytes.extend_from_slice(&left);
    bytes.extend_from_slice(&[0x81, 0x04, 0x24]); // add dword ptr [rsp], imm32
    bytes.extend_from_slice(&right);
    bytes.extend_from_slice(&[0x8b, 0x3c, 0x24]); // mov edi, dword ptr [rsp]
    bytes.extend_from_slice(&[0x48, 0x83, 0xc4, 0x08]); // add rsp, 8

    Ok(bytes)
}

fn sub_stack_slot_exit_body(left: u64, right: u64) -> Result<Vec<u8>, CodegenError> {
    let left = i32_immediate(left, "left operand")?.to_le_bytes();
    let right = i32_immediate(right, "right operand")?.to_le_bytes();
    let mut bytes = Vec::with_capacity(25);

    bytes.extend_from_slice(&[0x48, 0x83, 0xec, 0x08]); // sub rsp, 8
    bytes.extend_from_slice(&[0xc7, 0x04, 0x24]); // mov dword ptr [rsp], imm32
    bytes.extend_from_slice(&left);
    bytes.extend_from_slice(&[0x81, 0x2c, 0x24]); // sub dword ptr [rsp], imm32
    bytes.extend_from_slice(&right);
    bytes.extend_from_slice(&[0x8b, 0x3c, 0x24]); // mov edi, dword ptr [rsp]
    bytes.extend_from_slice(&[0x48, 0x83, 0xc4, 0x08]); // add rsp, 8

    Ok(bytes)
}

fn mul_stack_slot_exit_body(left: u64, right: u64) -> Result<Vec<u8>, CodegenError> {
    let left = i32_immediate(left, "left operand")?.to_le_bytes();
    let right = i32_immediate(right, "right operand")?.to_le_bytes();
    let mut bytes = Vec::with_capacity(22);

    bytes.extend_from_slice(&[0x48, 0x83, 0xec, 0x08]); // sub rsp, 8
    bytes.extend_from_slice(&[0xc7, 0x04, 0x24]); // mov dword ptr [rsp], imm32
    bytes.extend_from_slice(&left);
    bytes.extend_from_slice(&[0x69, 0x3c, 0x24]); // imul edi, dword ptr [rsp], imm32
    bytes.extend_from_slice(&right);
    bytes.extend_from_slice(&[0x48, 0x83, 0xc4, 0x08]); // add rsp, 8

    Ok(bytes)
}

fn i32_immediate(value: u64, label: &str) -> Result<i32, CodegenError> {
    if value <= i32::MAX as u64 {
        Ok(value as i32)
    } else {
        Err(CodegenError {
            message: format!("{label} must fit in signed 32-bit immediate: {value}"),
        })
    }
}

fn unsupported_shape() -> CodegenError {
    CodegenError {
        message: "unsupported executable startup shape".to_string(),
    }
}

fn metadata_decoder_error() -> CodegenError {
    CodegenError {
        message: "ECS metadata decoder executable requires final `exit 0`".to_string(),
    }
}

fn metadata_startup_payload_error() -> CodegenError {
    CodegenError {
        message: "ECS metadata decoder executable requires a 4-byte resource payload, one or two two-component spawn operations, and `run Demo.Main`"
            .to_string(),
    }
}

fn native_move_query_loop_observable_error() -> CodegenError {
    CodegenError {
        message: "native query-loop observable requires the supported Demo.Move Core query loop"
            .to_string(),
    }
}

fn native_storage_compatibility_model(
    plan: &NativeWorldStoragePlan,
    query_loop_observable: &NativeMoveQueryLoopObservable,
) -> Result<NativeStorageCompatibilityModel, CodegenError> {
    let [table] = plan.tables.as_slice() else {
        return Err(native_storage_compatibility_error());
    };
    if table.columns.len() != 2 {
        return Err(native_storage_compatibility_error());
    }
    let position_column = table
        .columns
        .iter()
        .find(|column| column.schema.id == query_loop_observable.position_component_id)
        .ok_or_else(native_storage_compatibility_error)?;
    let velocity_column = table
        .columns
        .iter()
        .find(|column| column.schema.id == query_loop_observable.velocity_component_id)
        .ok_or_else(native_storage_compatibility_error)?;
    if table.rows.is_empty()
        || table.rows.len() > 2
        || position_column.schema.id == velocity_column.schema.id
        || position_column.schema.size != u32::from(NATIVE_ECS_QWORD_BYTE_LEN)
        || velocity_column.schema.size != u32::from(NATIVE_ECS_QWORD_BYTE_LEN)
    {
        return Err(native_storage_compatibility_error());
    }

    let fixed_catalog = NATIVE_ECS_TABLE_MODEL.storage_catalog.table_rows[0];
    let catalog_table = NativeStorageCatalogTableRow {
        slots: NativeStorageCatalogTableRowSlots {
            column_count: native_ecs_slot(table.catalog.column_count),
            row_count_address: native_ecs_slot(table.catalog.row_count_address),
            capacity: native_ecs_slot(table.catalog.capacity),
            row_stride: native_ecs_slot(table.catalog.row_stride),
        },
        storage: fixed_catalog.storage,
        columns: [
            NativeStorageCatalogColumnRow {
                slots: native_storage_catalog_column_slots(position_column.catalog),
                descriptor: fixed_catalog.columns[0].descriptor,
                payload_column: fixed_catalog.columns[0].payload_column,
            },
            NativeStorageCatalogColumnRow {
                slots: native_storage_catalog_column_slots(velocity_column.catalog),
                descriptor: fixed_catalog.columns[1].descriptor,
                payload_column: fixed_catalog.columns[1].payload_column,
            },
        ],
    };

    Ok(NativeStorageCompatibilityModel {
        catalog_table,
        capacity: u64::from(table.capacity),
        row_stride: u64::from(table.logical_row_stride),
    })
}

fn native_ecs_slot(slot: NativeSlot) -> NativeEcsSlot {
    NativeEcsSlot {
        offset: slot.offset,
        byte_len: slot.byte_len,
    }
}

fn native_storage_catalog_column_slots(
    slots: NativeCatalogColumnSlots,
) -> NativeStorageCatalogColumnRowSlots {
    NativeStorageCatalogColumnRowSlots {
        component_id: native_ecs_slot(slots.component_id),
        element_size: native_ecs_slot(slots.element_size),
        element_align: native_ecs_slot(slots.element_align),
        payload_base_address: native_ecs_slot(slots.payload_base_address),
    }
}

fn native_query_plan_iteration_row(
    compatibility: NativeStorageCompatibilityModel,
) -> NativeQueryPlanTableIterationRow {
    let mut row = ECS_QUERY_PLAN_TABLE_ITERATION_ROWS[0];
    row.build_row.catalog_column_count_slot = compatibility.catalog_table.slots.column_count.offset;
    row.build_row.catalog_row_count_address_slot =
        compatibility.catalog_table.slots.row_count_address.offset;
    for (term, column) in row
        .build_row
        .terms
        .iter_mut()
        .zip(compatibility.catalog_table.columns)
    {
        term.catalog_component_id_slot = column.slots.component_id.offset;
        term.catalog_element_size_slot = column.slots.element_size.offset;
        term.catalog_payload_base_address_slot = column.slots.payload_base_address.offset;
    }
    row
}

fn native_storage_compatibility_error() -> CodegenError {
    CodegenError {
        message: "native startup/query compatibility requires one one-or-two-row table containing the observable's two distinct eight-byte component columns"
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs_metadata;
    use crate::lexer;
    use crate::native_world_plan::{
        NativeByteRange, NativeCatalogTableSlots, NativeColumnStoragePlan, NativeComponentSchema,
        NativePlannedSpawnRow, NativeTableStoragePlan, NativeTableStorageSlots,
    };
    use crate::parser;
    use crate::runtime_assembly;

    #[test]
    fn defines_native_ecs_execution_state_layout() {
        let layout = NATIVE_ECS_EXECUTION_STATE_LAYOUT;

        assert_eq!(layout.frame_size, 1088);
        let expected_zeroed_qword_offsets: Vec<u16> = (0..=1080).step_by(8).collect();
        assert_eq!(
            layout.zeroed_qword_offsets.as_slice(),
            expected_zeroed_qword_offsets.as_slice()
        );
        assert_eq!(
            layout.descriptor_counts,
            NativeDescriptorCountSlots {
                components: NativeEcsSlot {
                    offset: 0,
                    byte_len: 8,
                },
                resources: NativeEcsSlot {
                    offset: 8,
                    byte_len: 8,
                },
                systems: NativeEcsSlot {
                    offset: 16,
                    byte_len: 8,
                },
                queries: NativeEcsSlot {
                    offset: 24,
                    byte_len: 8,
                },
                schedules: NativeEcsSlot {
                    offset: 32,
                    byte_len: 8,
                },
            }
        );
        assert_eq!(
            layout.descriptor_records,
            NativeDescriptorRecordStateSlots {
                components: NativeDescriptorSectionSlots {
                    payload_offset: NativeEcsSlot {
                        offset: 96,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 104,
                        byte_len: 8,
                    },
                },
                resources: NativeDescriptorSectionSlots {
                    payload_offset: NativeEcsSlot {
                        offset: 112,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 120,
                        byte_len: 8,
                    },
                },
                systems: NativeDescriptorSectionSlots {
                    payload_offset: NativeEcsSlot {
                        offset: 128,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 136,
                        byte_len: 8,
                    },
                },
                queries: NativeDescriptorSectionSlots {
                    payload_offset: NativeEcsSlot {
                        offset: 144,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 152,
                        byte_len: 8,
                    },
                },
                schedules: NativeDescriptorSectionSlots {
                    payload_offset: NativeEcsSlot {
                        offset: 160,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 168,
                        byte_len: 8,
                    },
                },
            }
        );
        assert_eq!(
            layout.startup_state,
            NativeStartupStateSlots {
                time_payload: NativeEcsSlot {
                    offset: 40,
                    byte_len: 4,
                },
                row_count: NativeEcsSlot {
                    offset: 48,
                    byte_len: 8,
                },
                spawn_payload_rows: [
                    NativeSpawnPayloadStorageSlots {
                        position_payload: NativeEcsSlot {
                            offset: 56,
                            byte_len: 8,
                        },
                        velocity_payload: NativeEcsSlot {
                            offset: 64,
                            byte_len: 8,
                        },
                    },
                    NativeSpawnPayloadStorageSlots {
                        position_payload: NativeEcsSlot {
                            offset: 920,
                            byte_len: 8,
                        },
                        velocity_payload: NativeEcsSlot {
                            offset: 928,
                            byte_len: 8,
                        },
                    },
                ],
            }
        );
        assert_eq!(
            layout.startup_dispatch,
            NativeStartupDispatchSlots {
                operation_count: NativeEcsSlot {
                    offset: 176,
                    byte_len: 8,
                },
                resource_dispatch_count: NativeEcsSlot {
                    offset: 184,
                    byte_len: 8,
                },
                spawn_dispatch_count: NativeEcsSlot {
                    offset: 192,
                    byte_len: 8,
                },
                run_schedule_dispatch_count: NativeEcsSlot {
                    offset: 200,
                    byte_len: 8,
                },
            }
        );
        assert_eq!(
            layout.compiled_move,
            NativeCompiledMoveSlots {
                target_position_payload: NativeEcsSlot {
                    offset: 72,
                    byte_len: 8,
                },
                scanned_row_count: NativeEcsSlot {
                    offset: 80,
                    byte_len: 8,
                },
                field_product_payload: NativeEcsSlot {
                    offset: 88,
                    byte_len: 8,
                },
            }
        );
        assert_eq!(
            layout.query_plan,
            NativeQueryPlanSlots {
                matched_row_count: NativeEcsSlot {
                    offset: 208,
                    byte_len: 8,
                },
                position_payload_address: NativeEcsSlot {
                    offset: 216,
                    byte_len: 8,
                },
                velocity_payload_address: NativeEcsSlot {
                    offset: 224,
                    byte_len: 8,
                },
            }
        );
        assert_eq!(
            layout.compiled_schedule,
            NativeCompiledScheduleSlots {
                schedule_id: NativeEcsSlot {
                    offset: 232,
                    byte_len: 8,
                },
                scheduled_system_id: NativeEcsSlot {
                    offset: 240,
                    byte_len: 8,
                },
                scheduled_system_count: NativeEcsSlot {
                    offset: 248,
                    byte_len: 8,
                },
            }
        );
        assert_eq!(
            layout.component_resource_descriptors,
            NativeComponentResourceDescriptorTableSlots {
                position: NativeXyDescriptorSlots {
                    id: NativeEcsSlot {
                        offset: 256,
                        byte_len: 8,
                    },
                    size: NativeEcsSlot {
                        offset: 264,
                        byte_len: 8,
                    },
                    align: NativeEcsSlot {
                        offset: 272,
                        byte_len: 8,
                    },
                    field_count: NativeEcsSlot {
                        offset: 280,
                        byte_len: 8,
                    },
                    x_field_offset: NativeEcsSlot {
                        offset: 288,
                        byte_len: 8,
                    },
                    y_field_offset: NativeEcsSlot {
                        offset: 296,
                        byte_len: 8,
                    },
                },
                velocity: NativeXyDescriptorSlots {
                    id: NativeEcsSlot {
                        offset: 304,
                        byte_len: 8,
                    },
                    size: NativeEcsSlot {
                        offset: 312,
                        byte_len: 8,
                    },
                    align: NativeEcsSlot {
                        offset: 320,
                        byte_len: 8,
                    },
                    field_count: NativeEcsSlot {
                        offset: 328,
                        byte_len: 8,
                    },
                    x_field_offset: NativeEcsSlot {
                        offset: 336,
                        byte_len: 8,
                    },
                    y_field_offset: NativeEcsSlot {
                        offset: 344,
                        byte_len: 8,
                    },
                },
                time: NativeTimeDescriptorSlots {
                    id: NativeEcsSlot {
                        offset: 352,
                        byte_len: 8,
                    },
                    size: NativeEcsSlot {
                        offset: 360,
                        byte_len: 8,
                    },
                    align: NativeEcsSlot {
                        offset: 368,
                        byte_len: 8,
                    },
                    field_count: NativeEcsSlot {
                        offset: 376,
                        byte_len: 8,
                    },
                    delta_field_offset: NativeEcsSlot {
                        offset: 384,
                        byte_len: 8,
                    },
                },
            }
        );
        assert_eq!(
            layout.system_query_schedule_descriptors,
            NativeSystemQueryScheduleDescriptorTableSlots {
                move_system: NativeMoveSystemDescriptorSlots {
                    id: NativeEcsSlot {
                        offset: 392,
                        byte_len: 8,
                    },
                    param_count: NativeEcsSlot {
                        offset: 400,
                        byte_len: 8,
                    },
                    resource_param_kind: NativeEcsSlot {
                        offset: 408,
                        byte_len: 8,
                    },
                    resource_param_resource_id: NativeEcsSlot {
                        offset: 416,
                        byte_len: 8,
                    },
                    query_param_kind: NativeEcsSlot {
                        offset: 424,
                        byte_len: 8,
                    },
                    query_param_term_count: NativeEcsSlot {
                        offset: 432,
                        byte_len: 8,
                    },
                    query_term0_access: NativeEcsSlot {
                        offset: 440,
                        byte_len: 8,
                    },
                    query_term0_component_id: NativeEcsSlot {
                        offset: 448,
                        byte_len: 8,
                    },
                    query_term1_access: NativeEcsSlot {
                        offset: 456,
                        byte_len: 8,
                    },
                    query_term1_component_id: NativeEcsSlot {
                        offset: 464,
                        byte_len: 8,
                    },
                },
                movers_query: NativeMoversQueryDescriptorSlots {
                    id: NativeEcsSlot {
                        offset: 472,
                        byte_len: 8,
                    },
                    term_count: NativeEcsSlot {
                        offset: 480,
                        byte_len: 8,
                    },
                    term0_access: NativeEcsSlot {
                        offset: 488,
                        byte_len: 8,
                    },
                    term0_component_id: NativeEcsSlot {
                        offset: 496,
                        byte_len: 8,
                    },
                    term1_access: NativeEcsSlot {
                        offset: 504,
                        byte_len: 8,
                    },
                    term1_component_id: NativeEcsSlot {
                        offset: 512,
                        byte_len: 8,
                    },
                },
                main_schedule: NativeMainScheduleDescriptorSlots {
                    id: NativeEcsSlot {
                        offset: 520,
                        byte_len: 8,
                    },
                    item_count: NativeEcsSlot {
                        offset: 528,
                        byte_len: 8,
                    },
                    run_item_kind: NativeEcsSlot {
                        offset: 536,
                        byte_len: 8,
                    },
                    run_system_id: NativeEcsSlot {
                        offset: 544,
                        byte_len: 8,
                    },
                },
            }
        );
        assert_eq!(
            layout.startup_operations,
            NativeStartupOperationTableSlots {
                resource: NativeResourceStartupOperationSlots {
                    kind: NativeEcsSlot {
                        offset: 552,
                        byte_len: 8,
                    },
                    resource_id: NativeEcsSlot {
                        offset: 560,
                        byte_len: 8,
                    },
                    payload_offset: NativeEcsSlot {
                        offset: 568,
                        byte_len: 8,
                    },
                    payload_len: NativeEcsSlot {
                        offset: 576,
                        byte_len: 8,
                    },
                },
                spawn_rows: [
                    NativeSpawnStartupOperationSlots {
                        kind: NativeEcsSlot {
                            offset: 584,
                            byte_len: 8,
                        },
                        component_count: NativeEcsSlot {
                            offset: 592,
                            byte_len: 8,
                        },
                        position_component_id: NativeEcsSlot {
                            offset: 600,
                            byte_len: 8,
                        },
                        position_payload_offset: NativeEcsSlot {
                            offset: 608,
                            byte_len: 8,
                        },
                        position_payload_len: NativeEcsSlot {
                            offset: 616,
                            byte_len: 8,
                        },
                        velocity_component_id: NativeEcsSlot {
                            offset: 624,
                            byte_len: 8,
                        },
                        velocity_payload_offset: NativeEcsSlot {
                            offset: 632,
                            byte_len: 8,
                        },
                        velocity_payload_len: NativeEcsSlot {
                            offset: 640,
                            byte_len: 8,
                        },
                    },
                    NativeSpawnStartupOperationSlots {
                        kind: NativeEcsSlot {
                            offset: 856,
                            byte_len: 8,
                        },
                        component_count: NativeEcsSlot {
                            offset: 864,
                            byte_len: 8,
                        },
                        position_component_id: NativeEcsSlot {
                            offset: 872,
                            byte_len: 8,
                        },
                        position_payload_offset: NativeEcsSlot {
                            offset: 880,
                            byte_len: 8,
                        },
                        position_payload_len: NativeEcsSlot {
                            offset: 888,
                            byte_len: 8,
                        },
                        velocity_component_id: NativeEcsSlot {
                            offset: 896,
                            byte_len: 8,
                        },
                        velocity_payload_offset: NativeEcsSlot {
                            offset: 904,
                            byte_len: 8,
                        },
                        velocity_payload_len: NativeEcsSlot {
                            offset: 912,
                            byte_len: 8,
                        },
                    },
                ],
                run_schedule: NativeRunScheduleStartupOperationSlots {
                    kind: NativeEcsSlot {
                        offset: 648,
                        byte_len: 8,
                    },
                    schedule_id: NativeEcsSlot {
                        offset: 656,
                        byte_len: 8,
                    },
                },
            }
        );
        assert_eq!(
            layout.descriptor_backed_query_plan,
            NativeDescriptorBackedQueryPlanSlots {
                query_id: NativeEcsSlot {
                    offset: 664,
                    byte_len: 8,
                },
                term_count: NativeEcsSlot {
                    offset: 672,
                    byte_len: 8,
                },
                position: NativePlannedComponentDescriptorSlots {
                    access: NativeEcsSlot {
                        offset: 680,
                        byte_len: 8,
                    },
                    component_id: NativeEcsSlot {
                        offset: 688,
                        byte_len: 8,
                    },
                    size: NativeEcsSlot {
                        offset: 696,
                        byte_len: 8,
                    },
                    x_field_offset: NativeEcsSlot {
                        offset: 704,
                        byte_len: 8,
                    },
                    y_field_offset: NativeEcsSlot {
                        offset: 712,
                        byte_len: 8,
                    },
                },
                velocity: NativePlannedComponentDescriptorSlots {
                    access: NativeEcsSlot {
                        offset: 720,
                        byte_len: 8,
                    },
                    component_id: NativeEcsSlot {
                        offset: 728,
                        byte_len: 8,
                    },
                    size: NativeEcsSlot {
                        offset: 736,
                        byte_len: 8,
                    },
                    x_field_offset: NativeEcsSlot {
                        offset: 744,
                        byte_len: 8,
                    },
                    y_field_offset: NativeEcsSlot {
                        offset: 752,
                        byte_len: 8,
                    },
                },
            }
        );
        assert_eq!(
            layout.descriptor_names,
            NativeDescriptorNameTableSlots {
                position: NativeNameReferenceSlots {
                    byte_offset: NativeEcsSlot {
                        offset: 760,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 768,
                        byte_len: 8,
                    },
                },
                velocity: NativeNameReferenceSlots {
                    byte_offset: NativeEcsSlot {
                        offset: 776,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 784,
                        byte_len: 8,
                    },
                },
                time: NativeNameReferenceSlots {
                    byte_offset: NativeEcsSlot {
                        offset: 792,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 800,
                        byte_len: 8,
                    },
                },
                move_system: NativeNameReferenceSlots {
                    byte_offset: NativeEcsSlot {
                        offset: 808,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 816,
                        byte_len: 8,
                    },
                },
                movers_query: NativeNameReferenceSlots {
                    byte_offset: NativeEcsSlot {
                        offset: 824,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 832,
                        byte_len: 8,
                    },
                },
                main_schedule: NativeNameReferenceSlots {
                    byte_offset: NativeEcsSlot {
                        offset: 840,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 848,
                        byte_len: 8,
                    },
                },
            }
        );
        assert_eq!(
            layout.archetype_storage,
            NativeArchetypeTableStorageSlots {
                row_count: NativeEcsSlot {
                    offset: 936,
                    byte_len: 8,
                },
                capacity: NativeEcsSlot {
                    offset: 944,
                    byte_len: 8,
                },
                row_stride: NativeEcsSlot {
                    offset: 952,
                    byte_len: 8,
                },
                position_column: NativeComponentColumnPayloadSlots {
                    payload_rows: [
                        NativeEcsSlot {
                            offset: 960,
                            byte_len: 8,
                        },
                        NativeEcsSlot {
                            offset: 968,
                            byte_len: 8,
                        },
                    ],
                },
                velocity_column: NativeComponentColumnPayloadSlots {
                    payload_rows: [
                        NativeEcsSlot {
                            offset: 976,
                            byte_len: 8,
                        },
                        NativeEcsSlot {
                            offset: 984,
                            byte_len: 8,
                        },
                    ],
                },
            }
        );
        assert_eq!(
            layout.storage_catalog,
            NativeStorageCatalogSlots {
                table_rows: [NativeStorageCatalogTableRowSlots {
                    column_count: NativeEcsSlot {
                        offset: 992,
                        byte_len: 8,
                    },
                    row_count_address: NativeEcsSlot {
                        offset: 1000,
                        byte_len: 8,
                    },
                    capacity: NativeEcsSlot {
                        offset: 1008,
                        byte_len: 8,
                    },
                    row_stride: NativeEcsSlot {
                        offset: 1016,
                        byte_len: 8,
                    },
                }],
                column_rows: [
                    NativeStorageCatalogColumnRowSlots {
                        component_id: NativeEcsSlot {
                            offset: 1024,
                            byte_len: 8,
                        },
                        element_size: NativeEcsSlot {
                            offset: 1032,
                            byte_len: 8,
                        },
                        element_align: NativeEcsSlot {
                            offset: 1040,
                            byte_len: 8,
                        },
                        payload_base_address: NativeEcsSlot {
                            offset: 1048,
                            byte_len: 8,
                        },
                    },
                    NativeStorageCatalogColumnRowSlots {
                        component_id: NativeEcsSlot {
                            offset: 1056,
                            byte_len: 8,
                        },
                        element_size: NativeEcsSlot {
                            offset: 1064,
                            byte_len: 8,
                        },
                        element_align: NativeEcsSlot {
                            offset: 1072,
                            byte_len: 8,
                        },
                        payload_base_address: NativeEcsSlot {
                            offset: 1080,
                            byte_len: 8,
                        },
                    },
                ],
            }
        );
        assert_eq!(ECS_DESCRIPTOR_REGISTRY_SLOTS, [0, 8, 16, 24, 32]);
        assert_eq!(ECS_DESCRIPTOR_RECORD_OFFSET_SLOTS, [96, 112, 128, 144, 160]);
        assert_eq!(
            ECS_DESCRIPTOR_RECORD_BYTE_LEN_SLOTS,
            [104, 120, 136, 152, 168]
        );
        assert_eq!(ECS_RESOURCE_PAYLOAD_STORAGE_SLOT, 40);
        assert_eq!(ECS_SPAWN_ROW_COUNT_SLOT, 48);
        assert_eq!(ECS_POSITION_PAYLOAD_STORAGE_SLOT, 56);
        assert_eq!(ECS_VELOCITY_PAYLOAD_STORAGE_SLOT, 64);
        assert_eq!(ECS_QUERY_LOOP_TARGET_POSITION_SLOT, 72);
        assert_eq!(ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT, 80);
        assert_eq!(ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT, 88);
        assert_eq!(ECS_STARTUP_OPERATION_COUNT_SLOT, 176);
        assert_eq!(ECS_STARTUP_RESOURCE_DISPATCH_COUNT_SLOT, 184);
        assert_eq!(ECS_STARTUP_SPAWN_DISPATCH_COUNT_SLOT, 192);
        assert_eq!(ECS_STARTUP_RUN_SCHEDULE_DISPATCH_COUNT_SLOT, 200);
        assert_eq!(ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT, 208);
        assert_eq!(ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT, 216);
        assert_eq!(ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT, 224);
        assert_eq!(ECS_COMPILED_SCHEDULE_ID_SLOT, 232);
        assert_eq!(ECS_COMPILED_SCHEDULED_SYSTEM_ID_SLOT, 240);
        assert_eq!(ECS_COMPILED_SCHEDULED_SYSTEM_COUNT_SLOT, 248);
        assert_eq!(ECS_POSITION_DESCRIPTOR_ID_SLOT, 256);
        assert_eq!(ECS_POSITION_DESCRIPTOR_SIZE_SLOT, 264);
        assert_eq!(ECS_POSITION_DESCRIPTOR_ALIGN_SLOT, 272);
        assert_eq!(ECS_POSITION_DESCRIPTOR_FIELD_COUNT_SLOT, 280);
        assert_eq!(ECS_POSITION_DESCRIPTOR_X_FIELD_OFFSET_SLOT, 288);
        assert_eq!(ECS_POSITION_DESCRIPTOR_Y_FIELD_OFFSET_SLOT, 296);
        assert_eq!(ECS_VELOCITY_DESCRIPTOR_ID_SLOT, 304);
        assert_eq!(ECS_VELOCITY_DESCRIPTOR_SIZE_SLOT, 312);
        assert_eq!(ECS_VELOCITY_DESCRIPTOR_ALIGN_SLOT, 320);
        assert_eq!(ECS_VELOCITY_DESCRIPTOR_FIELD_COUNT_SLOT, 328);
        assert_eq!(ECS_VELOCITY_DESCRIPTOR_X_FIELD_OFFSET_SLOT, 336);
        assert_eq!(ECS_VELOCITY_DESCRIPTOR_Y_FIELD_OFFSET_SLOT, 344);
        assert_eq!(ECS_TIME_DESCRIPTOR_ID_SLOT, 352);
        assert_eq!(ECS_TIME_DESCRIPTOR_SIZE_SLOT, 360);
        assert_eq!(ECS_TIME_DESCRIPTOR_ALIGN_SLOT, 368);
        assert_eq!(ECS_TIME_DESCRIPTOR_FIELD_COUNT_SLOT, 376);
        assert_eq!(ECS_TIME_DESCRIPTOR_DELTA_FIELD_OFFSET_SLOT, 384);
        assert_eq!(ECS_MOVE_SYSTEM_DESCRIPTOR_ID_SLOT, 392);
        assert_eq!(ECS_MOVE_SYSTEM_DESCRIPTOR_PARAM_COUNT_SLOT, 400);
        assert_eq!(ECS_MOVE_SYSTEM_RESOURCE_PARAM_KIND_SLOT, 408);
        assert_eq!(ECS_MOVE_SYSTEM_RESOURCE_PARAM_RESOURCE_ID_SLOT, 416);
        assert_eq!(ECS_MOVE_SYSTEM_QUERY_PARAM_KIND_SLOT, 424);
        assert_eq!(ECS_MOVE_SYSTEM_QUERY_PARAM_TERM_COUNT_SLOT, 432);
        assert_eq!(ECS_MOVE_SYSTEM_QUERY_TERM0_ACCESS_SLOT, 440);
        assert_eq!(ECS_MOVE_SYSTEM_QUERY_TERM0_COMPONENT_ID_SLOT, 448);
        assert_eq!(ECS_MOVE_SYSTEM_QUERY_TERM1_ACCESS_SLOT, 456);
        assert_eq!(ECS_MOVE_SYSTEM_QUERY_TERM1_COMPONENT_ID_SLOT, 464);
        assert_eq!(ECS_MOVERS_QUERY_DESCRIPTOR_ID_SLOT, 472);
        assert_eq!(ECS_MOVERS_QUERY_DESCRIPTOR_TERM_COUNT_SLOT, 480);
        assert_eq!(ECS_MOVERS_QUERY_TERM0_ACCESS_SLOT, 488);
        assert_eq!(ECS_MOVERS_QUERY_TERM0_COMPONENT_ID_SLOT, 496);
        assert_eq!(ECS_MOVERS_QUERY_TERM1_ACCESS_SLOT, 504);
        assert_eq!(ECS_MOVERS_QUERY_TERM1_COMPONENT_ID_SLOT, 512);
        assert_eq!(ECS_MAIN_SCHEDULE_DESCRIPTOR_ID_SLOT, 520);
        assert_eq!(ECS_MAIN_SCHEDULE_DESCRIPTOR_ITEM_COUNT_SLOT, 528);
        assert_eq!(ECS_MAIN_SCHEDULE_RUN_ITEM_KIND_SLOT, 536);
        assert_eq!(ECS_MAIN_SCHEDULE_RUN_SYSTEM_ID_SLOT, 544);
        assert_eq!(ECS_STARTUP_TABLE_RESOURCE_KIND_SLOT, 552);
        assert_eq!(ECS_STARTUP_TABLE_RESOURCE_ID_SLOT, 560);
        assert_eq!(ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_OFFSET_SLOT, 568);
        assert_eq!(ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_LEN_SLOT, 576);
        assert_eq!(ECS_STARTUP_TABLE_SPAWN_KIND_SLOT, 584);
        assert_eq!(ECS_STARTUP_TABLE_SPAWN_COMPONENT_COUNT_SLOT, 592);
        assert_eq!(ECS_STARTUP_TABLE_POSITION_COMPONENT_ID_SLOT, 600);
        assert_eq!(ECS_STARTUP_TABLE_POSITION_PAYLOAD_OFFSET_SLOT, 608);
        assert_eq!(ECS_STARTUP_TABLE_POSITION_PAYLOAD_LEN_SLOT, 616);
        assert_eq!(ECS_STARTUP_TABLE_VELOCITY_COMPONENT_ID_SLOT, 624);
        assert_eq!(ECS_STARTUP_TABLE_VELOCITY_PAYLOAD_OFFSET_SLOT, 632);
        assert_eq!(ECS_STARTUP_TABLE_VELOCITY_PAYLOAD_LEN_SLOT, 640);
        assert_eq!(ECS_STARTUP_TABLE_RUN_SCHEDULE_KIND_SLOT, 648);
        assert_eq!(ECS_STARTUP_TABLE_RUN_SCHEDULE_ID_SLOT, 656);
        assert_eq!(ECS_DESCRIPTOR_QUERY_PLAN_QUERY_ID_SLOT, 664);
        assert_eq!(ECS_DESCRIPTOR_QUERY_PLAN_TERM_COUNT_SLOT, 672);
        assert_eq!(ECS_DESCRIPTOR_QUERY_PLAN_POSITION_ACCESS_SLOT, 680);
        assert_eq!(ECS_DESCRIPTOR_QUERY_PLAN_POSITION_COMPONENT_ID_SLOT, 688);
        assert_eq!(ECS_DESCRIPTOR_QUERY_PLAN_POSITION_SIZE_SLOT, 696);
        assert_eq!(ECS_DESCRIPTOR_QUERY_PLAN_POSITION_X_FIELD_OFFSET_SLOT, 704);
        assert_eq!(ECS_DESCRIPTOR_QUERY_PLAN_POSITION_Y_FIELD_OFFSET_SLOT, 712);
        assert_eq!(ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_ACCESS_SLOT, 720);
        assert_eq!(ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_COMPONENT_ID_SLOT, 728);
        assert_eq!(ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_SIZE_SLOT, 736);
        assert_eq!(ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_X_FIELD_OFFSET_SLOT, 744);
        assert_eq!(ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_Y_FIELD_OFFSET_SLOT, 752);
        assert_eq!(ECS_POSITION_DESCRIPTOR_NAME_OFFSET_SLOT, 760);
        assert_eq!(ECS_POSITION_DESCRIPTOR_NAME_LEN_SLOT, 768);
        assert_eq!(ECS_VELOCITY_DESCRIPTOR_NAME_OFFSET_SLOT, 776);
        assert_eq!(ECS_VELOCITY_DESCRIPTOR_NAME_LEN_SLOT, 784);
        assert_eq!(ECS_TIME_DESCRIPTOR_NAME_OFFSET_SLOT, 792);
        assert_eq!(ECS_TIME_DESCRIPTOR_NAME_LEN_SLOT, 800);
        assert_eq!(ECS_MOVE_SYSTEM_DESCRIPTOR_NAME_OFFSET_SLOT, 808);
        assert_eq!(ECS_MOVE_SYSTEM_DESCRIPTOR_NAME_LEN_SLOT, 816);
        assert_eq!(ECS_MOVERS_QUERY_DESCRIPTOR_NAME_OFFSET_SLOT, 824);
        assert_eq!(ECS_MOVERS_QUERY_DESCRIPTOR_NAME_LEN_SLOT, 832);
        assert_eq!(ECS_MAIN_SCHEDULE_DESCRIPTOR_NAME_OFFSET_SLOT, 840);
        assert_eq!(ECS_MAIN_SCHEDULE_DESCRIPTOR_NAME_LEN_SLOT, 848);
        assert_eq!(ECS_ARCHETYPE_STORAGE_ROW_COUNT_SLOT, 936);
        assert_eq!(ECS_ARCHETYPE_STORAGE_CAPACITY_SLOT, 944);
        assert_eq!(ECS_ARCHETYPE_STORAGE_ROW_STRIDE_SLOT, 952);
        assert_eq!(ECS_ARCHETYPE_STORAGE_POSITION_ROW0_PAYLOAD_SLOT, 960);
        assert_eq!(ECS_ARCHETYPE_STORAGE_POSITION_ROW1_PAYLOAD_SLOT, 968);
        assert_eq!(ECS_ARCHETYPE_STORAGE_VELOCITY_ROW0_PAYLOAD_SLOT, 976);
        assert_eq!(ECS_ARCHETYPE_STORAGE_VELOCITY_ROW1_PAYLOAD_SLOT, 984);

        let slots = [
            layout.descriptor_counts.components,
            layout.descriptor_counts.resources,
            layout.descriptor_counts.systems,
            layout.descriptor_counts.queries,
            layout.descriptor_counts.schedules,
            layout.descriptor_records.components.payload_offset,
            layout.descriptor_records.components.byte_len,
            layout.descriptor_records.resources.payload_offset,
            layout.descriptor_records.resources.byte_len,
            layout.descriptor_records.systems.payload_offset,
            layout.descriptor_records.systems.byte_len,
            layout.descriptor_records.queries.payload_offset,
            layout.descriptor_records.queries.byte_len,
            layout.descriptor_records.schedules.payload_offset,
            layout.descriptor_records.schedules.byte_len,
            layout.startup_state.time_payload,
            layout.startup_state.row_count,
            layout.startup_state.spawn_payload_rows[0].position_payload,
            layout.startup_state.spawn_payload_rows[0].velocity_payload,
            layout.startup_state.spawn_payload_rows[1].position_payload,
            layout.startup_state.spawn_payload_rows[1].velocity_payload,
            layout.startup_dispatch.operation_count,
            layout.startup_dispatch.resource_dispatch_count,
            layout.startup_dispatch.spawn_dispatch_count,
            layout.startup_dispatch.run_schedule_dispatch_count,
            layout.query_plan.matched_row_count,
            layout.query_plan.position_payload_address,
            layout.query_plan.velocity_payload_address,
            layout.compiled_schedule.schedule_id,
            layout.compiled_schedule.scheduled_system_id,
            layout.compiled_schedule.scheduled_system_count,
            layout.compiled_move.target_position_payload,
            layout.compiled_move.scanned_row_count,
            layout.compiled_move.field_product_payload,
            layout.component_resource_descriptors.position.id,
            layout.component_resource_descriptors.position.size,
            layout.component_resource_descriptors.position.align,
            layout.component_resource_descriptors.position.field_count,
            layout
                .component_resource_descriptors
                .position
                .x_field_offset,
            layout
                .component_resource_descriptors
                .position
                .y_field_offset,
            layout.component_resource_descriptors.velocity.id,
            layout.component_resource_descriptors.velocity.size,
            layout.component_resource_descriptors.velocity.align,
            layout.component_resource_descriptors.velocity.field_count,
            layout
                .component_resource_descriptors
                .velocity
                .x_field_offset,
            layout
                .component_resource_descriptors
                .velocity
                .y_field_offset,
            layout.component_resource_descriptors.time.id,
            layout.component_resource_descriptors.time.size,
            layout.component_resource_descriptors.time.align,
            layout.component_resource_descriptors.time.field_count,
            layout
                .component_resource_descriptors
                .time
                .delta_field_offset,
            layout.system_query_schedule_descriptors.move_system.id,
            layout
                .system_query_schedule_descriptors
                .move_system
                .param_count,
            layout
                .system_query_schedule_descriptors
                .move_system
                .resource_param_kind,
            layout
                .system_query_schedule_descriptors
                .move_system
                .resource_param_resource_id,
            layout
                .system_query_schedule_descriptors
                .move_system
                .query_param_kind,
            layout
                .system_query_schedule_descriptors
                .move_system
                .query_param_term_count,
            layout
                .system_query_schedule_descriptors
                .move_system
                .query_term0_access,
            layout
                .system_query_schedule_descriptors
                .move_system
                .query_term0_component_id,
            layout
                .system_query_schedule_descriptors
                .move_system
                .query_term1_access,
            layout
                .system_query_schedule_descriptors
                .move_system
                .query_term1_component_id,
            layout.system_query_schedule_descriptors.movers_query.id,
            layout
                .system_query_schedule_descriptors
                .movers_query
                .term_count,
            layout
                .system_query_schedule_descriptors
                .movers_query
                .term0_access,
            layout
                .system_query_schedule_descriptors
                .movers_query
                .term0_component_id,
            layout
                .system_query_schedule_descriptors
                .movers_query
                .term1_access,
            layout
                .system_query_schedule_descriptors
                .movers_query
                .term1_component_id,
            layout.system_query_schedule_descriptors.main_schedule.id,
            layout
                .system_query_schedule_descriptors
                .main_schedule
                .item_count,
            layout
                .system_query_schedule_descriptors
                .main_schedule
                .run_item_kind,
            layout
                .system_query_schedule_descriptors
                .main_schedule
                .run_system_id,
            layout.startup_operations.resource.kind,
            layout.startup_operations.resource.resource_id,
            layout.startup_operations.resource.payload_offset,
            layout.startup_operations.resource.payload_len,
            layout.startup_operations.spawn_rows[0].kind,
            layout.startup_operations.spawn_rows[0].component_count,
            layout.startup_operations.spawn_rows[0].position_component_id,
            layout.startup_operations.spawn_rows[0].position_payload_offset,
            layout.startup_operations.spawn_rows[0].position_payload_len,
            layout.startup_operations.spawn_rows[0].velocity_component_id,
            layout.startup_operations.spawn_rows[0].velocity_payload_offset,
            layout.startup_operations.spawn_rows[0].velocity_payload_len,
            layout.startup_operations.spawn_rows[1].kind,
            layout.startup_operations.spawn_rows[1].component_count,
            layout.startup_operations.spawn_rows[1].position_component_id,
            layout.startup_operations.spawn_rows[1].position_payload_offset,
            layout.startup_operations.spawn_rows[1].position_payload_len,
            layout.startup_operations.spawn_rows[1].velocity_component_id,
            layout.startup_operations.spawn_rows[1].velocity_payload_offset,
            layout.startup_operations.spawn_rows[1].velocity_payload_len,
            layout.startup_operations.run_schedule.kind,
            layout.startup_operations.run_schedule.schedule_id,
            layout.descriptor_backed_query_plan.query_id,
            layout.descriptor_backed_query_plan.term_count,
            layout.descriptor_backed_query_plan.position.access,
            layout.descriptor_backed_query_plan.position.component_id,
            layout.descriptor_backed_query_plan.position.size,
            layout.descriptor_backed_query_plan.position.x_field_offset,
            layout.descriptor_backed_query_plan.position.y_field_offset,
            layout.descriptor_backed_query_plan.velocity.access,
            layout.descriptor_backed_query_plan.velocity.component_id,
            layout.descriptor_backed_query_plan.velocity.size,
            layout.descriptor_backed_query_plan.velocity.x_field_offset,
            layout.descriptor_backed_query_plan.velocity.y_field_offset,
            layout.descriptor_names.position.byte_offset,
            layout.descriptor_names.position.byte_len,
            layout.descriptor_names.velocity.byte_offset,
            layout.descriptor_names.velocity.byte_len,
            layout.descriptor_names.time.byte_offset,
            layout.descriptor_names.time.byte_len,
            layout.descriptor_names.move_system.byte_offset,
            layout.descriptor_names.move_system.byte_len,
            layout.descriptor_names.movers_query.byte_offset,
            layout.descriptor_names.movers_query.byte_len,
            layout.descriptor_names.main_schedule.byte_offset,
            layout.descriptor_names.main_schedule.byte_len,
            layout.archetype_storage.row_count,
            layout.archetype_storage.capacity,
            layout.archetype_storage.row_stride,
            layout.archetype_storage.position_column.payload_rows[0],
            layout.archetype_storage.position_column.payload_rows[1],
            layout.archetype_storage.velocity_column.payload_rows[0],
            layout.archetype_storage.velocity_column.payload_rows[1],
            layout.storage_catalog.table_rows[0].column_count,
            layout.storage_catalog.table_rows[0].row_count_address,
            layout.storage_catalog.table_rows[0].capacity,
            layout.storage_catalog.table_rows[0].row_stride,
            layout.storage_catalog.column_rows[0].component_id,
            layout.storage_catalog.column_rows[0].element_size,
            layout.storage_catalog.column_rows[0].element_align,
            layout.storage_catalog.column_rows[0].payload_base_address,
            layout.storage_catalog.column_rows[1].component_id,
            layout.storage_catalog.column_rows[1].element_size,
            layout.storage_catalog.column_rows[1].element_align,
            layout.storage_catalog.column_rows[1].payload_base_address,
        ];
        for slot in slots {
            assert!(
                slot.offset + slot.byte_len <= layout.frame_size,
                "slot {:?} should fit in the native ECS frame",
                slot
            );
        }
        for (left_index, left) in slots.iter().enumerate() {
            for right in slots.iter().skip(left_index + 1) {
                let left_start = left.offset;
                let left_end = left_start + left.byte_len;
                let right_start = right.offset;
                let right_end = right_start + right.byte_len;
                assert!(
                    left_end <= right_start || right_end <= left_start,
                    "semantic slots should not overlap: {:?} and {:?}",
                    left,
                    right
                );
            }
        }
        assert!(
            layout.zeroed_qword_offsets.contains(&40),
            "the 4-byte Time payload lives inside the zeroed qword at [rsp + 40]"
        );

        assert_eq!(
            runtime_create_prefix(layout.frame_size).as_slice(),
            expected_runtime_create_prefix(&layout).as_slice()
        );
        assert_eq!(
            runtime_destroy_suffix(layout.frame_size).as_slice(),
            expected_runtime_destroy_suffix(&layout).as_slice()
        );
    }

    #[test]
    fn defines_reusable_native_ecs_table_model() {
        let layout = NATIVE_ECS_EXECUTION_STATE_LAYOUT;
        let model = NATIVE_ECS_TABLE_MODEL;

        assert_eq!(layout.frame_size, 1088);
        assert_eq!(model.descriptors.component_rows.len(), 2);
        assert_eq!(model.descriptors.resource_rows.len(), 1);
        assert_eq!(model.descriptors.system_rows.len(), 1);
        assert_eq!(model.descriptors.query_rows.len(), 1);
        assert_eq!(model.descriptors.schedule_rows.len(), 1);
        assert_eq!(model.startup_operations.resource_payload_rows.len(), 1);
        assert_eq!(model.startup_operations.spawn_rows.len(), 2);
        assert_eq!(model.startup_operations.run_schedule_rows.len(), 1);
        assert_eq!(model.compiled_schedules.rows.len(), 1);
        assert_eq!(model.query_plans.rows.len(), 1);
        assert_eq!(model.archetype_storage, layout.archetype_storage);
        assert_eq!(model.storage_catalog.table_rows.len(), 1);
        assert_eq!(model.storage_catalog.table_rows[0].columns.len(), 2);

        assert_eq!(
            model.descriptors.component_rows,
            [
                NativeXyDescriptorTableRow {
                    slots: layout.component_resource_descriptors.position,
                    name: layout.descriptor_names.position,
                },
                NativeXyDescriptorTableRow {
                    slots: layout.component_resource_descriptors.velocity,
                    name: layout.descriptor_names.velocity,
                },
            ]
        );
        assert_eq!(
            model.descriptors.resource_rows,
            [NativeTimeDescriptorTableRow {
                slots: layout.component_resource_descriptors.time,
                name: layout.descriptor_names.time,
            }]
        );
        assert_eq!(
            model.descriptors.system_rows,
            [NativeMoveSystemDescriptorTableRow {
                slots: layout.system_query_schedule_descriptors.move_system,
                name: layout.descriptor_names.move_system,
            }]
        );
        assert_eq!(
            model.descriptors.query_rows,
            [NativeMoversQueryDescriptorTableRow {
                slots: layout.system_query_schedule_descriptors.movers_query,
                name: layout.descriptor_names.movers_query,
            }]
        );
        assert_eq!(
            model.descriptors.schedule_rows,
            [NativeMainScheduleDescriptorTableRow {
                slots: layout.system_query_schedule_descriptors.main_schedule,
                name: layout.descriptor_names.main_schedule,
            }]
        );
        assert_eq!(
            model.startup_operations.resource_payload_rows,
            [layout.startup_operations.resource]
        );
        assert_eq!(
            model.startup_operations.spawn_rows,
            layout.startup_operations.spawn_rows
        );
        assert_eq!(
            model.startup_operations.run_schedule_rows,
            [layout.startup_operations.run_schedule]
        );
        assert_eq!(model.compiled_schedules.rows, [layout.compiled_schedule]);
        assert_eq!(
            model.query_plans.rows,
            [layout.descriptor_backed_query_plan]
        );
        assert_eq!(
            model.archetype_storage,
            NativeArchetypeTableStorageSlots {
                row_count: layout.archetype_storage.row_count,
                capacity: layout.archetype_storage.capacity,
                row_stride: layout.archetype_storage.row_stride,
                position_column: layout.archetype_storage.position_column,
                velocity_column: layout.archetype_storage.velocity_column,
            }
        );

        let position = model.descriptors.component_rows[0];
        assert_eq!(
            [
                position.slots.id.offset,
                position.slots.size.offset,
                position.slots.align.offset,
                position.slots.field_count.offset,
                position.slots.x_field_offset.offset,
                position.slots.y_field_offset.offset,
            ],
            [256, 264, 272, 280, 288, 296]
        );
        assert_eq!(
            [
                position.name.byte_offset.offset,
                position.name.byte_len.offset
            ],
            [760, 768]
        );
        let velocity = model.descriptors.component_rows[1];
        assert_eq!(
            [
                velocity.slots.id.offset,
                velocity.slots.size.offset,
                velocity.slots.align.offset,
                velocity.slots.field_count.offset,
                velocity.slots.x_field_offset.offset,
                velocity.slots.y_field_offset.offset,
            ],
            [304, 312, 320, 328, 336, 344]
        );
        assert_eq!(
            [
                velocity.name.byte_offset.offset,
                velocity.name.byte_len.offset
            ],
            [776, 784]
        );
        let time = model.descriptors.resource_rows[0];
        assert_eq!(
            [
                time.slots.id.offset,
                time.slots.size.offset,
                time.slots.align.offset,
                time.slots.field_count.offset,
                time.slots.delta_field_offset.offset,
            ],
            [352, 360, 368, 376, 384]
        );
        assert_eq!(
            [time.name.byte_offset.offset, time.name.byte_len.offset],
            [792, 800]
        );

        let move_system = model.descriptors.system_rows[0];
        assert_eq!(
            [
                move_system.slots.id.offset,
                move_system.slots.param_count.offset,
                move_system.slots.resource_param_kind.offset,
                move_system.slots.resource_param_resource_id.offset,
                move_system.slots.query_param_kind.offset,
                move_system.slots.query_param_term_count.offset,
                move_system.slots.query_term0_access.offset,
                move_system.slots.query_term0_component_id.offset,
                move_system.slots.query_term1_access.offset,
                move_system.slots.query_term1_component_id.offset,
            ],
            [392, 400, 408, 416, 424, 432, 440, 448, 456, 464]
        );
        assert_eq!(
            [
                move_system.name.byte_offset.offset,
                move_system.name.byte_len.offset
            ],
            [808, 816]
        );
        let movers_query = model.descriptors.query_rows[0];
        assert_eq!(
            [
                movers_query.slots.id.offset,
                movers_query.slots.term_count.offset,
                movers_query.slots.term0_access.offset,
                movers_query.slots.term0_component_id.offset,
                movers_query.slots.term1_access.offset,
                movers_query.slots.term1_component_id.offset,
            ],
            [472, 480, 488, 496, 504, 512]
        );
        assert_eq!(
            [
                movers_query.name.byte_offset.offset,
                movers_query.name.byte_len.offset
            ],
            [824, 832]
        );
        let main_schedule = model.descriptors.schedule_rows[0];
        assert_eq!(
            [
                main_schedule.slots.id.offset,
                main_schedule.slots.item_count.offset,
                main_schedule.slots.run_item_kind.offset,
                main_schedule.slots.run_system_id.offset,
            ],
            [520, 528, 536, 544]
        );
        assert_eq!(
            [
                main_schedule.name.byte_offset.offset,
                main_schedule.name.byte_len.offset
            ],
            [840, 848]
        );

        let resource = model.startup_operations.resource_payload_rows[0];
        assert_eq!(
            [
                resource.kind.offset,
                resource.resource_id.offset,
                resource.payload_offset.offset,
                resource.payload_len.offset,
            ],
            [552, 560, 568, 576]
        );
        let spawn = model.startup_operations.spawn_rows[0];
        assert_eq!(
            [
                spawn.kind.offset,
                spawn.component_count.offset,
                spawn.position_component_id.offset,
                spawn.position_payload_offset.offset,
                spawn.position_payload_len.offset,
                spawn.velocity_component_id.offset,
                spawn.velocity_payload_offset.offset,
                spawn.velocity_payload_len.offset,
            ],
            [584, 592, 600, 608, 616, 624, 632, 640]
        );
        let second_spawn = model.startup_operations.spawn_rows[1];
        assert_eq!(
            [
                second_spawn.kind.offset,
                second_spawn.component_count.offset,
                second_spawn.position_component_id.offset,
                second_spawn.position_payload_offset.offset,
                second_spawn.position_payload_len.offset,
                second_spawn.velocity_component_id.offset,
                second_spawn.velocity_payload_offset.offset,
                second_spawn.velocity_payload_len.offset,
            ],
            [856, 864, 872, 880, 888, 896, 904, 912]
        );
        let run_schedule = model.startup_operations.run_schedule_rows[0];
        assert_eq!(
            [run_schedule.kind.offset, run_schedule.schedule_id.offset],
            [648, 656]
        );

        let compiled_schedule = model.compiled_schedules.rows[0];
        assert_eq!(
            [
                compiled_schedule.schedule_id.offset,
                compiled_schedule.scheduled_system_id.offset,
                compiled_schedule.scheduled_system_count.offset,
            ],
            [232, 240, 248]
        );
        let query_plan = model.query_plans.rows[0];
        assert_eq!(
            [
                query_plan.query_id.offset,
                query_plan.term_count.offset,
                query_plan.position.access.offset,
                query_plan.position.component_id.offset,
                query_plan.position.size.offset,
                query_plan.position.x_field_offset.offset,
                query_plan.position.y_field_offset.offset,
                query_plan.velocity.access.offset,
                query_plan.velocity.component_id.offset,
                query_plan.velocity.size.offset,
                query_plan.velocity.x_field_offset.offset,
                query_plan.velocity.y_field_offset.offset,
            ],
            [664, 672, 680, 688, 696, 704, 712, 720, 728, 736, 744, 752]
        );
        let storage = model.archetype_storage;
        assert_eq!(
            [
                storage.row_count.offset,
                storage.capacity.offset,
                storage.row_stride.offset,
                storage.position_column.payload_rows[0].offset,
                storage.position_column.payload_rows[1].offset,
                storage.velocity_column.payload_rows[0].offset,
                storage.velocity_column.payload_rows[1].offset,
            ],
            [936, 944, 952, 960, 968, 976, 984]
        );
    }

    #[test]
    fn defines_native_archetype_table_storage_model() {
        let layout = NATIVE_ECS_EXECUTION_STATE_LAYOUT;
        let model = NATIVE_ECS_TABLE_MODEL;
        let storage = model.archetype_storage;

        assert_eq!(layout.frame_size, 1088);
        assert_eq!(storage, layout.archetype_storage);
        assert_eq!(
            storage,
            NativeArchetypeTableStorageSlots {
                row_count: NativeEcsSlot {
                    offset: 936,
                    byte_len: 8,
                },
                capacity: NativeEcsSlot {
                    offset: 944,
                    byte_len: 8,
                },
                row_stride: NativeEcsSlot {
                    offset: 952,
                    byte_len: 8,
                },
                position_column: NativeComponentColumnPayloadSlots {
                    payload_rows: [
                        NativeEcsSlot {
                            offset: 960,
                            byte_len: 8,
                        },
                        NativeEcsSlot {
                            offset: 968,
                            byte_len: 8,
                        },
                    ],
                },
                velocity_column: NativeComponentColumnPayloadSlots {
                    payload_rows: [
                        NativeEcsSlot {
                            offset: 976,
                            byte_len: 8,
                        },
                        NativeEcsSlot {
                            offset: 984,
                            byte_len: 8,
                        },
                    ],
                },
            }
        );
        assert_eq!(storage.capacity.byte_len, NATIVE_ECS_QWORD_BYTE_LEN);
        assert_eq!(storage.row_stride.byte_len, NATIVE_ECS_QWORD_BYTE_LEN);
        assert_eq!(storage.position_column.payload_rows.len(), 2);
        assert_eq!(storage.velocity_column.payload_rows.len(), 2);
        assert_eq!(
            [
                ECS_ARCHETYPE_STORAGE_ROW_COUNT_SLOT,
                ECS_ARCHETYPE_STORAGE_CAPACITY_SLOT,
                ECS_ARCHETYPE_STORAGE_ROW_STRIDE_SLOT,
                ECS_ARCHETYPE_STORAGE_POSITION_ROW0_PAYLOAD_SLOT,
                ECS_ARCHETYPE_STORAGE_POSITION_ROW1_PAYLOAD_SLOT,
                ECS_ARCHETYPE_STORAGE_VELOCITY_ROW0_PAYLOAD_SLOT,
                ECS_ARCHETYPE_STORAGE_VELOCITY_ROW1_PAYLOAD_SLOT,
            ],
            [936, 944, 952, 960, 968, 976, 984]
        );
        assert!(
            layout.zeroed_qword_offsets.contains(&984),
            "the final storage payload row should be zeroed by the runtime wrapper"
        );
    }

    #[test]
    fn defines_native_storage_catalog_model() {
        let layout = NATIVE_ECS_EXECUTION_STATE_LAYOUT;
        let model = NATIVE_ECS_TABLE_MODEL;
        let catalog_slots = layout.storage_catalog;
        let catalog = model.storage_catalog;

        assert_eq!(layout.frame_size, 1088);
        assert_eq!(layout.zeroed_qword_offsets.len(), 136);
        assert_eq!(catalog.table_rows.len(), 1);
        assert_eq!(catalog.table_rows[0].columns.len(), 2);
        assert_eq!(
            catalog_slots,
            NativeStorageCatalogSlots {
                table_rows: [NativeStorageCatalogTableRowSlots {
                    column_count: NativeEcsSlot {
                        offset: 992,
                        byte_len: 8,
                    },
                    row_count_address: NativeEcsSlot {
                        offset: 1000,
                        byte_len: 8,
                    },
                    capacity: NativeEcsSlot {
                        offset: 1008,
                        byte_len: 8,
                    },
                    row_stride: NativeEcsSlot {
                        offset: 1016,
                        byte_len: 8,
                    },
                }],
                column_rows: [
                    NativeStorageCatalogColumnRowSlots {
                        component_id: NativeEcsSlot {
                            offset: 1024,
                            byte_len: 8,
                        },
                        element_size: NativeEcsSlot {
                            offset: 1032,
                            byte_len: 8,
                        },
                        element_align: NativeEcsSlot {
                            offset: 1040,
                            byte_len: 8,
                        },
                        payload_base_address: NativeEcsSlot {
                            offset: 1048,
                            byte_len: 8,
                        },
                    },
                    NativeStorageCatalogColumnRowSlots {
                        component_id: NativeEcsSlot {
                            offset: 1056,
                            byte_len: 8,
                        },
                        element_size: NativeEcsSlot {
                            offset: 1064,
                            byte_len: 8,
                        },
                        element_align: NativeEcsSlot {
                            offset: 1072,
                            byte_len: 8,
                        },
                        payload_base_address: NativeEcsSlot {
                            offset: 1080,
                            byte_len: 8,
                        },
                    },
                ],
            }
        );

        let table = catalog.table_rows[0];
        assert_eq!(table.slots, catalog_slots.table_rows[0]);
        assert_eq!(table.storage, layout.archetype_storage);
        assert_eq!(
            table.columns,
            [
                NativeStorageCatalogColumnRow {
                    slots: catalog_slots.column_rows[0],
                    descriptor: layout.component_resource_descriptors.position,
                    payload_column: layout.archetype_storage.position_column,
                },
                NativeStorageCatalogColumnRow {
                    slots: catalog_slots.column_rows[1],
                    descriptor: layout.component_resource_descriptors.velocity,
                    payload_column: layout.archetype_storage.velocity_column,
                },
            ]
        );
        assert_eq!(table.storage.row_count, layout.archetype_storage.row_count);
        assert_eq!(
            [
                table.slots.column_count.offset,
                table.slots.row_count_address.offset,
                table.slots.capacity.offset,
                table.slots.row_stride.offset,
                table.columns[0].slots.component_id.offset,
                table.columns[0].slots.element_size.offset,
                table.columns[0].slots.element_align.offset,
                table.columns[0].slots.payload_base_address.offset,
                table.columns[1].slots.component_id.offset,
                table.columns[1].slots.element_size.offset,
                table.columns[1].slots.element_align.offset,
                table.columns[1].slots.payload_base_address.offset,
            ],
            [992, 1000, 1008, 1016, 1024, 1032, 1040, 1048, 1056, 1064, 1072, 1080,]
        );
        assert_eq!(
            layout.archetype_storage.velocity_column.payload_rows[1].offset,
            984
        );
        assert!(
            layout.zeroed_qword_offsets.contains(&1080),
            "the final storage catalog column field should be zeroed by the runtime wrapper"
        );
    }

    #[test]
    fn defines_native_table_iteration_cursor_model() {
        let layout = NATIVE_ECS_EXECUTION_STATE_LAYOUT;
        let table_model = NATIVE_ECS_TABLE_MODEL;
        let cursors = NATIVE_ECS_TABLE_ITERATION_CURSORS;

        assert_eq!(layout.frame_size, 1088);
        assert_eq!(cursors.component_descriptors.expected_row_count, 2);
        assert_eq!(cursors.resource_descriptors.expected_row_count, 1);
        assert_eq!(cursors.system_descriptors.expected_row_count, 1);
        assert_eq!(cursors.query_descriptors.expected_row_count, 1);
        assert_eq!(cursors.schedule_descriptors.expected_row_count, 1);
        assert_eq!(cursors.startup_operations.expected_row_count, 4);
        assert_eq!(cursors.compiled_schedules.expected_row_count, 1);
        assert_eq!(cursors.query_plans.expected_row_count, 1);

        assert_eq!(
            [
                cursors.component_descriptors.count_slot,
                cursors.resource_descriptors.count_slot,
                cursors.system_descriptors.count_slot,
                cursors.query_descriptors.count_slot,
                cursors.schedule_descriptors.count_slot,
            ],
            [
                Some(layout.descriptor_counts.components),
                Some(layout.descriptor_counts.resources),
                Some(layout.descriptor_counts.systems),
                Some(layout.descriptor_counts.queries),
                Some(layout.descriptor_counts.schedules),
            ]
        );
        assert_eq!(
            cursors.startup_operations.count_slot,
            Some(layout.startup_dispatch.operation_count)
        );
        assert_eq!(cursors.compiled_schedules.count_slot, None);
        assert_eq!(cursors.query_plans.count_slot, None);

        assert_eq!(
            cursors.component_descriptors,
            NativeTableIterationCursor {
                table: NativeTableIterationKind::ComponentDescriptors,
                expected_row_count: 2,
                count_slot: Some(layout.descriptor_counts.components),
                rows: [
                    NativeTableIterationRow {
                        row_kind: NativeTableIterationRowKind::ComponentDescriptor,
                        row_index: 0,
                        primary_slot: table_model.descriptors.component_rows[0].slots.id,
                    },
                    NativeTableIterationRow {
                        row_kind: NativeTableIterationRowKind::ComponentDescriptor,
                        row_index: 1,
                        primary_slot: table_model.descriptors.component_rows[1].slots.id,
                    },
                ],
            }
        );
        assert_eq!(
            cursors.resource_descriptors.rows,
            [NativeTableIterationRow {
                row_kind: NativeTableIterationRowKind::ResourceDescriptor,
                row_index: 0,
                primary_slot: table_model.descriptors.resource_rows[0].slots.id,
            }]
        );
        assert_eq!(
            cursors.system_descriptors.rows,
            [NativeTableIterationRow {
                row_kind: NativeTableIterationRowKind::SystemDescriptor,
                row_index: 0,
                primary_slot: table_model.descriptors.system_rows[0].slots.id,
            }]
        );
        assert_eq!(
            cursors.query_descriptors.rows,
            [NativeTableIterationRow {
                row_kind: NativeTableIterationRowKind::QueryDescriptor,
                row_index: 0,
                primary_slot: table_model.descriptors.query_rows[0].slots.id,
            }]
        );
        assert_eq!(
            cursors.schedule_descriptors.rows,
            [NativeTableIterationRow {
                row_kind: NativeTableIterationRowKind::ScheduleDescriptor,
                row_index: 0,
                primary_slot: table_model.descriptors.schedule_rows[0].slots.id,
            }]
        );
        assert_eq!(
            cursors.startup_operations,
            NativeTableIterationCursor {
                table: NativeTableIterationKind::StartupOperations,
                expected_row_count: 4,
                count_slot: Some(layout.startup_dispatch.operation_count),
                rows: [
                    NativeTableIterationRow {
                        row_kind: NativeTableIterationRowKind::StartupResourcePayload,
                        row_index: 0,
                        primary_slot: table_model.startup_operations.resource_payload_rows[0].kind,
                    },
                    NativeTableIterationRow {
                        row_kind: NativeTableIterationRowKind::StartupSpawn,
                        row_index: 1,
                        primary_slot: table_model.startup_operations.spawn_rows[0].kind,
                    },
                    NativeTableIterationRow {
                        row_kind: NativeTableIterationRowKind::StartupRunSchedule,
                        row_index: 2,
                        primary_slot: table_model.startup_operations.run_schedule_rows[0].kind,
                    },
                    NativeTableIterationRow {
                        row_kind: NativeTableIterationRowKind::StartupSpawn,
                        row_index: 3,
                        primary_slot: table_model.startup_operations.spawn_rows[1].kind,
                    },
                ],
            }
        );
        assert_eq!(
            cursors.compiled_schedules.rows,
            [NativeTableIterationRow {
                row_kind: NativeTableIterationRowKind::CompiledSchedule,
                row_index: 0,
                primary_slot: table_model.compiled_schedules.rows[0].schedule_id,
            }]
        );
        assert_eq!(
            cursors.query_plans.rows,
            [NativeTableIterationRow {
                row_kind: NativeTableIterationRowKind::QueryPlan,
                row_index: 0,
                primary_slot: table_model.query_plans.rows[0].query_id,
            }]
        );
    }

    #[test]
    fn iterates_native_descriptor_table_rows_by_count() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let cursors = NATIVE_ECS_TABLE_ITERATION_CURSORS;
        let rows = ECS_DESCRIPTOR_TABLE_ITERATION_ROWS;
        assert_eq!(
            rows,
            [
                NativeDescriptorTableIterationRow {
                    cursor_table: NativeTableIterationKind::ComponentDescriptors,
                    cursor_row_index: 0,
                    expected_table_count: 2,
                    count_slot: cursors.component_descriptors.count_slot.unwrap(),
                    primary_slot: cursors.component_descriptors.rows[0].primary_slot,
                    decode_family: NativeDescriptorDecodeFamily::ComponentResource,
                    qword_load_start: 0,
                    qword_load_len: 1,
                    dword_load_start: 0,
                    dword_load_len: 5,
                },
                NativeDescriptorTableIterationRow {
                    cursor_table: NativeTableIterationKind::ComponentDescriptors,
                    cursor_row_index: 1,
                    expected_table_count: 2,
                    count_slot: cursors.component_descriptors.count_slot.unwrap(),
                    primary_slot: cursors.component_descriptors.rows[1].primary_slot,
                    decode_family: NativeDescriptorDecodeFamily::ComponentResource,
                    qword_load_start: 1,
                    qword_load_len: 1,
                    dword_load_start: 5,
                    dword_load_len: 5,
                },
                NativeDescriptorTableIterationRow {
                    cursor_table: NativeTableIterationKind::ResourceDescriptors,
                    cursor_row_index: 0,
                    expected_table_count: 1,
                    count_slot: cursors.resource_descriptors.count_slot.unwrap(),
                    primary_slot: cursors.resource_descriptors.rows[0].primary_slot,
                    decode_family: NativeDescriptorDecodeFamily::ComponentResource,
                    qword_load_start: 2,
                    qword_load_len: 1,
                    dword_load_start: 10,
                    dword_load_len: 4,
                },
                NativeDescriptorTableIterationRow {
                    cursor_table: NativeTableIterationKind::SystemDescriptors,
                    cursor_row_index: 0,
                    expected_table_count: 1,
                    count_slot: cursors.system_descriptors.count_slot.unwrap(),
                    primary_slot: cursors.system_descriptors.rows[0].primary_slot,
                    decode_family: NativeDescriptorDecodeFamily::SystemQuerySchedule,
                    qword_load_start: 0,
                    qword_load_len: 4,
                    dword_load_start: 0,
                    dword_load_len: 6,
                },
                NativeDescriptorTableIterationRow {
                    cursor_table: NativeTableIterationKind::QueryDescriptors,
                    cursor_row_index: 0,
                    expected_table_count: 1,
                    count_slot: cursors.query_descriptors.count_slot.unwrap(),
                    primary_slot: cursors.query_descriptors.rows[0].primary_slot,
                    decode_family: NativeDescriptorDecodeFamily::SystemQuerySchedule,
                    qword_load_start: 4,
                    qword_load_len: 3,
                    dword_load_start: 6,
                    dword_load_len: 3,
                },
                NativeDescriptorTableIterationRow {
                    cursor_table: NativeTableIterationKind::ScheduleDescriptors,
                    cursor_row_index: 0,
                    expected_table_count: 1,
                    count_slot: cursors.schedule_descriptors.count_slot.unwrap(),
                    primary_slot: cursors.schedule_descriptors.rows[0].primary_slot,
                    decode_family: NativeDescriptorDecodeFamily::SystemQuerySchedule,
                    qword_load_start: 7,
                    qword_load_len: 2,
                    dword_load_start: 9,
                    dword_load_len: 2,
                },
            ]
        );
        assert_eq!(rows[0].count_slot.offset, 0);
        assert_eq!(rows[1].count_slot.offset, 0);
        assert_eq!(rows[2].count_slot.offset, 8);
        assert_eq!(rows[3].count_slot.offset, 16);
        assert_eq!(rows[4].count_slot.offset, 24);
        assert_eq!(rows[5].count_slot.offset, 32);

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        let mut search_start = 0usize;
        for row in rows {
            let count_sequence =
                compare_stack_slot_sequence(row.count_slot.offset, row.expected_table_count);
            let (qword_loads, dword_loads): (&[(i32, u16)], &[(i32, u16)]) = match row.decode_family
            {
                NativeDescriptorDecodeFamily::ComponentResource => (
                    &ECS_COMPONENT_RESOURCE_DESCRIPTOR_QWORD_LOADS,
                    &ECS_COMPONENT_RESOURCE_DESCRIPTOR_DWORD_LOADS,
                ),
                NativeDescriptorDecodeFamily::SystemQuerySchedule => (
                    &ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_QWORD_LOADS,
                    &ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_DWORD_LOADS,
                ),
            };
            let (first_metadata_offset, first_stack_slot) = qword_loads[row.qword_load_start];
            let first_load_sequence =
                metadata_qword_load_store_sequence(first_metadata_offset, first_stack_slot);
            let count_index = find_subsequence_from(&text, &count_sequence, search_start)
                .expect("descriptor row should compare its table count before loading");
            let first_load_index = find_subsequence_from(&text, &first_load_sequence, count_index)
                .expect("descriptor row should load fields after count validation");
            assert!(
                count_index < first_load_index,
                "descriptor row {:?} should count-check before its first qword load",
                row
            );
            search_start = first_load_index + first_load_sequence.len();

            for (metadata_offset, stack_slot) in qword_loads
                .iter()
                .copied()
                .skip(row.qword_load_start)
                .take(row.qword_load_len)
            {
                assert!(
                    contains_subsequence(
                        &text,
                        &metadata_qword_load_store_sequence(metadata_offset, stack_slot),
                    ),
                    "descriptor row {:?} should load qword metadata offset {} into stack slot {}",
                    row,
                    metadata_offset,
                    stack_slot
                );
            }
            for (metadata_offset, stack_slot) in dword_loads
                .iter()
                .copied()
                .skip(row.dword_load_start)
                .take(row.dword_load_len)
            {
                assert!(
                    contains_subsequence(
                        &text,
                        &metadata_dword_disp32_load_qword_store_sequence(
                            metadata_offset,
                            stack_slot,
                        ),
                    ),
                    "descriptor row {:?} should load dword metadata offset {} into stack slot {}",
                    row,
                    metadata_offset,
                    stack_slot
                );
            }
        }

        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "descriptor row iteration should preserve compiled Move success"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_STARTUP_STATE_FAILURE_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "descriptor row count mismatch should use the descriptor/startup-state failure exit"
        );
    }

    #[test]
    fn decodes_native_descriptor_names_into_table_state() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert_eq!(NATIVE_ECS_EXECUTION_STATE_LAYOUT.frame_size, 1088);
        assert_eq!(
            NATIVE_ECS_EXECUTION_STATE_LAYOUT.descriptor_names,
            NativeDescriptorNameTableSlots {
                position: NativeNameReferenceSlots {
                    byte_offset: NativeEcsSlot {
                        offset: 760,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 768,
                        byte_len: 8,
                    },
                },
                velocity: NativeNameReferenceSlots {
                    byte_offset: NativeEcsSlot {
                        offset: 776,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 784,
                        byte_len: 8,
                    },
                },
                time: NativeNameReferenceSlots {
                    byte_offset: NativeEcsSlot {
                        offset: 792,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 800,
                        byte_len: 8,
                    },
                },
                move_system: NativeNameReferenceSlots {
                    byte_offset: NativeEcsSlot {
                        offset: 808,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 816,
                        byte_len: 8,
                    },
                },
                movers_query: NativeNameReferenceSlots {
                    byte_offset: NativeEcsSlot {
                        offset: 824,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 832,
                        byte_len: 8,
                    },
                },
                main_schedule: NativeNameReferenceSlots {
                    byte_offset: NativeEcsSlot {
                        offset: 840,
                        byte_len: 8,
                    },
                    byte_len: NativeEcsSlot {
                        offset: 848,
                        byte_len: 8,
                    },
                },
            }
        );
        assert_eq!(
            ECS_DESCRIPTOR_NAME_REFERENCES,
            [
                NativeDescriptorNameReference {
                    name: "Demo.Position",
                    byte_len_offset: 120,
                    byte_offset: 124,
                    byte_offset_slot: ECS_POSITION_DESCRIPTOR_NAME_OFFSET_SLOT,
                    byte_len_slot: ECS_POSITION_DESCRIPTOR_NAME_LEN_SLOT,
                },
                NativeDescriptorNameReference {
                    name: "Demo.Velocity",
                    byte_len_offset: 189,
                    byte_offset: 193,
                    byte_offset_slot: ECS_VELOCITY_DESCRIPTOR_NAME_OFFSET_SLOT,
                    byte_len_slot: ECS_VELOCITY_DESCRIPTOR_NAME_LEN_SLOT,
                },
                NativeDescriptorNameReference {
                    name: "Demo.Time",
                    byte_len_offset: 258,
                    byte_offset: 262,
                    byte_offset_slot: ECS_TIME_DESCRIPTOR_NAME_OFFSET_SLOT,
                    byte_len_slot: ECS_TIME_DESCRIPTOR_NAME_LEN_SLOT,
                },
                NativeDescriptorNameReference {
                    name: "Demo.Move",
                    byte_len_offset: 311,
                    byte_offset: 315,
                    byte_offset_slot: ECS_MOVE_SYSTEM_DESCRIPTOR_NAME_OFFSET_SLOT,
                    byte_len_slot: ECS_MOVE_SYSTEM_DESCRIPTOR_NAME_LEN_SLOT,
                },
                NativeDescriptorNameReference {
                    name: "Demo.Move.movers",
                    byte_len_offset: 445,
                    byte_offset: 449,
                    byte_offset_slot: ECS_MOVERS_QUERY_DESCRIPTOR_NAME_OFFSET_SLOT,
                    byte_len_slot: ECS_MOVERS_QUERY_DESCRIPTOR_NAME_LEN_SLOT,
                },
                NativeDescriptorNameReference {
                    name: "Demo.Main",
                    byte_len_offset: 535,
                    byte_offset: 539,
                    byte_offset_slot: ECS_MAIN_SCHEDULE_DESCRIPTOR_NAME_OFFSET_SLOT,
                    byte_len_slot: ECS_MAIN_SCHEDULE_DESCRIPTOR_NAME_LEN_SLOT,
                },
            ]
        );
        assert_eq!(
            NATIVE_ECS_TABLE_MODEL.descriptors.component_rows[0].name,
            NATIVE_ECS_EXECUTION_STATE_LAYOUT.descriptor_names.position
        );
        assert_eq!(
            NATIVE_ECS_TABLE_MODEL.descriptors.component_rows[1].name,
            NATIVE_ECS_EXECUTION_STATE_LAYOUT.descriptor_names.velocity
        );
        assert_eq!(
            NATIVE_ECS_TABLE_MODEL.descriptors.resource_rows[0].name,
            NATIVE_ECS_EXECUTION_STATE_LAYOUT.descriptor_names.time
        );
        assert_eq!(
            NATIVE_ECS_TABLE_MODEL.descriptors.system_rows[0].name,
            NATIVE_ECS_EXECUTION_STATE_LAYOUT
                .descriptor_names
                .move_system
        );
        assert_eq!(
            NATIVE_ECS_TABLE_MODEL.descriptors.query_rows[0].name,
            NATIVE_ECS_EXECUTION_STATE_LAYOUT
                .descriptor_names
                .movers_query
        );
        assert_eq!(
            NATIVE_ECS_TABLE_MODEL.descriptors.schedule_rows[0].name,
            NATIVE_ECS_EXECUTION_STATE_LAYOUT
                .descriptor_names
                .main_schedule
        );

        for reference in ECS_DESCRIPTOR_NAME_REFERENCES {
            assert!(
                contains_subsequence(
                    &text,
                    &mov_rax_immediate_store_sequence(
                        reference.byte_offset,
                        reference.byte_offset_slot,
                    ),
                ),
                "generated text should store descriptor name byte offset {} into stack slot {}",
                reference.byte_offset,
                reference.byte_offset_slot
            );
            assert!(
                contains_subsequence(
                    &text,
                    &metadata_dword_disp32_load_qword_store_sequence(
                        reference.byte_len_offset,
                        reference.byte_len_slot,
                    ),
                ),
                "generated text should load descriptor name length at metadata offset {} into stack slot {}",
                reference.byte_len_offset,
                reference.byte_len_slot
            );
            assert!(
                contains_subsequence(
                    &text,
                    &compare_stack_slot_sequence(reference.byte_offset_slot, reference.byte_offset),
                ),
                "generated text should validate descriptor name offset for {}",
                reference.name
            );
            assert!(
                contains_subsequence(
                    &text,
                    &compare_stack_slot_sequence(
                        reference.byte_len_slot,
                        reference.name.len() as u64,
                    ),
                ),
                "generated text should validate descriptor name length for {}",
                reference.name
            );
            for sequence in metadata_ascii_compare_sequences(
                reference.byte_offset as i32,
                reference.name.as_bytes(),
            ) {
                assert!(
                    contains_subsequence(&text, &sequence),
                    "generated text should validate descriptor name bytes for {}",
                    reference.name
                );
            }
        }
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should preserve compiled Move success"
        );
    }

    #[test]
    fn materializes_native_descriptor_record_state() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert_eq!(
            ECS_DESCRIPTOR_RECORD_OFFSET_FIELD_OFFSETS,
            [20, 36, 52, 68, 84]
        );
        assert_eq!(
            ECS_DESCRIPTOR_RECORD_BYTE_LEN_FIELD_OFFSETS,
            [24, 40, 56, 72, 88]
        );
        assert_eq!(ECS_DESCRIPTOR_RECORD_OFFSET_SLOTS, [96, 112, 128, 144, 160]);
        assert_eq!(
            ECS_DESCRIPTOR_RECORD_BYTE_LEN_SLOTS,
            [104, 120, 136, 152, 168]
        );
        assert_eq!(
            ECS_EXPECTED_DESCRIPTOR_RECORD_OFFSETS,
            [112, 250, 303, 437, 527]
        );
        assert_eq!(
            ECS_EXPECTED_DESCRIPTOR_RECORD_BYTE_LENS,
            [138, 53, 134, 90, 50]
        );

        for (metadata_offset, stack_slot) in ECS_DESCRIPTOR_RECORD_OFFSET_FIELD_OFFSETS
            .iter()
            .zip(ECS_DESCRIPTOR_RECORD_OFFSET_SLOTS)
        {
            assert!(
                contains_subsequence(
                    &text,
                    &metadata_dword_load_store_sequence(*metadata_offset, stack_slot),
                ),
                "generated text should store descriptor section offset {} into stack slot {}",
                metadata_offset,
                stack_slot
            );
        }
        for (metadata_offset, stack_slot) in ECS_DESCRIPTOR_RECORD_BYTE_LEN_FIELD_OFFSETS
            .iter()
            .zip(ECS_DESCRIPTOR_RECORD_BYTE_LEN_SLOTS)
        {
            assert!(
                contains_subsequence(
                    &text,
                    &metadata_dword_load_store_sequence(*metadata_offset, stack_slot),
                ),
                "generated text should store descriptor section length {} into stack slot {}",
                metadata_offset,
                stack_slot
            );
        }
        assert!(
            contains_subsequence(&text, &compare_stack_slot_sequence(128, 303),),
            "generated text should validate the materialized system record offset"
        );
        assert!(
            contains_subsequence(&text, &compare_stack_slot_sequence(168, 50),),
            "generated text should validate the materialized schedule record byte length"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should preserve the compiled Move success code"
        );
    }

    #[test]
    fn decodes_native_component_resource_descriptor_records() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let storage_compatibility = storage_compatibility_for_program(&program);
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert_eq!(
            ECS_COMPONENT_RESOURCE_DESCRIPTOR_QWORD_LOADS,
            [
                (112, ECS_POSITION_DESCRIPTOR_ID_SLOT),
                (181, ECS_VELOCITY_DESCRIPTOR_ID_SLOT),
                (250, ECS_TIME_DESCRIPTOR_ID_SLOT),
            ]
        );
        assert_eq!(
            ECS_COMPONENT_RESOURCE_DESCRIPTOR_DWORD_LOADS,
            [
                (137, ECS_POSITION_DESCRIPTOR_SIZE_SLOT),
                (141, ECS_POSITION_DESCRIPTOR_ALIGN_SLOT),
                (145, ECS_POSITION_DESCRIPTOR_FIELD_COUNT_SLOT),
                (161, ECS_POSITION_DESCRIPTOR_X_FIELD_OFFSET_SLOT),
                (177, ECS_POSITION_DESCRIPTOR_Y_FIELD_OFFSET_SLOT),
                (206, ECS_VELOCITY_DESCRIPTOR_SIZE_SLOT),
                (210, ECS_VELOCITY_DESCRIPTOR_ALIGN_SLOT),
                (214, ECS_VELOCITY_DESCRIPTOR_FIELD_COUNT_SLOT),
                (230, ECS_VELOCITY_DESCRIPTOR_X_FIELD_OFFSET_SLOT),
                (246, ECS_VELOCITY_DESCRIPTOR_Y_FIELD_OFFSET_SLOT),
                (271, ECS_TIME_DESCRIPTOR_SIZE_SLOT),
                (275, ECS_TIME_DESCRIPTOR_ALIGN_SLOT),
                (279, ECS_TIME_DESCRIPTOR_FIELD_COUNT_SLOT),
                (299, ECS_TIME_DESCRIPTOR_DELTA_FIELD_OFFSET_SLOT),
            ]
        );
        assert_eq!(
            ECS_COMPONENT_RESOURCE_DESCRIPTOR_EXPECTED,
            [
                (ECS_POSITION_DESCRIPTOR_ID_SLOT, DEMO_POSITION_COMPONENT_ID),
                (ECS_POSITION_DESCRIPTOR_SIZE_SLOT, 8),
                (ECS_POSITION_DESCRIPTOR_ALIGN_SLOT, 4),
                (ECS_POSITION_DESCRIPTOR_FIELD_COUNT_SLOT, 2),
                (ECS_POSITION_DESCRIPTOR_X_FIELD_OFFSET_SLOT, 0),
                (ECS_POSITION_DESCRIPTOR_Y_FIELD_OFFSET_SLOT, 4),
                (ECS_VELOCITY_DESCRIPTOR_ID_SLOT, DEMO_VELOCITY_COMPONENT_ID),
                (ECS_VELOCITY_DESCRIPTOR_SIZE_SLOT, 8),
                (ECS_VELOCITY_DESCRIPTOR_ALIGN_SLOT, 4),
                (ECS_VELOCITY_DESCRIPTOR_FIELD_COUNT_SLOT, 2),
                (ECS_VELOCITY_DESCRIPTOR_X_FIELD_OFFSET_SLOT, 0),
                (ECS_VELOCITY_DESCRIPTOR_Y_FIELD_OFFSET_SLOT, 4),
                (ECS_TIME_DESCRIPTOR_ID_SLOT, DEMO_TIME_RESOURCE_ID),
                (ECS_TIME_DESCRIPTOR_SIZE_SLOT, 4),
                (ECS_TIME_DESCRIPTOR_ALIGN_SLOT, 4),
                (ECS_TIME_DESCRIPTOR_FIELD_COUNT_SLOT, 1),
                (ECS_TIME_DESCRIPTOR_DELTA_FIELD_OFFSET_SLOT, 0),
            ]
        );

        for (metadata_offset, stack_slot) in ECS_COMPONENT_RESOURCE_DESCRIPTOR_QWORD_LOADS {
            assert!(
                contains_subsequence(
                    &text,
                    &metadata_qword_load_store_sequence(metadata_offset, stack_slot),
                ),
                "generated text should load descriptor qword at metadata offset {} into stack slot {}",
                metadata_offset,
                stack_slot
            );
        }
        for (metadata_offset, stack_slot) in ECS_COMPONENT_RESOURCE_DESCRIPTOR_DWORD_LOADS {
            assert!(
                contains_subsequence(
                    &text,
                    &metadata_dword_disp32_load_qword_store_sequence(metadata_offset, stack_slot),
                ),
                "generated text should load descriptor dword at metadata offset {} into stack slot {}",
                metadata_offset,
                stack_slot
            );
        }
        for (stack_slot, expected) in ECS_COMPONENT_RESOURCE_DESCRIPTOR_EXPECTED {
            assert!(
                contains_subsequence(&text, &compare_stack_slot_sequence(stack_slot, expected),),
                "generated text should validate descriptor stack slot {} against {}",
                stack_slot,
                expected
            );
        }
        assert!(
            contains_subsequence(
                &text,
                &load_qword_at_stack_address_store_sequence(
                    storage_compatibility
                        .catalog_table
                        .slots
                        .row_count_address
                        .offset,
                    ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT,
                ),
            ),
            "generated text should preserve catalog-backed native query-plan construction"
        );
        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    ECS_STARTUP_TABLE_RUN_SCHEDULE_ID_SLOT,
                    ECS_COMPILED_SCHEDULE_ID_SLOT,
                ),
            ),
            "generated text should preserve compiled schedule state"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should preserve the compiled Move success code"
        );
    }

    #[test]
    fn decodes_native_system_query_schedule_descriptor_records() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let storage_compatibility = storage_compatibility_for_program(&program);
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert_eq!(
            ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_QWORD_LOADS,
            [
                (303, ECS_MOVE_SYSTEM_DESCRIPTOR_ID_SLOT),
                (340, ECS_MOVE_SYSTEM_RESOURCE_PARAM_RESOURCE_ID_SLOT),
                (383, ECS_MOVE_SYSTEM_QUERY_TERM0_COMPONENT_ID_SLOT),
                (412, ECS_MOVE_SYSTEM_QUERY_TERM1_COMPONENT_ID_SLOT),
                (437, ECS_MOVERS_QUERY_DESCRIPTOR_ID_SLOT),
                (473, ECS_MOVERS_QUERY_TERM0_COMPONENT_ID_SLOT),
                (502, ECS_MOVERS_QUERY_TERM1_COMPONENT_ID_SLOT),
                (527, ECS_MAIN_SCHEDULE_DESCRIPTOR_ID_SLOT),
                (556, ECS_MAIN_SCHEDULE_RUN_SYSTEM_ID_SLOT),
            ]
        );
        assert_eq!(
            ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_DWORD_LOADS,
            [
                (324, ECS_MOVE_SYSTEM_DESCRIPTOR_PARAM_COUNT_SLOT),
                (336, ECS_MOVE_SYSTEM_RESOURCE_PARAM_KIND_SLOT),
                (371, ECS_MOVE_SYSTEM_QUERY_PARAM_KIND_SLOT),
                (375, ECS_MOVE_SYSTEM_QUERY_PARAM_TERM_COUNT_SLOT),
                (379, ECS_MOVE_SYSTEM_QUERY_TERM0_ACCESS_SLOT),
                (408, ECS_MOVE_SYSTEM_QUERY_TERM1_ACCESS_SLOT),
                (465, ECS_MOVERS_QUERY_DESCRIPTOR_TERM_COUNT_SLOT),
                (469, ECS_MOVERS_QUERY_TERM0_ACCESS_SLOT),
                (498, ECS_MOVERS_QUERY_TERM1_ACCESS_SLOT),
                (548, ECS_MAIN_SCHEDULE_DESCRIPTOR_ITEM_COUNT_SLOT),
                (552, ECS_MAIN_SCHEDULE_RUN_ITEM_KIND_SLOT),
            ]
        );
        assert_eq!(
            ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_EXPECTED,
            [
                (ECS_MOVE_SYSTEM_DESCRIPTOR_ID_SLOT, DEMO_MOVE_SYSTEM_ID),
                (ECS_MOVE_SYSTEM_DESCRIPTOR_PARAM_COUNT_SLOT, 2),
                (ECS_MOVE_SYSTEM_RESOURCE_PARAM_KIND_SLOT, 1),
                (
                    ECS_MOVE_SYSTEM_RESOURCE_PARAM_RESOURCE_ID_SLOT,
                    DEMO_TIME_RESOURCE_ID,
                ),
                (ECS_MOVE_SYSTEM_QUERY_PARAM_KIND_SLOT, 2),
                (ECS_MOVE_SYSTEM_QUERY_PARAM_TERM_COUNT_SLOT, 2),
                (ECS_MOVE_SYSTEM_QUERY_TERM0_ACCESS_SLOT, 2),
                (
                    ECS_MOVE_SYSTEM_QUERY_TERM0_COMPONENT_ID_SLOT,
                    DEMO_POSITION_COMPONENT_ID,
                ),
                (ECS_MOVE_SYSTEM_QUERY_TERM1_ACCESS_SLOT, 1),
                (
                    ECS_MOVE_SYSTEM_QUERY_TERM1_COMPONENT_ID_SLOT,
                    DEMO_VELOCITY_COMPONENT_ID,
                ),
                (ECS_MOVERS_QUERY_DESCRIPTOR_ID_SLOT, DEMO_MOVERS_QUERY_ID),
                (ECS_MOVERS_QUERY_DESCRIPTOR_TERM_COUNT_SLOT, 2),
                (ECS_MOVERS_QUERY_TERM0_ACCESS_SLOT, 2),
                (
                    ECS_MOVERS_QUERY_TERM0_COMPONENT_ID_SLOT,
                    DEMO_POSITION_COMPONENT_ID,
                ),
                (ECS_MOVERS_QUERY_TERM1_ACCESS_SLOT, 1),
                (
                    ECS_MOVERS_QUERY_TERM1_COMPONENT_ID_SLOT,
                    DEMO_VELOCITY_COMPONENT_ID,
                ),
                (ECS_MAIN_SCHEDULE_DESCRIPTOR_ID_SLOT, DEMO_MAIN_SCHEDULE_ID),
                (ECS_MAIN_SCHEDULE_DESCRIPTOR_ITEM_COUNT_SLOT, 1),
                (ECS_MAIN_SCHEDULE_RUN_ITEM_KIND_SLOT, 1),
                (ECS_MAIN_SCHEDULE_RUN_SYSTEM_ID_SLOT, DEMO_MOVE_SYSTEM_ID),
            ]
        );

        for (metadata_offset, stack_slot) in ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_QWORD_LOADS {
            assert!(
                contains_subsequence(
                    &text,
                    &metadata_qword_load_store_sequence(metadata_offset, stack_slot),
                ),
                "generated text should load system/query/schedule qword at metadata offset {} into stack slot {}",
                metadata_offset,
                stack_slot
            );
        }
        for (metadata_offset, stack_slot) in ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_DWORD_LOADS {
            assert!(
                contains_subsequence(
                    &text,
                    &metadata_dword_disp32_load_qword_store_sequence(metadata_offset, stack_slot),
                ),
                "generated text should load system/query/schedule dword at metadata offset {} into stack slot {}",
                metadata_offset,
                stack_slot
            );
        }
        for (stack_slot, expected) in ECS_SYSTEM_QUERY_SCHEDULE_DESCRIPTOR_EXPECTED {
            assert!(
                contains_subsequence(&text, &compare_stack_slot_sequence(stack_slot, expected),),
                "generated text should validate system/query/schedule stack slot {} against {}",
                stack_slot,
                expected
            );
        }
        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    ECS_STARTUP_TABLE_RUN_SCHEDULE_ID_SLOT,
                    ECS_COMPILED_SCHEDULE_ID_SLOT,
                ),
            ),
            "generated text should preserve compiled schedule state from startup run"
        );
        assert!(
            contains_subsequence(
                &text,
                &load_qword_at_stack_address_store_sequence(
                    storage_compatibility
                        .catalog_table
                        .slots
                        .row_count_address
                        .offset,
                    ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT,
                ),
            ),
            "generated text should preserve catalog-backed native query-plan construction"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_component_field_multiply_sequence(
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "generated text should preserve compiled Demo.Move field math"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should preserve the compiled Move success code"
        );
    }

    #[test]
    fn defines_native_move_query_loop_observable() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let core = core_lower::lower_program_to_core(&program).expect("move_system.arc lowers");
        let startup_payloads = EcsStartupPayloads {
            startup_record_count: 3,
            resource_operation_kind_offset: 577,
            resource_id_offset: 581,
            resource_id: DEMO_TIME_RESOURCE_ID,
            resource_payload_len_offset: 602,
            resource_payload_offset: 606,
            resource_payload: [0x00, 0x00, 0x80, 0x3f],
            spawn_operation_kind_offset: 610,
            spawn_component_count_offset: 614,
            spawn_component_count: 2,
            position_component_id_offset: 618,
            position_component_id: DEMO_POSITION_COMPONENT_ID,
            position_payload_len_offset: 643,
            position_payload_offset: 647,
            position_payload: [0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40],
            velocity_component_id_offset: 655,
            velocity_component_id: DEMO_VELOCITY_COMPONENT_ID,
            velocity_payload_len_offset: 680,
            velocity_payload_offset: 684,
            velocity_payload: [0x00, 0x00, 0x40, 0x40, 0x00, 0x00, 0x80, 0x40],
            spawn_operations: vec![ParsedSpawnOperation {
                operation_kind_offset: 610,
                component_count_offset: 614,
                component_count: 2,
                position_component_id_offset: 618,
                position_component_id: DEMO_POSITION_COMPONENT_ID,
                position_payload_len_offset: 643,
                position_payload_offset: 647,
                position_payload: [0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40],
                velocity_component_id_offset: 655,
                velocity_component_id: DEMO_VELOCITY_COMPONENT_ID,
                velocity_payload_len_offset: 680,
                velocity_payload_offset: 684,
                velocity_payload: [0x00, 0x00, 0x40, 0x40, 0x00, 0x00, 0x80, 0x40],
            }],
            run_schedule_operation_kind_offset: 692,
            run_schedule_id_offset: 696,
            run_schedule_id: DEMO_MAIN_SCHEDULE_ID,
        };

        let observable = native_move_query_loop_observable_from_core(&core, &startup_payloads)
            .expect("native query-loop observable is defined");

        assert_eq!(
            observable,
            NativeMoveQueryLoopObservable {
                schedule_name: "Demo.Main".to_string(),
                schedule_id: DEMO_MAIN_SCHEDULE_ID,
                schedule_run_system_id: DEMO_MOVE_SYSTEM_ID,
                schedule_run_system_name: "Demo.Move".to_string(),
                system_name: "Move".to_string(),
                query_param: "movers".to_string(),
                position_binding: "pos".to_string(),
                velocity_binding: "vel".to_string(),
                position_component_id: DEMO_POSITION_COMPONENT_ID,
                position_component_name: "Demo.Position".to_string(),
                velocity_component_id: DEMO_VELOCITY_COMPONENT_ID,
                velocity_component_name: "Demo.Velocity".to_string(),
                time_param: "time".to_string(),
                time_resource_id: DEMO_TIME_RESOURCE_ID,
                time_resource_name: "Demo.Time".to_string(),
                updates: vec![
                    NativeMoveQueryLoopUpdate {
                        target_field: "x".to_string(),
                        velocity_field: "x".to_string(),
                        time_field: "delta".to_string(),
                    },
                    NativeMoveQueryLoopUpdate {
                        target_field: "y".to_string(),
                        velocity_field: "y".to_string(),
                        time_field: "delta".to_string(),
                    },
                ],
                target_position_payload: [0x00, 0x00, 0x80, 0x40, 0x00, 0x00, 0xc0, 0x40,],
                field_product_payload: [0x00, 0x00, 0x40, 0x40, 0x00, 0x00, 0x80, 0x40,],
                rows: vec![NativeMoveQueryLoopRowObservable {
                    row_index: 0,
                    target_position_payload: [0x00, 0x00, 0x80, 0x40, 0x00, 0x00, 0xc0, 0x40,],
                    field_product_payload: [0x00, 0x00, 0x40, 0x40, 0x00, 0x00, 0x80, 0x40,],
                }],
            }
        );
    }

    #[test]
    fn emits_native_query_loop_row_scan_skeleton() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT,
                    ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT,
                ),
            ),
            "generated text should carry the planned matched row count into the scan-count slot"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slot_sequence(ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT, 1),
            ),
            "generated text should require exactly one scanned bootstrap row"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_QUERY_LOOP_SCAN_FAILURE_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "generated text should expose a row-scan failure code"
        );
    }

    #[test]
    fn emits_native_query_loop_field_multiply() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert!(
            contains_subsequence(
                &text,
                &query_plan_component_field_multiply_sequence(
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "generated text should compute Velocity.x * Time.delta through the planned Velocity address"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_component_field_multiply_sequence(
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                    4,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT + 4,
                ),
            ),
            "generated text should compute Velocity.y * Time.delta through the planned Velocity address"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_QUERY_LOOP_FIELD_MATH_FAILURE_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "generated text should expose a field-math failure code"
        );
    }

    #[test]
    fn emits_native_query_loop_position_stores() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert!(
            contains_subsequence(
                &text,
                &query_plan_position_store_sequence(
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "generated text should update Position.x through the planned Position address"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_position_store_sequence(
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                    4,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT + 4,
                ),
            ),
            "generated text should update Position.y through the planned Position address"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should expose the compiled Move success code"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_QUERY_LOOP_POSITION_STORE_FAILURE_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "generated text should expose a Position-store failure code"
        );
    }

    #[test]
    fn replaces_bootstrap_move_helper_with_compiled_query_loop() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let core = core_lower::lower_program_to_core(&program).expect("move_system.arc lowers");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");
        let startup_payloads = startup_payloads(&metadata).expect("startup payloads parse");
        let observable = native_move_query_loop_observable_from_core(&core, &startup_payloads)
            .expect("compiled Move query-loop proof is derived from Core");

        assert_eq!(observable.schedule_name, "Demo.Main");
        assert_eq!(observable.schedule_id, DEMO_MAIN_SCHEDULE_ID);
        assert_eq!(observable.schedule_run_system_id, DEMO_MOVE_SYSTEM_ID);
        assert_eq!(observable.schedule_run_system_name, "Demo.Move");
        assert_eq!(observable.system_name, "Move");
        assert_eq!(
            observable.target_position_payload,
            [0x00, 0x00, 0x80, 0x40, 0x00, 0x00, 0xc0, 0x40,]
        );

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    ECS_STARTUP_TABLE_RUN_SCHEDULE_ID_SLOT,
                    ECS_COMPILED_SCHEDULE_ID_SLOT,
                ),
            ),
            "generated text should materialize startup run Demo.Main into compiled schedule state"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slots_equal_sequence(
                    ECS_COMPILED_SCHEDULE_ID_SLOT,
                    ECS_MAIN_SCHEDULE_DESCRIPTOR_ID_SLOT,
                ),
            ),
            "generated text should validate compiled schedule state against decoded Demo.Main"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_component_field_multiply_sequence(
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "generated text should execute compiled Demo.Move field multiplication"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_position_store_sequence(
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "generated text should execute compiled Demo.Move Position store"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should expose compiled Move success"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_RUN_SCHEDULE_DISPATCH_FAILURE_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "generated text should expose run schedule dispatch failure"
        );
    }

    #[test]
    fn dispatches_native_startup_operations() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");
        let startup_payloads = startup_payloads(&metadata).expect("startup payloads parse");

        assert_eq!(startup_payloads.startup_record_count, 3);
        assert_eq!(startup_payloads.resource_operation_kind_offset, 577);
        assert_eq!(startup_payloads.spawn_operation_kind_offset, 610);
        assert_eq!(startup_payloads.run_schedule_operation_kind_offset, 692);

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert!(
            contains_subsequence(
                &text,
                &metadata_dword_load_store_sequence(
                    ECS_STARTUP_RECORD_COUNT_OFFSET,
                    ECS_STARTUP_OPERATION_COUNT_SLOT,
                ),
            ),
            "generated text should store the startup operation count"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slot_sequence(
                    ECS_STARTUP_TABLE_RESOURCE_KIND_SLOT,
                    ECS_STARTUP_OP_RESOURCE_PAYLOAD as u64,
                ),
            ),
            "generated text should check the resource operation kind from the startup table"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slot_sequence(
                    ECS_STARTUP_TABLE_SPAWN_KIND_SLOT,
                    ECS_STARTUP_OP_SPAWN as u64,
                ),
            ),
            "generated text should check the spawn operation kind from the startup table"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slot_sequence(
                    ECS_STARTUP_TABLE_RUN_SCHEDULE_KIND_SLOT,
                    ECS_STARTUP_OP_RUN_SCHEDULE as u64,
                ),
            ),
            "generated text should check the run schedule operation kind from the startup table"
        );
        assert!(
            contains_subsequence(
                &text,
                &mov_eax_one_store_sequence(ECS_STARTUP_RESOURCE_DISPATCH_COUNT_SLOT),
            ),
            "generated text should record one resource dispatch"
        );
        assert!(
            contains_subsequence(
                &text,
                &mov_eax_one_store_sequence(ECS_STARTUP_SPAWN_DISPATCH_COUNT_SLOT),
            ),
            "generated text should record one spawn dispatch"
        );
        assert!(
            contains_subsequence(
                &text,
                &mov_eax_one_store_sequence(ECS_STARTUP_RUN_SCHEDULE_DISPATCH_COUNT_SLOT),
            ),
            "generated text should record one run-schedule dispatch"
        );
        assert!(
            contains_subsequence(
                &text,
                &metadata_dword_via_offset_slot_to_dword_store_sequence(
                    ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_OFFSET_SLOT,
                    ECS_RESOURCE_PAYLOAD_STORAGE_SLOT,
                ),
            ),
            "generated text should preserve the table-driven resource payload handler"
        );
        assert!(
            contains_subsequence(&text, &compare_stack_slot_sequence(48, 1)),
            "generated text should preserve the spawn row-count handler"
        );
        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    ECS_STARTUP_TABLE_RUN_SCHEDULE_ID_SLOT,
                    ECS_COMPILED_SCHEDULE_ID_SLOT,
                )
            ),
            "generated text should preserve run Demo.Main materialization"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slots_equal_sequence(
                    ECS_COMPILED_SCHEDULE_ID_SLOT,
                    ECS_MAIN_SCHEDULE_DESCRIPTOR_ID_SLOT,
                ),
            ),
            "generated text should preserve decoded-table run Demo.Main validation"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_RUN_SCHEDULE_DISPATCH_FAILURE_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "generated text should expose startup dispatch failure"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should preserve compiled Move success"
        );
    }

    #[test]
    fn materializes_native_storage_catalog_from_descriptors() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let core = core_lower::lower_program_to_core(&program).expect("move_system Core lowers");
        let storage_plan =
            derive_native_world_storage_plan(&core, &assembly, NATIVE_STORAGE_BASE_OFFSET)
                .expect("move_system storage plan derives");
        let table = &storage_plan.tables[0];
        let storage_compatibility = storage_compatibility_for_program(&program);
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");
        let startup_payloads = startup_payloads(&metadata).expect("startup payloads parse");
        let catalog_table = storage_compatibility.catalog_table;

        assert_eq!(startup_payloads.spawn_operations[0].component_count, 2);
        assert_eq!(catalog_table.columns.len(), 2);

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        let final_descriptor_decode =
            metadata_qword_load_store_sequence(556, ECS_MAIN_SCHEDULE_RUN_SYSTEM_ID_SLOT);
        let final_name_reference = ECS_DESCRIPTOR_NAME_REFERENCES[5];
        let final_name_decode = metadata_ascii_compare_sequences(
            final_name_reference.byte_offset as i32,
            final_name_reference.name.as_bytes(),
        )
        .pop()
        .expect("Demo.Main name decode has at least one comparison");
        let final_startup_table_decode = metadata_qword_load_store_sequence(
            startup_payloads.run_schedule_id_offset,
            ECS_STARTUP_TABLE_RUN_SCHEDULE_ID_SLOT,
        );
        let catalog_materialization =
            native_storage_catalog_materialization_sequence(&storage_plan);
        let startup_record_count_materialization = metadata_dword_load_store_sequence(
            ECS_STARTUP_RECORD_COUNT_OFFSET,
            ECS_STARTUP_OPERATION_COUNT_SLOT,
        );
        let first_dispatch = compare_stack_slot_sequence(
            ECS_STARTUP_TABLE_RESOURCE_KIND_SLOT,
            ECS_STARTUP_OP_RESOURCE_PAYLOAD as u64,
        );

        assert_eq!(
            count_subsequence(&text, &catalog_materialization),
            1,
            "the complete descriptor-backed catalog should materialize exactly once"
        );

        let descriptor_decode_index = find_subsequence_from(&text, &final_descriptor_decode, 0)
            .expect("final descriptor row should decode");
        let name_decode_index = find_subsequence_from(
            &text,
            &final_name_decode,
            descriptor_decode_index + final_descriptor_decode.len(),
        )
        .expect("final descriptor name should decode after descriptor rows");
        let startup_table_decode_index = find_subsequence_from(
            &text,
            &final_startup_table_decode,
            name_decode_index + final_name_decode.len(),
        )
        .expect("final startup table field should decode after descriptor names");
        let catalog_materialization_index = find_subsequence_from(
            &text,
            &catalog_materialization,
            startup_table_decode_index + final_startup_table_decode.len(),
        )
        .expect("storage catalog should materialize after the complete startup table");
        let startup_record_count_index = find_subsequence_from(
            &text,
            &startup_record_count_materialization,
            catalog_materialization_index + catalog_materialization.len(),
        )
        .expect("startup record count should load after the complete storage catalog");
        let first_dispatch_index = find_subsequence_from(
            &text,
            &first_dispatch,
            startup_record_count_index + startup_record_count_materialization.len(),
        )
        .expect("startup dispatch should follow storage catalog materialization");
        assert!(
            descriptor_decode_index < name_decode_index
                && name_decode_index < startup_table_decode_index
                && startup_table_decode_index < catalog_materialization_index
                && catalog_materialization_index < startup_record_count_index
                && startup_record_count_index < first_dispatch_index,
            "catalog materialization should follow descriptor/name/startup decoding and precede dispatch"
        );

        assert!(
            contains_subsequence(
                &text,
                &lea_stack_address_store_sequence(
                    table.storage.row_count.offset,
                    catalog_table.slots.row_count_address.offset,
                ),
            ),
            "catalog row-count field should hold the authoritative storage row-count address"
        );

        for slot in [table.storage.capacity, table.catalog.capacity] {
            assert!(contains_subsequence(
                &text,
                &u64_immediate_store_sequence(u64::from(table.capacity), slot.offset),
            ));
        }
        for slot in [table.storage.row_stride, table.catalog.row_stride] {
            assert!(contains_subsequence(
                &text,
                &u64_immediate_store_sequence(u64::from(table.logical_row_stride), slot.offset,),
            ));
        }

        for column in &table.columns {
            for (value, catalog_slot) in [
                (column.schema.id, column.catalog.component_id),
                (u64::from(column.schema.size), column.catalog.element_size),
                (u64::from(column.schema.align), column.catalog.element_align),
            ] {
                assert!(
                    contains_subsequence(
                        &text,
                        &u64_immediate_store_sequence(value, catalog_slot.offset),
                    ),
                    "catalog field at {} should materialize descriptor-derived value {}",
                    catalog_slot.offset,
                    value
                );
            }
            assert!(
                contains_subsequence(
                    &text,
                    &lea_stack_address_store_sequence(
                        column.payload.offset,
                        column.catalog.payload_base_address.offset,
                    ),
                ),
                "catalog column should address its physical row-zero payload base"
            );
        }

        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "catalog materialization should preserve valid move_system exit 47"
        );
    }

    #[test]
    fn emits_descriptor_sized_native_tables_and_columns() {
        let source = include_str!("../../../examples/move_system_two_rows.arc");
        let tokens = lexer::lex(source).expect("two-row movement fixture lexes");
        let program = parser::parse_program(&tokens).expect("two-row movement fixture parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("two-row movement fixture assembles");
        let metadata = ecs_metadata::encode_ecs_metadata(&assembly)
            .expect("two-row movement metadata encodes");
        let demo_plan = storage_plan_for_program(&program);

        assert_eq!(demo_plan.frame_size, 1088);
        assert_eq!(demo_plan.tables.len(), 1);
        assert_eq!(demo_plan.tables[0].columns.len(), 2);
        assert!(demo_plan.tables[0]
            .columns
            .iter()
            .all(|column| column.schema.size == 8 && column.schema.align == 4));

        let startup = startup_payloads(&metadata).expect("two-row startup payloads parse");
        let observable = native_move_query_loop_observable(&program, &startup)
            .expect("two-row query observable exists");
        let mut reordered_role_plan = demo_plan.clone();
        reordered_role_plan.tables[0].columns.reverse();
        let role_compatibility =
            native_storage_compatibility_model(&reordered_role_plan, &observable)
                .expect("query component ids select catalog roles independently of plan order");
        let position_column = reordered_role_plan.tables[0]
            .columns
            .iter()
            .find(|column| column.schema.id == observable.position_component_id)
            .expect("planned Position column exists");
        let velocity_column = reordered_role_plan.tables[0]
            .columns
            .iter()
            .find(|column| column.schema.id == observable.velocity_component_id)
            .expect("planned Velocity column exists");
        assert_eq!(
            role_compatibility.catalog_table.columns[0]
                .slots
                .payload_base_address
                .offset,
            position_column.catalog.payload_base_address.offset,
        );
        assert_eq!(
            role_compatibility.catalog_table.columns[1]
                .slots
                .payload_base_address
                .offset,
            velocity_column.catalog.payload_base_address.offset,
        );

        let demo_text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("the descriptor-sized Demo plan emits");
        assert!(demo_text.starts_with(&runtime_create_prefix(demo_plan.frame_size)));
        assert!(demo_text.ends_with(&runtime_destroy_suffix(demo_plan.frame_size)));
        assert_eq!(
            count_subsequence(
                &demo_text,
                &native_storage_catalog_materialization_sequence(&demo_plan),
            ),
            1,
            "codegen should consume the complete derived Demo plan exactly once"
        );

        let arena_plan = descriptor_sized_arena_emission_plan();
        assert_eq!(arena_plan.frame_size, 1328);
        assert_eq!(arena_plan.tables.len(), 2);
        assert_eq!(
            arena_plan
                .tables
                .iter()
                .map(|table| table.columns.len())
                .collect::<Vec<_>>(),
            [3, 2]
        );
        assert_eq!(
            arena_plan.tables[0]
                .columns
                .iter()
                .map(|column| column.schema.size)
                .collect::<Vec<_>>(),
            [12, 8, 4]
        );
        assert!(arena_plan
            .tables
            .iter()
            .all(|table| table.columns.iter().all(|column| column.schema.align == 4)));

        let mut arena_catalog = Vec::new();
        emit_native_storage_catalog_materialization(&mut arena_catalog, &arena_plan);
        assert_eq!(
            arena_catalog,
            native_storage_catalog_materialization_sequence(&arena_plan),
            "every table and column should be emitted from the world plan in plan order"
        );
        for table in &arena_plan.tables {
            assert!(contains_subsequence(
                &arena_catalog,
                &u64_immediate_store_sequence(
                    table.columns.len() as u64,
                    table.catalog.column_count.offset,
                ),
            ));
            for column in &table.columns {
                for (value, slot) in [
                    (column.schema.id, column.catalog.component_id),
                    (u64::from(column.schema.size), column.catalog.element_size),
                    (u64::from(column.schema.align), column.catalog.element_align),
                ] {
                    assert!(contains_subsequence(
                        &arena_catalog,
                        &u64_immediate_store_sequence(value, slot.offset),
                    ));
                }
                assert!(contains_subsequence(
                    &arena_catalog,
                    &lea_stack_address_store_sequence(
                        column.payload.offset,
                        column.catalog.payload_base_address.offset,
                    ),
                ));
            }
        }

        let aligned_plan = aligned_synthetic_emission_plan();
        let aligned_columns = &aligned_plan.tables[0].columns;
        assert_eq!(
            aligned_columns
                .iter()
                .map(|column| (
                    column.schema.size,
                    column.schema.align,
                    column.payload.offset
                ))
                .collect::<Vec<_>>(),
            [(8, 8, 960), (16, 16, 976)]
        );
        assert!(
            u32::from(aligned_columns[0].payload.offset)
                + u32::from(aligned_columns[0].payload.byte_len)
                <= u32::from(aligned_columns[1].payload.offset),
            "checked 16-byte padding should keep synthetic columns disjoint"
        );
        let mut aligned_catalog = Vec::new();
        emit_native_storage_catalog_materialization(&mut aligned_catalog, &aligned_plan);
        for column in aligned_columns {
            assert!(contains_subsequence(
                &aligned_catalog,
                &u64_immediate_store_sequence(
                    u64::from(column.schema.align),
                    column.catalog.element_align.offset,
                ),
            ));
            assert!(contains_subsequence(
                &aligned_catalog,
                &lea_stack_address_store_sequence(
                    column.payload.offset,
                    column.catalog.payload_base_address.offset,
                ),
            ));
        }
    }

    #[test]
    fn materializes_spawn_rows_through_storage_catalog() {
        for (fixture_name, source, expected_row_count) in [
            (
                "move_system.arc",
                include_str!("../../../examples/move_system.arc"),
                1usize,
            ),
            (
                "move_system_two_rows.arc",
                include_str!("../../../examples/move_system_two_rows.arc"),
                2usize,
            ),
        ] {
            let tokens =
                lexer::lex(source).unwrap_or_else(|error| panic!("{fixture_name}: {error:?}"));
            let program = parser::parse_program(&tokens)
                .unwrap_or_else(|error| panic!("{fixture_name}: {error:?}"));
            let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
                .unwrap_or_else(|error| panic!("{fixture_name}: {error:?}"));
            let storage_compatibility = storage_compatibility_for_program(&program);
            let catalog_table = storage_compatibility.catalog_table;
            let metadata = ecs_metadata::encode_ecs_metadata(&assembly)
                .unwrap_or_else(|error| panic!("{fixture_name}: {error:?}"));
            let startup_payloads = startup_payloads(&metadata)
                .unwrap_or_else(|error| panic!("{fixture_name}: {error:?}"));
            assert_eq!(startup_payloads.spawn_operations.len(), expected_row_count);

            let text = ecs_metadata_decoder_text_payload(&program, &metadata)
                .unwrap_or_else(|error| panic!("{fixture_name}: {error:?}"));

            for row_index in 0..expected_row_count {
                let spawn_slots = startup_spawn_slots(row_index).expect("bounded spawn row exists");
                let payload_slots = startup_payload_storage_slots(row_index)
                    .expect("bounded payload staging row exists");
                let physical_slots =
                    archetype_storage_row_slots(row_index).expect("bounded physical row exists");
                let committed_count = (row_index + 1) as u32;

                let mut handler = Vec::new();
                let mut jump_offsets = Vec::new();
                emit_spawn_startup_operation_handler(
                    &mut handler,
                    row_index,
                    u64::from(committed_count),
                    storage_compatibility,
                    &mut jump_offsets,
                );

                for sequence in [
                    compare_stack_slots_equal_sequence(
                        spawn_slots.component_count.offset,
                        catalog_table.slots.column_count.offset,
                    ),
                    compare_stack_slot_sequence(
                        catalog_table.slots.column_count.offset,
                        catalog_table.columns.len() as u64,
                    ),
                    compare_stack_slot_sequence(ECS_SPAWN_ROW_COUNT_SLOT, row_index as u64),
                    compare_qword_at_stack_address_sequence(
                        catalog_table.slots.row_count_address.offset,
                        row_index as u64,
                    ),
                    compare_stack_slot_sequence(
                        catalog_table.slots.capacity.offset,
                        storage_compatibility.capacity,
                    ),
                    compare_stack_slot_sequence(
                        catalog_table.columns[0].slots.element_size.offset,
                        u64::from(NATIVE_ECS_QWORD_BYTE_LEN),
                    ),
                    compare_stack_slot_sequence(
                        catalog_table.columns[1].slots.element_size.offset,
                        u64::from(NATIVE_ECS_QWORD_BYTE_LEN),
                    ),
                ] {
                    assert!(
                        contains_subsequence(&handler, &sequence),
                        "{fixture_name} row {row_index} should validate catalog bounds/count state"
                    );
                }
                for (startup_slot, catalog_slot) in
                    startup_spawn_catalog_relations(spawn_slots, catalog_table)
                {
                    assert!(
                        contains_subsequence(
                            &handler,
                            &compare_stack_slots_equal_sequence(startup_slot, catalog_slot),
                        ),
                        "{fixture_name} row {row_index} should validate startup field {startup_slot} against catalog field {catalog_slot}"
                    );
                }

                let position_stage = metadata_qword_via_offset_slot_to_qword_store_sequence(
                    spawn_slots.position_payload_offset.offset,
                    payload_slots.position_payload.offset,
                );
                let velocity_stage = metadata_qword_via_offset_slot_to_qword_store_sequence(
                    spawn_slots.velocity_payload_offset.offset,
                    payload_slots.velocity_payload.offset,
                );
                let position_store = catalog_payload_store_sequence(
                    catalog_table.columns[0],
                    row_index,
                    payload_slots.position_payload.offset,
                );
                let velocity_store = catalog_payload_store_sequence(
                    catalog_table.columns[1],
                    row_index,
                    payload_slots.velocity_payload.offset,
                );
                let count_commit = catalog_row_count_commit_sequence(
                    catalog_table.slots.row_count_address.offset,
                    committed_count,
                    ECS_SPAWN_ROW_COUNT_SLOT,
                );
                let commit_tail = [
                    position_store.as_slice(),
                    velocity_store.as_slice(),
                    count_commit.as_slice(),
                ]
                .concat();

                let position_stage_index = find_subsequence_from(&handler, &position_stage, 0)
                    .expect("Position payload should stage");
                let velocity_stage_index = find_subsequence_from(
                    &handler,
                    &velocity_stage,
                    position_stage_index + position_stage.len(),
                )
                .expect("Velocity payload should stage after Position");
                let position_store_index = find_subsequence_from(
                    &handler,
                    &position_store,
                    velocity_stage_index + velocity_stage.len(),
                )
                .expect("Position payload should store through its catalog base");
                let velocity_store_index = find_subsequence_from(
                    &handler,
                    &velocity_store,
                    position_store_index + position_store.len(),
                )
                .expect("Velocity payload should store through its catalog base");
                let count_commit_index = find_subsequence_from(
                    &handler,
                    &count_commit,
                    velocity_store_index + velocity_store.len(),
                )
                .expect("row count should commit after both payload stores");

                assert!(jump_offsets
                    .iter()
                    .all(|offset| *offset < position_stage_index));
                assert!(
                    handler.ends_with(&commit_tail),
                    "{fixture_name} row {row_index} should end with one contiguous dual-write/count-publication tail"
                );
                assert!(
                    !handler[position_store_index..count_commit_index]
                        .windows(2)
                        .any(|window| window == [0x0f, 0x85]),
                    "{fixture_name} row {row_index} should not branch between storage writes and count commit"
                );
                assert!(
                    contains_subsequence(&text, &commit_tail),
                    "{fixture_name} row {row_index} should embed the isolated contiguous commit tail"
                );

                assert!(
                    !contains_subsequence(
                        &handler,
                        &load_store_stack_slot_sequence(
                            payload_slots.position_payload.offset,
                            physical_slots.position_payload.offset,
                        ),
                    ),
                    "{fixture_name} row {row_index} must not copy staged Position directly to a physical slot"
                );
                assert!(
                    !contains_subsequence(
                        &handler,
                        &load_store_stack_slot_sequence(
                            payload_slots.velocity_payload.offset,
                            physical_slots.velocity_payload.offset,
                        ),
                    ),
                    "{fixture_name} row {row_index} must not copy staged Velocity directly to a physical slot"
                );
            }

            assert!(
                contains_subsequence(
                    &text,
                    &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
                ),
                "{fixture_name} should preserve compiled success exit 47"
            );
        }
    }

    #[test]
    fn iterates_native_startup_operation_table_generically() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        assert_eq!(
            ECS_STARTUP_OPERATION_DISPATCH_ROWS,
            [
                NativeStartupOperationDispatchRow {
                    handler: NativeStartupOperationHandler::ResourcePayload,
                    expected_kind: ECS_STARTUP_OP_RESOURCE_PAYLOAD,
                    kind_slot: ECS_STARTUP_TABLE_RESOURCE_KIND_SLOT,
                    dispatch_count_slot: ECS_STARTUP_RESOURCE_DISPATCH_COUNT_SLOT,
                    dispatch_count_after_row: 1,
                },
                NativeStartupOperationDispatchRow {
                    handler: NativeStartupOperationHandler::Spawn,
                    expected_kind: ECS_STARTUP_OP_SPAWN,
                    kind_slot: ECS_STARTUP_TABLE_SPAWN_KIND_SLOT,
                    dispatch_count_slot: ECS_STARTUP_SPAWN_DISPATCH_COUNT_SLOT,
                    dispatch_count_after_row: 1,
                },
                NativeStartupOperationDispatchRow {
                    handler: NativeStartupOperationHandler::RunSchedule,
                    expected_kind: ECS_STARTUP_OP_RUN_SCHEDULE,
                    kind_slot: ECS_STARTUP_TABLE_RUN_SCHEDULE_KIND_SLOT,
                    dispatch_count_slot: ECS_STARTUP_RUN_SCHEDULE_DISPATCH_COUNT_SLOT,
                    dispatch_count_after_row: 1,
                },
            ]
        );

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert!(
            contains_ordered_subsequences(
                &text,
                &[
                    compare_stack_slot_sequence(
                        ECS_STARTUP_TABLE_RESOURCE_KIND_SLOT,
                        ECS_STARTUP_OP_RESOURCE_PAYLOAD as u64,
                    ),
                    metadata_dword_via_offset_slot_to_dword_store_sequence(
                        ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_OFFSET_SLOT,
                        ECS_RESOURCE_PAYLOAD_STORAGE_SLOT,
                    ),
                    compare_stack_slot_sequence(
                        ECS_STARTUP_TABLE_SPAWN_KIND_SLOT,
                        ECS_STARTUP_OP_SPAWN as u64,
                    ),
                    metadata_qword_via_offset_slot_to_qword_store_sequence(
                        ECS_STARTUP_TABLE_POSITION_PAYLOAD_OFFSET_SLOT,
                        ECS_POSITION_PAYLOAD_STORAGE_SLOT,
                    ),
                    metadata_qword_via_offset_slot_to_qword_store_sequence(
                        ECS_STARTUP_TABLE_VELOCITY_PAYLOAD_OFFSET_SLOT,
                        ECS_VELOCITY_PAYLOAD_STORAGE_SLOT,
                    ),
                    mov_eax_one_store_sequence(ECS_SPAWN_ROW_COUNT_SLOT),
                    compare_stack_slot_sequence(
                        ECS_STARTUP_TABLE_RUN_SCHEDULE_KIND_SLOT,
                        ECS_STARTUP_OP_RUN_SCHEDULE as u64,
                    ),
                    load_store_stack_slot_sequence(
                        ECS_STARTUP_TABLE_RUN_SCHEDULE_ID_SLOT,
                        ECS_COMPILED_SCHEDULE_ID_SLOT,
                    ),
                ],
            ),
            "generated text should walk startup operation rows and invoke handlers in source order"
        );

        for row in ECS_STARTUP_OPERATION_DISPATCH_ROWS {
            assert!(
                contains_subsequence(&text, &mov_eax_one_store_sequence(row.dispatch_count_slot)),
                "generated text should record dispatch count for {:?}",
                row.handler
            );
            assert!(
                contains_subsequence(
                    &text,
                    &compare_stack_slot_sequence(row.dispatch_count_slot, 1)
                ),
                "generated text should validate dispatch count for {:?}",
                row.handler
            );
        }
        assert!(
            contains_subsequence(
                &text,
                &query_plan_component_field_multiply_sequence(
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "generated text should preserve compiled Demo.Move field math"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_position_store_sequence(
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "generated text should preserve compiled Demo.Move Position stores"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should preserve compiled Move success"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_RUN_SCHEDULE_DISPATCH_FAILURE_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "generated text should preserve startup dispatch failure"
        );
    }

    #[test]
    fn materializes_native_spawn_rows_into_archetype_storage() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let storage_plan = storage_plan_for_program(&program);
        let planned_table = &storage_plan.tables[0];
        let storage_compatibility = storage_compatibility_for_program(&program);
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");
        let startup_payloads = startup_payloads(&metadata).expect("startup payloads parse");

        assert_eq!(startup_payloads.spawn_operations.len(), 1);
        let storage = NATIVE_ECS_TABLE_MODEL.archetype_storage;
        let catalog_table = storage_compatibility.catalog_table;
        let row0_storage = archetype_storage_row_slots(0).expect("row 0 storage slots are defined");
        assert_eq!(
            storage.row_count.offset,
            ECS_ARCHETYPE_STORAGE_ROW_COUNT_SLOT
        );
        assert_eq!(storage.capacity.offset, ECS_ARCHETYPE_STORAGE_CAPACITY_SLOT);
        assert_eq!(
            storage.row_stride.offset,
            ECS_ARCHETYPE_STORAGE_ROW_STRIDE_SLOT
        );
        assert_eq!(
            row0_storage,
            NativeArchetypeTableStorageRowSlots {
                position_payload: NativeEcsSlot {
                    offset: ECS_ARCHETYPE_STORAGE_POSITION_ROW0_PAYLOAD_SLOT,
                    byte_len: 8,
                },
                velocity_payload: NativeEcsSlot {
                    offset: ECS_ARCHETYPE_STORAGE_VELOCITY_ROW0_PAYLOAD_SLOT,
                    byte_len: 8,
                },
            }
        );

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert!(contains_subsequence(
            &text,
            &catalog_row_count_commit_sequence(
                catalog_table.slots.row_count_address.offset,
                1,
                ECS_SPAWN_ROW_COUNT_SLOT,
            ),
        ));
        assert!(
            contains_subsequence(
                &text,
                &u64_immediate_store_sequence(
                    u64::from(planned_table.capacity),
                    planned_table.storage.capacity.offset,
                ),
            ),
            "generated startup should initialize descriptor-planned storage capacity"
        );
        assert!(
            contains_subsequence(
                &text,
                &u64_immediate_store_sequence(
                    u64::from(planned_table.logical_row_stride),
                    planned_table.catalog.row_stride.offset,
                ),
            ),
            "generated startup should materialize descriptor-planned catalog row stride"
        );
        assert!(
            contains_ordered_subsequences(
                &text,
                &[
                    metadata_qword_via_offset_slot_to_qword_store_sequence(
                        ECS_STARTUP_TABLE_POSITION_PAYLOAD_OFFSET_SLOT,
                        ECS_POSITION_PAYLOAD_STORAGE_SLOT,
                    ),
                    metadata_qword_via_offset_slot_to_qword_store_sequence(
                        ECS_STARTUP_TABLE_VELOCITY_PAYLOAD_OFFSET_SLOT,
                        ECS_VELOCITY_PAYLOAD_STORAGE_SLOT,
                    ),
                    catalog_payload_store_sequence(
                        catalog_table.columns[0],
                        0,
                        ECS_POSITION_PAYLOAD_STORAGE_SLOT,
                    ),
                    catalog_payload_store_sequence(
                        catalog_table.columns[1],
                        0,
                        ECS_VELOCITY_PAYLOAD_STORAGE_SLOT,
                    ),
                ],
            ),
            "generated startup should stage both payloads and write them through catalog columns"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_qword_at_stack_address_sequence(
                    catalog_table.slots.row_count_address.offset,
                    startup_payloads.spawn_operations.len() as u64,
                ),
            ),
            "startup validation should prove catalog-addressed storage row count"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_qword_at_stack_address_sequence(
                    catalog_table.columns[0].slots.payload_base_address.offset,
                    u64::from_le_bytes(startup_payloads.spawn_operations[0].position_payload),
                ),
            ),
            "startup validation should prove Position through the catalog base"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "storage materialization should preserve valid move_system exit 47"
        );
    }

    #[test]
    fn iterates_native_startup_operation_table_rows_by_count() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let cursors = NATIVE_ECS_TABLE_ITERATION_CURSORS;
        let rows = ECS_STARTUP_OPERATION_TABLE_ITERATION_ROWS;
        assert_eq!(
            rows,
            [
                NativeStartupOperationTableIterationRow {
                    cursor_table: NativeTableIterationKind::StartupOperations,
                    cursor_row_index: 0,
                    expected_table_count: 3,
                    count_slot: cursors.startup_operations.count_slot.unwrap(),
                    primary_slot: cursors.startup_operations.rows[0].primary_slot,
                    dispatch_row: ECS_STARTUP_OPERATION_DISPATCH_ROWS[0],
                },
                NativeStartupOperationTableIterationRow {
                    cursor_table: NativeTableIterationKind::StartupOperations,
                    cursor_row_index: 1,
                    expected_table_count: 3,
                    count_slot: cursors.startup_operations.count_slot.unwrap(),
                    primary_slot: cursors.startup_operations.rows[1].primary_slot,
                    dispatch_row: ECS_STARTUP_OPERATION_DISPATCH_ROWS[1],
                },
                NativeStartupOperationTableIterationRow {
                    cursor_table: NativeTableIterationKind::StartupOperations,
                    cursor_row_index: 2,
                    expected_table_count: 3,
                    count_slot: cursors.startup_operations.count_slot.unwrap(),
                    primary_slot: cursors.startup_operations.rows[2].primary_slot,
                    dispatch_row: ECS_STARTUP_OPERATION_DISPATCH_ROWS[2],
                },
            ]
        );

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        let mut search_start = 0usize;
        for row in rows {
            assert_eq!(row.primary_slot.offset, row.dispatch_row.kind_slot);
            let count_sequence =
                compare_stack_slot_sequence(row.count_slot.offset, row.expected_table_count);
            let dispatch_sequence = compare_stack_slot_sequence(
                row.dispatch_row.kind_slot,
                row.dispatch_row.expected_kind as u64,
            );
            let count_index = find_subsequence_from(&text, &count_sequence, search_start)
                .expect("startup row should compare its table count before dispatch");
            let dispatch_index = find_subsequence_from(&text, &dispatch_sequence, count_index)
                .expect("startup row should dispatch after count validation");
            assert!(
                count_index < dispatch_index,
                "startup row {:?} should count-check before dispatch",
                row
            );
            search_start = dispatch_index + dispatch_sequence.len();
        }

        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "startup row count iteration should preserve compiled Move success"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_RUN_SCHEDULE_DISPATCH_FAILURE_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "startup row count mismatch should use startup dispatch failure"
        );
    }

    #[test]
    fn materializes_native_startup_operation_table() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");
        let startup_payloads = startup_payloads(&metadata).expect("startup payloads parse");

        assert_eq!(NATIVE_ECS_EXECUTION_STATE_LAYOUT.frame_size, 1088);
        assert_eq!(startup_payloads.resource_operation_kind_offset, 577);
        assert_eq!(startup_payloads.resource_id_offset, 581);
        assert_eq!(startup_payloads.resource_id, DEMO_TIME_RESOURCE_ID);
        assert_eq!(startup_payloads.resource_payload_len_offset, 602);
        assert_eq!(startup_payloads.resource_payload_offset, 606);
        assert_eq!(startup_payloads.spawn_operation_kind_offset, 610);
        assert_eq!(startup_payloads.spawn_component_count_offset, 614);
        assert_eq!(startup_payloads.spawn_component_count, 2);
        assert_eq!(startup_payloads.position_component_id_offset, 618);
        assert_eq!(
            startup_payloads.position_component_id,
            DEMO_POSITION_COMPONENT_ID
        );
        assert_eq!(startup_payloads.position_payload_len_offset, 643);
        assert_eq!(startup_payloads.position_payload_offset, 647);
        assert_eq!(startup_payloads.velocity_component_id_offset, 655);
        assert_eq!(
            startup_payloads.velocity_component_id,
            DEMO_VELOCITY_COMPONENT_ID
        );
        assert_eq!(startup_payloads.velocity_payload_len_offset, 680);
        assert_eq!(startup_payloads.velocity_payload_offset, 684);
        assert_eq!(startup_payloads.run_schedule_operation_kind_offset, 692);
        assert_eq!(startup_payloads.run_schedule_id_offset, 696);
        assert_eq!(startup_payloads.run_schedule_id, DEMO_MAIN_SCHEDULE_ID);
        assert_eq!(
            ECS_STARTUP_OPERATION_TABLE_QWORD_LOADS,
            [
                (581, ECS_STARTUP_TABLE_RESOURCE_ID_SLOT),
                (618, ECS_STARTUP_TABLE_POSITION_COMPONENT_ID_SLOT),
                (655, ECS_STARTUP_TABLE_VELOCITY_COMPONENT_ID_SLOT),
                (696, ECS_STARTUP_TABLE_RUN_SCHEDULE_ID_SLOT),
            ]
        );
        assert_eq!(
            ECS_STARTUP_OPERATION_TABLE_DWORD_LOADS,
            [
                (577, ECS_STARTUP_TABLE_RESOURCE_KIND_SLOT),
                (602, ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_LEN_SLOT),
                (610, ECS_STARTUP_TABLE_SPAWN_KIND_SLOT),
                (614, ECS_STARTUP_TABLE_SPAWN_COMPONENT_COUNT_SLOT),
                (643, ECS_STARTUP_TABLE_POSITION_PAYLOAD_LEN_SLOT),
                (680, ECS_STARTUP_TABLE_VELOCITY_PAYLOAD_LEN_SLOT),
                (692, ECS_STARTUP_TABLE_RUN_SCHEDULE_KIND_SLOT),
            ]
        );
        assert_eq!(
            ECS_STARTUP_OPERATION_TABLE_PAYLOAD_OFFSETS,
            [
                (606, ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_OFFSET_SLOT),
                (647, ECS_STARTUP_TABLE_POSITION_PAYLOAD_OFFSET_SLOT),
                (684, ECS_STARTUP_TABLE_VELOCITY_PAYLOAD_OFFSET_SLOT),
            ]
        );

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        for (metadata_offset, stack_slot) in ECS_STARTUP_OPERATION_TABLE_QWORD_LOADS {
            assert!(
                contains_subsequence(
                    &text,
                    &metadata_qword_load_store_sequence(metadata_offset, stack_slot),
                ),
                "generated text should load startup table qword at metadata offset {} into stack slot {}",
                metadata_offset,
                stack_slot
            );
        }
        for (metadata_offset, stack_slot) in ECS_STARTUP_OPERATION_TABLE_DWORD_LOADS {
            assert!(
                contains_subsequence(
                    &text,
                    &metadata_dword_disp32_load_qword_store_sequence(metadata_offset, stack_slot),
                ),
                "generated text should load startup table dword at metadata offset {} into stack slot {}",
                metadata_offset,
                stack_slot
            );
        }
        for (payload_offset, stack_slot) in ECS_STARTUP_OPERATION_TABLE_PAYLOAD_OFFSETS {
            assert!(
                contains_subsequence(
                    &text,
                    &mov_rax_immediate_store_sequence(payload_offset, stack_slot),
                ),
                "generated text should materialize payload byte offset {} into stack slot {}",
                payload_offset,
                stack_slot
            );
        }
        for (stack_slot, expected) in ECS_STARTUP_OPERATION_TABLE_EXPECTED {
            assert!(
                contains_subsequence(&text, &compare_stack_slot_sequence(stack_slot, expected),),
                "generated text should validate startup table stack slot {} against {}",
                stack_slot,
                expected
            );
        }
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slot_sequence(
                    ECS_STARTUP_TABLE_RESOURCE_KIND_SLOT,
                    ECS_STARTUP_OP_RESOURCE_PAYLOAD as u64,
                ),
            ),
            "generated text should dispatch resource operations from the table"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slot_sequence(
                    ECS_STARTUP_TABLE_SPAWN_KIND_SLOT,
                    ECS_STARTUP_OP_SPAWN as u64,
                ),
            ),
            "generated text should dispatch spawn operations from the table"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slot_sequence(
                    ECS_STARTUP_TABLE_RUN_SCHEDULE_KIND_SLOT,
                    ECS_STARTUP_OP_RUN_SCHEDULE as u64,
                ),
            ),
            "generated text should dispatch run-schedule operations from the table"
        );
        assert!(
            contains_subsequence(
                &text,
                &mov_eax_one_store_sequence(ECS_STARTUP_RESOURCE_DISPATCH_COUNT_SLOT),
            ),
            "generated text should record one table-driven resource dispatch"
        );
        assert!(
            contains_subsequence(
                &text,
                &mov_eax_one_store_sequence(ECS_STARTUP_SPAWN_DISPATCH_COUNT_SLOT),
            ),
            "generated text should record one table-driven spawn dispatch"
        );
        assert!(
            contains_subsequence(
                &text,
                &mov_eax_one_store_sequence(ECS_STARTUP_RUN_SCHEDULE_DISPATCH_COUNT_SLOT),
            ),
            "generated text should record one table-driven run-schedule dispatch"
        );
        assert!(
            contains_subsequence(
                &text,
                &metadata_dword_via_offset_slot_to_dword_store_sequence(
                    ECS_STARTUP_TABLE_RESOURCE_PAYLOAD_OFFSET_SLOT,
                    ECS_RESOURCE_PAYLOAD_STORAGE_SLOT,
                ),
            ),
            "generated text should load resource payload through the startup table"
        );
        assert!(
            contains_subsequence(
                &text,
                &metadata_qword_via_offset_slot_to_qword_store_sequence(
                    ECS_STARTUP_TABLE_POSITION_PAYLOAD_OFFSET_SLOT,
                    ECS_POSITION_PAYLOAD_STORAGE_SLOT,
                ),
            ),
            "generated text should load Position payload through the startup table"
        );
        assert!(
            contains_subsequence(
                &text,
                &metadata_qword_via_offset_slot_to_qword_store_sequence(
                    ECS_STARTUP_TABLE_VELOCITY_PAYLOAD_OFFSET_SLOT,
                    ECS_VELOCITY_PAYLOAD_STORAGE_SLOT,
                ),
            ),
            "generated text should load Velocity payload through the startup table"
        );
        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    ECS_STARTUP_TABLE_RUN_SCHEDULE_ID_SLOT,
                    ECS_COMPILED_SCHEDULE_ID_SLOT,
                ),
            ),
            "generated text should materialize compiled schedule state from the startup table"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should preserve compiled Move success"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_RUN_SCHEDULE_DISPATCH_FAILURE_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "generated text should preserve startup dispatch failure"
        );
    }

    #[test]
    fn materializes_native_query_planning_state() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let storage_compatibility = storage_compatibility_for_program(&program);
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    ECS_MOVERS_QUERY_DESCRIPTOR_ID_SLOT,
                    ECS_DESCRIPTOR_QUERY_PLAN_QUERY_ID_SLOT,
                ),
            ),
            "generated text should materialize query descriptor identity into query-plan state"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slots_equal_sequence(
                    ECS_DESCRIPTOR_QUERY_PLAN_POSITION_COMPONENT_ID_SLOT,
                    ECS_POSITION_DESCRIPTOR_ID_SLOT,
                ),
            ),
            "generated text should validate planned Position against decoded Position descriptor state"
        );
        assert!(
            contains_subsequence(
                &text,
                &load_qword_at_stack_address_store_sequence(
                    storage_compatibility
                        .catalog_table
                        .slots
                        .row_count_address
                        .offset,
                    ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT,
                ),
            ),
            "generated text should materialize the query-plan matched row count"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slot_sequence(ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT, 1),
            ),
            "generated text should require one planned query row"
        );
        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    storage_compatibility.catalog_table.columns[0]
                        .slots
                        .payload_base_address
                        .offset,
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                ),
            ),
            "generated text should materialize the planned Position payload address"
        );
        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    storage_compatibility.catalog_table.columns[1]
                        .slots
                        .payload_base_address
                        .offset,
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                ),
            ),
            "generated text should materialize the planned Velocity payload address"
        );
        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT,
                    ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT,
                ),
            ),
            "compiled Move should scan through the query-plan row count"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_component_field_multiply_sequence(
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "compiled Move should load Velocity.x through the planned component address"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_position_store_sequence(
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "compiled Move should store Position.x through the planned component address"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_QUERY_LOOP_SCAN_FAILURE_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "generated text should preserve query-plan scan failure"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should preserve compiled Move success"
        );
    }

    #[test]
    fn builds_native_query_plan_from_table_rows_generically() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let row = ECS_QUERY_PLAN_BUILD_ROWS[0];
        let model = NATIVE_ECS_TABLE_MODEL;
        let movers_query = model.descriptors.query_rows[0].slots;
        let move_system = model.descriptors.system_rows[0].slots;
        let position_descriptor = model.descriptors.component_rows[0].slots;
        let velocity_descriptor = model.descriptors.component_rows[1].slots;
        let catalog_table = model.storage_catalog.table_rows[0];
        let query_plan = model.query_plans.rows[0];

        assert_eq!(ECS_QUERY_PLAN_BUILD_ROWS.len(), 1);
        assert_eq!(row.query_id_slot, movers_query.id.offset);
        assert_eq!(row.query_term_count_slot, movers_query.term_count.offset);
        assert_eq!(
            row.system_query_term_count_slot,
            move_system.query_param_term_count.offset
        );
        assert_eq!(
            row.catalog_column_count_slot,
            catalog_table.slots.column_count.offset
        );
        assert_eq!(
            row.catalog_row_count_address_slot,
            catalog_table.slots.row_count_address.offset
        );
        assert_eq!(row.plan_query_id_slot, query_plan.query_id.offset);
        assert_eq!(row.plan_term_count_slot, query_plan.term_count.offset);
        assert_eq!(
            row.matched_row_count_slot,
            ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT
        );
        assert_eq!(
            row.terms[0],
            NativeQueryPlanTermBuildRow {
                role: NativeQueryPlanTermRole::Position,
                query_access_slot: movers_query.term0_access.offset,
                query_component_id_slot: movers_query.term0_component_id.offset,
                system_access_slot: move_system.query_term0_access.offset,
                system_component_id_slot: move_system.query_term0_component_id.offset,
                component_descriptor_id_slot: position_descriptor.id.offset,
                component_size_slot: position_descriptor.size.offset,
                component_x_field_offset_slot: position_descriptor.x_field_offset.offset,
                component_y_field_offset_slot: position_descriptor.y_field_offset.offset,
                catalog_component_id_slot: catalog_table.columns[0].slots.component_id.offset,
                catalog_element_size_slot: catalog_table.columns[0].slots.element_size.offset,
                catalog_payload_base_address_slot: catalog_table.columns[0]
                    .slots
                    .payload_base_address
                    .offset,
                plan_access_slot: query_plan.position.access.offset,
                plan_component_id_slot: query_plan.position.component_id.offset,
                plan_size_slot: query_plan.position.size.offset,
                plan_x_field_offset_slot: query_plan.position.x_field_offset.offset,
                plan_y_field_offset_slot: query_plan.position.y_field_offset.offset,
                planned_payload_address_slot: ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                expected_access: 2,
                expected_size: u64::from(NATIVE_ECS_QWORD_BYTE_LEN),
                expected_x_field_offset: 0,
                expected_y_field_offset: 4,
            }
        );
        assert_eq!(
            row.terms[1],
            NativeQueryPlanTermBuildRow {
                role: NativeQueryPlanTermRole::Velocity,
                query_access_slot: movers_query.term1_access.offset,
                query_component_id_slot: movers_query.term1_component_id.offset,
                system_access_slot: move_system.query_term1_access.offset,
                system_component_id_slot: move_system.query_term1_component_id.offset,
                component_descriptor_id_slot: velocity_descriptor.id.offset,
                component_size_slot: velocity_descriptor.size.offset,
                component_x_field_offset_slot: velocity_descriptor.x_field_offset.offset,
                component_y_field_offset_slot: velocity_descriptor.y_field_offset.offset,
                catalog_component_id_slot: catalog_table.columns[1].slots.component_id.offset,
                catalog_element_size_slot: catalog_table.columns[1].slots.element_size.offset,
                catalog_payload_base_address_slot: catalog_table.columns[1]
                    .slots
                    .payload_base_address
                    .offset,
                plan_access_slot: query_plan.velocity.access.offset,
                plan_component_id_slot: query_plan.velocity.component_id.offset,
                plan_size_slot: query_plan.velocity.size.offset,
                plan_x_field_offset_slot: query_plan.velocity.x_field_offset.offset,
                plan_y_field_offset_slot: query_plan.velocity.y_field_offset.offset,
                planned_payload_address_slot: ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                expected_access: 1,
                expected_size: u64::from(NATIVE_ECS_QWORD_BYTE_LEN),
                expected_x_field_offset: 0,
                expected_y_field_offset: 4,
            }
        );

        let row =
            native_query_plan_iteration_row(storage_compatibility_for_program(&program)).build_row;
        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(row.query_id_slot, row.plan_query_id_slot),
            ),
            "generated text should copy query table-row identity into query-plan state"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slots_equal_sequence(
                    row.plan_term_count_slot,
                    row.system_query_term_count_slot,
                ),
            ),
            "generated text should validate query row terms against system query-param row terms"
        );
        for term in row.terms {
            assert!(
                contains_subsequence(
                    &text,
                    &load_store_stack_slot_sequence(
                        term.catalog_component_id_slot,
                        term.plan_component_id_slot,
                    ),
                ),
                "generated text should copy {:?} component id from catalog into query plan",
                term.role
            );
            for identity_slot in [
                term.query_component_id_slot,
                term.system_component_id_slot,
                term.component_descriptor_id_slot,
            ] {
                assert!(contains_subsequence(
                    &text,
                    &compare_stack_slots_equal_sequence(term.plan_component_id_slot, identity_slot,),
                ));
            }
            assert!(
                contains_subsequence(
                    &text,
                    &compare_stack_slots_equal_sequence(
                        term.plan_size_slot,
                        term.component_size_slot,
                    ),
                ),
                "generated text should validate {:?} catalog size against its descriptor row",
                term.role
            );
            assert!(
                contains_subsequence(
                    &text,
                    &load_store_stack_slot_sequence(
                        term.catalog_payload_base_address_slot,
                        term.planned_payload_address_slot,
                    ),
                ),
                "generated text should seed {:?} planned payload address from the catalog base",
                term.role
            );
        }
        assert!(
            contains_subsequence(
                &text,
                &query_plan_component_field_multiply_sequence(
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "compiled Move should keep consuming planned Velocity payload addresses"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_position_store_sequence(
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "compiled Move should keep consuming planned Position payload addresses"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should preserve compiled Move success"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_QUERY_LOOP_SCAN_FAILURE_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "generated text should preserve query-plan failure"
        );
    }

    #[test]
    fn builds_query_plan_through_storage_catalog() {
        let build_row = ECS_QUERY_PLAN_BUILD_ROWS[0];
        let catalog_table = NATIVE_ECS_TABLE_MODEL.storage_catalog.table_rows[0];
        assert_eq!(
            build_row.catalog_column_count_slot,
            catalog_table.slots.column_count.offset
        );
        assert_eq!(
            build_row.catalog_row_count_address_slot,
            catalog_table.slots.row_count_address.offset
        );

        for (term, column) in build_row.terms.into_iter().zip(catalog_table.columns) {
            assert_eq!(
                term.catalog_component_id_slot,
                column.slots.component_id.offset
            );
            assert_eq!(
                term.catalog_element_size_slot,
                column.slots.element_size.offset
            );
            assert_eq!(
                term.catalog_payload_base_address_slot,
                column.slots.payload_base_address.offset
            );
        }

        for (fixture_name, source, expected_rows) in [
            (
                "move_system.arc",
                include_str!("../../../examples/move_system.arc"),
                1usize,
            ),
            (
                "move_system_two_rows.arc",
                include_str!("../../../examples/move_system_two_rows.arc"),
                2usize,
            ),
        ] {
            let tokens = lexer::lex(source).expect("movement fixture lexes");
            let program = parser::parse_program(&tokens).expect("movement fixture parses");
            let storage_compatibility = storage_compatibility_for_program(&program);
            let build_row = native_query_plan_iteration_row(storage_compatibility).build_row;
            let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
                .expect("movement fixture assembles");
            let metadata = ecs_metadata::encode_ecs_metadata(&assembly)
                .expect("movement fixture metadata encodes");
            let startup = startup_payloads(&metadata).expect("startup payloads parse");
            let observable = native_move_query_loop_observable(&program, &startup)
                .expect("movement query observable exists");
            assert_eq!(observable.rows.len(), expected_rows);

            let mut isolated_builder = Vec::new();
            let mut isolated_jump_offsets = Vec::new();
            emit_native_query_plan_build_row(
                &mut isolated_builder,
                build_row,
                expected_rows as u64,
                &mut isolated_jump_offsets,
            );
            let mut expected_builder_store_slots =
                vec![build_row.plan_query_id_slot, build_row.plan_term_count_slot];
            for term in build_row.terms {
                expected_builder_store_slots.extend([
                    term.plan_access_slot,
                    term.plan_component_id_slot,
                    term.plan_size_slot,
                    term.plan_x_field_offset_slot,
                    term.plan_y_field_offset_slot,
                ]);
            }
            expected_builder_store_slots.push(build_row.matched_row_count_slot);
            expected_builder_store_slots.extend(
                build_row
                    .terms
                    .into_iter()
                    .map(|term| term.planned_payload_address_slot),
            );
            assert_eq!(
                qword_stack_store_slots(&isolated_builder),
                expected_builder_store_slots,
                "{fixture_name} isolated builder should write only the ordered query-plan targets"
            );

            let text = ecs_metadata_decoder_text_payload(&program, &metadata)
                .expect("movement native text emits");
            let builder_index = find_emitted_block_ignoring_rel32(
                &text,
                &isolated_builder,
                &isolated_jump_offsets,
            )
            .unwrap_or_else(|| {
                panic!("{fixture_name} should embed the isolated query-plan builder contiguously")
            });
            let first_math_index = find_subsequence_from(
                &text,
                &query_plan_component_field_multiply_sequence(
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
                builder_index + isolated_builder.len(),
            )
            .unwrap_or_else(|| panic!("{fixture_name} should execute planned math after building"));
            assert!(builder_index + isolated_builder.len() <= first_math_index);
            let writes_before_first_math = qword_stack_store_slots(
                &text[builder_index + isolated_builder.len()..first_math_index],
            );
            for slot in expected_builder_store_slots {
                assert!(
                    !writes_before_first_math.contains(&slot),
                    "{fixture_name} should not overwrite query-plan slot {slot} between the isolated builder and first planned math"
                );
            }
            assert!(contains_subsequence(
                &text,
                &load_qword_at_stack_address_store_sequence(
                    build_row.catalog_row_count_address_slot,
                    build_row.matched_row_count_slot,
                ),
            ));
            assert!(contains_subsequence(
                &text,
                &compare_stack_slots_equal_sequence(
                    build_row.plan_term_count_slot,
                    build_row.catalog_column_count_slot,
                ),
            ));

            for term in build_row.terms {
                assert!(contains_subsequence(
                    &text,
                    &load_store_stack_slot_sequence(
                        term.catalog_component_id_slot,
                        term.plan_component_id_slot,
                    ),
                ));
                assert!(contains_subsequence(
                    &text,
                    &load_store_stack_slot_sequence(
                        term.catalog_element_size_slot,
                        term.plan_size_slot,
                    ),
                ));
                assert!(contains_subsequence(
                    &text,
                    &load_store_stack_slot_sequence(
                        term.catalog_payload_base_address_slot,
                        term.planned_payload_address_slot,
                    ),
                ));
                assert!(contains_subsequence(
                    &text,
                    &compare_stack_slot_sequence(term.plan_size_slot, term.expected_size),
                ));
                let advance = advance_planned_address_sequence(
                    term.planned_payload_address_slot,
                    term.plan_size_slot,
                );
                assert_eq!(
                    count_subsequence(&text, &advance),
                    expected_rows.saturating_sub(1),
                    "{fixture_name} should advance each catalog-planned column once per later row"
                );
            }

            for row in &observable.rows {
                assert!(contains_subsequence(
                    &text,
                    &compare_qword_at_stack_address_sequence(
                        ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                        u64::from_le_bytes(row.target_position_payload),
                    ),
                ));
            }

            assert!(!contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    ECS_ARCHETYPE_STORAGE_ROW_COUNT_SLOT,
                    ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT,
                ),
            ));
            assert!(!contains_subsequence(
                &text,
                &compare_stack_slot_sequence(
                    ECS_ARCHETYPE_STORAGE_ROW_COUNT_SLOT,
                    expected_rows as u64,
                ),
            ));
            assert!(!contains_subsequence(
                &text,
                &compare_stack_slot_sequence(
                    ECS_ARCHETYPE_STORAGE_CAPACITY_SLOT,
                    ECS_ARCHETYPE_STORAGE_CAPACITY,
                ),
            ));
            assert!(!contains_subsequence(
                &text,
                &compare_stack_slot_sequence(
                    ECS_ARCHETYPE_STORAGE_ROW_STRIDE_SLOT,
                    ECS_ARCHETYPE_STORAGE_ROW_STRIDE,
                ),
            ));
            for (row_index, (spawn, row)) in startup
                .spawn_operations
                .iter()
                .zip(&observable.rows)
                .enumerate()
            {
                let physical = archetype_storage_row_slots(row_index)
                    .expect("bounded physical storage row exists");
                for (slot, expected) in [
                    (
                        physical.position_payload.offset,
                        u64::from_le_bytes(spawn.position_payload),
                    ),
                    (
                        physical.position_payload.offset,
                        u64::from_le_bytes(row.target_position_payload),
                    ),
                    (
                        physical.velocity_payload.offset,
                        u64::from_le_bytes(spawn.velocity_payload),
                    ),
                ] {
                    assert!(
                        !contains_subsequence(
                            &text,
                            &compare_stack_slot_sequence(slot, expected),
                        ),
                        "{fixture_name} must validate storage through catalog/planned addresses, not physical slot {slot}"
                    );
                }
            }
            for physical_slot in [
                ECS_ARCHETYPE_STORAGE_POSITION_ROW0_PAYLOAD_SLOT,
                ECS_ARCHETYPE_STORAGE_POSITION_ROW1_PAYLOAD_SLOT,
                ECS_ARCHETYPE_STORAGE_VELOCITY_ROW0_PAYLOAD_SLOT,
                ECS_ARCHETYPE_STORAGE_VELOCITY_ROW1_PAYLOAD_SLOT,
            ] {
                for planned_slot in [
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                ] {
                    assert!(!contains_subsequence(
                        &text,
                        &lea_stack_address_store_sequence(physical_slot, planned_slot),
                    ));
                }
            }
            assert!(contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ));
        }
    }

    #[test]
    fn builds_native_query_plan_from_archetype_storage() {
        builds_query_plan_through_storage_catalog();
    }

    #[test]
    fn executes_compiled_move_from_native_storage_columns() {
        let one_row_source = include_str!("../../../examples/move_system.arc");
        let one_row_tokens = lexer::lex(one_row_source).expect("move_system.arc lexes");
        let one_row_program =
            parser::parse_program(&one_row_tokens).expect("move_system.arc parses");
        let one_row_assembly =
            runtime_assembly::assemble_runtime_program_from_source(&one_row_program)
                .expect("move_system.arc assembles");
        let one_row_metadata =
            ecs_metadata::encode_ecs_metadata(&one_row_assembly).expect("metadata encodes");
        let one_row_startup =
            startup_payloads(&one_row_metadata).expect("one-row startup payloads parse");
        let one_row_observable =
            native_move_query_loop_observable(&one_row_program, &one_row_startup)
                .expect("one-row native query-loop observable is defined");

        assert_eq!(one_row_observable.rows.len(), 1);
        assert_eq!(one_row_observable.rows[0].row_index, 0);

        let one_row_text = ecs_metadata_decoder_text_payload(&one_row_program, &one_row_metadata)
            .expect("move_system ECS decoder text emits");
        let one_row_catalog = storage_compatibility_for_program(&one_row_program).catalog_table;
        let one_row_position_address = find_subsequence_from(
            &one_row_text,
            &load_store_stack_slot_sequence(
                one_row_catalog.columns[0].slots.payload_base_address.offset,
                ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
            ),
            0,
        )
        .expect("one-row compiled Move should plan Position from the catalog base");
        let one_row_velocity_address = find_subsequence_from(
            &one_row_text,
            &load_store_stack_slot_sequence(
                one_row_catalog.columns[1].slots.payload_base_address.offset,
                ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
            ),
            one_row_position_address,
        )
        .expect("one-row compiled Move should plan Velocity from the catalog base");
        let one_row_math = find_subsequence_from(
            &one_row_text,
            &query_plan_component_field_multiply_sequence(
                ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                0,
                ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
            ),
            one_row_velocity_address,
        )
        .expect("one-row compiled Move should read Velocity through the catalog-backed plan");
        let one_row_store = find_subsequence_from(
            &one_row_text,
            &compare_qword_at_stack_address_sequence(
                ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                u64::from_le_bytes(one_row_observable.rows[0].target_position_payload),
            ),
            one_row_math,
        )
        .expect("one-row compiled Move should validate Position through its planned address");
        assert!(
            find_subsequence_from(
                &one_row_text,
                &lea_stack_address_store_sequence(
                    ECS_POSITION_PAYLOAD_STORAGE_SLOT,
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                ),
                one_row_position_address,
            )
            .is_none(),
            "compiled Move should not fall back to direct startup Position payload addresses"
        );

        let two_row_source = include_str!("../../../examples/move_system_two_rows.arc");
        let two_row_tokens = lexer::lex(two_row_source).expect("move_system_two_rows.arc lexes");
        let two_row_program =
            parser::parse_program(&two_row_tokens).expect("move_system_two_rows.arc parses");
        let two_row_assembly =
            runtime_assembly::assemble_runtime_program_from_source(&two_row_program)
                .expect("move_system_two_rows.arc assembles");
        let two_row_metadata =
            ecs_metadata::encode_ecs_metadata(&two_row_assembly).expect("two-row metadata encodes");
        let two_row_startup =
            startup_payloads(&two_row_metadata).expect("two-row startup payloads parse");
        let two_row_observable =
            native_move_query_loop_observable(&two_row_program, &two_row_startup)
                .expect("two-row native query-loop observable is defined");

        assert_eq!(two_row_observable.rows.len(), 2);
        assert_eq!(two_row_observable.rows[0].row_index, 0);
        assert_eq!(two_row_observable.rows[1].row_index, 1);

        let two_row_text = ecs_metadata_decoder_text_payload(&two_row_program, &two_row_metadata)
            .expect("two-row ECS decoder text emits");
        let two_row_catalog = storage_compatibility_for_program(&two_row_program).catalog_table;
        let row0_position_address = find_subsequence_from(
            &two_row_text,
            &load_store_stack_slot_sequence(
                two_row_catalog.columns[0].slots.payload_base_address.offset,
                ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
            ),
            0,
        )
        .expect("row 0 Position address should come from the catalog base");
        let row0_velocity_address = find_subsequence_from(
            &two_row_text,
            &load_store_stack_slot_sequence(
                two_row_catalog.columns[1].slots.payload_base_address.offset,
                ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
            ),
            row0_position_address,
        )
        .expect("row 0 Velocity address should come from the catalog base");
        let two_row_scan = find_subsequence_from(
            &two_row_text,
            &compare_stack_slot_sequence(ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT, 2),
            row0_velocity_address,
        )
        .expect("two-row compiled Move should scan two catalog-matched rows");
        let row0_store = find_subsequence_from(
            &two_row_text,
            &compare_qword_at_stack_address_sequence(
                ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                u64::from_le_bytes(two_row_observable.rows[0].target_position_payload),
            ),
            two_row_scan,
        )
        .expect("row 0 Position update should validate through its planned address");
        let row1_position_address = find_subsequence_from(
            &two_row_text,
            &advance_planned_address_sequence(
                ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                ECS_DESCRIPTOR_QUERY_PLAN_POSITION_SIZE_SLOT,
            ),
            row0_store,
        )
        .expect("row 1 Position address should advance by the catalog-derived element size");
        let row1_velocity_address = find_subsequence_from(
            &two_row_text,
            &advance_planned_address_sequence(
                ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                ECS_DESCRIPTOR_QUERY_PLAN_VELOCITY_SIZE_SLOT,
            ),
            row1_position_address,
        )
        .expect("row 1 Velocity address should advance by the catalog-derived element size");
        let row1_math = find_subsequence_from(
            &two_row_text,
            &query_plan_component_field_multiply_sequence(
                ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                0,
                ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
            ),
            row1_velocity_address,
        )
        .expect("row 1 compiled Move should read Velocity through the advanced catalog plan");
        let row1_store = find_subsequence_from(
            &two_row_text,
            &compare_qword_at_stack_address_sequence(
                ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                u64::from_le_bytes(two_row_observable.rows[1].target_position_payload),
            ),
            row1_math,
        )
        .expect("row 1 Position update should validate through its planned address");
        let success_index = find_subsequence_from(
            &two_row_text,
            &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            row1_store,
        )
        .expect("two-row storage-backed compiled Move should exit 47");
        assert!(
            one_row_store < one_row_text.len()
                && row0_position_address < row0_velocity_address
                && row0_velocity_address < two_row_scan
                && two_row_scan < row0_store
                && row0_store < row1_position_address
                && row1_position_address < row1_velocity_address
                && row1_velocity_address < row1_math
                && row1_math < row1_store
                && row1_store < success_index,
            "compiled Demo.Move should update storage rows in order before success"
        );
    }

    #[test]
    fn builds_native_query_plan_from_iterated_table_rows() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let cursors = NATIVE_ECS_TABLE_ITERATION_CURSORS;
        let rows = ECS_QUERY_PLAN_TABLE_ITERATION_ROWS;
        let schedule_row = ECS_COMPILED_SCHEDULE_BUILD_ROWS[0];
        assert_eq!(
            rows,
            [NativeQueryPlanTableIterationRow {
                cursor_table: NativeTableIterationKind::QueryPlans,
                cursor_row_index: 0,
                primary_slot: cursors.query_plans.rows[0].primary_slot,
                build_row: ECS_QUERY_PLAN_BUILD_ROWS[0],
            }]
        );
        assert_eq!(
            rows[schedule_row.query_plan_row_index].primary_slot.offset,
            rows[schedule_row.query_plan_row_index]
                .build_row
                .plan_query_id_slot
        );

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");
        let iterated_row =
            native_query_plan_iteration_row(storage_compatibility_for_program(&program));
        let build_row = iterated_row.build_row;

        let query_plan_identity =
            load_store_stack_slot_sequence(build_row.query_id_slot, build_row.plan_query_id_slot);
        let term_count_validation = compare_stack_slots_equal_sequence(
            build_row.plan_term_count_slot,
            build_row.system_query_term_count_slot,
        );
        let move_field_math = query_plan_component_field_multiply_sequence(
            ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
            0,
            ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
        );

        let identity_index = find_subsequence_from(&text, &query_plan_identity, 0)
            .expect("compiled schedule should dispatch into iterated query-plan row");
        let validation_index = find_subsequence_from(&text, &term_count_validation, identity_index)
            .expect("iterated query-plan row should validate descriptor-backed query state");
        let move_math_index = find_subsequence_from(&text, &move_field_math, validation_index)
            .expect("compiled Move should consume the iterated query-plan result");
        assert!(
            identity_index < validation_index && validation_index < move_math_index,
            "iterated query-plan row should build and validate state before compiled Move"
        );

        for term in build_row.terms {
            assert!(
                contains_subsequence(
                    &text,
                    &load_store_stack_slot_sequence(
                        term.catalog_payload_base_address_slot,
                        term.planned_payload_address_slot,
                    ),
                ),
                "iterated query-plan row should seed {:?} payload address from the catalog",
                term.role
            );
        }
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "iterated query-plan row should preserve compiled Move success"
        );
    }

    #[test]
    fn executes_multi_row_native_ecs_table_proof() {
        let source = include_str!("../../../examples/move_system_two_rows.arc");
        let tokens = lexer::lex(source).expect("move_system_two_rows.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system_two_rows.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system_two_rows.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("two-row metadata encodes");
        let startup_payloads = startup_payloads(&metadata).expect("two-row startup payloads parse");
        let observable = native_move_query_loop_observable(&program, &startup_payloads)
            .expect("two-row native query-loop observable is defined");

        assert_eq!(startup_payloads.startup_record_count, 4);
        assert_eq!(startup_payloads.spawn_operations.len(), 2);
        assert_eq!(startup_payloads.run_schedule_operation_kind_offset, 774);
        assert_eq!(startup_payloads.run_schedule_id_offset, 778);

        let first_spawn = startup_payloads.spawn_operations[0];
        assert_eq!(first_spawn.operation_kind_offset, 610);
        assert_eq!(first_spawn.position_payload_offset, 647);
        assert_eq!(first_spawn.velocity_payload_offset, 684);

        let second_spawn = startup_payloads.spawn_operations[1];
        assert_eq!(second_spawn.operation_kind_offset, 692);
        assert_eq!(second_spawn.component_count_offset, 696);
        assert_eq!(second_spawn.position_component_id_offset, 700);
        assert_eq!(second_spawn.position_payload_len_offset, 725);
        assert_eq!(second_spawn.position_payload_offset, 729);
        assert_eq!(second_spawn.velocity_component_id_offset, 737);
        assert_eq!(second_spawn.velocity_payload_len_offset, 762);
        assert_eq!(second_spawn.velocity_payload_offset, 766);
        assert_eq!(
            second_spawn.position_payload,
            [0x00, 0x00, 0x20, 0x41, 0x00, 0x00, 0xa0, 0x41]
        );
        assert_eq!(
            second_spawn.velocity_payload,
            [0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40]
        );

        assert_eq!(observable.rows.len(), 2);
        assert_eq!(
            observable.rows[0].target_position_payload,
            [0x00, 0x00, 0x80, 0x40, 0x00, 0x00, 0xc0, 0x40]
        );
        assert_eq!(
            observable.rows[1].target_position_payload,
            [0x00, 0x00, 0x30, 0x41, 0x00, 0x00, 0xb0, 0x41]
        );
        assert_eq!(
            observable.rows[1].field_product_payload,
            [0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40]
        );

        let rows = ECS_TWO_SPAWN_STARTUP_OPERATION_TABLE_ITERATION_ROWS;
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows[2].primary_slot.offset,
            ECS_SECOND_STARTUP_TABLE_SPAWN_KIND_SLOT
        );
        assert_eq!(rows[2].dispatch_row.dispatch_count_after_row, 2);
        assert_eq!(
            rows[3].primary_slot.offset,
            ECS_STARTUP_TABLE_RUN_SCHEDULE_KIND_SLOT
        );

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("two-row ECS decoder text emits");

        assert!(contains_subsequence(
            &text,
            &metadata_dword_disp32_load_qword_store_sequence(
                second_spawn.operation_kind_offset,
                ECS_SECOND_STARTUP_TABLE_SPAWN_KIND_SLOT,
            ),
        ));
        assert!(contains_subsequence(
            &text,
            &metadata_qword_load_store_sequence(
                second_spawn.position_component_id_offset,
                ECS_SECOND_STARTUP_TABLE_POSITION_COMPONENT_ID_SLOT,
            ),
        ));
        assert!(contains_subsequence(
            &text,
            &metadata_dword_disp32_load_qword_store_sequence(
                second_spawn.component_count_offset,
                ECS_SECOND_STARTUP_TABLE_SPAWN_COMPONENT_COUNT_SLOT,
            ),
        ));
        assert!(contains_subsequence(
            &text,
            &mov_rax_immediate_store_sequence(
                second_spawn.position_payload_offset as u64,
                ECS_SECOND_STARTUP_TABLE_POSITION_PAYLOAD_OFFSET_SLOT,
            ),
        ));
        assert!(contains_subsequence(
            &text,
            &metadata_dword_disp32_load_qword_store_sequence(
                second_spawn.position_payload_len_offset,
                ECS_SECOND_STARTUP_TABLE_POSITION_PAYLOAD_LEN_SLOT,
            ),
        ));
        assert!(contains_subsequence(
            &text,
            &metadata_qword_load_store_sequence(
                second_spawn.velocity_component_id_offset,
                ECS_SECOND_STARTUP_TABLE_VELOCITY_COMPONENT_ID_SLOT,
            ),
        ));
        assert!(contains_subsequence(
            &text,
            &mov_rax_immediate_store_sequence(
                second_spawn.velocity_payload_offset as u64,
                ECS_SECOND_STARTUP_TABLE_VELOCITY_PAYLOAD_OFFSET_SLOT,
            ),
        ));
        assert!(contains_subsequence(
            &text,
            &metadata_dword_disp32_load_qword_store_sequence(
                second_spawn.velocity_payload_len_offset,
                ECS_SECOND_STARTUP_TABLE_VELOCITY_PAYLOAD_LEN_SLOT,
            ),
        ));
        assert!(contains_subsequence(
            &text,
            &metadata_qword_via_offset_slot_to_qword_store_sequence(
                ECS_SECOND_STARTUP_TABLE_POSITION_PAYLOAD_OFFSET_SLOT,
                ECS_SECOND_POSITION_PAYLOAD_STORAGE_SLOT,
            ),
        ));
        assert!(contains_subsequence(
            &text,
            &metadata_qword_via_offset_slot_to_qword_store_sequence(
                ECS_SECOND_STARTUP_TABLE_VELOCITY_PAYLOAD_OFFSET_SLOT,
                ECS_SECOND_VELOCITY_PAYLOAD_STORAGE_SLOT,
            ),
        ));
        assert!(contains_subsequence(
            &text,
            &mov_eax_immediate_store_sequence(2, ECS_SPAWN_ROW_COUNT_SLOT),
        ));
        assert!(contains_subsequence(
            &text,
            &mov_eax_immediate_store_sequence(2, ECS_STARTUP_SPAWN_DISPATCH_COUNT_SLOT),
        ));

        let mut search_start = 0usize;
        for row in rows {
            let count_sequence =
                compare_stack_slot_sequence(row.count_slot.offset, row.expected_table_count);
            let dispatch_sequence = compare_stack_slot_sequence(
                row.dispatch_row.kind_slot,
                row.dispatch_row.expected_kind as u64,
            );
            let count_index = find_subsequence_from(&text, &count_sequence, search_start)
                .expect("two-row startup row should count-check before dispatch");
            let dispatch_index = find_subsequence_from(&text, &dispatch_sequence, count_index)
                .expect("two-row startup row should dispatch after count validation");
            assert!(
                count_index < dispatch_index,
                "two-row startup row {:?} should count-check before dispatch",
                row
            );
            search_start = dispatch_index + dispatch_sequence.len();
        }

        assert!(contains_subsequence(
            &text,
            &load_qword_at_stack_address_store_sequence(
                ECS_QUERY_PLAN_BUILD_ROWS[0].catalog_row_count_address_slot,
                ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT,
            ),
        ));
        assert!(contains_subsequence(
            &text,
            &compare_stack_slot_sequence(ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT, 2),
        ));
        assert!(contains_subsequence(
            &text,
            &load_store_stack_slot_sequence(
                ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT,
                ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT,
            ),
        ));
        assert!(contains_subsequence(
            &text,
            &compare_stack_slot_sequence(ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT, 2),
        ));

        let scanned_count_index = find_subsequence_from(
            &text,
            &compare_stack_slot_sequence(ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT, 2),
            0,
        )
        .expect("two-row scan count should be validated");
        let row0_field_math_index = find_subsequence_from(
            &text,
            &query_plan_component_field_multiply_sequence(
                ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                0,
                ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
            ),
            scanned_count_index,
        )
        .expect("row 0 field math should execute through catalog-seeded payload addresses");
        let row0_position_store_index = find_subsequence_from(
            &text,
            &compare_qword_at_stack_address_sequence(
                ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                u64::from_le_bytes(observable.rows[0].target_position_payload),
            ),
            row0_field_math_index,
        )
        .expect("row 0 updated Position payload should be validated through its planned address");
        let row1_position_address_index = find_subsequence_from(
            &text,
            &advance_planned_address_sequence(
                ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                ECS_DESCRIPTOR_QUERY_PLAN_POSITION_SIZE_SLOT,
            ),
            row0_position_store_index,
        )
        .expect("row 1 Position payload address should advance after row 0 update");
        let row1_field_math_index = find_subsequence_from(
            &text,
            &query_plan_component_field_multiply_sequence(
                ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                0,
                ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
            ),
            row1_position_address_index,
        )
        .expect("row 1 field math should execute after row 1 payload addresses");
        let row1_position_store_index = find_subsequence_from(
            &text,
            &compare_qword_at_stack_address_sequence(
                ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                u64::from_le_bytes(observable.rows[1].target_position_payload),
            ),
            row1_field_math_index,
        )
        .expect("row 1 updated Position payload should be validated");
        let success_index = find_subsequence_from(
            &text,
            &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            row1_position_store_index,
        )
        .expect("two-row native proof should finish with compiled Move success");
        assert!(
            scanned_count_index < row0_field_math_index
                && row0_field_math_index < row0_position_store_index
                && row0_position_store_index < row1_position_address_index
                && row1_position_address_index < row1_field_math_index
                && row1_field_math_index < row1_position_store_index
                && row1_position_store_index < success_index,
            "compiled Demo.Move should update row 0 and row 1 before success"
        );
    }

    #[test]
    fn builds_native_query_plan_from_descriptor_records() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert_eq!(storage_plan_for_program(&program).frame_size, 1072);
        let row =
            native_query_plan_iteration_row(storage_compatibility_for_program(&program)).build_row;
        for (source_slot, target_slot) in [
            (row.query_id_slot, row.plan_query_id_slot),
            (row.query_term_count_slot, row.plan_term_count_slot),
        ] {
            assert!(
                contains_subsequence(
                    &text,
                    &load_store_stack_slot_sequence(source_slot, target_slot),
                ),
                "generated text should copy decoded descriptor slot {} into query-plan slot {}",
                source_slot,
                target_slot
            );
        }
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slots_equal_sequence(
                    row.plan_term_count_slot,
                    row.system_query_term_count_slot,
                ),
            ),
            "generated text should compare query-plan term count with decoded system query-param term count"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slot_sequence(row.plan_term_count_slot, 2),
            ),
            "generated text should validate the supported two-term query-plan shape"
        );
        for term in row.terms {
            for (source_slot, target_slot) in [
                (term.query_access_slot, term.plan_access_slot),
                (term.catalog_component_id_slot, term.plan_component_id_slot),
                (term.catalog_element_size_slot, term.plan_size_slot),
                (
                    term.component_x_field_offset_slot,
                    term.plan_x_field_offset_slot,
                ),
                (
                    term.component_y_field_offset_slot,
                    term.plan_y_field_offset_slot,
                ),
            ] {
                assert!(
                    contains_subsequence(
                        &text,
                        &load_store_stack_slot_sequence(source_slot, target_slot),
                    ),
                    "generated text should copy table-row slot {} into query-plan slot {}",
                    source_slot,
                    target_slot
                );
            }
            for (stack_slot, expected) in [
                (term.plan_access_slot, term.expected_access),
                (term.plan_x_field_offset_slot, term.expected_x_field_offset),
                (term.plan_y_field_offset_slot, term.expected_y_field_offset),
            ] {
                assert!(
                    contains_subsequence(&text, &compare_stack_slot_sequence(stack_slot, expected)),
                    "generated text should validate query-plan stack slot {} against {}",
                    stack_slot,
                    expected
                );
            }
            for (left_slot, right_slot) in [
                (term.plan_access_slot, term.system_access_slot),
                (term.plan_component_id_slot, term.query_component_id_slot),
                (term.plan_component_id_slot, term.system_component_id_slot),
                (
                    term.plan_component_id_slot,
                    term.component_descriptor_id_slot,
                ),
                (term.plan_size_slot, term.component_size_slot),
            ] {
                assert!(
                    contains_subsequence(
                        &text,
                        &compare_stack_slots_equal_sequence(left_slot, right_slot),
                    ),
                    "generated text should compare query-plan slot {} against table-row slot {}",
                    left_slot,
                    right_slot
                );
            }
            assert!(
                contains_subsequence(
                    &text,
                    &load_store_stack_slot_sequence(
                        term.catalog_payload_base_address_slot,
                        term.planned_payload_address_slot,
                    ),
                ),
                "generated text should materialize planned payload address for {:?}",
                term.role
            );
        }
        assert!(
            contains_subsequence(
                &text,
                &load_qword_at_stack_address_store_sequence(
                    row.catalog_row_count_address_slot,
                    row.matched_row_count_slot,
                ),
            ),
            "generated text should materialize matched rows through the catalog row-count address"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slot_sequence(row.matched_row_count_slot, 1),
            ),
            "generated text should still require one matched row after descriptor-backed planning"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_component_field_multiply_sequence(
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "compiled Move should keep consuming planned Velocity payload addresses"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_position_store_sequence(
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "compiled Move should keep consuming planned Position payload addresses"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should preserve compiled Move success"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_QUERY_LOOP_SCAN_FAILURE_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "generated text should preserve query-plan failure"
        );
    }

    #[test]
    fn executes_compiled_schedule_from_native_state() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");
        let schedule_row = ECS_COMPILED_SCHEDULE_BUILD_ROWS[0];

        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    schedule_row.startup_schedule_id_slot,
                    schedule_row.compiled_schedule_id_slot,
                ),
            ),
            "generated text should load the run Demo.Main schedule id into compiled schedule state"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slots_equal_sequence(
                    schedule_row.compiled_schedule_id_slot,
                    schedule_row.descriptor_schedule_id_slot,
                ),
            ),
            "generated text should validate the compiled Demo.Main schedule id against decoded schedule state"
        );
        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    schedule_row.descriptor_run_system_id_slot,
                    schedule_row.compiled_scheduled_system_id_slot,
                ),
            ),
            "generated text should copy the decoded scheduled Demo.Move system id"
        );
        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    schedule_row.descriptor_item_count_slot,
                    schedule_row.compiled_scheduled_system_count_slot,
                ),
            ),
            "generated text should copy the decoded scheduled system count"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slot_sequence(
                    schedule_row.compiled_scheduled_system_count_slot,
                    schedule_row.expected_scheduled_system_count,
                ),
            ),
            "generated text should validate the compiled scheduled system count"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slots_equal_sequence(
                    schedule_row.compiled_scheduled_system_id_slot,
                    schedule_row.system_descriptor_id_slot,
                ),
            ),
            "generated text should validate the compiled scheduled system id against decoded system state before query planning"
        );
        assert!(
            contains_subsequence(
                &text,
                &load_qword_at_stack_address_store_sequence(
                    native_query_plan_iteration_row(storage_compatibility_for_program(&program))
                        .build_row
                        .catalog_row_count_address_slot,
                    ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT,
                ),
            ),
            "compiled schedule execution should build catalog-backed query-plan state before Demo.Move"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_component_field_multiply_sequence(
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "compiled schedule execution should emit Demo.Move field math"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_position_store_sequence(
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "compiled schedule execution should emit Demo.Move Position stores"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should preserve compiled schedule success"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_RUN_SCHEDULE_DISPATCH_FAILURE_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "generated text should expose compiled schedule dispatch failure"
        );
    }

    #[test]
    fn executes_compiled_schedules_from_table_rows_generically() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let model = NATIVE_ECS_TABLE_MODEL;
        let startup_run = model.startup_operations.run_schedule_rows[0];
        let main_schedule = model.descriptors.schedule_rows[0].slots;
        let move_system = model.descriptors.system_rows[0].slots;
        let compiled_schedule = model.compiled_schedules.rows[0];
        let row = ECS_COMPILED_SCHEDULE_BUILD_ROWS[0];

        assert_eq!(ECS_COMPILED_SCHEDULE_BUILD_ROWS.len(), 1);
        assert_eq!(
            row,
            NativeCompiledScheduleBuildRow {
                startup_schedule_id_slot: startup_run.schedule_id.offset,
                descriptor_schedule_id_slot: main_schedule.id.offset,
                descriptor_item_count_slot: main_schedule.item_count.offset,
                descriptor_run_system_id_slot: main_schedule.run_system_id.offset,
                system_descriptor_id_slot: move_system.id.offset,
                compiled_schedule_id_slot: compiled_schedule.schedule_id.offset,
                compiled_scheduled_system_id_slot: compiled_schedule.scheduled_system_id.offset,
                compiled_scheduled_system_count_slot: compiled_schedule
                    .scheduled_system_count
                    .offset,
                expected_scheduled_system_count: 1,
                expected_scheduled_system_id: DEMO_MOVE_SYSTEM_ID,
                query_plan_row_index: 0,
            }
        );
        assert_eq!(
            ECS_QUERY_PLAN_TABLE_ITERATION_ROWS[row.query_plan_row_index]
                .build_row
                .plan_query_id_slot,
            model.query_plans.rows[0].query_id.offset
        );

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    row.startup_schedule_id_slot,
                    row.compiled_schedule_id_slot,
                ),
            ),
            "generated text should copy the startup run row schedule id through the schedule build row"
        );
        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    row.descriptor_run_system_id_slot,
                    row.compiled_scheduled_system_id_slot,
                ),
            ),
            "generated text should copy the decoded scheduled system id through the schedule build row"
        );
        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    row.descriptor_item_count_slot,
                    row.compiled_scheduled_system_count_slot,
                ),
            ),
            "generated text should copy the decoded scheduled system count through the schedule build row"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slots_equal_sequence(
                    row.compiled_schedule_id_slot,
                    row.descriptor_schedule_id_slot,
                ),
            ),
            "generated text should validate compiled schedule id against decoded schedule row"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slots_equal_sequence(
                    row.compiled_scheduled_system_id_slot,
                    row.system_descriptor_id_slot,
                ),
            ),
            "generated text should validate compiled system id against decoded system row"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slot_sequence(
                    row.compiled_scheduled_system_count_slot,
                    row.expected_scheduled_system_count,
                ),
            ),
            "generated text should validate the expected scheduled-system count"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slot_sequence(
                    row.compiled_scheduled_system_id_slot,
                    row.expected_scheduled_system_id,
                ),
            ),
            "generated text should validate the expected scheduled Demo.Move system id"
        );

        let query_plan_row =
            ECS_QUERY_PLAN_TABLE_ITERATION_ROWS[row.query_plan_row_index].build_row;
        assert!(
            contains_subsequence(
                &text,
                &load_store_stack_slot_sequence(
                    query_plan_row.query_id_slot,
                    query_plan_row.plan_query_id_slot,
                ),
            ),
            "compiled schedule execution should dispatch into the table-row query-plan builder"
        );
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slots_equal_sequence(
                    query_plan_row.plan_term_count_slot,
                    query_plan_row.system_query_term_count_slot,
                ),
            ),
            "query planning should still validate against decoded system query-param state"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_component_field_multiply_sequence(
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "compiled schedule execution should still emit compiled Demo.Move field math"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_position_store_sequence(
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "compiled schedule execution should still emit compiled Demo.Move Position stores"
        );
        assert!(
            contains_subsequence(
                &text,
                &[0xbf, ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE, 0x00, 0x00, 0x00],
            ),
            "generated text should preserve compiled schedule success"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_RUN_SCHEDULE_DISPATCH_FAILURE_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "generated text should expose compiled schedule dispatch failure"
        );
    }

    #[test]
    fn executes_move_system_from_decoded_native_ecs_tables() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(&program)
            .expect("move_system.arc assembles");
        let metadata =
            ecs_metadata::encode_ecs_metadata(&assembly).expect("move_system metadata encodes");

        let text = ecs_metadata_decoder_text_payload(&program, &metadata)
            .expect("move_system ECS decoder text emits");

        assert_eq!(storage_plan_for_program(&program).frame_size, 1072);
        for (startup_table_slot, descriptor_slot) in ECS_RESOURCE_STARTUP_DESCRIPTOR_RELATIONS {
            assert!(
                contains_subsequence(
                    &text,
                    &compare_stack_slots_equal_sequence(startup_table_slot, descriptor_slot),
                ),
                "resource startup table slot {} should be checked against decoded descriptor slot {}",
                startup_table_slot,
                descriptor_slot
            );
        }
        let storage_compatibility = storage_compatibility_for_program(&program);
        let catalog_table = storage_compatibility.catalog_table;
        let first_spawn = NATIVE_ECS_TABLE_MODEL.startup_operations.spawn_rows[0];
        for (startup_table_slot, catalog_slot) in
            startup_spawn_catalog_relations(first_spawn, catalog_table)
        {
            assert!(
                contains_subsequence(
                    &text,
                    &compare_stack_slots_equal_sequence(startup_table_slot, catalog_slot),
                ),
                "spawn startup table slot {} should be checked against catalog slot {}",
                startup_table_slot,
                catalog_slot
            );
        }
        let schedule_build_row = ECS_COMPILED_SCHEDULE_BUILD_ROWS[0];
        for (source_slot, target_slot) in [
            (
                schedule_build_row.startup_schedule_id_slot,
                schedule_build_row.compiled_schedule_id_slot,
            ),
            (
                schedule_build_row.descriptor_run_system_id_slot,
                schedule_build_row.compiled_scheduled_system_id_slot,
            ),
            (
                schedule_build_row.descriptor_item_count_slot,
                schedule_build_row.compiled_scheduled_system_count_slot,
            ),
        ] {
            assert!(
                contains_subsequence(
                    &text,
                    &load_store_stack_slot_sequence(source_slot, target_slot),
                ),
                "compiled schedule should copy decoded/table slot {} into slot {} through its build row",
                source_slot,
                target_slot
            );
        }
        for (compiled_schedule_slot, descriptor_slot) in [
            (
                schedule_build_row.compiled_schedule_id_slot,
                schedule_build_row.descriptor_schedule_id_slot,
            ),
            (
                schedule_build_row.compiled_scheduled_system_id_slot,
                schedule_build_row.system_descriptor_id_slot,
            ),
        ] {
            assert!(
                contains_subsequence(
                    &text,
                    &compare_stack_slots_equal_sequence(compiled_schedule_slot, descriptor_slot),
                ),
                "compiled schedule slot {} should be checked against decoded descriptor slot {} through its build row",
                compiled_schedule_slot,
                descriptor_slot
            );
        }
        let query_plan_build_row = native_query_plan_iteration_row(storage_compatibility).build_row;
        assert!(
            contains_subsequence(
                &text,
                &compare_stack_slots_equal_sequence(
                    query_plan_build_row.plan_term_count_slot,
                    query_plan_build_row.system_query_term_count_slot,
                ),
            ),
            "query plan term count should be checked against decoded system query-param term count"
        );
        for term in query_plan_build_row.terms {
            assert!(
                contains_subsequence(
                    &text,
                    &compare_stack_slots_equal_sequence(term.plan_access_slot, term.system_access_slot),
                ),
                "query plan access slot {} should be checked against decoded system query-param slot {}",
                term.plan_access_slot,
                term.system_access_slot
            );
            assert!(
                contains_subsequence(
                    &text,
                    &compare_stack_slots_equal_sequence(
                        term.plan_component_id_slot,
                        term.system_component_id_slot,
                    ),
                ),
                "query plan component slot {} should be checked against decoded system query-param slot {}",
                term.plan_component_id_slot,
                term.system_component_id_slot
            );
            assert!(
                contains_subsequence(
                    &text,
                    &compare_stack_slots_equal_sequence(
                        term.plan_component_id_slot,
                        term.component_descriptor_id_slot,
                    ),
                ),
                "query plan component slot {} should be checked against descriptor slot {}",
                term.plan_component_id_slot,
                term.component_descriptor_id_slot
            );
            assert!(contains_subsequence(
                &text,
                &compare_stack_slots_equal_sequence(term.plan_size_slot, term.component_size_slot,),
            ));
        }
        assert!(
            contains_subsequence(
                &text,
                &query_plan_component_field_multiply_sequence(
                    ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "decoded-table execution should still run compiled Demo.Move field math"
        );
        assert!(
            contains_subsequence(
                &text,
                &query_plan_position_store_sequence(
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                    0,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
                ),
            ),
            "decoded-table execution should still run compiled Demo.Move Position stores"
        );
        for exit_code in [
            ECS_COMPILED_MOVE_SUCCESS_EXIT_CODE,
            ECS_METADATA_FAILURE_EXIT_CODE,
            ECS_STARTUP_STATE_FAILURE_EXIT_CODE,
            ECS_QUERY_LOOP_SCAN_FAILURE_EXIT_CODE,
            ECS_QUERY_LOOP_FIELD_MATH_FAILURE_EXIT_CODE,
            ECS_QUERY_LOOP_POSITION_STORE_FAILURE_EXIT_CODE,
            ECS_RUN_SCHEDULE_DISPATCH_FAILURE_EXIT_CODE,
        ] {
            assert!(
                contains_subsequence(&text, &[0xbf, exit_code, 0x00, 0x00, 0x00]),
                "generated text should expose exit code {}",
                exit_code
            );
        }
    }

    fn contains_subsequence(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    fn count_subsequence(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .filter(|window| *window == needle)
            .count()
    }

    fn find_subsequence_from(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
        haystack[start..]
            .windows(needle.len())
            .position(|relative_index| relative_index == needle)
            .map(|relative_index| start + relative_index)
    }

    fn contains_ordered_subsequences(haystack: &[u8], needles: &[Vec<u8>]) -> bool {
        let mut search_start = 0usize;
        for needle in needles {
            let Some(relative_index) = haystack[search_start..]
                .windows(needle.len())
                .position(|window| window == needle)
            else {
                return false;
            };
            search_start += relative_index + needle.len();
        }
        true
    }

    fn metadata_dword_load_store_sequence(metadata_offset: u8, stack_slot: u16) -> Vec<u8> {
        let mut bytes = vec![0x8b, 0x46, metadata_offset]; // mov eax, dword ptr [rsi + offset]
        append_rax_qword_store(&mut bytes, stack_slot);
        bytes
    }

    fn metadata_dword_disp32_load_qword_store_sequence(
        metadata_offset: i32,
        stack_slot: u16,
    ) -> Vec<u8> {
        let mut bytes = vec![0x8b, 0x86]; // mov eax, dword ptr [rsi + offset]
        bytes.extend_from_slice(&metadata_offset.to_le_bytes());
        append_rax_qword_store(&mut bytes, stack_slot);
        bytes
    }

    fn metadata_qword_load_store_sequence(metadata_offset: i32, stack_slot: u16) -> Vec<u8> {
        let mut bytes = vec![0x48, 0x8b, 0x86]; // mov rax, qword ptr [rsi + offset]
        bytes.extend_from_slice(&metadata_offset.to_le_bytes());
        append_rax_qword_store(&mut bytes, stack_slot);
        bytes
    }

    fn metadata_ascii_compare_sequences(metadata_offset: i32, expected: &[u8]) -> Vec<Vec<u8>> {
        let mut sequences = Vec::new();
        let mut offset = 0usize;
        while expected.len() - offset >= 8 {
            let mut chunk = [0u8; 8];
            chunk.copy_from_slice(&expected[offset..offset + 8]);
            let mut bytes = vec![0x48, 0xb8]; // mov rax, imm64
            bytes.extend_from_slice(&u64::from_le_bytes(chunk).to_le_bytes());
            bytes.extend_from_slice(&[0x48, 0x39, 0x86]); // cmp qword ptr [rsi + offset], rax
            bytes.extend_from_slice(&(metadata_offset + offset as i32).to_le_bytes());
            sequences.push(bytes);
            offset += 8;
        }
        if expected.len() - offset >= 4 {
            let mut chunk = [0u8; 4];
            chunk.copy_from_slice(&expected[offset..offset + 4]);
            let mut bytes = vec![0xb8]; // mov eax, imm32
            bytes.extend_from_slice(&u32::from_le_bytes(chunk).to_le_bytes());
            bytes.extend_from_slice(&[0x39, 0x86]); // cmp dword ptr [rsi + offset], eax
            bytes.extend_from_slice(&(metadata_offset + offset as i32).to_le_bytes());
            sequences.push(bytes);
            offset += 4;
        }
        while offset < expected.len() {
            let mut bytes = vec![0x80, 0xbe]; // cmp byte ptr [rsi + offset], imm8
            bytes.extend_from_slice(&(metadata_offset + offset as i32).to_le_bytes());
            bytes.push(expected[offset]);
            sequences.push(bytes);
            offset += 1;
        }
        sequences
    }

    fn mov_eax_one_store_sequence(stack_slot: u16) -> Vec<u8> {
        mov_eax_immediate_store_sequence(1, stack_slot)
    }

    fn mov_eax_immediate_store_sequence(value: u32, stack_slot: u16) -> Vec<u8> {
        let mut bytes = vec![0xb8]; // mov eax, imm32
        bytes.extend_from_slice(&value.to_le_bytes());
        append_rax_qword_store(&mut bytes, stack_slot);
        bytes
    }

    fn mov_rax_immediate_store_sequence(value: u64, stack_slot: u16) -> Vec<u8> {
        let mut bytes = vec![0x48, 0xb8]; // mov rax, imm64
        bytes.extend_from_slice(&value.to_le_bytes());
        append_rax_qword_store(&mut bytes, stack_slot);
        bytes
    }

    fn u64_immediate_store_sequence(value: u64, stack_slot: u16) -> Vec<u8> {
        mov_rax_immediate_store_sequence(value, stack_slot)
    }

    fn storage_plan_for_program(program: &Program) -> NativeWorldStoragePlan {
        let core = core_lower::lower_program_to_core(program).expect("fixture lowers to Core");
        let assembly = runtime_assembly::assemble_runtime_program_from_source(program)
            .expect("fixture assembles");
        derive_native_world_storage_plan(&core, &assembly, NATIVE_STORAGE_BASE_OFFSET)
            .expect("fixture has a native world storage plan")
    }

    fn storage_compatibility_for_program(program: &Program) -> NativeStorageCompatibilityModel {
        let assembly = runtime_assembly::assemble_runtime_program_from_source(program)
            .expect("fixture assembles for storage compatibility");
        let metadata = ecs_metadata::encode_ecs_metadata(&assembly)
            .expect("fixture metadata encodes for storage compatibility");
        let startup = startup_payloads(&metadata)
            .expect("fixture startup payloads parse for storage compatibility");
        let observable = native_move_query_loop_observable(program, &startup)
            .expect("fixture query observable exists for storage compatibility");
        native_storage_compatibility_model(&storage_plan_for_program(program), &observable)
            .expect("fixture fits the bounded native execution compatibility model")
    }

    fn descriptor_sized_arena_emission_plan() -> NativeWorldStoragePlan {
        let regeneration = planned_schema(10, "Regeneration", 12, 4);
        let vitality = planned_schema(20, "Vitality", 8, 4);
        let faction = planned_schema(30, "Faction", 4, 4);
        NativeWorldStoragePlan {
            frame_size: 1328,
            tables: vec![
                NativeTableStoragePlan {
                    key: vec![10, 20, 30].into_boxed_slice(),
                    rows: vec![
                        planned_spawn_row(0),
                        planned_spawn_row(1),
                        planned_spawn_row(2),
                    ],
                    capacity_steps: vec![1, 2, 4].into_boxed_slice(),
                    capacity: 4,
                    logical_row_stride: 24,
                    storage: planned_table_storage_slots(936),
                    catalog: planned_catalog_table_slots(1104),
                    columns: vec![
                        planned_column(regeneration, 960, 48, 1168),
                        planned_column(vitality.clone(), 1008, 32, 1200),
                        planned_column(faction.clone(), 1040, 16, 1232),
                    ],
                },
                NativeTableStoragePlan {
                    key: vec![20, 30].into_boxed_slice(),
                    rows: vec![planned_spawn_row(3), planned_spawn_row(4)],
                    capacity_steps: vec![1, 2].into_boxed_slice(),
                    capacity: 2,
                    logical_row_stride: 12,
                    storage: planned_table_storage_slots(1056),
                    catalog: planned_catalog_table_slots(1136),
                    columns: vec![
                        planned_column(vitality, 1080, 16, 1264),
                        planned_column(faction, 1096, 8, 1296),
                    ],
                },
            ],
        }
    }

    fn aligned_synthetic_emission_plan() -> NativeWorldStoragePlan {
        NativeWorldStoragePlan {
            frame_size: 1088,
            tables: vec![NativeTableStoragePlan {
                key: vec![40, 50].into_boxed_slice(),
                rows: vec![planned_spawn_row(0)],
                capacity_steps: vec![1].into_boxed_slice(),
                capacity: 1,
                logical_row_stride: 24,
                storage: planned_table_storage_slots(936),
                catalog: planned_catalog_table_slots(992),
                columns: vec![
                    planned_column(planned_schema(40, "Aligned8", 8, 8), 960, 8, 1024),
                    planned_column(planned_schema(50, "Aligned16", 16, 16), 976, 16, 1056),
                ],
            }],
        }
    }

    fn planned_spawn_row(spawn_ordinal: u32) -> NativePlannedSpawnRow {
        NativePlannedSpawnRow {
            spawn_ordinal,
            startup_operation_index: spawn_ordinal,
        }
    }

    fn planned_schema(id: u64, name: &str, size: u32, align: u32) -> NativeComponentSchema {
        NativeComponentSchema {
            id,
            name: name.to_string(),
            size,
            align,
            fields: Vec::new(),
        }
    }

    fn planned_column(
        schema: NativeComponentSchema,
        payload_offset: u16,
        payload_byte_len: u16,
        catalog_offset: u16,
    ) -> NativeColumnStoragePlan {
        NativeColumnStoragePlan {
            schema,
            payload: NativeByteRange {
                offset: payload_offset,
                byte_len: payload_byte_len,
            },
            catalog: planned_catalog_column_slots(catalog_offset),
        }
    }

    fn planned_table_storage_slots(offset: u16) -> NativeTableStorageSlots {
        NativeTableStorageSlots {
            row_count: planned_slot(offset),
            capacity: planned_slot(offset + 8),
            row_stride: planned_slot(offset + 16),
        }
    }

    fn planned_catalog_table_slots(offset: u16) -> NativeCatalogTableSlots {
        NativeCatalogTableSlots {
            column_count: planned_slot(offset),
            row_count_address: planned_slot(offset + 8),
            capacity: planned_slot(offset + 16),
            row_stride: planned_slot(offset + 24),
        }
    }

    fn planned_catalog_column_slots(offset: u16) -> NativeCatalogColumnSlots {
        NativeCatalogColumnSlots {
            component_id: planned_slot(offset),
            element_size: planned_slot(offset + 8),
            element_align: planned_slot(offset + 16),
            payload_base_address: planned_slot(offset + 24),
        }
    }

    fn planned_slot(offset: u16) -> NativeSlot {
        NativeSlot {
            offset,
            byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
        }
    }

    fn load_store_stack_slot_sequence(load_slot: u16, store_slot: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, load_slot);
        append_rax_qword_store(&mut bytes, store_slot);
        bytes
    }

    fn qword_stack_store_slots(bytes: &[u8]) -> Vec<u16> {
        let mut slots = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index..].starts_with(&[0x48, 0x89, 0x04, 0x24]) {
                slots.push(0);
                index += 4;
            } else if bytes[index..].starts_with(&[0x48, 0x89, 0x44, 0x24])
                && index + 5 <= bytes.len()
            {
                slots.push(u16::from(bytes[index + 4]));
                index += 5;
            } else if bytes[index..].starts_with(&[0x48, 0x89, 0x84, 0x24])
                && index + 8 <= bytes.len()
            {
                let slot = u32::from_le_bytes(
                    bytes[index + 4..index + 8]
                        .try_into()
                        .expect("four-byte stack displacement"),
                );
                slots.push(u16::try_from(slot).expect("native frame slot fits u16"));
                index += 8;
            } else {
                index += 1;
            }
        }
        slots
    }

    fn find_emitted_block_ignoring_rel32(
        bytes: &[u8],
        expected: &[u8],
        jump_offsets: &[usize],
    ) -> Option<usize> {
        if expected.len() > bytes.len() {
            return None;
        }

        (0..=bytes.len() - expected.len()).find(|start| {
            expected.iter().enumerate().all(|(index, byte)| {
                let is_patched_displacement = jump_offsets
                    .iter()
                    .any(|jump| index >= jump + 2 && index < jump + 6);
                is_patched_displacement || bytes[start + index] == *byte
            })
        })
    }

    fn load_qword_at_stack_address_store_sequence(address_slot: u16, store_slot: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, address_slot);
        bytes.extend_from_slice(&[0x48, 0x8b, 0x00]);
        append_rax_qword_store(&mut bytes, store_slot);
        bytes
    }

    fn advance_planned_address_sequence(address_slot: u16, size_slot: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, address_slot);
        append_add_stack_slot_to_rax(&mut bytes, size_slot);
        append_rax_qword_store(&mut bytes, address_slot);
        bytes
    }

    fn native_storage_catalog_materialization_sequence(
        storage_plan: &NativeWorldStoragePlan,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        for table in &storage_plan.tables {
            bytes.extend_from_slice(&u64_immediate_store_sequence(
                table.columns.len() as u64,
                table.catalog.column_count.offset,
            ));
            bytes.extend_from_slice(&lea_stack_address_store_sequence(
                table.storage.row_count.offset,
                table.catalog.row_count_address.offset,
            ));
            for (value, slot) in [
                (u64::from(table.capacity), table.storage.capacity),
                (u64::from(table.capacity), table.catalog.capacity),
                (
                    u64::from(table.logical_row_stride),
                    table.storage.row_stride,
                ),
                (
                    u64::from(table.logical_row_stride),
                    table.catalog.row_stride,
                ),
            ] {
                bytes.extend_from_slice(&u64_immediate_store_sequence(value, slot.offset));
            }
            for column in &table.columns {
                for (value, slot) in [
                    (column.schema.id, column.catalog.component_id),
                    (u64::from(column.schema.size), column.catalog.element_size),
                    (u64::from(column.schema.align), column.catalog.element_align),
                ] {
                    bytes.extend_from_slice(&u64_immediate_store_sequence(value, slot.offset));
                }
                bytes.extend_from_slice(&lea_stack_address_store_sequence(
                    column.payload.offset,
                    column.catalog.payload_base_address.offset,
                ));
            }
        }
        bytes
    }

    fn catalog_payload_store_sequence(
        column: NativeStorageCatalogColumnRow,
        row_index: usize,
        payload_slot: u16,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, column.slots.payload_base_address.offset);
        for _ in 0..row_index {
            append_add_stack_slot_to_rax(&mut bytes, column.slots.element_size.offset);
        }
        append_load_stack_slot_to_rdx(&mut bytes, payload_slot);
        bytes.extend_from_slice(&[0x48, 0x89, 0x10]);
        bytes
    }

    fn catalog_row_count_commit_sequence(
        row_count_address_slot: u16,
        committed_count: u32,
        logical_count_slot: u16,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, row_count_address_slot);
        bytes.push(0xba);
        bytes.extend_from_slice(&committed_count.to_le_bytes());
        bytes.extend_from_slice(&[0x48, 0x89, 0x10]);
        bytes.extend_from_slice(&mov_eax_immediate_store_sequence(
            committed_count,
            logical_count_slot,
        ));
        bytes
    }

    fn compare_qword_at_stack_address_sequence(address_slot: u16, expected: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, address_slot);
        bytes.extend_from_slice(&[0x48, 0x8b, 0x00, 0x48, 0xba]);
        bytes.extend_from_slice(&expected.to_le_bytes());
        bytes.extend_from_slice(&[0x48, 0x39, 0xd0]);
        bytes
    }

    fn metadata_dword_via_offset_slot_to_dword_store_sequence(
        offset_slot: u16,
        target_slot: u16,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, offset_slot);
        bytes.extend_from_slice(&[0x8b, 0x04, 0x06]); // mov eax, dword ptr [rsi + rax]
        append_eax_dword_store(&mut bytes, target_slot);
        bytes
    }

    fn metadata_qword_via_offset_slot_to_qword_store_sequence(
        offset_slot: u16,
        target_slot: u16,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, offset_slot);
        bytes.extend_from_slice(&[0x48, 0x8b, 0x04, 0x06]); // mov rax, qword ptr [rsi + rax]
        append_rax_qword_store(&mut bytes, target_slot);
        bytes
    }

    fn lea_stack_address_store_sequence(source_slot: u16, store_slot: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_lea_stack_address_to_rax(&mut bytes, source_slot);
        append_rax_qword_store(&mut bytes, store_slot);
        bytes
    }

    fn query_plan_component_field_multiply_sequence(
        address_slot: u16,
        field_offset: u8,
        product_slot: u16,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, address_slot);
        append_movss_xmm_from_rax(&mut bytes, 0, field_offset);
        append_movss_xmm_from_stack(&mut bytes, 1, ECS_RESOURCE_PAYLOAD_STORAGE_SLOT);
        bytes.extend_from_slice(&[0xf3, 0x0f, 0x59, 0xc1]); // mulss xmm0, xmm1
        append_movss_stack_from_xmm(&mut bytes, product_slot, 0);
        bytes
    }

    fn query_plan_position_store_sequence(
        address_slot: u16,
        field_offset: u8,
        product_slot: u16,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, address_slot);
        append_movss_xmm_from_rax(&mut bytes, 0, field_offset);
        append_movss_xmm_from_stack(&mut bytes, 1, product_slot);
        bytes.extend_from_slice(&[0xf3, 0x0f, 0x58, 0xc1]); // addss xmm0, xmm1
        append_movss_rax_from_xmm(&mut bytes, field_offset, 0);
        bytes
    }

    fn compare_stack_slot_sequence(stack_slot: u16, expected: u64) -> Vec<u8> {
        let mut bytes = vec![0x48, 0xb8]; // mov rax, imm64
        bytes.extend_from_slice(&expected.to_le_bytes());
        if stack_slot == 0 {
            bytes.extend_from_slice(&[0x48, 0x39, 0x04, 0x24]);
        } else if stack_slot <= 127 {
            bytes.extend_from_slice(&[0x48, 0x39, 0x44, 0x24, stack_slot as u8]);
        } else {
            bytes.extend_from_slice(&[0x48, 0x39, 0x84, 0x24]);
            bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
        }
        bytes
    }

    fn compare_stack_slots_equal_sequence(left_slot: u16, right_slot: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, left_slot);
        if right_slot == 0 {
            bytes.extend_from_slice(&[0x48, 0x39, 0x04, 0x24]);
        } else if right_slot <= 127 {
            bytes.extend_from_slice(&[0x48, 0x39, 0x44, 0x24, right_slot as u8]);
        } else {
            bytes.extend_from_slice(&[0x48, 0x39, 0x84, 0x24]);
            bytes.extend_from_slice(&(right_slot as u32).to_le_bytes());
        }
        bytes
    }

    fn expected_runtime_create_prefix(layout: &NativeEcsExecutionStateLayout) -> Vec<u8> {
        let mut bytes = Vec::new();
        if layout.frame_size <= 127 {
            bytes.extend_from_slice(&[0x48, 0x83, 0xec, layout.frame_size as u8]);
        } else {
            bytes.extend_from_slice(&[0x48, 0x81, 0xec]);
            bytes.extend_from_slice(&(layout.frame_size as u32).to_le_bytes());
        }
        bytes.extend_from_slice(&[0x31, 0xc0]); // xor eax, eax
        for offset in layout.zeroed_qword_offsets {
            append_zero_qword_store(&mut bytes, offset);
        }
        bytes
    }

    fn expected_runtime_destroy_suffix(layout: &NativeEcsExecutionStateLayout) -> Vec<u8> {
        let mut bytes = vec![0x31, 0xc0]; // xor eax, eax
        for offset in layout.zeroed_qword_offsets {
            append_zero_qword_store(&mut bytes, offset);
        }
        if layout.frame_size <= 127 {
            bytes.extend_from_slice(&[0x48, 0x83, 0xc4, layout.frame_size as u8]);
        } else {
            bytes.extend_from_slice(&[0x48, 0x81, 0xc4]);
            bytes.extend_from_slice(&(layout.frame_size as u32).to_le_bytes());
        }
        bytes.extend_from_slice(&[
            0xb8, 0x3c, 0x00, 0x00, 0x00, // mov eax, 60
            0x0f, 0x05, // syscall
        ]);
        bytes
    }

    fn append_zero_qword_store(bytes: &mut Vec<u8>, offset: u16) {
        append_rax_qword_store(bytes, offset);
    }

    fn append_rax_qword_store(bytes: &mut Vec<u8>, offset: u16) {
        if offset == 0 {
            bytes.extend_from_slice(&[0x48, 0x89, 0x04, 0x24]);
        } else if offset <= 127 {
            bytes.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, offset as u8]);
        } else {
            bytes.extend_from_slice(&[0x48, 0x89, 0x84, 0x24]);
            bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        }
    }

    fn append_eax_dword_store(bytes: &mut Vec<u8>, offset: u16) {
        if offset == 0 {
            bytes.extend_from_slice(&[0x89, 0x04, 0x24]);
        } else if offset <= 127 {
            bytes.extend_from_slice(&[0x89, 0x44, 0x24, offset as u8]);
        } else {
            bytes.extend_from_slice(&[0x89, 0x84, 0x24]);
            bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        }
    }

    fn append_load_stack_slot_to_rax(bytes: &mut Vec<u8>, offset: u16) {
        if offset == 0 {
            bytes.extend_from_slice(&[0x48, 0x8b, 0x04, 0x24]);
        } else if offset <= 127 {
            bytes.extend_from_slice(&[0x48, 0x8b, 0x44, 0x24, offset as u8]);
        } else {
            bytes.extend_from_slice(&[0x48, 0x8b, 0x84, 0x24]);
            bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        }
    }

    fn append_load_stack_slot_to_rdx(bytes: &mut Vec<u8>, offset: u16) {
        if offset == 0 {
            bytes.extend_from_slice(&[0x48, 0x8b, 0x14, 0x24]);
        } else if offset <= 127 {
            bytes.extend_from_slice(&[0x48, 0x8b, 0x54, 0x24, offset as u8]);
        } else {
            bytes.extend_from_slice(&[0x48, 0x8b, 0x94, 0x24]);
            bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        }
    }

    fn append_add_stack_slot_to_rax(bytes: &mut Vec<u8>, offset: u16) {
        if offset == 0 {
            bytes.extend_from_slice(&[0x48, 0x03, 0x04, 0x24]);
        } else if offset <= 127 {
            bytes.extend_from_slice(&[0x48, 0x03, 0x44, 0x24, offset as u8]);
        } else {
            bytes.extend_from_slice(&[0x48, 0x03, 0x84, 0x24]);
            bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        }
    }

    fn append_lea_stack_address_to_rax(bytes: &mut Vec<u8>, offset: u16) {
        if offset <= 127 {
            bytes.extend_from_slice(&[0x48, 0x8d, 0x44, 0x24, offset as u8]);
        } else {
            bytes.extend_from_slice(&[0x48, 0x8d, 0x84, 0x24]);
            bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        }
    }

    fn append_movss_xmm_from_stack(bytes: &mut Vec<u8>, xmm_register: u8, stack_slot: u16) {
        bytes.extend_from_slice(&[0xf3, 0x0f, 0x10]);
        if stack_slot <= 127 {
            bytes.push(0x44 | (xmm_register << 3));
            bytes.extend_from_slice(&[0x24, stack_slot as u8]);
        } else {
            bytes.push(0x84 | (xmm_register << 3));
            bytes.push(0x24);
            bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
        }
    }

    fn append_movss_stack_from_xmm(bytes: &mut Vec<u8>, stack_slot: u16, xmm_register: u8) {
        bytes.extend_from_slice(&[0xf3, 0x0f, 0x11]);
        if stack_slot <= 127 {
            bytes.push(0x44 | (xmm_register << 3));
            bytes.extend_from_slice(&[0x24, stack_slot as u8]);
        } else {
            bytes.push(0x84 | (xmm_register << 3));
            bytes.push(0x24);
            bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
        }
    }

    fn append_movss_xmm_from_rax(bytes: &mut Vec<u8>, xmm_register: u8, field_offset: u8) {
        bytes.extend_from_slice(&[0xf3, 0x0f, 0x10]);
        bytes.push(0x40 | (xmm_register << 3));
        bytes.push(field_offset);
    }

    fn append_movss_rax_from_xmm(bytes: &mut Vec<u8>, field_offset: u8, xmm_register: u8) {
        bytes.extend_from_slice(&[0xf3, 0x0f, 0x11]);
        bytes.push(0x40 | (xmm_register << 3));
        bytes.push(field_offset);
    }
}
