use crate::parser::{BinaryOperator, Expression, Program, Statement};

const RUNTIME_CREATE_PREFIX: &[u8] = &[
    0x48, 0x83, 0xec, 0x18, // sub rsp, 24
    0x31, 0xc0, // xor eax, eax
    0x48, 0x89, 0x04, 0x24, // mov qword ptr [rsp], rax
    0x48, 0x89, 0x44, 0x24, 0x08, // mov qword ptr [rsp + 8], rax
    0x48, 0x89, 0x44, 0x24, 0x10, // mov qword ptr [rsp + 16], rax
];

const RUNTIME_DESTROY_SUFFIX: &[u8] = &[
    0x31, 0xc0, // xor eax, eax
    0x48, 0x89, 0x04, 0x24, // mov qword ptr [rsp], rax
    0x48, 0x89, 0x44, 0x24, 0x08, // mov qword ptr [rsp + 8], rax
    0x48, 0x89, 0x44, 0x24, 0x10, // mov qword ptr [rsp + 16], rax
    0x48, 0x83, 0xc4, 0x18, // add rsp, 24
    0xb8, 0x3c, 0x00, 0x00, 0x00, // mov eax, 60
    0x0f, 0x05, // syscall
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodegenError {
    pub message: String,
}

const ECS_METADATA_ENVELOPE_SIZE: usize = 112;
const ECS_METADATA_FAILURE_EXIT_CODE: u8 = 16;

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

    let startup_body = ecs_metadata_decoder_body(&metadata_payload[..ECS_METADATA_ENVELOPE_SIZE]);
    Ok(runtime_wrapped_payload(&startup_body))
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

fn ecs_metadata_decoder_body(envelope: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut jump_to_failure_offsets = Vec::new();

    bytes.extend_from_slice(&[0x48, 0x8d, 0x35, 0x00, 0x00, 0x00, 0x00]); // lea rsi, [rip + metadata]

    for (index, chunk) in envelope.chunks_exact(8).enumerate() {
        bytes.extend_from_slice(&[0x48, 0xb8]); // mov rax, imm64
        bytes.extend_from_slice(chunk);
        bytes.extend_from_slice(&[0x48, 0x39, 0x46, (index * 8) as u8]); // cmp [rsi + offset], rax

        let jump_offset = bytes.len();
        bytes.extend_from_slice(&[0x0f, 0x85, 0x00, 0x00, 0x00, 0x00]); // jne failure
        jump_to_failure_offsets.push(jump_offset);
    }

    bytes.extend_from_slice(&[0xbf, 0x00, 0x00, 0x00, 0x00]); // mov edi, 0
    let jump_to_done_offset = bytes.len();
    bytes.extend_from_slice(&[0xe9, 0x00, 0x00, 0x00, 0x00]); // jmp done

    let failure_offset = bytes.len();
    bytes.extend_from_slice(&[
        0xbf,
        ECS_METADATA_FAILURE_EXIT_CODE,
        0x00,
        0x00,
        0x00, // mov edi, failure
    ]);
    let done_offset = bytes.len();

    for jump_offset in jump_to_failure_offsets {
        patch_rel32(&mut bytes, jump_offset + 2, failure_offset, jump_offset + 6);
    }
    patch_rel32(
        &mut bytes,
        jump_to_done_offset + 1,
        done_offset,
        jump_to_done_offset + 5,
    );

    let metadata_displacement = (bytes.len() + RUNTIME_DESTROY_SUFFIX.len() - 7) as i32;
    patch_i32(&mut bytes, 3, metadata_displacement);

    bytes
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
