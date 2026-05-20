use crate::core::{
    CoreProgram, CoreQueryAccess, CoreSystemBinaryOp, CoreSystemExpression, CoreSystemParamKind,
    CoreSystemPlace, CoreSystemStatement,
};
use crate::core_lower;
use crate::parser::{BinaryOperator, Expression, Program, Statement};

const RUNTIME_CREATE_PREFIX: &[u8] = &[
    0x48, 0x83, 0xec, 0x60, // sub rsp, 96
    0x31, 0xc0, // xor eax, eax
    0x48, 0x89, 0x04, 0x24, // mov qword ptr [rsp], rax
    0x48, 0x89, 0x44, 0x24, 0x08, // mov qword ptr [rsp + 8], rax
    0x48, 0x89, 0x44, 0x24, 0x10, // mov qword ptr [rsp + 16], rax
    0x48, 0x89, 0x44, 0x24, 0x18, // mov qword ptr [rsp + 24], rax
    0x48, 0x89, 0x44, 0x24, 0x20, // mov qword ptr [rsp + 32], rax
    0x48, 0x89, 0x44, 0x24, 0x28, // mov qword ptr [rsp + 40], rax
    0x48, 0x89, 0x44, 0x24, 0x30, // mov qword ptr [rsp + 48], rax
    0x48, 0x89, 0x44, 0x24, 0x38, // mov qword ptr [rsp + 56], rax
    0x48, 0x89, 0x44, 0x24, 0x40, // mov qword ptr [rsp + 64], rax
    0x48, 0x89, 0x44, 0x24, 0x48, // mov qword ptr [rsp + 72], rax
    0x48, 0x89, 0x44, 0x24, 0x50, // mov qword ptr [rsp + 80], rax
    0x48, 0x89, 0x44, 0x24, 0x58, // mov qword ptr [rsp + 88], rax
];

const RUNTIME_DESTROY_SUFFIX: &[u8] = &[
    0x31, 0xc0, // xor eax, eax
    0x48, 0x89, 0x04, 0x24, // mov qword ptr [rsp], rax
    0x48, 0x89, 0x44, 0x24, 0x08, // mov qword ptr [rsp + 8], rax
    0x48, 0x89, 0x44, 0x24, 0x10, // mov qword ptr [rsp + 16], rax
    0x48, 0x89, 0x44, 0x24, 0x18, // mov qword ptr [rsp + 24], rax
    0x48, 0x89, 0x44, 0x24, 0x20, // mov qword ptr [rsp + 32], rax
    0x48, 0x89, 0x44, 0x24, 0x28, // mov qword ptr [rsp + 40], rax
    0x48, 0x89, 0x44, 0x24, 0x30, // mov qword ptr [rsp + 48], rax
    0x48, 0x89, 0x44, 0x24, 0x38, // mov qword ptr [rsp + 56], rax
    0x48, 0x89, 0x44, 0x24, 0x40, // mov qword ptr [rsp + 64], rax
    0x48, 0x89, 0x44, 0x24, 0x48, // mov qword ptr [rsp + 72], rax
    0x48, 0x89, 0x44, 0x24, 0x50, // mov qword ptr [rsp + 80], rax
    0x48, 0x89, 0x44, 0x24, 0x58, // mov qword ptr [rsp + 88], rax
    0x48, 0x83, 0xc4, 0x60, // add rsp, 96
    0xb8, 0x3c, 0x00, 0x00, 0x00, // mov eax, 60
    0x0f, 0x05, // syscall
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
const ECS_QUERY_LOOP_POSITION_STORE_SUCCESS_EXIT_CODE: u8 = 46;
const ECS_STARTUP_SECTION_DIRECTORY_OFFSET: usize = 16 + 5 * 16;
const ECS_SECTION_OFFSET_FIELD_OFFSET: usize = 4;
const ECS_SECTION_RECORD_COUNT_FIELD_OFFSET: usize = 12;
const ECS_STARTUP_OP_RESOURCE_PAYLOAD: u32 = 1;
const ECS_STARTUP_OP_SPAWN: u32 = 2;
const ECS_EXPECTED_DESCRIPTOR_COUNTS: [u64; 5] = [2, 1, 1, 1, 1];
const ECS_DESCRIPTOR_RECORD_COUNT_OFFSETS: [u8; 5] = [28, 44, 60, 76, 92];
const ECS_DESCRIPTOR_REGISTRY_SLOTS: [u8; 5] = [0, 8, 16, 24, 32];
const ECS_RESOURCE_PAYLOAD_STORAGE_SLOT: u8 = 40;
const ECS_SPAWN_ROW_COUNT_SLOT: u8 = 48;
const ECS_POSITION_PAYLOAD_STORAGE_SLOT: u8 = 56;
const ECS_VELOCITY_PAYLOAD_STORAGE_SLOT: u8 = 64;
const ECS_QUERY_LOOP_TARGET_POSITION_SLOT: u8 = 72;
const ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT: u8 = 80;
const ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT: u8 = 88;

const DEMO_POSITION_COMPONENT_ID: u64 = 0x002202c6aeb4f27b;
const DEMO_VELOCITY_COMPONENT_ID: u64 = 0x2cf8a68bcb7f913b;
const DEMO_TIME_RESOURCE_ID: u64 = 0x7924ce11db524521;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EcsStartupPayloads {
    resource_payload_offset: i32,
    resource_payload: [u8; 4],
    position_payload_offset: i32,
    position_payload: [u8; 8],
    velocity_payload_offset: i32,
    velocity_payload: [u8; 8],
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeMoveQueryLoopObservable {
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
    let target_position_payload = target_position_payload(startup_payloads);
    let field_product_payload = field_product_payload(startup_payloads);

    Ok(NativeMoveQueryLoopObservable {
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

    if startup_record_count < 2 {
        return Err(metadata_startup_payload_error());
    }

    let mut offset = startup_section_offset;
    let resource_payload = parse_resource_payload_operation(metadata_payload, &mut offset)?;
    let (position_payload, velocity_payload) =
        parse_spawn_operation(metadata_payload, &mut offset)?;

    Ok(EcsStartupPayloads {
        resource_payload_offset: resource_payload.0,
        resource_payload: resource_payload.1,
        position_payload_offset: position_payload.0,
        position_payload: position_payload.1,
        velocity_payload_offset: velocity_payload.0,
        velocity_payload: velocity_payload.1,
    })
}

fn parse_resource_payload_operation(
    metadata_payload: &[u8],
    offset: &mut usize,
) -> Result<(i32, [u8; 4]), CodegenError> {
    let operation_kind = read_metadata_u32(metadata_payload, *offset)?;
    *offset += 4;

    if operation_kind != ECS_STARTUP_OP_RESOURCE_PAYLOAD {
        return Err(metadata_startup_payload_error());
    }

    checked_metadata_range(metadata_payload, *offset, 8)?;
    *offset += 8;
    skip_metadata_string(metadata_payload, offset)?;

    parse_payload_offset_and_bytes(metadata_payload, offset)
}

fn parse_spawn_operation(
    metadata_payload: &[u8],
    offset: &mut usize,
) -> Result<((i32, [u8; 8]), (i32, [u8; 8])), CodegenError> {
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

    Ok((position_payload, velocity_payload))
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

    bytes.extend_from_slice(&[0x8b, 0x86]); // mov eax, dword ptr [rsi + offset]
    bytes.extend_from_slice(&startup_payloads.resource_payload_offset.to_le_bytes());
    bytes.extend_from_slice(&[
        0x89,
        0x44,
        0x24,
        ECS_RESOURCE_PAYLOAD_STORAGE_SLOT, // mov dword ptr [rsp + 40], eax
    ]);

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

    bytes.extend_from_slice(&[0x48, 0xb8]); // mov rax, target Position payload
    bytes.extend_from_slice(&query_loop_observable.target_position_payload);
    store_rax_to_stack_slot(&mut bytes, ECS_QUERY_LOOP_TARGET_POSITION_SLOT);
    compare_stack_slot_to_u64(
        &mut bytes,
        ECS_QUERY_LOOP_TARGET_POSITION_SLOT,
        u64::from_le_bytes(query_loop_observable.target_position_payload),
        &mut jump_to_startup_state_failure_offsets,
    );

    load_stack_slot_to_rax(&mut bytes, ECS_SPAWN_ROW_COUNT_SLOT);
    store_rax_to_stack_slot(&mut bytes, ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT);
    compare_stack_slot_to_u64(
        &mut bytes,
        ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT,
        1,
        &mut jump_to_query_loop_scan_failure_offsets,
    );

    emit_query_loop_field_multiply(&mut bytes);
    compare_stack_slot_to_u64(
        &mut bytes,
        ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT,
        u64::from_le_bytes(query_loop_observable.field_product_payload),
        &mut jump_to_query_loop_field_math_failure_offsets,
    );

    emit_query_loop_position_stores(&mut bytes);
    compare_stack_slot_to_u64(
        &mut bytes,
        ECS_POSITION_PAYLOAD_STORAGE_SLOT,
        u64::from_le_bytes(query_loop_observable.target_position_payload),
        &mut jump_to_query_loop_position_store_failure_offsets,
    );

    move_edi_exit_code(&mut bytes, ECS_QUERY_LOOP_POSITION_STORE_SUCCESS_EXIT_CODE);

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
    } else {
        bytes.extend_from_slice(&[0x48, 0x39, 0x44, 0x24, stack_slot]); // cmp qword ptr [rsp + slot], rax
    }

    let jump_offset = bytes.len();
    bytes.extend_from_slice(&[0x0f, 0x85, 0x00, 0x00, 0x00, 0x00]); // jne failure
    jump_offsets.push(jump_offset);
}

fn emit_query_loop_field_multiply(bytes: &mut Vec<u8>) {
    emit_movss_xmm_from_stack(bytes, 0, ECS_VELOCITY_PAYLOAD_STORAGE_SLOT);
    emit_movss_xmm_from_stack(bytes, 1, ECS_RESOURCE_PAYLOAD_STORAGE_SLOT);
    emit_mulss_xmm(bytes, 0, 1);
    emit_movss_stack_from_xmm(bytes, ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT, 0);

    emit_movss_xmm_from_stack(bytes, 0, ECS_VELOCITY_PAYLOAD_STORAGE_SLOT + 4);
    emit_movss_xmm_from_stack(bytes, 1, ECS_RESOURCE_PAYLOAD_STORAGE_SLOT);
    emit_mulss_xmm(bytes, 0, 1);
    emit_movss_stack_from_xmm(bytes, ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT + 4, 0);
}

fn emit_query_loop_position_stores(bytes: &mut Vec<u8>) {
    emit_movss_xmm_from_stack(bytes, 0, ECS_POSITION_PAYLOAD_STORAGE_SLOT);
    emit_movss_xmm_from_stack(bytes, 1, ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT);
    emit_addss_xmm(bytes, 0, 1);
    emit_movss_stack_from_xmm(bytes, ECS_POSITION_PAYLOAD_STORAGE_SLOT, 0);

    emit_movss_xmm_from_stack(bytes, 0, ECS_POSITION_PAYLOAD_STORAGE_SLOT + 4);
    emit_movss_xmm_from_stack(bytes, 1, ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT + 4);
    emit_addss_xmm(bytes, 0, 1);
    emit_movss_stack_from_xmm(bytes, ECS_POSITION_PAYLOAD_STORAGE_SLOT + 4, 0);
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
    } else {
        bytes.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, stack_slot]); // mov qword ptr [rsp + slot], rax
    }
}

fn load_stack_slot_to_rax(bytes: &mut Vec<u8>, stack_slot: u8) {
    if stack_slot == 0 {
        bytes.extend_from_slice(&[0x48, 0x8b, 0x04, 0x24]); // mov rax, qword ptr [rsp]
    } else {
        bytes.extend_from_slice(&[0x48, 0x8b, 0x44, 0x24, stack_slot]); // mov rax, qword ptr [rsp + slot]
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
        message: "ECS metadata decoder executable requires a 4-byte resource payload followed by a two-component spawn operation"
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
    fn defines_native_move_query_loop_observable() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let core = core_lower::lower_program_to_core(&program).expect("move_system.arc lowers");
        let startup_payloads = EcsStartupPayloads {
            resource_payload_offset: 609,
            resource_payload: [0x00, 0x00, 0x80, 0x3f],
            position_payload_offset: 653,
            position_payload: [0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40],
            velocity_payload_offset: 687,
            velocity_payload: [0x00, 0x00, 0x40, 0x40, 0x00, 0x00, 0x80, 0x40],
        };

        let observable = native_move_query_loop_observable_from_core(&core, &startup_payloads)
            .expect("native query-loop observable is defined");

        assert_eq!(
            observable,
            NativeMoveQueryLoopObservable {
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
                &[
                    0x48,
                    0x8b,
                    0x44,
                    0x24,
                    ECS_SPAWN_ROW_COUNT_SLOT, // mov rax, qword ptr [rsp + 48]
                    0x48,
                    0x89,
                    0x44,
                    0x24,
                    ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT, // mov qword ptr [rsp + 80], rax
                ],
            ),
            "generated text should carry the row count into the scan-count slot"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0x48,
                    0xb8,
                    0x01,
                    0x00,
                    0x00,
                    0x00,
                    0x00,
                    0x00,
                    0x00,
                    0x00,
                    0x48,
                    0x39,
                    0x44,
                    0x24,
                    ECS_QUERY_LOOP_SCANNED_ROW_COUNT_SLOT, // cmp qword ptr [rsp + 80], rax
                ],
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
                &[
                    0xf3,
                    0x0f,
                    0x10,
                    0x44,
                    0x24,
                    ECS_VELOCITY_PAYLOAD_STORAGE_SLOT, // movss xmm0, dword ptr [rsp + 64]
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
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT, // movss dword ptr [rsp + 88], xmm0
                ],
            ),
            "generated text should compute Velocity.x * Time.delta"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xf3,
                    0x0f,
                    0x10,
                    0x44,
                    0x24,
                    ECS_VELOCITY_PAYLOAD_STORAGE_SLOT + 4, // movss xmm0, dword ptr [rsp + 68]
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
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT + 4, // movss dword ptr [rsp + 92], xmm0
                ],
            ),
            "generated text should compute Velocity.y * Time.delta"
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
                &[
                    0xf3,
                    0x0f,
                    0x10,
                    0x44,
                    0x24,
                    ECS_POSITION_PAYLOAD_STORAGE_SLOT, // movss xmm0, dword ptr [rsp + 56]
                    0xf3,
                    0x0f,
                    0x10,
                    0x4c,
                    0x24,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT, // movss xmm1, dword ptr [rsp + 88]
                    0xf3,
                    0x0f,
                    0x58,
                    0xc1, // addss xmm0, xmm1
                    0xf3,
                    0x0f,
                    0x11,
                    0x44,
                    0x24,
                    ECS_POSITION_PAYLOAD_STORAGE_SLOT, // movss dword ptr [rsp + 56], xmm0
                ],
            ),
            "generated text should update Position.x from its computed product"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xf3,
                    0x0f,
                    0x10,
                    0x44,
                    0x24,
                    ECS_POSITION_PAYLOAD_STORAGE_SLOT + 4, // movss xmm0, dword ptr [rsp + 60]
                    0xf3,
                    0x0f,
                    0x10,
                    0x4c,
                    0x24,
                    ECS_QUERY_LOOP_FIELD_PRODUCT_SLOT + 4, // movss xmm1, dword ptr [rsp + 92]
                    0xf3,
                    0x0f,
                    0x58,
                    0xc1, // addss xmm0, xmm1
                    0xf3,
                    0x0f,
                    0x11,
                    0x44,
                    0x24,
                    ECS_POSITION_PAYLOAD_STORAGE_SLOT + 4, // movss dword ptr [rsp + 60], xmm0
                ],
            ),
            "generated text should update Position.y from its computed product"
        );
        assert!(
            contains_subsequence(
                &text,
                &[
                    0xbf,
                    ECS_QUERY_LOOP_POSITION_STORE_SUCCESS_EXIT_CODE,
                    0x00,
                    0x00,
                    0x00
                ],
            ),
            "generated text should expose the M18-004 Position-store success code"
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

    fn contains_subsequence(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }
}
