use crate::core::{
    CoreProgram, CoreQueryAccess, CoreScheduleItem, CoreSystemBinaryOp, CoreSystemExpression,
    CoreSystemParamKind, CoreSystemPlace, CoreSystemStatement,
};
use crate::core_lower;
use crate::parser::{BinaryOperator, Expression, Program, Statement};

const NATIVE_ECS_QWORD_BYTE_LEN: u8 = 8;
const NATIVE_ECS_DWORD_BYTE_LEN: u8 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeEcsSlot {
    offset: u8,
    byte_len: u8,
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
struct NativeStartupStateSlots {
    time_payload: NativeEcsSlot,
    row_count: NativeEcsSlot,
    position_payload: NativeEcsSlot,
    velocity_payload: NativeEcsSlot,
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
struct NativeCompiledMoveSlots {
    target_position_payload: NativeEcsSlot,
    scanned_row_count: NativeEcsSlot,
    field_product_payload: NativeEcsSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeEcsExecutionStateLayout {
    frame_size: u8,
    zeroed_qword_offsets: [u8; 29],
    descriptor_counts: NativeDescriptorCountSlots,
    descriptor_records: NativeDescriptorRecordStateSlots,
    startup_state: NativeStartupStateSlots,
    startup_dispatch: NativeStartupDispatchSlots,
    query_plan: NativeQueryPlanSlots,
    compiled_move: NativeCompiledMoveSlots,
}

const NATIVE_ECS_EXECUTION_STATE_LAYOUT: NativeEcsExecutionStateLayout =
    NativeEcsExecutionStateLayout {
        frame_size: 232,
        zeroed_qword_offsets: [
            0, 8, 16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96, 104, 112, 120, 128, 136, 144, 152,
            160, 168, 176, 184, 192, 200, 208, 216, 224,
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
            position_payload: NativeEcsSlot {
                offset: 56,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
            velocity_payload: NativeEcsSlot {
                offset: 64,
                byte_len: NATIVE_ECS_QWORD_BYTE_LEN,
            },
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
    };
const NATIVE_ECS_EXECUTION_STATE_FRAME_SIZE: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT.frame_size;

const RUNTIME_CREATE_PREFIX: &[u8] = &[
    0x48,
    0x81,
    0xec,
    NATIVE_ECS_EXECUTION_STATE_FRAME_SIZE,
    0x00,
    0x00,
    0x00, // sub rsp, frame size
    0x31,
    0xc0, // xor eax, eax
    0x48,
    0x89,
    0x04,
    0x24, // mov qword ptr [rsp], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x08, // mov qword ptr [rsp + 8], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x10, // mov qword ptr [rsp + 16], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x18, // mov qword ptr [rsp + 24], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x20, // mov qword ptr [rsp + 32], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x28, // mov qword ptr [rsp + 40], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x30, // mov qword ptr [rsp + 48], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x38, // mov qword ptr [rsp + 56], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x40, // mov qword ptr [rsp + 64], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x48, // mov qword ptr [rsp + 72], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x50, // mov qword ptr [rsp + 80], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x58, // mov qword ptr [rsp + 88], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x60, // mov qword ptr [rsp + 96], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x68, // mov qword ptr [rsp + 104], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x70, // mov qword ptr [rsp + 112], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x78, // mov qword ptr [rsp + 120], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0x80,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 128], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0x88,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 136], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0x90,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 144], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0x98,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 152], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xa0,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 160], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xa8,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 168], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xb0,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 176], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xb8,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 184], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xc0,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 192], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xc8,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 200], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xd0,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 208], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xd8,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 216], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xe0,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 224], rax
];

const RUNTIME_DESTROY_SUFFIX: &[u8] = &[
    0x31,
    0xc0, // xor eax, eax
    0x48,
    0x89,
    0x04,
    0x24, // mov qword ptr [rsp], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x08, // mov qword ptr [rsp + 8], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x10, // mov qword ptr [rsp + 16], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x18, // mov qword ptr [rsp + 24], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x20, // mov qword ptr [rsp + 32], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x28, // mov qword ptr [rsp + 40], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x30, // mov qword ptr [rsp + 48], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x38, // mov qword ptr [rsp + 56], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x40, // mov qword ptr [rsp + 64], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x48, // mov qword ptr [rsp + 72], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x50, // mov qword ptr [rsp + 80], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x58, // mov qword ptr [rsp + 88], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x60, // mov qword ptr [rsp + 96], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x68, // mov qword ptr [rsp + 104], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x70, // mov qword ptr [rsp + 112], rax
    0x48,
    0x89,
    0x44,
    0x24,
    0x78, // mov qword ptr [rsp + 120], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0x80,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 128], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0x88,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 136], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0x90,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 144], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0x98,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 152], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xa0,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 160], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xa8,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 168], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xb0,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 176], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xb8,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 184], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xc0,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 192], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xc8,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 200], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xd0,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 208], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xd8,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 216], rax
    0x48,
    0x89,
    0x84,
    0x24,
    0xe0,
    0x00,
    0x00,
    0x00, // mov qword ptr [rsp + 224], rax
    0x48,
    0x81,
    0xc4,
    NATIVE_ECS_EXECUTION_STATE_FRAME_SIZE,
    0x00,
    0x00,
    0x00, // add rsp, frame size
    0xb8,
    0x3c,
    0x00,
    0x00,
    0x00, // mov eax, 60
    0x0f,
    0x05, // syscall
];

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
const ECS_DESCRIPTOR_REGISTRY_SLOTS: [u8; 5] = [
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
const ECS_DESCRIPTOR_RECORD_OFFSET_SLOTS: [u8; 5] = [
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
const ECS_DESCRIPTOR_RECORD_BYTE_LEN_SLOTS: [u8; 5] = [
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
const ECS_RESOURCE_PAYLOAD_STORAGE_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_state
    .time_payload
    .offset;
const ECS_SPAWN_ROW_COUNT_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_state
    .row_count
    .offset;
const ECS_POSITION_PAYLOAD_STORAGE_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_state
    .position_payload
    .offset;
const ECS_VELOCITY_PAYLOAD_STORAGE_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_state
    .velocity_payload
    .offset;
const ECS_QUERY_LOOP_TARGET_POSITION_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .compiled_move
    .target_position_payload
    .offset;
const ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .compiled_move
    .scanned_row_count
    .offset;
const ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .compiled_move
    .field_product_payload
    .offset;
const ECS_STARTUP_OPERATION_COUNT_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_dispatch
    .operation_count
    .offset;
const ECS_STARTUP_RESOURCE_DISPATCH_COUNT_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_dispatch
    .resource_dispatch_count
    .offset;
const ECS_STARTUP_SPAWN_DISPATCH_COUNT_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_dispatch
    .spawn_dispatch_count
    .offset;
const ECS_STARTUP_RUN_SCHEDULE_DISPATCH_COUNT_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .startup_dispatch
    .run_schedule_dispatch_count
    .offset;
const ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .query_plan
    .matched_row_count
    .offset;
const ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .query_plan
    .position_payload_address
    .offset;
const ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT: u8 = NATIVE_ECS_EXECUTION_STATE_LAYOUT
    .query_plan
    .velocity_payload_address
    .offset;

const DEMO_POSITION_COMPONENT_ID: u64 = 0x002202c6aeb4f27b;
const DEMO_VELOCITY_COMPONENT_ID: u64 = 0x2cf8a68bcb7f913b;
const DEMO_TIME_RESOURCE_ID: u64 = 0x7924ce11db524521;
const DEMO_MOVE_SYSTEM_ID: u64 = 0x723b6b52df270ed5;
const DEMO_MAIN_SCHEDULE_ID: u64 = 0xed3d905325519b05;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EcsStartupPayloads {
    startup_record_count: u32,
    resource_operation_kind_offset: i32,
    resource_payload_offset: i32,
    resource_payload: [u8; 4],
    spawn_operation_kind_offset: i32,
    position_payload_offset: i32,
    position_payload: [u8; 8],
    velocity_payload_offset: i32,
    velocity_payload: [u8; 8],
    run_schedule_operation_kind_offset: i32,
    run_schedule_id_offset: i32,
    run_schedule_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParsedResourcePayloadOperation {
    operation_kind_offset: i32,
    payload_offset: i32,
    payload: [u8; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParsedSpawnOperation {
    operation_kind_offset: i32,
    position_payload_offset: i32,
    position_payload: [u8; 8],
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

    Ok(runtime_wrapped_payload(&startup_body))
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

    let startup_payloads = startup_payloads(metadata_payload)?;
    let query_loop_observable = native_move_query_loop_observable(program, &startup_payloads)?;
    let startup_body = ecs_metadata_decoder_body(
        &metadata_payload[..ECS_METADATA_ENVELOPE_SIZE],
        startup_payloads,
        query_loop_observable,
    );
    Ok(runtime_wrapped_payload(&startup_body))
}

fn native_move_query_loop_observable(
    program: &Program,
    startup_payloads: &EcsStartupPayloads,
) -> Result<NativeMoveQueryLoopObservable, CodegenError> {
    let core = core_lower::lower_program_to_core(program).map_err(|error| CodegenError {
        message: format!(
            "could not lower Core for native query-loop observable: {}",
            error.message
        ),
    })?;

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

    let target_position_payload = target_position_payload(startup_payloads);
    let field_product_payload = field_product_payload(startup_payloads);

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

fn target_position_payload(startup_payloads: &EcsStartupPayloads) -> [u8; 8] {
    let position_x = f32_from_le_bytes(&startup_payloads.position_payload[0..4]);
    let position_y = f32_from_le_bytes(&startup_payloads.position_payload[4..8]);
    let velocity_x = f32_from_le_bytes(&startup_payloads.velocity_payload[0..4]);
    let velocity_y = f32_from_le_bytes(&startup_payloads.velocity_payload[4..8]);
    let delta = f32_from_le_bytes(&startup_payloads.resource_payload);

    let mut payload = [0; 8];
    payload[0..4].copy_from_slice(&(position_x + velocity_x * delta).to_le_bytes());
    payload[4..8].copy_from_slice(&(position_y + velocity_y * delta).to_le_bytes());
    payload
}

fn field_product_payload(startup_payloads: &EcsStartupPayloads) -> [u8; 8] {
    let velocity_x = f32_from_le_bytes(&startup_payloads.velocity_payload[0..4]);
    let velocity_y = f32_from_le_bytes(&startup_payloads.velocity_payload[4..8]);
    let delta = f32_from_le_bytes(&startup_payloads.resource_payload);

    let mut payload = [0; 8];
    payload[0..4].copy_from_slice(&(velocity_x * delta).to_le_bytes());
    payload[4..8].copy_from_slice(&(velocity_y * delta).to_le_bytes());
    payload
}

fn f32_from_le_bytes(bytes: &[u8]) -> f32 {
    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
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

    if startup_record_count < 3 {
        return Err(metadata_startup_payload_error());
    }

    let mut offset = startup_section_offset;
    let resource_payload = parse_resource_payload_operation(metadata_payload, &mut offset)?;
    let spawn_operation = parse_spawn_operation(metadata_payload, &mut offset)?;
    let run_schedule = parse_run_schedule_operation(metadata_payload, &mut offset)?;

    Ok(EcsStartupPayloads {
        startup_record_count,
        resource_operation_kind_offset: resource_payload.operation_kind_offset,
        resource_payload_offset: resource_payload.payload_offset,
        resource_payload: resource_payload.payload,
        spawn_operation_kind_offset: spawn_operation.operation_kind_offset,
        position_payload_offset: spawn_operation.position_payload_offset,
        position_payload: spawn_operation.position_payload,
        velocity_payload_offset: spawn_operation.velocity_payload_offset,
        velocity_payload: spawn_operation.velocity_payload,
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
    *offset += 8;
    skip_metadata_string(metadata_payload, offset)?;

    let payload = parse_payload_offset_and_bytes(metadata_payload, offset)?;
    Ok(ParsedResourcePayloadOperation {
        operation_kind_offset,
        payload_offset: payload.0,
        payload: payload.1,
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

    let component_count = read_metadata_u32(metadata_payload, *offset)?;
    *offset += 4;

    if component_count != 2 {
        return Err(metadata_startup_payload_error());
    }

    let position_payload = parse_spawn_component_payload(metadata_payload, offset)?;
    let velocity_payload = parse_spawn_component_payload(metadata_payload, offset)?;

    Ok(ParsedSpawnOperation {
        operation_kind_offset,
        position_payload_offset: position_payload.0,
        position_payload: position_payload.1,
        velocity_payload_offset: velocity_payload.0,
        velocity_payload: velocity_payload.1,
    })
}

fn parse_spawn_component_payload(
    metadata_payload: &[u8],
    offset: &mut usize,
) -> Result<(i32, [u8; 8]), CodegenError> {
    checked_metadata_range(metadata_payload, *offset, 8)?;
    *offset += 8;
    skip_metadata_string(metadata_payload, offset)?;
    parse_payload_offset_and_bytes(metadata_payload, offset)
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
) -> Result<(i32, [u8; N]), CodegenError> {
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

    Ok((payload_offset as i32, payload))
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

    bytes.extend_from_slice(&[0x8b, 0x46, ECS_STARTUP_RECORD_COUNT_OFFSET]); // mov eax, dword ptr [rsi + offset]
    store_rax_to_stack_slot(&mut bytes, ECS_STARTUP_OPERATION_COUNT_SLOT);
    compare_stack_slot_to_u64(
        &mut bytes,
        ECS_STARTUP_OPERATION_COUNT_SLOT,
        startup_payloads.startup_record_count as u64,
        &mut jump_to_run_schedule_dispatch_failure_offsets,
    );

    emit_startup_operation_dispatch(
        &mut bytes,
        startup_payloads.resource_operation_kind_offset,
        ECS_STARTUP_OP_RESOURCE_PAYLOAD,
        ECS_STARTUP_RESOURCE_DISPATCH_COUNT_SLOT,
        &mut jump_to_run_schedule_dispatch_failure_offsets,
    );
    bytes.extend_from_slice(&[0x8b, 0x86]); // mov eax, dword ptr [rsi + offset]
    bytes.extend_from_slice(&startup_payloads.resource_payload_offset.to_le_bytes());
    bytes.extend_from_slice(&[
        0x89,
        0x44,
        0x24,
        ECS_RESOURCE_PAYLOAD_STORAGE_SLOT, // mov dword ptr [rsp + 40], eax
    ]);

    emit_startup_operation_dispatch(
        &mut bytes,
        startup_payloads.spawn_operation_kind_offset,
        ECS_STARTUP_OP_SPAWN,
        ECS_STARTUP_SPAWN_DISPATCH_COUNT_SLOT,
        &mut jump_to_run_schedule_dispatch_failure_offsets,
    );
    bytes.extend_from_slice(&[0xb8, 0x01, 0x00, 0x00, 0x00]); // mov eax, 1
    store_rax_to_stack_slot(&mut bytes, ECS_SPAWN_ROW_COUNT_SLOT);

    bytes.extend_from_slice(&[0x48, 0x8b, 0x86]); // mov rax, qword ptr [rsi + offset]
    bytes.extend_from_slice(&startup_payloads.position_payload_offset.to_le_bytes());
    store_rax_to_stack_slot(&mut bytes, ECS_POSITION_PAYLOAD_STORAGE_SLOT);

    bytes.extend_from_slice(&[0x48, 0x8b, 0x86]); // mov rax, qword ptr [rsi + offset]
    bytes.extend_from_slice(&startup_payloads.velocity_payload_offset.to_le_bytes());
    store_rax_to_stack_slot(&mut bytes, ECS_VELOCITY_PAYLOAD_STORAGE_SLOT);

    for (expected_count, stack_slot) in ECS_EXPECTED_DESCRIPTOR_COUNTS
        .iter()
        .zip(ECS_DESCRIPTOR_REGISTRY_SLOTS)
    {
        compare_stack_slot_to_u64(
            &mut bytes,
            stack_slot,
            *expected_count,
            &mut jump_to_startup_state_failure_offsets,
        );
    }
    for (expected_offset, stack_slot) in ECS_EXPECTED_DESCRIPTOR_RECORD_OFFSETS
        .iter()
        .zip(ECS_DESCRIPTOR_RECORD_OFFSET_SLOTS)
    {
        compare_stack_slot_to_u64(
            &mut bytes,
            stack_slot,
            *expected_offset,
            &mut jump_to_startup_state_failure_offsets,
        );
    }
    for (expected_byte_len, stack_slot) in ECS_EXPECTED_DESCRIPTOR_RECORD_BYTE_LENS
        .iter()
        .zip(ECS_DESCRIPTOR_RECORD_BYTE_LEN_SLOTS)
    {
        compare_stack_slot_to_u64(
            &mut bytes,
            stack_slot,
            *expected_byte_len,
            &mut jump_to_startup_state_failure_offsets,
        );
    }
    compare_stack_slot_to_u64(
        &mut bytes,
        ECS_RESOURCE_PAYLOAD_STORAGE_SLOT,
        u64::from(u32::from_le_bytes(startup_payloads.resource_payload)),
        &mut jump_to_startup_state_failure_offsets,
    );
    compare_stack_slot_to_u64(
        &mut bytes,
        ECS_SPAWN_ROW_COUNT_SLOT,
        1,
        &mut jump_to_startup_state_failure_offsets,
    );
    compare_stack_slot_to_u64(
        &mut bytes,
        ECS_POSITION_PAYLOAD_STORAGE_SLOT,
        u64::from_le_bytes(startup_payloads.position_payload),
        &mut jump_to_startup_state_failure_offsets,
    );
    compare_stack_slot_to_u64(
        &mut bytes,
        ECS_VELOCITY_PAYLOAD_STORAGE_SLOT,
        u64::from_le_bytes(startup_payloads.velocity_payload),
        &mut jump_to_startup_state_failure_offsets,
    );

    compare_stack_slot_to_u64(
        &mut bytes,
        ECS_STARTUP_RESOURCE_DISPATCH_COUNT_SLOT,
        1,
        &mut jump_to_run_schedule_dispatch_failure_offsets,
    );
    compare_stack_slot_to_u64(
        &mut bytes,
        ECS_STARTUP_SPAWN_DISPATCH_COUNT_SLOT,
        1,
        &mut jump_to_run_schedule_dispatch_failure_offsets,
    );
    bytes.extend_from_slice(&[0x48, 0xb8]); // mov rax, target Position payload
    bytes.extend_from_slice(&query_loop_observable.target_position_payload);
    store_rax_to_stack_slot(&mut bytes, ECS_QUERY_LOOP_TARGET_POSITION_SLOT);
    compare_stack_slot_to_u64(
        &mut bytes,
        ECS_QUERY_LOOP_TARGET_POSITION_SLOT,
        u64::from_le_bytes(query_loop_observable.target_position_payload),
        &mut jump_to_startup_state_failure_offsets,
    );

    emit_startup_operation_dispatch(
        &mut bytes,
        startup_payloads.run_schedule_operation_kind_offset,
        ECS_STARTUP_OP_RUN_SCHEDULE,
        ECS_STARTUP_RUN_SCHEDULE_DISPATCH_COUNT_SLOT,
        &mut jump_to_run_schedule_dispatch_failure_offsets,
    );
    compare_stack_slot_to_u64(
        &mut bytes,
        ECS_STARTUP_RUN_SCHEDULE_DISPATCH_COUNT_SLOT,
        1,
        &mut jump_to_run_schedule_dispatch_failure_offsets,
    );
    compare_metadata_slot_to_u64(
        &mut bytes,
        startup_payloads.run_schedule_id_offset,
        DEMO_MAIN_SCHEDULE_ID,
        &mut jump_to_run_schedule_dispatch_failure_offsets,
    );
    emit_native_query_plan_builder(&mut bytes, &mut jump_to_query_loop_scan_failure_offsets);
    emit_compiled_demo_move_query_loop(
        &mut bytes,
        &query_loop_observable,
        &mut jump_to_query_loop_scan_failure_offsets,
        &mut jump_to_query_loop_field_math_failure_offsets,
        &mut jump_to_query_loop_position_store_failure_offsets,
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

    let metadata_displacement = (bytes.len() + RUNTIME_DESTROY_SUFFIX.len() - 7) as i32;
    patch_i32(&mut bytes, 3, metadata_displacement);

    bytes
}

fn compare_stack_slot_to_u64(
    bytes: &mut Vec<u8>,
    stack_slot: u8,
    expected: u64,
    jump_offsets: &mut Vec<usize>,
) {
    bytes.extend_from_slice(&[0x48, 0xb8]); // mov rax, imm64
    bytes.extend_from_slice(&expected.to_le_bytes());
    if stack_slot == 0 {
        bytes.extend_from_slice(&[0x48, 0x39, 0x04, 0x24]); // cmp qword ptr [rsp], rax
    } else if stack_slot <= 127 {
        bytes.extend_from_slice(&[0x48, 0x39, 0x44, 0x24, stack_slot]); // cmp qword ptr [rsp + slot], rax
    } else {
        bytes.extend_from_slice(&[0x48, 0x39, 0x84, 0x24]); // cmp qword ptr [rsp + slot], rax
        bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
    }

    let jump_offset = bytes.len();
    bytes.extend_from_slice(&[0x0f, 0x85, 0x00, 0x00, 0x00, 0x00]); // jne failure
    jump_offsets.push(jump_offset);
}

fn compare_metadata_slot_to_u64(
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

fn emit_startup_operation_dispatch(
    bytes: &mut Vec<u8>,
    operation_kind_offset: i32,
    expected_kind: u32,
    dispatch_count_slot: u8,
    jump_offsets: &mut Vec<usize>,
) {
    compare_metadata_slot_to_u32(bytes, operation_kind_offset, expected_kind, jump_offsets);
    bytes.extend_from_slice(&[0xb8, 0x01, 0x00, 0x00, 0x00]); // mov eax, 1
    store_rax_to_stack_slot(bytes, dispatch_count_slot);
}

fn compare_metadata_slot_to_u32(
    bytes: &mut Vec<u8>,
    metadata_offset: i32,
    expected: u32,
    jump_offsets: &mut Vec<usize>,
) {
    bytes.extend_from_slice(&[0x81, 0xbe]); // cmp dword ptr [rsi + offset], imm32
    bytes.extend_from_slice(&metadata_offset.to_le_bytes());
    bytes.extend_from_slice(&expected.to_le_bytes());

    let jump_offset = bytes.len();
    bytes.extend_from_slice(&[0x0f, 0x85, 0x00, 0x00, 0x00, 0x00]); // jne failure
    jump_offsets.push(jump_offset);
}

fn emit_native_query_plan_builder(bytes: &mut Vec<u8>, scan_failure_offsets: &mut Vec<usize>) {
    load_stack_slot_to_rax(bytes, ECS_SPAWN_ROW_COUNT_SLOT);
    store_rax_to_stack_slot(bytes, ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT);
    compare_stack_slot_to_u64(
        bytes,
        ECS_QUERY_PLAN_MATCHED_ROW_COUNT_SLOT,
        1,
        scan_failure_offsets,
    );

    emit_lea_stack_address_to_rax(bytes, ECS_POSITION_PAYLOAD_STORAGE_SLOT);
    store_rax_to_stack_slot(bytes, ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT);
    emit_lea_stack_address_to_rax(bytes, ECS_VELOCITY_PAYLOAD_STORAGE_SLOT);
    store_rax_to_stack_slot(bytes, ECS_QUERY_PLAN_VELOCITY_PAYLOAD_ADDRESS_SLOT);
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
        1,
        scan_failure_offsets,
    );

    emit_query_loop_field_multiply(bytes);
    compare_stack_slot_to_u64(
        bytes,
        ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
        u64::from_le_bytes(query_loop_observable.field_product_payload),
        field_math_failure_offsets,
    );

    emit_query_loop_position_stores(bytes);
    compare_stack_slot_to_u64(
        bytes,
        ECS_POSITION_PAYLOAD_STORAGE_SLOT,
        u64::from_le_bytes(query_loop_observable.target_position_payload),
        position_store_failure_offsets,
    );
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

fn emit_movss_xmm_from_stack(bytes: &mut Vec<u8>, xmm_register: u8, stack_slot: u8) {
    bytes.extend_from_slice(&[0xf3, 0x0f, 0x10]);
    bytes.push(0x44 | (xmm_register << 3));
    bytes.extend_from_slice(&[0x24, stack_slot]);
}

fn emit_movss_stack_from_xmm(bytes: &mut Vec<u8>, stack_slot: u8, xmm_register: u8) {
    bytes.extend_from_slice(&[0xf3, 0x0f, 0x11]);
    bytes.push(0x44 | (xmm_register << 3));
    bytes.extend_from_slice(&[0x24, stack_slot]);
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

fn store_rax_to_stack_slot(bytes: &mut Vec<u8>, stack_slot: u8) {
    if stack_slot == 0 {
        bytes.extend_from_slice(&[0x48, 0x89, 0x04, 0x24]); // mov qword ptr [rsp], rax
    } else if stack_slot <= 127 {
        bytes.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, stack_slot]); // mov qword ptr [rsp + slot], rax
    } else {
        bytes.extend_from_slice(&[0x48, 0x89, 0x84, 0x24]); // mov qword ptr [rsp + slot], rax
        bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
    }
}

fn load_stack_slot_to_rax(bytes: &mut Vec<u8>, stack_slot: u8) {
    if stack_slot == 0 {
        bytes.extend_from_slice(&[0x48, 0x8b, 0x04, 0x24]); // mov rax, qword ptr [rsp]
    } else if stack_slot <= 127 {
        bytes.extend_from_slice(&[0x48, 0x8b, 0x44, 0x24, stack_slot]); // mov rax, qword ptr [rsp + slot]
    } else {
        bytes.extend_from_slice(&[0x48, 0x8b, 0x84, 0x24]); // mov rax, qword ptr [rsp + slot]
        bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
    }
}

fn emit_lea_stack_address_to_rax(bytes: &mut Vec<u8>, stack_slot: u8) {
    if stack_slot <= 127 {
        bytes.extend_from_slice(&[0x48, 0x8d, 0x44, 0x24, stack_slot]); // lea rax, [rsp + slot]
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

fn runtime_wrapped_payload(startup_body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(
        RUNTIME_CREATE_PREFIX.len() + startup_body.len() + RUNTIME_DESTROY_SUFFIX.len(),
    );

    bytes.extend_from_slice(RUNTIME_CREATE_PREFIX);
    bytes.extend_from_slice(startup_body);
    bytes.extend_from_slice(RUNTIME_DESTROY_SUFFIX);

    bytes
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
        message: "ECS metadata decoder executable requires a 4-byte resource payload, a two-component spawn operation, and `run Demo.Main`"
            .to_string(),
    }
}

fn native_move_query_loop_observable_error() -> CodegenError {
    CodegenError {
        message: "native query-loop observable requires the supported Demo.Move Core query loop"
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs_metadata;
    use crate::lexer;
    use crate::parser;
    use crate::runtime_assembly;

    #[test]
    fn defines_native_ecs_execution_state_layout() {
        let layout = NATIVE_ECS_EXECUTION_STATE_LAYOUT;

        assert_eq!(layout.frame_size, 232);
        assert_eq!(
            layout.zeroed_qword_offsets,
            [
                0, 8, 16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96, 104, 112, 120, 128, 136, 144,
                152, 160, 168, 176, 184, 192, 200, 208, 216, 224,
            ]
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
                position_payload: NativeEcsSlot {
                    offset: 56,
                    byte_len: 8,
                },
                velocity_payload: NativeEcsSlot {
                    offset: 64,
                    byte_len: 8,
                },
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
            layout.startup_state.position_payload,
            layout.startup_state.velocity_payload,
            layout.startup_dispatch.operation_count,
            layout.startup_dispatch.resource_dispatch_count,
            layout.startup_dispatch.spawn_dispatch_count,
            layout.startup_dispatch.run_schedule_dispatch_count,
            layout.query_plan.matched_row_count,
            layout.query_plan.position_payload_address,
            layout.query_plan.velocity_payload_address,
            layout.compiled_move.target_position_payload,
            layout.compiled_move.scanned_row_count,
            layout.compiled_move.field_product_payload,
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
                assert!(
                    left.offset + left.byte_len <= right.offset
                        || right.offset + right.byte_len <= left.offset,
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
            RUNTIME_CREATE_PREFIX,
            expected_runtime_create_prefix(&layout).as_slice()
        );
        assert_eq!(
            RUNTIME_DESTROY_SUFFIX,
            expected_runtime_destroy_suffix(&layout).as_slice()
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
    fn defines_native_move_query_loop_observable() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let core = core_lower::lower_program_to_core(&program).expect("move_system.arc lowers");
        let startup_payloads = EcsStartupPayloads {
            startup_record_count: 3,
            resource_operation_kind_offset: 577,
            resource_payload_offset: 606,
            resource_payload: [0x00, 0x00, 0x80, 0x3f],
            spawn_operation_kind_offset: 610,
            position_payload_offset: 647,
            position_payload: [0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40],
            velocity_payload_offset: 684,
            velocity_payload: [0x00, 0x00, 0x40, 0x40, 0x00, 0x00, 0x80, 0x40],
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

        let mut run_schedule_check = vec![0x48, 0xb8];
        run_schedule_check.extend_from_slice(&DEMO_MAIN_SCHEDULE_ID.to_le_bytes());
        run_schedule_check.extend_from_slice(&[0x48, 0x39, 0x86]);
        run_schedule_check.extend_from_slice(&696_i32.to_le_bytes());
        run_schedule_check.extend_from_slice(&[0x0f, 0x85]);
        assert!(
            contains_subsequence(&text, &run_schedule_check),
            "generated text should read and validate startup run Demo.Main"
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
                &metadata_dword_compare_sequence(577, ECS_STARTUP_OP_RESOURCE_PAYLOAD),
            ),
            "generated text should check the resource operation kind at runtime"
        );
        assert!(
            contains_subsequence(
                &text,
                &metadata_dword_compare_sequence(610, ECS_STARTUP_OP_SPAWN),
            ),
            "generated text should check the spawn operation kind at runtime"
        );
        assert!(
            contains_subsequence(
                &text,
                &metadata_dword_compare_sequence(692, ECS_STARTUP_OP_RUN_SCHEDULE),
            ),
            "generated text should check the run schedule operation kind at runtime"
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
                &metadata_dword_disp32_load_dword_store_sequence(
                    startup_payloads.resource_payload_offset,
                    ECS_RESOURCE_PAYLOAD_STORAGE_SLOT,
                ),
            ),
            "generated text should preserve the resource payload handler"
        );
        assert!(
            contains_subsequence(&text, &compare_stack_slot_sequence(48, 1)),
            "generated text should preserve the spawn row-count handler"
        );
        assert!(
            contains_subsequence(
                &text,
                &metadata_u64_compare_sequence(696, DEMO_MAIN_SCHEDULE_ID)
            ),
            "generated text should preserve run Demo.Main validation"
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
    fn materializes_native_query_planning_state() {
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
                    ECS_SPAWN_ROW_COUNT_SLOT,
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
                &lea_stack_address_store_sequence(
                    ECS_POSITION_PAYLOAD_STORAGE_SLOT,
                    ECS_QUERY_PLAN_POSITION_PAYLOAD_ADDRESS_SLOT,
                ),
            ),
            "generated text should materialize the planned Position payload address"
        );
        assert!(
            contains_subsequence(
                &text,
                &lea_stack_address_store_sequence(
                    ECS_VELOCITY_PAYLOAD_STORAGE_SLOT,
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

    fn contains_subsequence(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    fn metadata_dword_load_store_sequence(metadata_offset: u8, stack_slot: u8) -> Vec<u8> {
        let mut bytes = vec![0x8b, 0x46, metadata_offset]; // mov eax, dword ptr [rsi + offset]
        append_rax_qword_store(&mut bytes, stack_slot);
        bytes
    }

    fn metadata_dword_disp32_load_dword_store_sequence(
        metadata_offset: i32,
        stack_slot: u8,
    ) -> Vec<u8> {
        let mut bytes = vec![0x8b, 0x86]; // mov eax, dword ptr [rsi + offset]
        bytes.extend_from_slice(&metadata_offset.to_le_bytes());
        bytes.extend_from_slice(&[0x89, 0x44, 0x24, stack_slot]);
        bytes
    }

    fn metadata_dword_compare_sequence(metadata_offset: i32, expected: u32) -> Vec<u8> {
        let mut bytes = vec![0x81, 0xbe]; // cmp dword ptr [rsi + offset], imm32
        bytes.extend_from_slice(&metadata_offset.to_le_bytes());
        bytes.extend_from_slice(&expected.to_le_bytes());
        bytes.extend_from_slice(&[0x0f, 0x85]); // jne failure
        bytes
    }

    fn metadata_u64_compare_sequence(metadata_offset: i32, expected: u64) -> Vec<u8> {
        let mut bytes = vec![0x48, 0xb8]; // mov rax, imm64
        bytes.extend_from_slice(&expected.to_le_bytes());
        bytes.extend_from_slice(&[0x48, 0x39, 0x86]); // cmp qword ptr [rsi + offset], rax
        bytes.extend_from_slice(&metadata_offset.to_le_bytes());
        bytes.extend_from_slice(&[0x0f, 0x85]); // jne failure
        bytes
    }

    fn mov_eax_one_store_sequence(stack_slot: u8) -> Vec<u8> {
        let mut bytes = vec![0xb8, 0x01, 0x00, 0x00, 0x00]; // mov eax, 1
        append_rax_qword_store(&mut bytes, stack_slot);
        bytes
    }

    fn load_store_stack_slot_sequence(load_slot: u8, store_slot: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, load_slot);
        append_rax_qword_store(&mut bytes, store_slot);
        bytes
    }

    fn lea_stack_address_store_sequence(source_slot: u8, store_slot: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_lea_stack_address_to_rax(&mut bytes, source_slot);
        append_rax_qword_store(&mut bytes, store_slot);
        bytes
    }

    fn query_plan_component_field_multiply_sequence(
        address_slot: u8,
        field_offset: u8,
        product_slot: u8,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, address_slot);
        append_movss_xmm_from_rax(&mut bytes, 0, field_offset);
        bytes.extend_from_slice(&[
            0xf3,
            0x0f,
            0x10,
            0x4c,
            0x24,
            ECS_RESOURCE_PAYLOAD_STORAGE_SLOT, // movss xmm1, dword ptr [rsp + 40]
            0xf3,
            0x0f,
            0x59,
            0xc1, // mulss xmm0, xmm1
            0xf3,
            0x0f,
            0x11,
            0x44,
            0x24,
            product_slot, // movss dword ptr [rsp + product slot], xmm0
        ]);
        bytes
    }

    fn query_plan_position_store_sequence(
        address_slot: u8,
        field_offset: u8,
        product_slot: u8,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_load_stack_slot_to_rax(&mut bytes, address_slot);
        append_movss_xmm_from_rax(&mut bytes, 0, field_offset);
        bytes.extend_from_slice(&[
            0xf3,
            0x0f,
            0x10,
            0x4c,
            0x24,
            product_slot, // movss xmm1, dword ptr [rsp + product slot]
            0xf3,
            0x0f,
            0x58,
            0xc1, // addss xmm0, xmm1
        ]);
        append_movss_rax_from_xmm(&mut bytes, field_offset, 0);
        bytes
    }

    fn compare_stack_slot_sequence(stack_slot: u8, expected: u64) -> Vec<u8> {
        let mut bytes = vec![0x48, 0xb8]; // mov rax, imm64
        bytes.extend_from_slice(&expected.to_le_bytes());
        if stack_slot == 0 {
            bytes.extend_from_slice(&[0x48, 0x39, 0x04, 0x24]);
        } else if stack_slot <= 127 {
            bytes.extend_from_slice(&[0x48, 0x39, 0x44, 0x24, stack_slot]);
        } else {
            bytes.extend_from_slice(&[0x48, 0x39, 0x84, 0x24]);
            bytes.extend_from_slice(&(stack_slot as u32).to_le_bytes());
        }
        bytes
    }

    fn expected_runtime_create_prefix(layout: &NativeEcsExecutionStateLayout) -> Vec<u8> {
        let mut bytes = Vec::new();
        if layout.frame_size <= 127 {
            bytes.extend_from_slice(&[0x48, 0x83, 0xec, layout.frame_size]);
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
            bytes.extend_from_slice(&[0x48, 0x83, 0xc4, layout.frame_size]);
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

    fn append_zero_qword_store(bytes: &mut Vec<u8>, offset: u8) {
        append_rax_qword_store(bytes, offset);
    }

    fn append_rax_qword_store(bytes: &mut Vec<u8>, offset: u8) {
        if offset == 0 {
            bytes.extend_from_slice(&[0x48, 0x89, 0x04, 0x24]);
        } else if offset <= 127 {
            bytes.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, offset]);
        } else {
            bytes.extend_from_slice(&[0x48, 0x89, 0x84, 0x24]);
            bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        }
    }

    fn append_load_stack_slot_to_rax(bytes: &mut Vec<u8>, offset: u8) {
        if offset == 0 {
            bytes.extend_from_slice(&[0x48, 0x8b, 0x04, 0x24]);
        } else if offset <= 127 {
            bytes.extend_from_slice(&[0x48, 0x8b, 0x44, 0x24, offset]);
        } else {
            bytes.extend_from_slice(&[0x48, 0x8b, 0x84, 0x24]);
            bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        }
    }

    fn append_lea_stack_address_to_rax(bytes: &mut Vec<u8>, offset: u8) {
        if offset <= 127 {
            bytes.extend_from_slice(&[0x48, 0x8d, 0x44, 0x24, offset]);
        } else {
            bytes.extend_from_slice(&[0x48, 0x8d, 0x84, 0x24]);
            bytes.extend_from_slice(&(offset as u32).to_le_bytes());
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
