use std::collections::HashMap;
use std::fmt::Write;

use crate::core::{
    CoreBinaryOp, CoreFunction, CoreInstruction, CoreProgram, CoreSpawnFieldValue, CoreTerminator,
    CoreType, LocalId, ValueId,
};

pub fn format_core_program(program: &CoreProgram) -> String {
    let mut formatter = CoreFormatter::new();
    formatter.format_program(program);
    formatter.output
}

struct CoreFormatter {
    output: String,
}

impl CoreFormatter {
    fn new() -> Self {
        Self {
            output: String::new(),
        }
    }

    fn format_program(&mut self, program: &CoreProgram) {
        let _ = write!(self.output, "world {}", program.world.name);

        for function in &program.functions {
            self.format_function(function);
        }
    }

    fn format_function(&mut self, function: &CoreFunction) {
        let local_names = function
            .locals
            .iter()
            .map(|local| (local.id, local.name.as_str()))
            .collect::<HashMap<_, _>>();

        let _ = write!(self.output, "\n\nfunction {} {{", function.name);

        for local in &function.locals {
            let _ = write!(
                self.output,
                "\n  local {}: {}",
                local.name,
                format_type(local.ty)
            );
        }

        for block in &function.blocks {
            for instruction in &block.instructions {
                self.format_instruction(instruction, &local_names);
            }

            self.format_terminator(&block.terminator);
        }

        self.output.push_str("\n}");
    }

    fn format_instruction(
        &mut self,
        instruction: &CoreInstruction,
        local_names: &HashMap<LocalId, &str>,
    ) {
        match instruction {
            CoreInstruction::Spawn { components } => {
                let _ = write!(self.output, "\n  spawn");
                for component in components {
                    let _ = write!(
                        self.output,
                        "\n    component {} id 0x{:016x}",
                        component.name, component.component_id
                    );
                    for field in &component.fields {
                        let _ = write!(
                            self.output,
                            "\n      field {} = {}",
                            field.name,
                            format_spawn_field_value(&field.value)
                        );
                    }
                }
            }
            CoreInstruction::I32Const { result, value } => {
                let _ = write!(
                    self.output,
                    "\n  {} = i32.const {}",
                    format_value(*result),
                    value
                );
            }
            CoreInstruction::I32Binary {
                result,
                op,
                left,
                right,
            } => {
                let _ = write!(
                    self.output,
                    "\n  {} = {} {}, {}",
                    format_value(*result),
                    format_binary_op(*op),
                    format_value(*left),
                    format_value(*right)
                );
            }
            CoreInstruction::LocalStore { local, value } => {
                let _ = write!(
                    self.output,
                    "\n  local.store {}, {}",
                    format_local(*local, local_names),
                    format_value(*value)
                );
            }
            CoreInstruction::LocalLoad { result, local } => {
                let _ = write!(
                    self.output,
                    "\n  {} = local.load {}",
                    format_value(*result),
                    format_local(*local, local_names)
                );
            }
        }
    }

    fn format_terminator(&mut self, terminator: &CoreTerminator) {
        match terminator {
            CoreTerminator::Exit { value } => {
                let _ = write!(self.output, "\n  exit {}", format_value(*value));
            }
        }
    }
}

fn format_value(value: ValueId) -> String {
    format!("%{}", value.0)
}

fn format_local(local: LocalId, local_names: &HashMap<LocalId, &str>) -> String {
    local_names
        .get(&local)
        .copied()
        .map(str::to_string)
        .unwrap_or_else(|| format!("<local {}>", local.0))
}

fn format_type(ty: CoreType) -> &'static str {
    match ty {
        CoreType::I32 => "i32",
    }
}

fn format_binary_op(op: CoreBinaryOp) -> &'static str {
    match op {
        CoreBinaryOp::Add => "i32.add",
        CoreBinaryOp::Subtract => "i32.sub",
        CoreBinaryOp::Multiply => "i32.mul",
    }
}

fn format_spawn_field_value(value: &CoreSpawnFieldValue) -> String {
    match value {
        CoreSpawnFieldValue::F32Bits(bits) => format!("f32.bits 0x{bits:08x}"),
    }
}
