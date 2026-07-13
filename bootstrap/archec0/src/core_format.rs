use std::collections::HashMap;
use std::fmt::Write;

use crate::core::{
    CoreBinaryOp, CoreFunction, CoreInstruction, CoreProgram, CoreQueryAccess, CoreQueryLoop,
    CoreSpawnFieldValue, CoreSystem, CoreSystemBinaryOp, CoreSystemExpression, CoreSystemParam,
    CoreSystemParamKind, CoreSystemPlace, CoreSystemStatement, CoreTerminator, CoreType, LocalId,
    ValueId,
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

        for system in &program.systems {
            if !system.body.statements.is_empty() {
                self.format_system(system);
            }
        }

        for function in &program.functions {
            self.format_function(function);
        }
    }

    fn format_system(&mut self, system: &CoreSystem) {
        let _ = write!(self.output, "\n\nsystem {} {{", system.name);

        for param in &system.params {
            self.format_system_param(param);
        }

        for statement in &system.body.statements {
            self.format_system_statement(statement, 2);
        }

        self.output.push_str("\n}");
    }

    fn format_system_param(&mut self, param: &CoreSystemParam) {
        match &param.kind {
            CoreSystemParamKind::ReadResource { resource_id, name } => {
                let _ = write!(
                    self.output,
                    "\n  param {}: read {} id 0x{:016x}",
                    param.name, name, resource_id
                );
            }
            CoreSystemParamKind::Query { terms } => {
                let _ = write!(self.output, "\n  param {}: query", param.name);
                for term in terms {
                    let _ = write!(
                        self.output,
                        "\n    {} {} id 0x{:016x}",
                        format_query_access(term.access),
                        term.name,
                        term.component_id
                    );
                }
            }
        }
    }

    fn format_system_statement(&mut self, statement: &CoreSystemStatement, indent: usize) {
        match statement {
            CoreSystemStatement::QueryLoop(query_loop) => {
                self.format_query_loop(query_loop, indent);
            }
            CoreSystemStatement::Expression(expression) => {
                let _ = write!(
                    self.output,
                    "\n{}expr {}",
                    " ".repeat(indent),
                    format_system_expression(expression)
                );
            }
            CoreSystemStatement::AddAssign { target, value } => {
                let _ = write!(
                    self.output,
                    "\n{}add_assign {}, {}",
                    " ".repeat(indent),
                    format_system_place(target),
                    format_system_expression(value)
                );
            }
        }
    }

    fn format_query_loop(&mut self, query_loop: &CoreQueryLoop, indent: usize) {
        let leading = " ".repeat(indent);
        let _ = write!(
            self.output,
            "\n{}for {} {{",
            leading, query_loop.query_param
        );

        for binding in &query_loop.bindings {
            let _ = write!(
                self.output,
                "\n{}  bind {}: {} {} id 0x{:016x}",
                leading,
                binding.name,
                format_query_access(binding.access),
                binding.component_name,
                binding.component_id
            );
        }

        for statement in &query_loop.body {
            self.format_system_statement(statement, indent + 2);
        }

        let _ = write!(self.output, "\n{}}}", leading);
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
        CoreType::F32 => "f32",
    }
}

fn format_binary_op(op: CoreBinaryOp) -> &'static str {
    match op {
        CoreBinaryOp::Add => "i32.add",
        CoreBinaryOp::Subtract => "i32.sub",
        CoreBinaryOp::Multiply => "i32.mul",
    }
}

fn format_query_access(access: CoreQueryAccess) -> &'static str {
    match access {
        CoreQueryAccess::Read => "read",
        CoreQueryAccess::Mut => "mut",
    }
}

fn format_system_place(place: &CoreSystemPlace) -> String {
    match place {
        CoreSystemPlace::ComponentField {
            binding,
            field_name,
            ..
        } => format!("{binding}.{field_name}"),
    }
}

fn format_system_expression(expression: &CoreSystemExpression) -> String {
    match expression {
        CoreSystemExpression::ResourceField {
            param, field_name, ..
        } => format!("{param}.{field_name}"),
        CoreSystemExpression::ComponentField {
            binding,
            field_name,
            ..
        } => format!("{binding}.{field_name}"),
        CoreSystemExpression::Binary { op, left, right } => format!(
            "{} {}, {}",
            format_system_binary_op(*op),
            format_system_expression(left),
            format_system_expression(right)
        ),
    }
}

fn format_system_binary_op(op: CoreSystemBinaryOp) -> &'static str {
    match op {
        CoreSystemBinaryOp::F32Multiply => "f32.mul",
    }
}

fn format_spawn_field_value(value: &CoreSpawnFieldValue) -> String {
    match value {
        CoreSpawnFieldValue::F32Bits(bits) => format!("f32.bits 0x{bits:08x}"),
        CoreSpawnFieldValue::I32(value) => format!("i32 {value}"),
    }
}
