#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use crate::core::{
    CoreBlock, CoreComponent, CoreField, CoreFunction, CoreInstruction, CoreProgram,
    CoreQueryAccess, CoreQueryLoop, CoreQueryLoopBinding, CoreResource, CoreSpawnFieldValue,
    CoreSystem, CoreSystemExpression, CoreSystemParam, CoreSystemParamKind, CoreSystemPlace,
    CoreSystemStatement, CoreTerminator, CoreType, LocalId, ValueId,
};
use crate::{layout, runtime};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreVerifyError {
    pub message: String,
}

struct CoreSchemas<'a> {
    components_by_id: HashMap<u64, &'a CoreComponent>,
    components_by_name: HashMap<&'a str, &'a CoreComponent>,
    resources_by_id: HashMap<u64, &'a CoreResource>,
    resources_by_name: HashMap<&'a str, &'a CoreResource>,
    systems_by_id: HashMap<u64, &'a CoreSystem>,
    systems_by_name: HashMap<&'a str, &'a CoreSystem>,
}

pub fn verify_core_program(program: &CoreProgram) -> Result<(), CoreVerifyError> {
    let schemas = verify_schemas(program)?;

    for system in &program.systems {
        verify_system(system, &schemas)?;
    }
    verify_schedules(program, &schemas)?;

    let mut function_names = HashSet::new();
    for function in &program.functions {
        if !function_names.insert(function.name.as_str()) {
            return Err(verify_error(format!(
                "duplicate function name `{}`",
                function.name
            )));
        }
        verify_function(function, &schemas)?;
    }

    Ok(())
}

fn verify_schemas(program: &CoreProgram) -> Result<CoreSchemas<'_>, CoreVerifyError> {
    let mut components_by_id = HashMap::new();
    let mut components_by_name = HashMap::new();
    for component in &program.components {
        insert_schema(
            "component",
            component.id,
            &component.name,
            component,
            &mut components_by_id,
            &mut components_by_name,
        )?;
        let local_name = require_qualified_name(&program.world.name, &component.name, "component")?;
        let expected_id = layout::stable_component_id(&program.world.name, local_name).0;
        if component.id != expected_id {
            return Err(verify_error(format!(
                "component `{}` id 0x{:016x} does not match stable id 0x{expected_id:016x}",
                component.name, component.id
            )));
        }
        verify_schema_fields("component", &component.name, &component.fields)?;
    }

    let mut resources_by_id = HashMap::new();
    let mut resources_by_name = HashMap::new();
    for resource in &program.resources {
        insert_schema(
            "resource",
            resource.id,
            &resource.name,
            resource,
            &mut resources_by_id,
            &mut resources_by_name,
        )?;
        let local_name = require_qualified_name(&program.world.name, &resource.name, "resource")?;
        let expected_id = runtime::stable_resource_id(&program.world.name, local_name).0;
        if resource.id != expected_id {
            return Err(verify_error(format!(
                "resource `{}` id 0x{:016x} does not match stable id 0x{expected_id:016x}",
                resource.name, resource.id
            )));
        }
        verify_schema_fields("resource", &resource.name, &resource.fields)?;
    }

    let mut systems_by_id = HashMap::new();
    let mut systems_by_name = HashMap::new();
    for system in &program.systems {
        insert_schema(
            "system",
            system.id,
            &system.name,
            system,
            &mut systems_by_id,
            &mut systems_by_name,
        )?;
        let expected_id = runtime::stable_system_id(&program.world.name, &system.name).0;
        if system.id != expected_id {
            return Err(verify_error(format!(
                "system `{}` id 0x{:016x} does not match stable id 0x{expected_id:016x}",
                system.name, system.id
            )));
        }
    }

    Ok(CoreSchemas {
        components_by_id,
        components_by_name,
        resources_by_id,
        resources_by_name,
        systems_by_id,
        systems_by_name,
    })
}

fn insert_schema<'a, T>(
    kind: &str,
    id: u64,
    name: &'a str,
    value: &'a T,
    by_id: &mut HashMap<u64, &'a T>,
    by_name: &mut HashMap<&'a str, &'a T>,
) -> Result<(), CoreVerifyError> {
    if by_id.insert(id, value).is_some() {
        return Err(verify_error(format!("duplicate {kind} id 0x{id:016x}")));
    }
    if by_name.insert(name, value).is_some() {
        return Err(verify_error(format!("duplicate {kind} name `{name}`")));
    }
    Ok(())
}

fn require_qualified_name<'a>(
    world_name: &str,
    name: &'a str,
    kind: &str,
) -> Result<&'a str, CoreVerifyError> {
    let prefix = format!("{world_name}.");
    name.strip_prefix(&prefix)
        .filter(|local| !local.is_empty() && !local.contains('.'))
        .ok_or_else(|| {
            verify_error(format!(
                "{kind} name `{name}` is not qualified by world `{world_name}`"
            ))
        })
}

fn verify_schema_fields(
    kind: &str,
    owner: &str,
    fields: &[CoreField],
) -> Result<(), CoreVerifyError> {
    let mut names = HashSet::new();
    for field in fields {
        if !names.insert(field.name.as_str()) {
            return Err(verify_error(format!(
                "duplicate field `{}` in Core {kind} `{owner}`",
                field.name
            )));
        }
    }
    Ok(())
}

fn verify_system(system: &CoreSystem, schemas: &CoreSchemas<'_>) -> Result<(), CoreVerifyError> {
    let mut params = HashMap::new();
    let mut query_accesses = HashMap::new();

    for param in &system.params {
        if params.insert(param.name.as_str(), param).is_some() {
            return Err(verify_error(format!(
                "duplicate parameter `{}` in Core system `{}`",
                param.name, system.name
            )));
        }

        match &param.kind {
            CoreSystemParamKind::ReadResource { resource_id, name } => {
                resolve_resource(schemas, *resource_id, name)?;
            }
            CoreSystemParamKind::Query { terms } => {
                for term in terms {
                    resolve_component(schemas, term.component_id, &term.name)?;
                    if let Some(previous) = query_accesses.get(&term.component_id) {
                        if *previous == CoreQueryAccess::Mut || term.access == CoreQueryAccess::Mut
                        {
                            return Err(verify_error(format!(
                                "conflicting Core query access for component `{}`",
                                term.name
                            )));
                        }
                    } else {
                        query_accesses.insert(term.component_id, term.access);
                    }
                }
            }
        }
    }

    let bindings = HashMap::new();
    verify_system_statements(&system.body.statements, schemas, &params, &bindings)
}

fn verify_system_statements<'a>(
    statements: &'a [CoreSystemStatement],
    schemas: &CoreSchemas<'a>,
    params: &HashMap<&'a str, &'a CoreSystemParam>,
    bindings: &HashMap<&'a str, &'a CoreQueryLoopBinding>,
) -> Result<(), CoreVerifyError> {
    for statement in statements {
        match statement {
            CoreSystemStatement::QueryLoop(query_loop) => {
                if !bindings.is_empty() {
                    return Err(verify_error("nested Core query loops are not supported"));
                }
                verify_query_loop(query_loop, schemas, params)?;
            }
            CoreSystemStatement::Expression(expression) => {
                verify_system_expression(expression, schemas, params, bindings)?;
            }
            CoreSystemStatement::AddAssign { target, value } => {
                let target_type = verify_system_place(target, schemas, bindings)?;
                let value_type = verify_system_expression(value, schemas, params, bindings)?;
                if target_type != CoreType::F32 || value_type != target_type {
                    return Err(verify_error(
                        "Core add-assign requires matching f32 target and value types",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn verify_query_loop<'a>(
    query_loop: &'a CoreQueryLoop,
    schemas: &CoreSchemas<'a>,
    params: &HashMap<&'a str, &'a CoreSystemParam>,
) -> Result<(), CoreVerifyError> {
    let param = params
        .get(query_loop.query_param.as_str())
        .copied()
        .ok_or_else(|| {
            verify_error(format!(
                "unknown Core query parameter `{}`",
                query_loop.query_param
            ))
        })?;
    let CoreSystemParamKind::Query { terms } = &param.kind else {
        return Err(verify_error(format!(
            "Core query loop target `{}` is not a query parameter",
            query_loop.query_param
        )));
    };
    if query_loop.bindings.len() != terms.len() {
        return Err(verify_error(format!(
            "Core query loop binding count {} does not match term count {}",
            query_loop.bindings.len(),
            terms.len()
        )));
    }

    let mut bindings = HashMap::new();
    for (binding, term) in query_loop.bindings.iter().zip(terms) {
        if bindings.insert(binding.name.as_str(), binding).is_some() {
            return Err(verify_error(format!(
                "duplicate Core query loop binding `{}`",
                binding.name
            )));
        }
        if binding.component_id != term.component_id
            || binding.component_name != term.name
            || binding.access != term.access
        {
            return Err(verify_error(format!(
                "Core query binding `{}` does not match its query term",
                binding.name
            )));
        }
        resolve_component(schemas, binding.component_id, &binding.component_name)?;
    }

    verify_system_statements(&query_loop.body, schemas, params, &bindings)
}

fn verify_system_place(
    place: &CoreSystemPlace,
    schemas: &CoreSchemas<'_>,
    bindings: &HashMap<&str, &CoreQueryLoopBinding>,
) -> Result<CoreType, CoreVerifyError> {
    match place {
        CoreSystemPlace::ComponentField {
            binding,
            component_id,
            component_name,
            field_name,
        } => {
            let resolved_binding = bindings.get(binding.as_str()).copied().ok_or_else(|| {
                verify_error(format!("unknown Core component binding `{binding}`"))
            })?;
            if resolved_binding.access != CoreQueryAccess::Mut {
                return Err(verify_error(format!(
                    "Core add-assign binding `{binding}` is not mutable"
                )));
            }
            if resolved_binding.component_id != *component_id
                || resolved_binding.component_name != *component_name
            {
                return Err(verify_error(format!(
                    "Core component place `{binding}.{field_name}` does not match its binding"
                )));
            }
            let component = resolve_component(schemas, *component_id, component_name)?;
            resolve_field("component", &component.name, &component.fields, field_name)
        }
    }
}

fn verify_system_expression(
    expression: &CoreSystemExpression,
    schemas: &CoreSchemas<'_>,
    params: &HashMap<&str, &CoreSystemParam>,
    bindings: &HashMap<&str, &CoreQueryLoopBinding>,
) -> Result<CoreType, CoreVerifyError> {
    match expression {
        CoreSystemExpression::ResourceField {
            param,
            resource_id,
            resource_name,
            field_name,
        } => {
            let resolved_param = params.get(param.as_str()).copied().ok_or_else(|| {
                verify_error(format!("unknown Core resource parameter `{param}`"))
            })?;
            let CoreSystemParamKind::ReadResource {
                resource_id: param_id,
                name: param_name,
            } = &resolved_param.kind
            else {
                return Err(verify_error(format!(
                    "Core parameter `{param}` is not a read resource"
                )));
            };
            if param_id != resource_id || param_name != resource_name {
                return Err(verify_error(format!(
                    "Core resource expression `{param}.{field_name}` does not match its parameter"
                )));
            }
            let resource = resolve_resource(schemas, *resource_id, resource_name)?;
            resolve_field("resource", &resource.name, &resource.fields, field_name)
        }
        CoreSystemExpression::ComponentField {
            binding,
            component_id,
            component_name,
            field_name,
        } => {
            let resolved_binding = bindings.get(binding.as_str()).copied().ok_or_else(|| {
                verify_error(format!("unknown Core component binding `{binding}`"))
            })?;
            if resolved_binding.component_id != *component_id
                || resolved_binding.component_name != *component_name
            {
                return Err(verify_error(format!(
                    "Core component expression `{binding}.{field_name}` does not match its binding"
                )));
            }
            let component = resolve_component(schemas, *component_id, component_name)?;
            resolve_field("component", &component.name, &component.fields, field_name)
        }
        CoreSystemExpression::Binary { left, right, .. } => {
            let left_type = verify_system_expression(left, schemas, params, bindings)?;
            let right_type = verify_system_expression(right, schemas, params, bindings)?;
            if left_type != CoreType::F32 || right_type != CoreType::F32 {
                return Err(verify_error(
                    "Core f32 multiplication requires f32 operands",
                ));
            }
            Ok(CoreType::F32)
        }
    }
}

fn resolve_component<'a>(
    schemas: &CoreSchemas<'a>,
    id: u64,
    name: &str,
) -> Result<&'a CoreComponent, CoreVerifyError> {
    let by_id = schemas.components_by_id.get(&id).copied();
    let by_name = schemas.components_by_name.get(name).copied();
    match (by_id, by_name) {
        (Some(component), Some(named)) if std::ptr::eq(component, named) => Ok(component),
        _ => Err(verify_error(format!(
            "unresolved Core component `{name}` id 0x{id:016x}"
        ))),
    }
}

fn resolve_resource<'a>(
    schemas: &CoreSchemas<'a>,
    id: u64,
    name: &str,
) -> Result<&'a CoreResource, CoreVerifyError> {
    let by_id = schemas.resources_by_id.get(&id).copied();
    let by_name = schemas.resources_by_name.get(name).copied();
    match (by_id, by_name) {
        (Some(resource), Some(named)) if std::ptr::eq(resource, named) => Ok(resource),
        _ => Err(verify_error(format!(
            "unresolved Core resource `{name}` id 0x{id:016x}"
        ))),
    }
}

fn resolve_field(
    kind: &str,
    owner: &str,
    fields: &[CoreField],
    field_name: &str,
) -> Result<CoreType, CoreVerifyError> {
    fields
        .iter()
        .find(|field| field.name == field_name)
        .map(|field| field.ty)
        .ok_or_else(|| {
            verify_error(format!(
                "unknown Core field `{field_name}` for {kind} `{owner}`"
            ))
        })
}

fn verify_schedules(
    program: &CoreProgram,
    schemas: &CoreSchemas<'_>,
) -> Result<(), CoreVerifyError> {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for schedule in &program.schedules {
        if !ids.insert(schedule.id) {
            return Err(verify_error(format!(
                "duplicate Core schedule id 0x{:016x}",
                schedule.id
            )));
        }
        if !names.insert(schedule.name.as_str()) {
            return Err(verify_error(format!(
                "duplicate Core schedule name `{}`",
                schedule.name
            )));
        }
        let expected_id = runtime::stable_schedule_id(&program.world.name, &schedule.name).0;
        if schedule.id != expected_id {
            return Err(verify_error(format!(
                "schedule `{}` id 0x{:016x} does not match stable id 0x{expected_id:016x}",
                schedule.name, schedule.id
            )));
        }

        for item in &schedule.items {
            match item {
                crate::core::CoreScheduleItem::Run {
                    system_id,
                    system_name,
                } => {
                    let system =
                        schemas
                            .systems_by_id
                            .get(system_id)
                            .copied()
                            .ok_or_else(|| {
                                verify_error(format!(
                                    "unresolved Core system `{system_name}` id 0x{system_id:016x}"
                                ))
                            })?;
                    let expected_name = format!("{}.{}", program.world.name, system.name);
                    if system_name != &expected_name
                        || schemas.systems_by_name.get(system.name.as_str()).copied()
                            != Some(system)
                    {
                        return Err(verify_error(format!(
                            "Core schedule system reference `{system_name}` does not match id 0x{system_id:016x}"
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

fn verify_function(
    function: &CoreFunction,
    schemas: &CoreSchemas<'_>,
) -> Result<(), CoreVerifyError> {
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

    let mut locals = HashMap::new();
    let mut local_names = HashSet::new();
    for local in &function.locals {
        if locals.insert(local.id, local.ty).is_some() {
            return Err(verify_error(format!("duplicate local id {}", local.id.0)));
        }
        if !local_names.insert(local.name.as_str()) {
            return Err(verify_error(format!(
                "duplicate local name `{}` in Core function `{}`",
                local.name, function.name
            )));
        }
    }

    for block in &function.blocks {
        verify_block(block, &locals, schemas)?;
    }

    Ok(())
}

fn verify_block(
    block: &CoreBlock,
    locals: &HashMap<LocalId, CoreType>,
    schemas: &CoreSchemas<'_>,
) -> Result<(), CoreVerifyError> {
    let mut values = HashMap::new();

    for instruction in &block.instructions {
        match instruction {
            CoreInstruction::Spawn { components } => verify_spawn(components, schemas)?,
            CoreInstruction::I32Const { result, .. } => {
                define_value(&mut values, *result, CoreType::I32)?;
            }
            CoreInstruction::I32Binary {
                result,
                left,
                right,
                ..
            } => {
                require_value(&values, *left, CoreType::I32)?;
                require_value(&values, *right, CoreType::I32)?;
                define_value(&mut values, *result, CoreType::I32)?;
            }
            CoreInstruction::LocalStore { local, value } => {
                let local_type = require_local(locals, *local)?;
                require_value(&values, *value, local_type)?;
            }
            CoreInstruction::LocalLoad { result, local } => {
                let local_type = require_local(locals, *local)?;
                define_value(&mut values, *result, local_type)?;
            }
        }
    }

    match block.terminator {
        CoreTerminator::Exit { value } => require_value(&values, value, CoreType::I32),
    }
}

fn verify_spawn(
    components: &[crate::core::CoreSpawnComponent],
    schemas: &CoreSchemas<'_>,
) -> Result<(), CoreVerifyError> {
    let mut component_ids = HashSet::new();
    for component in components {
        if !component_ids.insert(component.component_id) {
            return Err(verify_error(format!(
                "duplicate Core spawn component `{}`",
                component.name
            )));
        }
        let schema = resolve_component(schemas, component.component_id, &component.name)?;
        let mut fields = HashSet::new();
        for field in &component.fields {
            if !fields.insert(field.name.as_str()) {
                return Err(verify_error(format!(
                    "duplicate Core spawn field `{}.{}`",
                    component.name, field.name
                )));
            }
            let expected_type =
                resolve_field("component", &schema.name, &schema.fields, &field.name)?;
            let value_type = match field.value {
                CoreSpawnFieldValue::F32Bits(_) => CoreType::F32,
            };
            if value_type != expected_type {
                return Err(verify_error(format!(
                    "Core spawn field `{}.{}` has the wrong type",
                    component.name, field.name
                )));
            }
        }
    }
    Ok(())
}

fn define_value(
    values: &mut HashMap<ValueId, CoreType>,
    value: ValueId,
    ty: CoreType,
) -> Result<(), CoreVerifyError> {
    if values.insert(value, ty).is_none() {
        Ok(())
    } else {
        Err(verify_error(format!("duplicate value {}", value.0)))
    }
}

fn require_value(
    values: &HashMap<ValueId, CoreType>,
    value: ValueId,
    expected: CoreType,
) -> Result<(), CoreVerifyError> {
    match values.get(&value) {
        Some(actual) if *actual == expected => Ok(()),
        Some(actual) => Err(verify_error(format!(
            "value {} has type {actual:?}, expected {expected:?}",
            value.0
        ))),
        None => Err(verify_error(format!("undefined value {}", value.0))),
    }
}

fn require_local(
    locals: &HashMap<LocalId, CoreType>,
    local: LocalId,
) -> Result<CoreType, CoreVerifyError> {
    locals
        .get(&local)
        .copied()
        .ok_or_else(|| verify_error(format!("undefined local {}", local.0)))
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
    use crate::{core_lower, lexer, parser};

    fn lowered(source: &str) -> CoreProgram {
        let tokens = lexer::lex(source).expect("fixture lexes");
        let ast = parser::parse_program(&tokens).expect("fixture parses");
        core_lower::lower_program_to_core(&ast).expect("fixture lowers to Core")
    }

    #[test]
    fn core_verifier_accepts_lowered_math_and_ecs() {
        verify_core_program(&lowered(include_str!("../../../examples/math.arc")))
            .expect("lowered math Core verifies");
        verify_core_program(&lowered(include_str!("../../../examples/move_system.arc")))
            .expect("lowered ECS Core verifies");
    }

    #[test]
    fn core_verifier_rejects_invalid_value_reference() {
        let program = CoreProgram {
            world: CoreWorld {
                name: "Main".to_string(),
            },
            components: vec![],
            resources: vec![],
            systems: vec![],
            schedules: vec![],
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
        assert!(error.message.contains("undefined value"));
    }

    #[test]
    fn core_verifier_rejects_unresolved_ecs_field() {
        let mut program = lowered(include_str!("../../../examples/move_system.arc"));
        let CoreSystemStatement::QueryLoop(query_loop) = &mut program.systems[0].body.statements[0]
        else {
            panic!("expected query loop");
        };
        let CoreSystemStatement::AddAssign { value, .. } = &mut query_loop.body[0] else {
            panic!("expected add-assign");
        };
        let CoreSystemExpression::Binary { right, .. } = value else {
            panic!("expected product");
        };
        let CoreSystemExpression::ResourceField { field_name, .. } = &mut **right else {
            panic!("expected resource field");
        };
        *field_name = "missing".to_string();

        let error = verify_core_program(&program).expect_err("unknown Core field must fail");
        assert!(error.message.contains("unknown Core field `missing`"));
    }

    #[test]
    fn core_verifier_rejects_mismatched_query_binding_and_spawn_field() {
        let mut binding_program = lowered(include_str!("../../../examples/move_system.arc"));
        let CoreSystemStatement::QueryLoop(query_loop) =
            &mut binding_program.systems[0].body.statements[0]
        else {
            panic!("expected query loop");
        };
        query_loop.bindings[0].component_id = query_loop.bindings[1].component_id;
        let error =
            verify_core_program(&binding_program).expect_err("mismatched binding must fail");
        assert!(error.message.contains("does not match its query term"));

        let mut spawn_program = lowered(include_str!("../../../examples/spawn_position.arc"));
        let CoreInstruction::Spawn { components } =
            &mut spawn_program.functions[0].blocks[0].instructions[0]
        else {
            panic!("expected spawn");
        };
        components[0].fields[0].name = "missing".to_string();
        let error = verify_core_program(&spawn_program).expect_err("unknown spawn field must fail");
        assert!(error.message.contains("unknown Core field `missing`"));
    }

    #[test]
    fn core_verifier_rejects_schema_identity_and_schedule_target_mismatches() {
        let mut identity_program = lowered(include_str!("../../../examples/move_system.arc"));
        identity_program.components[0].id ^= 1;
        let error = verify_core_program(&identity_program).expect_err("wrong stable id must fail");
        assert!(error.message.contains("does not match stable id"));

        let mut resource_program = lowered(include_str!("../../../examples/move_system.arc"));
        let CoreSystemParamKind::ReadResource { name, .. } =
            &mut resource_program.systems[0].params[0].kind
        else {
            panic!("expected resource parameter");
        };
        *name = "Demo.Missing".to_string();
        let error =
            verify_core_program(&resource_program).expect_err("mismatched resource name must fail");
        assert!(error.message.contains("unresolved Core resource"));

        let mut schedule_program = lowered(include_str!("../../../examples/move_system.arc"));
        let crate::core::CoreScheduleItem::Run { system_name, .. } =
            &mut schedule_program.schedules[0].items[0];
        *system_name = "Demo.Missing".to_string();
        let error = verify_core_program(&schedule_program)
            .expect_err("mismatched schedule target must fail");
        assert!(error.message.contains("does not match id"));
    }

    #[test]
    fn core_verifier_rejects_read_only_updates_and_spawn_type_mismatches() {
        let mut mutability_program = lowered(include_str!("../../../examples/move_system.arc"));
        let CoreSystemParamKind::Query { terms } =
            &mut mutability_program.systems[0].params[1].kind
        else {
            panic!("expected query parameter");
        };
        terms[0].access = CoreQueryAccess::Read;
        let CoreSystemStatement::QueryLoop(query_loop) =
            &mut mutability_program.systems[0].body.statements[0]
        else {
            panic!("expected query loop");
        };
        query_loop.bindings[0].access = CoreQueryAccess::Read;
        let error =
            verify_core_program(&mutability_program).expect_err("read-only Core update must fail");
        assert!(error.message.contains("is not mutable"));

        let mut spawn_program = lowered(include_str!("../../../examples/spawn_position.arc"));
        spawn_program.components[0].fields[0].ty = CoreType::I32;
        let error =
            verify_core_program(&spawn_program).expect_err("spawn field type mismatch must fail");
        assert!(error.message.contains("has the wrong type"));
    }

    #[test]
    fn core_verifier_rejects_invalid_local_and_terminator_types() {
        let mut local_program = lowered(include_str!("../../../examples/math.arc"));
        let CoreInstruction::LocalStore { local, .. } =
            &mut local_program.functions[0].blocks[0].instructions[3]
        else {
            panic!("expected local store");
        };
        *local = LocalId(99);
        let error = verify_core_program(&local_program).expect_err("unknown local must fail");
        assert!(error.message.contains("undefined local 99"));

        let terminator_program = CoreProgram {
            world: CoreWorld {
                name: "Main".to_string(),
            },
            components: vec![],
            resources: vec![],
            systems: vec![],
            schedules: vec![],
            functions: vec![CoreFunction {
                name: "startup".to_string(),
                entry: BlockId(0),
                locals: vec![CoreLocal {
                    id: LocalId(0),
                    name: "value".to_string(),
                    ty: CoreType::F32,
                }],
                blocks: vec![CoreBlock {
                    id: BlockId(0),
                    instructions: vec![CoreInstruction::LocalLoad {
                        result: ValueId(0),
                        local: LocalId(0),
                    }],
                    terminator: CoreTerminator::Exit { value: ValueId(0) },
                }],
            }],
        };
        let error =
            verify_core_program(&terminator_program).expect_err("non-i32 exit value must fail");
        assert!(error.message.contains("expected I32"));
    }
}
