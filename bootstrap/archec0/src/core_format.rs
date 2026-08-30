use std::collections::HashMap;
use std::fmt;
use std::io;

use crate::core::{
    CoreBinaryOp, CoreComparisonOp, CoreComponentKind, CoreFunction, CoreInstruction, CoreProgram,
    CoreQueryAccess, CoreQueryLoop, CoreSpawnFieldValue, CoreSystem, CoreSystemBinaryOp,
    CoreSystemExpression, CoreSystemParam, CoreSystemParamKind, CoreSystemPlace,
    CoreSystemStatement, CoreSystemUnaryOp, CoreTerminator, CoreType, CoreUnaryOp, LocalId,
    ValueId,
};
use crate::core_verify::VerifiedExecutableCore;
use crate::execution_package_build::{canonical_core_ids, CanonicalCoreIds};

pub fn write_verified_core_program(
    output: &mut impl io::Write,
    core: &VerifiedExecutableCore,
) -> io::Result<()> {
    let ids = canonical_core_ids(core).map_err(io::Error::other)?;
    write_canonical_id_links(output, core, &ids)?;
    output.write_all(b"\n\n")?;
    write_core_program(output, core.program())
}

pub fn write_core_program(output: &mut impl io::Write, program: &CoreProgram) -> io::Result<()> {
    let mut formatter = CoreFormatter::new(IoFormatWriter::new(output));
    formatter.format_program(program);
    formatter.output.finish()
}

fn write_canonical_id_links(
    output: &mut impl io::Write,
    core: &VerifiedExecutableCore,
    ids: &CanonicalCoreIds,
) -> io::Result<()> {
    let program = core.program();
    output.write_all(b"canonical-ids {")?;

    for schema in &program.components {
        let kind = match schema.kind {
            CoreComponentKind::Component => "component",
            CoreComponentKind::Tag => "tag",
        };
        let id = ids
            .schema(schema.id)
            .ok_or_else(|| io::Error::other("verified Core schema has no canonical identifier"))?;
        write!(
            output,
            "\n  schema core-id @{} schema-id {} {kind} {}",
            schema.id, id, schema.name
        )?;
    }
    for resource in &program.resources {
        let id = ids.schema(resource.id).ok_or_else(|| {
            io::Error::other("verified Core resource has no canonical identifier")
        })?;
        write!(
            output,
            "\n  schema core-id @{} schema-id {} resource {}",
            resource.id, id, resource.name
        )?;
    }
    for system in &program.systems {
        let system_id = ids
            .system(system.id)
            .ok_or_else(|| io::Error::other("verified Core system has no canonical identifier"))?;
        write!(
            output,
            "\n  system core-id @{} system-id {} {}.{}",
            system.id, system_id, program.world.name, system.name
        )?;
        for parameter in &system.params {
            if !matches!(parameter.kind, CoreSystemParamKind::Query { .. }) {
                continue;
            }
            let query_id = ids.query(system.id, &parameter.name).ok_or_else(|| {
                io::Error::other("verified Core query has no canonical identifier")
            })?;
            write!(
                output,
                "\n  query system-core-id @{} parameter {} query-id {} {}.{}.{}",
                system.id,
                parameter.name,
                query_id,
                program.world.name,
                system.name,
                parameter.name
            )?;
        }
    }
    for schedule in &program.schedules {
        let id = ids.schedule(schedule.id).ok_or_else(|| {
            io::Error::other("verified Core schedule has no canonical identifier")
        })?;
        write!(
            output,
            "\n  schedule core-id @{} schedule-id {} {}.{}",
            schedule.id, id, program.world.name, schedule.name
        )?;
    }

    output.write_all(b"\n}")
}

#[cfg(test)]
pub fn format_core_program(program: &CoreProgram) -> String {
    let mut output = Vec::new();
    write_core_program(&mut output, program).expect("in-memory Core formatting succeeds");
    String::from_utf8(output).expect("Core formatting is UTF-8")
}

#[cfg(test)]
pub fn format_verified_core_program(core: &VerifiedExecutableCore) -> String {
    let mut output = Vec::new();
    write_verified_core_program(&mut output, core)
        .expect("in-memory verified Core formatting succeeds");
    String::from_utf8(output).expect("verified Core formatting is UTF-8")
}

struct IoFormatWriter<'a, W> {
    output: &'a mut W,
    error: Option<io::Error>,
}

impl<'a, W: io::Write> IoFormatWriter<'a, W> {
    fn new(output: &'a mut W) -> Self {
        Self {
            output,
            error: None,
        }
    }

    fn finish(self) -> io::Result<()> {
        self.error.map_or(Ok(()), Err)
    }
}

impl<W: io::Write> fmt::Write for IoFormatWriter<'_, W> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.error.is_some() {
            return Err(fmt::Error);
        }
        match self.output.write_all(value.as_bytes()) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.error = Some(error);
                Err(fmt::Error)
            }
        }
    }
}

struct CoreFormatter<W> {
    output: W,
}

impl<W: fmt::Write> CoreFormatter<W> {
    fn new(output: W) -> Self {
        Self { output }
    }

    fn format_program(&mut self, program: &CoreProgram) {
        let _ = write!(self.output, "world {}", program.world.name);

        for component in &program.components {
            let kind = match component.kind {
                CoreComponentKind::Component => "component",
                CoreComponentKind::Tag => "tag",
            };
            let _ = write!(
                self.output,
                "\n\n{kind} {} core-id 0x{:016x}",
                component.name, component.id
            );
            for field in &component.fields {
                let _ = write!(
                    self.output,
                    "\n  field {}: {}",
                    field.name,
                    format_type(field.ty)
                );
            }
        }

        for resource in &program.resources {
            let _ = write!(
                self.output,
                "\n\nresource {} core-id 0x{:016x}",
                resource.name, resource.id
            );
            for field in &resource.fields {
                let _ = write!(
                    self.output,
                    "\n  field {}: {}",
                    field.name,
                    format_type(field.ty)
                );
            }
        }

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

        let _ = self.output.write_str("\n}");
    }

    fn format_system_param(&mut self, param: &CoreSystemParam) {
        match &param.kind {
            CoreSystemParamKind::ReadResource { resource_id, name } => {
                let _ = write!(
                    self.output,
                    "\n  param {}: read {} core-id 0x{:016x}",
                    param.name, name, resource_id
                );
            }
            CoreSystemParamKind::MutResource { resource_id, name } => {
                let _ = write!(
                    self.output,
                    "\n  param {}: mut {} core-id 0x{:016x}",
                    param.name, name, resource_id
                );
            }
            CoreSystemParamKind::Query { terms } => {
                let _ = write!(self.output, "\n  param {}: query", param.name);
                for term in terms {
                    let _ = write!(
                        self.output,
                        "\n    {} {} core-id 0x{:016x}",
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
            CoreSystemStatement::Let {
                name,
                ty,
                mutable,
                value,
            } => {
                let _ = write!(
                    self.output,
                    "\n{}let {}{}: {} = {}",
                    " ".repeat(indent),
                    if *mutable { "mut " } else { "" },
                    name,
                    format_type(*ty),
                    format_system_expression(value)
                );
            }
            CoreSystemStatement::Assign { target, value } => {
                let _ = write!(
                    self.output,
                    "\n{}assign {}, {}",
                    " ".repeat(indent),
                    format_system_place(target),
                    format_system_expression(value)
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
            CoreSystemStatement::Block(statements) => {
                let leading = " ".repeat(indent);
                let _ = write!(self.output, "\n{leading}block {{");
                for child in statements {
                    self.format_system_statement(child, indent + 2);
                }
                let _ = write!(self.output, "\n{leading}}}");
            }
            CoreSystemStatement::If {
                condition,
                then_body,
                else_body,
            } => {
                let leading = " ".repeat(indent);
                let _ = write!(
                    self.output,
                    "\n{leading}if {} {{",
                    format_system_expression(condition)
                );
                for child in then_body {
                    self.format_system_statement(child, indent + 2);
                }
                let _ = write!(self.output, "\n{leading}}}");
                if !else_body.is_empty() {
                    let _ = write!(self.output, " else {{");
                    for child in else_body {
                        self.format_system_statement(child, indent + 2);
                    }
                    let _ = write!(self.output, "\n{leading}}}");
                }
            }
            CoreSystemStatement::While { condition, body } => {
                let leading = " ".repeat(indent);
                let _ = write!(
                    self.output,
                    "\n{leading}while {} {{",
                    format_system_expression(condition)
                );
                for child in body {
                    self.format_system_statement(child, indent + 2);
                }
                let _ = write!(self.output, "\n{leading}}}");
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
                "\n{}  bind {}: {} {} core-id 0x{:016x}",
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
            if function.blocks.len() > 1 {
                let _ = write!(self.output, "\n  block{}:", block.id.0);
            }
            for instruction in &block.instructions {
                self.format_instruction(instruction, &local_names);
            }

            self.format_terminator(&block.terminator);
        }

        let _ = self.output.write_str("\n}");
    }

    fn format_instruction(
        &mut self,
        instruction: &CoreInstruction,
        local_names: &HashMap<LocalId, &str>,
    ) {
        match instruction {
            CoreInstruction::InitializeResource {
                resource_id,
                resource_name,
                fields,
            } => {
                let _ = write!(
                    self.output,
                    "\n  resource {} core-id 0x{:016x}",
                    resource_name, resource_id
                );
                for field in fields {
                    let _ = write!(
                        self.output,
                        "\n    field {} = {} from {}",
                        field.name,
                        format_spawn_field_value(&field.value),
                        format_value(field.evaluation)
                    );
                }
            }
            CoreInstruction::Spawn { components } => {
                let _ = write!(self.output, "\n  spawn");
                for component in components {
                    let _ = write!(
                        self.output,
                        "\n    component {} core-id 0x{:016x}",
                        component.name, component.component_id
                    );
                    for field in &component.fields {
                        let _ = write!(
                            self.output,
                            "\n      field {} = {} from {}",
                            field.name,
                            format_spawn_field_value(&field.value),
                            format_value(field.evaluation)
                        );
                    }
                }
            }
            CoreInstruction::RunSchedule {
                schedule_id,
                schedule_name,
            } => {
                let _ = write!(
                    self.output,
                    "\n  run {} core-id 0x{:016x}",
                    schedule_name, schedule_id
                );
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
            CoreInstruction::I32Unary {
                result,
                op,
                operand,
            } => {
                let _ = write!(
                    self.output,
                    "\n  {} = i32.{} {}",
                    format_value(*result),
                    format_unary_op(*op),
                    format_value(*operand)
                );
            }
            CoreInstruction::F32Const { result, bits } => {
                let _ = write!(
                    self.output,
                    "\n  {} = f32.bits 0x{:08x}",
                    format_value(*result),
                    bits
                );
            }
            CoreInstruction::F32Unary {
                result,
                op,
                operand,
            } => {
                let _ = write!(
                    self.output,
                    "\n  {} = f32.{} {}",
                    format_value(*result),
                    format_unary_op(*op),
                    format_value(*operand)
                );
            }
            CoreInstruction::F32Binary {
                result,
                op,
                left,
                right,
            } => {
                let _ = write!(
                    self.output,
                    "\n  {} = f32.{} {}, {}",
                    format_value(*result),
                    format_binary_op_suffix(*op),
                    format_value(*left),
                    format_value(*right)
                );
            }
            CoreInstruction::Compare {
                result,
                op,
                left,
                right,
                operand_type,
            } => {
                let _ = write!(
                    self.output,
                    "\n  {} = {}.{} {}, {}",
                    format_value(*result),
                    format_type(*operand_type),
                    format_comparison_op(*op),
                    format_value(*left),
                    format_value(*right)
                );
            }
            CoreInstruction::BoolConst { result, value } => {
                let _ = write!(
                    self.output,
                    "\n  {} = bool.const {}",
                    format_value(*result),
                    value
                );
            }
            CoreInstruction::BoolNot { result, operand } => {
                let _ = write!(
                    self.output,
                    "\n  {} = bool.not {}",
                    format_value(*result),
                    format_value(*operand)
                );
            }
            CoreInstruction::Equal {
                result,
                left,
                right,
                operand_type,
                negate,
            } => {
                let _ = write!(
                    self.output,
                    "\n  {} = {}.{} {}, {}",
                    format_value(*result),
                    format_type(*operand_type),
                    if *negate { "ne" } else { "eq" },
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
            CoreTerminator::Jump { target } => {
                let _ = write!(self.output, "\n  jump block{}", target.0);
            }
            CoreTerminator::Branch {
                condition,
                then_block,
                else_block,
            } => {
                let _ = write!(
                    self.output,
                    "\n  branch {}, block{}, block{}",
                    format_value(*condition),
                    then_block.0,
                    else_block.0
                );
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
        CoreType::Bool => "bool",
    }
}

fn format_binary_op(op: CoreBinaryOp) -> &'static str {
    match op {
        CoreBinaryOp::Add => "i32.add",
        CoreBinaryOp::Subtract => "i32.sub",
        CoreBinaryOp::Multiply => "i32.mul",
        CoreBinaryOp::Divide => "i32.div",
        CoreBinaryOp::Remainder => "i32.rem",
        CoreBinaryOp::ShiftLeft => "i32.shl",
        CoreBinaryOp::ShiftRight => "i32.shr",
        CoreBinaryOp::BitAnd => "i32.and",
        CoreBinaryOp::BitXor => "i32.xor",
        CoreBinaryOp::BitOr => "i32.or",
    }
}

fn format_binary_op_suffix(op: CoreBinaryOp) -> &'static str {
    match op {
        CoreBinaryOp::Add => "add",
        CoreBinaryOp::Subtract => "sub",
        CoreBinaryOp::Multiply => "mul",
        CoreBinaryOp::Divide => "div",
        CoreBinaryOp::Remainder => "rem",
        CoreBinaryOp::ShiftLeft => "shl",
        CoreBinaryOp::ShiftRight => "shr",
        CoreBinaryOp::BitAnd => "and",
        CoreBinaryOp::BitXor => "xor",
        CoreBinaryOp::BitOr => "or",
    }
}

fn format_unary_op(op: CoreUnaryOp) -> &'static str {
    match op {
        CoreUnaryOp::Negate => "neg",
        CoreUnaryOp::BitNot => "not",
    }
}

fn format_comparison_op(op: CoreComparisonOp) -> &'static str {
    match op {
        CoreComparisonOp::Less => "lt",
        CoreComparisonOp::LessEqual => "le",
        CoreComparisonOp::Greater => "gt",
        CoreComparisonOp::GreaterEqual => "ge",
    }
}

fn format_system_unary_op(op: CoreSystemUnaryOp) -> &'static str {
    match op {
        CoreSystemUnaryOp::I32Negate => "i32.neg",
        CoreSystemUnaryOp::F32Negate => "f32.neg",
        CoreSystemUnaryOp::I32BitNot => "i32.not",
        CoreSystemUnaryOp::BoolNot => "bool.not",
    }
}

fn format_query_access(access: CoreQueryAccess) -> &'static str {
    match access {
        CoreQueryAccess::Read => "read",
        CoreQueryAccess::Mut => "mut",
        CoreQueryAccess::Exclude => "exclude",
    }
}

fn format_system_place(place: &CoreSystemPlace) -> String {
    match place {
        CoreSystemPlace::ComponentField {
            binding,
            field_name,
            ..
        } => format!("{binding}.{field_name}"),
        CoreSystemPlace::Local { name, .. } => name.to_string(),
        CoreSystemPlace::ResourceField {
            param, field_name, ..
        } => format!("{param}.{field_name}"),
    }
}

fn format_system_expression(expression: &CoreSystemExpression) -> String {
    match expression {
        CoreSystemExpression::I32Const(value) => format!("i32.const {value}"),
        CoreSystemExpression::F32Const(bits) => format!("f32.bits 0x{bits:08x}"),
        CoreSystemExpression::BoolConst(value) => format!("bool.const {value}"),
        CoreSystemExpression::Local { name, .. } => name.to_string(),
        CoreSystemExpression::ResourceField {
            param, field_name, ..
        } => format!("{param}.{field_name}"),
        CoreSystemExpression::ComponentField {
            binding,
            field_name,
            ..
        } => format!("{binding}.{field_name}"),
        CoreSystemExpression::BoolNot(operand) => {
            format!("bool.not {}", format_system_expression(operand))
        }
        CoreSystemExpression::Unary { op, operand } => format!(
            "{} {}",
            format_system_unary_op(*op),
            format_system_expression(operand)
        ),
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
        CoreSystemBinaryOp::I32Add => "i32.add",
        CoreSystemBinaryOp::I32Subtract => "i32.sub",
        CoreSystemBinaryOp::I32Multiply => "i32.mul",
        CoreSystemBinaryOp::I32Divide => "i32.div",
        CoreSystemBinaryOp::I32Remainder => "i32.rem",
        CoreSystemBinaryOp::I32ShiftLeft => "i32.shl",
        CoreSystemBinaryOp::I32ShiftRight => "i32.shr",
        CoreSystemBinaryOp::I32BitAnd => "i32.and",
        CoreSystemBinaryOp::I32BitXor => "i32.xor",
        CoreSystemBinaryOp::I32BitOr => "i32.or",
        CoreSystemBinaryOp::F32Add => "f32.add",
        CoreSystemBinaryOp::F32Subtract => "f32.sub",
        CoreSystemBinaryOp::F32Multiply => "f32.mul",
        CoreSystemBinaryOp::F32Divide => "f32.div",
        CoreSystemBinaryOp::I32Less => "i32.lt",
        CoreSystemBinaryOp::I32LessEqual => "i32.le",
        CoreSystemBinaryOp::I32Greater => "i32.gt",
        CoreSystemBinaryOp::I32GreaterEqual => "i32.ge",
        CoreSystemBinaryOp::F32Less => "f32.lt",
        CoreSystemBinaryOp::F32LessEqual => "f32.le",
        CoreSystemBinaryOp::F32Greater => "f32.gt",
        CoreSystemBinaryOp::F32GreaterEqual => "f32.ge",
        CoreSystemBinaryOp::Equal => "eq",
        CoreSystemBinaryOp::NotEqual => "ne",
        CoreSystemBinaryOp::LogicalAnd => "bool.and",
        CoreSystemBinaryOp::LogicalOr => "bool.or",
    }
}

fn format_spawn_field_value(value: &CoreSpawnFieldValue) -> String {
    match value {
        CoreSpawnFieldValue::F32Bits(bits) => format!("f32.bits 0x{bits:08x}"),
        CoreSpawnFieldValue::I32(value) => format!("i32 {value}"),
        CoreSpawnFieldValue::Bool(value) => format!("bool {value}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_output_links_every_canonical_id_kind() {
        let source = include_str!("../../../examples/m26_closure.arc");
        let tokens = crate::lexer::lex(source).expect("fixture lexes");
        let program = crate::parser::parse_program(&tokens).expect("fixture parses");
        crate::checker::check_program(&program).expect("fixture checks as executable");
        let core = crate::core_lower::lower_program_to_core(&program).expect("fixture lowers");
        let core = crate::core_verify::verify_executable_core(core).expect("fixture Core verifies");

        let output = format_verified_core_program(&core);
        let expected = include_str!("../../../tests/golden/m26_closure.core").replace("\r\n", "\n");
        assert_eq!(output, expected.trim_end());
        assert!(output.contains(
            "schema core-id @0 schema-id DD73DA45122DA6C43B963101BF8427BA component M26Closure.Position"
        ));
        assert!(output.contains(
            "system core-id @0 system-id B5774A60450757A71EB9432EF8ADA480 M26Closure.Advance"
        ));
        assert!(output.contains(
            "query system-core-id @0 parameter moving query-id 19C5FAFEBEF407EE5F046E6BCEC0B0CA M26Closure.Advance.moving"
        ));
        assert!(output.contains(
            "schedule core-id @0 schedule-id D96D851EFBBFE2A9F1C8AB4837B716A5 M26Closure.Warmup"
        ));
        assert!(output.contains("component M26Closure.Position core-id 0x0000000000000000"));
    }
}
