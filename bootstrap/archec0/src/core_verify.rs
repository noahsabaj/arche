#![allow(dead_code)]

use std::collections::HashSet;

use crate::core::{
    CoreBlock, CoreFunction, CoreInstruction, CoreProgram, CoreTerminator, LocalId, ValueId,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreVerifyError {
    pub message: String,
}

pub fn verify_core_program(program: &CoreProgram) -> Result<(), CoreVerifyError> {
    for function in &program.functions {
        verify_function(function)?;
    }

    Ok(())
}

fn verify_function(function: &CoreFunction) -> Result<(), CoreVerifyError> {
    let mut block_ids = HashSet::new();
    for block in &function.blocks {
        if !block_ids.insert(block.id) {
            return Err(verify_error(format!("duplicate block id {}", block.id.0)));
        }
    }

    if !block_ids.contains(&function.entry) {
        return Err(verify_error(format!(
            "entry block {} does not exist in function `{}`",
            function.entry.0, function.name
        )));
    }

    let mut local_ids = HashSet::new();
    for local in &function.locals {
        if !local_ids.insert(local.id) {
            return Err(verify_error(format!("duplicate local id {}", local.id.0)));
        }
    }

    for block in &function.blocks {
        verify_block(block, &local_ids)?;
    }

    Ok(())
}

fn verify_block(block: &CoreBlock, local_ids: &HashSet<LocalId>) -> Result<(), CoreVerifyError> {
    let mut defined_values = HashSet::new();

    for instruction in &block.instructions {
        match instruction {
            CoreInstruction::Spawn { .. } => {}
            CoreInstruction::I32Const { result, .. } => {
                define_value(&mut defined_values, *result)?;
            }
            CoreInstruction::I32Binary {
                result,
                left,
                right,
                ..
            } => {
                require_value(&defined_values, *left)?;
                require_value(&defined_values, *right)?;
                define_value(&mut defined_values, *result)?;
            }
            CoreInstruction::LocalStore { local, value } => {
                require_local(local_ids, *local)?;
                require_value(&defined_values, *value)?;
            }
            CoreInstruction::LocalLoad { result, local } => {
                require_local(local_ids, *local)?;
                define_value(&mut defined_values, *result)?;
            }
        }
    }

    match block.terminator {
        CoreTerminator::Exit { value } => require_value(&defined_values, value),
    }
}

fn define_value(
    defined_values: &mut HashSet<ValueId>,
    value: ValueId,
) -> Result<(), CoreVerifyError> {
    if defined_values.insert(value) {
        Ok(())
    } else {
        Err(verify_error(format!("duplicate value {}", value.0)))
    }
}

fn require_value(defined_values: &HashSet<ValueId>, value: ValueId) -> Result<(), CoreVerifyError> {
    if defined_values.contains(&value) {
        Ok(())
    } else {
        Err(verify_error(format!("undefined value {}", value.0)))
    }
}

fn require_local(local_ids: &HashSet<LocalId>, local: LocalId) -> Result<(), CoreVerifyError> {
    if local_ids.contains(&local) {
        Ok(())
    } else {
        Err(verify_error(format!("undefined local {}", local.0)))
    }
}

fn verify_error(message: impl Into<String>) -> CoreVerifyError {
    CoreVerifyError {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        BlockId, CoreBinaryOp, CoreBlock, CoreFunction, CoreInstruction, CoreLocal, CoreProgram,
        CoreTerminator, CoreType, CoreWorld,
    };
    use crate::core_lower;
    use crate::lexer;
    use crate::parser;

    #[test]
    fn core_verifier_accepts_lowered_math() {
        let source = include_str!("../../../examples/math.arc");
        let tokens = lexer::lex(source).expect("math.arc lexes");
        let ast = parser::parse_program(&tokens).expect("math.arc parses");
        let core = core_lower::lower_program_to_core(&ast).expect("math.arc lowers to Core");

        verify_core_program(&core).expect("lowered math Core verifies");
    }

    #[test]
    fn core_verifier_rejects_invalid_value_reference() {
        let program = CoreProgram {
            world: CoreWorld {
                name: "Main".to_string(),
            },
            systems: vec![],
            functions: vec![CoreFunction {
                name: "startup".to_string(),
                entry: BlockId(0),
                locals: vec![CoreLocal {
                    id: LocalId(0),
                    name: "x".to_string(),
                    ty: CoreType::I32,
                }],
                blocks: vec![CoreBlock {
                    id: BlockId(0),
                    instructions: vec![
                        CoreInstruction::I32Const {
                            result: ValueId(0),
                            value: 40,
                        },
                        CoreInstruction::I32Binary {
                            result: ValueId(1),
                            op: CoreBinaryOp::Add,
                            left: ValueId(0),
                            right: ValueId(99),
                        },
                    ],
                    terminator: CoreTerminator::Exit { value: ValueId(1) },
                }],
            }],
        };

        let error = verify_core_program(&program).expect_err("undefined value reference must fail");
        assert!(
            error.message.contains("undefined value"),
            "unexpected verifier error: {}",
            error.message
        );
    }
}
