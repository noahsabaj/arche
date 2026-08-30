use std::collections::{HashMap, HashSet, VecDeque};

use crate::core::{
    CoreBlock, CoreComponent, CoreComponentKind, CoreField, CoreFunction, CoreInstruction,
    CoreProgram, CoreQueryAccess, CoreQueryLoop, CoreQueryLoopBinding, CoreResource,
    CoreResourceField, CoreSchedule, CoreSourceSubject, CoreSpawnFieldValue, CoreSystem,
    CoreSystemExpression, CoreSystemParam, CoreSystemParamKind, CoreSystemPlace,
    CoreSystemStatement, CoreTerminator, CoreType, LocalId, ValueId,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreVerifyError {
    pub message: String,
}

/// An owned Core program that has passed both general Core verification and
/// the executable startup contract.
///
/// Keeping the program behind this brand prevents downstream executable
/// stages from accidentally treating merely constructed Core as verified.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedExecutableCore {
    program: CoreProgram,
    startup_effects: Vec<CoreInstructionLocation>,
}

impl VerifiedExecutableCore {
    pub fn program(&self) -> &CoreProgram {
        &self.program
    }

    pub fn startup_operations(&self) -> impl Iterator<Item = &CoreInstruction> {
        self.startup_effects.iter().map(|location| {
            let startup = self
                .program
                .functions
                .iter()
                .find(|function| function.name == "startup")
                .expect("verified executable Core retains startup");
            let block = startup
                .blocks
                .iter()
                .find(|block| block.id == location.block)
                .expect("verified executable Core retains startup block");
            let index = usize::try_from(location.instruction_index)
                .expect("verified instruction index fits the host address space");
            &block.instructions[index]
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CoreInstructionLocation {
    block: crate::core::BlockId,
    instruction_index: u64,
}

struct CoreSchemas<'a> {
    world_name: &'a str,
    components_by_id: HashMap<u64, &'a CoreComponent>,
    components_by_name: HashMap<&'a str, &'a CoreComponent>,
    resources_by_id: HashMap<u64, &'a CoreResource>,
    resources_by_name: HashMap<&'a str, &'a CoreResource>,
    systems_by_id: HashMap<u64, &'a CoreSystem>,
    systems_by_name: HashMap<&'a str, &'a CoreSystem>,
    schedules_by_id: HashMap<u64, &'a CoreSchedule>,
    schedules_by_name: HashMap<&'a str, &'a CoreSchedule>,
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

pub fn verify_executable_core(
    program: CoreProgram,
) -> Result<VerifiedExecutableCore, CoreVerifyError> {
    verify_core_program(&program)?;
    if program.functions.len() != 1 {
        return Err(verify_error(
            "executable Core must contain exactly one `startup` function and no other functions",
        ));
    }
    let schemas = verify_schemas(&program)?;

    let mut startup = None;
    for function in &program.functions {
        if function.name == "startup" && startup.replace(function).is_some() {
            return Err(verify_error(
                "executable Core must contain exactly one `startup` function",
            ));
        }
    }
    let startup = startup.ok_or_else(|| {
        verify_error("executable Core must contain exactly one `startup` function")
    })?;
    let exits = startup
        .blocks
        .iter()
        .filter(|block| matches!(block.terminator, CoreTerminator::Exit { .. }))
        .map(|block| block.id)
        .collect::<Vec<_>>();
    if exits.len() != 1 {
        return Err(verify_error(
            "executable Core `startup` must contain exactly one reachable exit",
        ));
    }
    verify_every_block_reaches_exit(startup, exits[0])?;
    verify_source_map(&program, startup)?;
    let startup_effects = ordered_startup_effects(startup)?;
    verify_startup_resource_flow(&startup_effects, startup, &schemas)?;

    Ok(VerifiedExecutableCore {
        program,
        startup_effects,
    })
}

fn verify_source_map(program: &CoreProgram, startup: &CoreFunction) -> Result<(), CoreVerifyError> {
    let mut actual = HashSet::new();
    for entry in &program.source_map.entries {
        if !valid_source_span(entry.span) {
            return Err(verify_error(format!(
                "invalid Core source span for {:?}",
                entry.subject
            )));
        }
        if !actual.insert(entry.subject.clone()) {
            return Err(verify_error(format!(
                "duplicate Core source-map subject {:?}",
                entry.subject
            )));
        }
    }

    let mut expected = HashSet::new();
    expected.insert(CoreSourceSubject::Program);
    expected.insert(CoreSourceSubject::World);
    expected.insert(CoreSourceSubject::Startup);
    for component in &program.components {
        expected.insert(CoreSourceSubject::Component {
            component_id: component.id,
        });
        for field_index in 0..component.fields.len() {
            expected.insert(CoreSourceSubject::ComponentField {
                component_id: component.id,
                field_index: u64::try_from(field_index)
                    .map_err(|_| verify_error("component field index exceeds u64"))?,
            });
        }
    }
    for resource in &program.resources {
        expected.insert(CoreSourceSubject::Resource {
            resource_id: resource.id,
        });
        for field_index in 0..resource.fields.len() {
            expected.insert(CoreSourceSubject::ResourceField {
                resource_id: resource.id,
                field_index: u64::try_from(field_index)
                    .map_err(|_| verify_error("resource field index exceeds u64"))?,
            });
        }
    }
    for system in &program.systems {
        expected.insert(CoreSourceSubject::System {
            system_id: system.id,
        });
        for (param_index, param) in system.params.iter().enumerate() {
            let param_index = u64::try_from(param_index)
                .map_err(|_| verify_error("system parameter index exceeds u64"))?;
            expected.insert(CoreSourceSubject::SystemParam {
                system_id: system.id,
                param_index,
            });
            if let CoreSystemParamKind::Query { terms } = &param.kind {
                for term_index in 0..terms.len() {
                    expected.insert(CoreSourceSubject::QueryTerm {
                        system_id: system.id,
                        param_index,
                        term_index: u64::try_from(term_index)
                            .map_err(|_| verify_error("query term index exceeds u64"))?,
                    });
                }
            }
        }
        let mut statement_ordinal = 0;
        let mut expression_ordinal = 0;
        let mut place_ordinal = 0;
        expected_system_subjects(
            system.id,
            &system.body.statements,
            &mut statement_ordinal,
            &mut expression_ordinal,
            &mut place_ordinal,
            &mut expected,
        )?;
    }
    for schedule in &program.schedules {
        expected.insert(CoreSourceSubject::Schedule {
            schedule_id: schedule.id,
        });
        for item_index in 0..schedule.items.len() {
            expected.insert(CoreSourceSubject::ScheduleItem {
                schedule_id: schedule.id,
                item_index: u64::try_from(item_index)
                    .map_err(|_| verify_error("schedule item index exceeds u64"))?,
            });
        }
    }
    for block in &startup.blocks {
        for instruction_index in 0..block.instructions.len() {
            expected.insert(CoreSourceSubject::StartupInstruction {
                block: block.id,
                instruction_index: u64::try_from(instruction_index)
                    .map_err(|_| verify_error("Core instruction index exceeds u64"))?,
            });
        }
        expected.insert(CoreSourceSubject::StartupTerminator { block: block.id });
    }

    if actual != expected {
        let missing = expected.difference(&actual).next();
        let extra = actual.difference(&expected).next();
        return Err(verify_error(format!(
            "Core source map is incomplete or non-canonical (missing {missing:?}, extra {extra:?})"
        )));
    }
    Ok(())
}

fn valid_source_span(span: crate::lexer::SourceSpan) -> bool {
    let empty_bytes = span.start.byte == span.end.byte;
    let identical_position =
        (span.start.line, span.start.column) == (span.end.line, span.end.column);

    span.start.byte <= span.end.byte
        && span.start.line >= 1
        && span.start.column >= 1
        && span.end.line >= span.start.line
        && span.end.column >= 1
        && (span.end.line > span.start.line || span.end.column >= span.start.column)
        && empty_bytes == identical_position
}

fn expected_system_subjects(
    system_id: u64,
    statements: &[CoreSystemStatement],
    statement_ordinal: &mut u64,
    expression_ordinal: &mut u64,
    place_ordinal: &mut u64,
    expected: &mut HashSet<CoreSourceSubject>,
) -> Result<(), CoreVerifyError> {
    for statement in statements {
        let ordinal = *statement_ordinal;
        *statement_ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| verify_error("system statement ordinal space exhausted"))?;
        expected.insert(CoreSourceSubject::SystemStatement {
            system_id,
            statement_ordinal: ordinal,
        });
        match statement {
            CoreSystemStatement::Expression(expression) => {
                expected_system_expression(system_id, expression, expression_ordinal, expected)?
            }
            CoreSystemStatement::Let { value, .. } => {
                expected_system_expression(system_id, value, expression_ordinal, expected)?
            }
            CoreSystemStatement::Assign { value, .. }
            | CoreSystemStatement::AddAssign { value, .. } => {
                let ordinal = *place_ordinal;
                *place_ordinal = ordinal
                    .checked_add(1)
                    .ok_or_else(|| verify_error("system place ordinal space exhausted"))?;
                expected.insert(CoreSourceSubject::SystemPlace {
                    system_id,
                    place_ordinal: ordinal,
                });
                expected_system_expression(system_id, value, expression_ordinal, expected)?;
            }
            CoreSystemStatement::QueryLoop(query) => expected_system_subjects(
                system_id,
                &query.body,
                statement_ordinal,
                expression_ordinal,
                place_ordinal,
                expected,
            )?,
            CoreSystemStatement::Block(body) => expected_system_subjects(
                system_id,
                body,
                statement_ordinal,
                expression_ordinal,
                place_ordinal,
                expected,
            )?,
            CoreSystemStatement::If {
                condition,
                then_body,
                else_body,
            } => {
                expected_system_expression(system_id, condition, expression_ordinal, expected)?;
                expected_system_subjects(
                    system_id,
                    then_body,
                    statement_ordinal,
                    expression_ordinal,
                    place_ordinal,
                    expected,
                )?;
                expected_system_subjects(
                    system_id,
                    else_body,
                    statement_ordinal,
                    expression_ordinal,
                    place_ordinal,
                    expected,
                )?;
            }
            CoreSystemStatement::While { condition, body } => {
                expected_system_expression(system_id, condition, expression_ordinal, expected)?;
                expected_system_subjects(
                    system_id,
                    body,
                    statement_ordinal,
                    expression_ordinal,
                    place_ordinal,
                    expected,
                )?;
            }
        }
    }
    Ok(())
}

fn expected_system_expression(
    system_id: u64,
    expression: &CoreSystemExpression,
    ordinal: &mut u64,
    expected: &mut HashSet<CoreSourceSubject>,
) -> Result<(), CoreVerifyError> {
    let current = *ordinal;
    *ordinal = current
        .checked_add(1)
        .ok_or_else(|| verify_error("system expression ordinal space exhausted"))?;
    expected.insert(CoreSourceSubject::SystemExpression {
        system_id,
        expression_ordinal: current,
    });
    match expression {
        CoreSystemExpression::BoolNot(operand) | CoreSystemExpression::Unary { operand, .. } => {
            expected_system_expression(system_id, operand, ordinal, expected)?;
        }
        CoreSystemExpression::Binary { left, right, .. } => {
            expected_system_expression(system_id, left, ordinal, expected)?;
            expected_system_expression(system_id, right, ordinal, expected)?;
        }
        CoreSystemExpression::I32Const(_)
        | CoreSystemExpression::F32Const(_)
        | CoreSystemExpression::BoolConst(_)
        | CoreSystemExpression::Local { .. }
        | CoreSystemExpression::ResourceField { .. }
        | CoreSystemExpression::ComponentField { .. } => {}
    }
    Ok(())
}

fn verify_schemas(program: &CoreProgram) -> Result<CoreSchemas<'_>, CoreVerifyError> {
    let mut components_by_id = HashMap::new();
    let mut components_by_name = HashMap::new();
    for (component_index, component) in program.components.iter().enumerate() {
        let kind = match component.kind {
            CoreComponentKind::Component => "component",
            CoreComponentKind::Tag => "tag",
        };
        insert_schema(
            kind,
            component.id,
            &component.name,
            component,
            &mut components_by_id,
            &mut components_by_name,
        )?;
        require_qualified_name(&program.world.name, &component.name, kind)?;
        let expected_id = dense_index(component_index, "component declaration")?;
        if component.id != expected_id {
            return Err(verify_error(format!(
                "component `{}` id {} does not match dense declaration id {expected_id}",
                component.name, component.id
            )));
        }
        if component.kind == CoreComponentKind::Tag && !component.fields.is_empty() {
            return Err(verify_error(format!(
                "Core tag `{}` must have no fields",
                component.name
            )));
        }
        verify_schema_fields(kind, &component.name, &component.fields)?;
    }

    let mut resources_by_id = HashMap::new();
    let mut resources_by_name = HashMap::new();
    for (resource_index, resource) in program.resources.iter().enumerate() {
        insert_schema(
            "resource",
            resource.id,
            &resource.name,
            resource,
            &mut resources_by_id,
            &mut resources_by_name,
        )?;
        require_qualified_name(&program.world.name, &resource.name, "resource")?;
        let schema_prefix = program.components.len();
        let expected_id = dense_offset(schema_prefix, resource_index, "resource declaration")?;
        if resource.id != expected_id {
            return Err(verify_error(format!(
                "resource `{}` id {} does not match dense declaration id {expected_id}",
                resource.name, resource.id
            )));
        }
        verify_schema_fields("resource", &resource.name, &resource.fields)?;
    }

    let mut systems_by_id = HashMap::new();
    let mut systems_by_name = HashMap::new();
    for (system_index, system) in program.systems.iter().enumerate() {
        insert_schema(
            "system",
            system.id,
            &system.name,
            system,
            &mut systems_by_id,
            &mut systems_by_name,
        )?;
        let expected_id = dense_index(system_index, "system declaration")?;
        if system.id != expected_id {
            return Err(verify_error(format!(
                "system `{}` id {} does not match dense declaration id {expected_id}",
                system.name, system.id
            )));
        }
    }

    let mut schedules_by_id = HashMap::new();
    let mut schedules_by_name = HashMap::new();
    for schedule in &program.schedules {
        insert_schema(
            "schedule",
            schedule.id,
            &schedule.name,
            schedule,
            &mut schedules_by_id,
            &mut schedules_by_name,
        )?;
    }

    Ok(CoreSchemas {
        world_name: &program.world.name,
        components_by_id,
        components_by_name,
        resources_by_id,
        resources_by_name,
        systems_by_id,
        systems_by_name,
        schedules_by_id,
        schedules_by_name,
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
    let mut resource_accesses = HashMap::new();

    for param in &system.params {
        if params.insert(param.name.as_str(), param).is_some() {
            return Err(verify_error(format!(
                "duplicate parameter `{}` in Core system `{}`",
                param.name, system.name
            )));
        }

        match &param.kind {
            CoreSystemParamKind::ReadResource { resource_id, name }
            | CoreSystemParamKind::MutResource { resource_id, name } => {
                resolve_resource(schemas, *resource_id, name)?;
                let mutable = matches!(&param.kind, CoreSystemParamKind::MutResource { .. });
                if let Some(previous) = resource_accesses.insert(*resource_id, mutable) {
                    if previous || mutable {
                        return Err(verify_error(format!(
                            "conflicting Core resource access for `{name}`"
                        )));
                    }
                }
            }
            CoreSystemParamKind::Query { terms } => {
                for term in terms {
                    let component = resolve_component(schemas, term.component_id, &term.name)?;
                    if component.kind == CoreComponentKind::Tag
                        && term.access == CoreQueryAccess::Mut
                    {
                        return Err(verify_error(format!(
                            "mutable Core tag query term `{}` is invalid",
                            term.name
                        )));
                    }
                    if let Some(previous) = query_accesses.get(&term.component_id).copied() {
                        if (previous == CoreQueryAccess::Exclude)
                            != (term.access == CoreQueryAccess::Exclude)
                        {
                            return Err(verify_error(format!(
                                "Core query both includes and excludes `{}`",
                                term.name
                            )));
                        }
                        if previous == CoreQueryAccess::Mut || term.access == CoreQueryAccess::Mut {
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
    let mut locals = HashMap::new();
    verify_system_statements(
        &system.body.statements,
        schemas,
        &params,
        &bindings,
        &mut locals,
        false,
    )
}

fn verify_system_statements<'a>(
    statements: &'a [CoreSystemStatement],
    schemas: &CoreSchemas<'a>,
    params: &HashMap<&'a str, &'a CoreSystemParam>,
    bindings: &HashMap<&'a str, &'a CoreQueryLoopBinding>,
    locals: &mut HashMap<&'a str, (CoreType, bool)>,
    inside_query: bool,
) -> Result<(), CoreVerifyError> {
    for statement in statements {
        match statement {
            CoreSystemStatement::QueryLoop(query_loop) => {
                if inside_query {
                    return Err(verify_error("nested Core query loops are not supported"));
                }
                verify_query_loop(query_loop, schemas, params, locals)?;
            }
            CoreSystemStatement::Expression(expression) => {
                verify_system_expression(expression, schemas, params, bindings, locals)?;
            }
            CoreSystemStatement::Let {
                name,
                ty,
                mutable,
                value,
            } => {
                if params.contains_key(name.as_str())
                    || bindings.contains_key(name.as_str())
                    || locals.contains_key(name.as_str())
                {
                    return Err(verify_error(format!(
                        "duplicate active Core binding `{name}`"
                    )));
                }
                let value_type =
                    verify_system_expression(value, schemas, params, bindings, locals)?;
                if value_type != *ty {
                    return Err(verify_error(format!(
                        "Core local `{name}` initializer has type {value_type:?}, expected {ty:?}"
                    )));
                }
                locals.insert(name.as_str(), (*ty, *mutable));
            }
            CoreSystemStatement::Assign { target, value } => {
                let target_type = verify_system_place(target, schemas, params, bindings, locals)?;
                let value_type =
                    verify_system_expression(value, schemas, params, bindings, locals)?;
                if target_type != value_type {
                    return Err(verify_error(
                        "Core assignment target and value types do not match",
                    ));
                }
            }
            CoreSystemStatement::AddAssign { target, value } => {
                let target_type = verify_system_place(target, schemas, params, bindings, locals)?;
                let value_type =
                    verify_system_expression(value, schemas, params, bindings, locals)?;
                if !matches!(target_type, CoreType::I32 | CoreType::F32)
                    || value_type != target_type
                {
                    return Err(verify_error(
                        "Core add-assign requires matching numeric target and value types",
                    ));
                }
            }
            CoreSystemStatement::Block(statements) => {
                let mut scoped = locals.clone();
                verify_system_statements(
                    statements,
                    schemas,
                    params,
                    bindings,
                    &mut scoped,
                    inside_query,
                )?;
            }
            CoreSystemStatement::If {
                condition,
                then_body,
                else_body,
            } => {
                if verify_system_expression(condition, schemas, params, bindings, locals)?
                    != CoreType::Bool
                {
                    return Err(verify_error("Core if condition must be bool"));
                }
                let mut then_locals = locals.clone();
                verify_system_statements(
                    then_body,
                    schemas,
                    params,
                    bindings,
                    &mut then_locals,
                    inside_query,
                )?;
                let mut else_locals = locals.clone();
                verify_system_statements(
                    else_body,
                    schemas,
                    params,
                    bindings,
                    &mut else_locals,
                    inside_query,
                )?;
            }
            CoreSystemStatement::While { condition, body } => {
                if verify_system_expression(condition, schemas, params, bindings, locals)?
                    != CoreType::Bool
                {
                    return Err(verify_error("Core while condition must be bool"));
                }
                let mut body_locals = locals.clone();
                verify_system_statements(
                    body,
                    schemas,
                    params,
                    bindings,
                    &mut body_locals,
                    inside_query,
                )?;
            }
        }
    }
    Ok(())
}

fn verify_query_loop<'a>(
    query_loop: &'a CoreQueryLoop,
    schemas: &CoreSchemas<'a>,
    params: &HashMap<&'a str, &'a CoreSystemParam>,
    outer_locals: &HashMap<&'a str, (CoreType, bool)>,
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
    let required_terms = terms
        .iter()
        .filter(|term| term.access != CoreQueryAccess::Exclude)
        .collect::<Vec<_>>();
    if query_loop.bindings.len() != required_terms.len() {
        return Err(verify_error(format!(
            "Core query loop binding count {} does not match term count {}",
            query_loop.bindings.len(),
            required_terms.len()
        )));
    }

    let mut bindings = HashMap::new();
    for (binding, term) in query_loop.bindings.iter().zip(required_terms) {
        if binding.component_id != term.component_id
            || binding.component_name != term.name
            || binding.access != term.access
        {
            return Err(verify_error(format!(
                "Core query binding `{}` does not match its query term",
                binding.name
            )));
        }
        let component = resolve_component(schemas, binding.component_id, &binding.component_name)?;
        if component.fields.is_empty() && binding.name != "_" {
            return Err(verify_error(format!(
                "zero-sized Core query term `{}` must bind to `_`",
                component.name
            )));
        }
        if binding.name != "_" {
            if params.contains_key(binding.name.as_str())
                || outer_locals.contains_key(binding.name.as_str())
            {
                return Err(verify_error(format!(
                    "duplicate active Core binding `{}`",
                    binding.name
                )));
            }
            if bindings.insert(binding.name.as_str(), binding).is_some() {
                return Err(verify_error(format!(
                    "duplicate Core query loop binding `{}`",
                    binding.name
                )));
            }
        }
    }

    let mut locals = outer_locals.clone();
    verify_system_statements(
        &query_loop.body,
        schemas,
        params,
        &bindings,
        &mut locals,
        true,
    )
}

fn verify_system_place(
    place: &CoreSystemPlace,
    schemas: &CoreSchemas<'_>,
    params: &HashMap<&str, &CoreSystemParam>,
    bindings: &HashMap<&str, &CoreQueryLoopBinding>,
    locals: &HashMap<&str, (CoreType, bool)>,
) -> Result<CoreType, CoreVerifyError> {
    match place {
        CoreSystemPlace::Local { name, ty, mutable } => {
            let (resolved_type, resolved_mutable) =
                locals.get(name.as_str()).copied().ok_or_else(|| {
                    verify_error(format!("unknown Core local assignment target `{name}`"))
                })?;
            if resolved_type != *ty || resolved_mutable != *mutable {
                return Err(verify_error(format!(
                    "Core local assignment target `{name}` does not match its declaration"
                )));
            }
            if !mutable {
                return Err(verify_error(format!("Core local `{name}` is not mutable")));
            }
            Ok(*ty)
        }
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
        CoreSystemPlace::ResourceField {
            param,
            resource_id,
            resource_name,
            field_name,
        } => {
            let resolved = params.get(param.as_str()).copied().ok_or_else(|| {
                verify_error(format!("unknown Core resource parameter `{param}`"))
            })?;
            let CoreSystemParamKind::MutResource {
                resource_id: expected_id,
                name: expected_name,
            } = &resolved.kind
            else {
                return Err(verify_error(format!(
                    "Core resource parameter `{param}` is not mutable"
                )));
            };
            if expected_id != resource_id || expected_name != resource_name {
                return Err(verify_error(
                    "Core resource place does not match its parameter",
                ));
            }
            let resource = resolve_resource(schemas, *resource_id, resource_name)?;
            resolve_field("resource", &resource.name, &resource.fields, field_name)
        }
    }
}

fn verify_system_expression(
    expression: &CoreSystemExpression,
    schemas: &CoreSchemas<'_>,
    params: &HashMap<&str, &CoreSystemParam>,
    bindings: &HashMap<&str, &CoreQueryLoopBinding>,
    locals: &HashMap<&str, (CoreType, bool)>,
) -> Result<CoreType, CoreVerifyError> {
    match expression {
        CoreSystemExpression::I32Const(_) => Ok(CoreType::I32),
        CoreSystemExpression::F32Const(_) => Ok(CoreType::F32),
        CoreSystemExpression::BoolConst(_) => Ok(CoreType::Bool),
        CoreSystemExpression::Local { name, ty } => {
            let (resolved_type, _) = locals
                .get(name.as_str())
                .copied()
                .ok_or_else(|| verify_error(format!("unknown Core local `{name}`")))?;
            if resolved_type != *ty {
                return Err(verify_error(format!(
                    "Core local expression `{name}` has mismatched type"
                )));
            }
            Ok(*ty)
        }
        CoreSystemExpression::ResourceField {
            param,
            resource_id,
            resource_name,
            field_name,
        } => {
            let resolved_param = params.get(param.as_str()).copied().ok_or_else(|| {
                verify_error(format!("unknown Core resource parameter `{param}`"))
            })?;
            let (param_id, param_name) = match &resolved_param.kind {
                CoreSystemParamKind::ReadResource { resource_id, name }
                | CoreSystemParamKind::MutResource { resource_id, name } => (resource_id, name),
                CoreSystemParamKind::Query { .. } => {
                    return Err(verify_error(format!(
                        "Core parameter `{param}` is not a resource"
                    )));
                }
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
        CoreSystemExpression::BoolNot(operand) => {
            let operand_type =
                verify_system_expression(operand, schemas, params, bindings, locals)?;
            if operand_type != CoreType::Bool {
                return Err(verify_error("Core bool.not requires a bool operand"));
            }
            Ok(CoreType::Bool)
        }
        CoreSystemExpression::Unary { op, operand } => {
            let operand_type =
                verify_system_expression(operand, schemas, params, bindings, locals)?;
            match (op, operand_type) {
                (crate::core::CoreSystemUnaryOp::I32Negate, CoreType::I32)
                | (crate::core::CoreSystemUnaryOp::I32BitNot, CoreType::I32) => Ok(CoreType::I32),
                (crate::core::CoreSystemUnaryOp::F32Negate, CoreType::F32) => Ok(CoreType::F32),
                (crate::core::CoreSystemUnaryOp::BoolNot, CoreType::Bool) => Ok(CoreType::Bool),
                _ => Err(verify_error(
                    "typed Core unary operator has wrong operand type",
                )),
            }
        }
        CoreSystemExpression::Binary { op, left, right } => {
            let left_type = verify_system_expression(left, schemas, params, bindings, locals)?;
            let right_type = verify_system_expression(right, schemas, params, bindings, locals)?;
            if left_type != right_type {
                return Err(verify_error(
                    "Core system binary operands must have matching types",
                ));
            }
            match op {
                crate::core::CoreSystemBinaryOp::F32Multiply
                | crate::core::CoreSystemBinaryOp::F32Add
                | crate::core::CoreSystemBinaryOp::F32Subtract
                | crate::core::CoreSystemBinaryOp::F32Divide
                    if left_type == CoreType::F32 =>
                {
                    Ok(CoreType::F32)
                }
                crate::core::CoreSystemBinaryOp::I32Add
                | crate::core::CoreSystemBinaryOp::I32Subtract
                | crate::core::CoreSystemBinaryOp::I32Multiply
                | crate::core::CoreSystemBinaryOp::I32Divide
                | crate::core::CoreSystemBinaryOp::I32Remainder
                | crate::core::CoreSystemBinaryOp::I32ShiftLeft
                | crate::core::CoreSystemBinaryOp::I32ShiftRight
                | crate::core::CoreSystemBinaryOp::I32BitAnd
                | crate::core::CoreSystemBinaryOp::I32BitXor
                | crate::core::CoreSystemBinaryOp::I32BitOr
                    if left_type == CoreType::I32 =>
                {
                    Ok(CoreType::I32)
                }
                crate::core::CoreSystemBinaryOp::Equal
                | crate::core::CoreSystemBinaryOp::NotEqual => Ok(CoreType::Bool),
                crate::core::CoreSystemBinaryOp::LogicalAnd
                | crate::core::CoreSystemBinaryOp::LogicalOr
                    if left_type == CoreType::Bool =>
                {
                    Ok(CoreType::Bool)
                }
                crate::core::CoreSystemBinaryOp::I32Less
                | crate::core::CoreSystemBinaryOp::I32LessEqual
                | crate::core::CoreSystemBinaryOp::I32Greater
                | crate::core::CoreSystemBinaryOp::I32GreaterEqual
                    if left_type == CoreType::I32 =>
                {
                    Ok(CoreType::Bool)
                }
                crate::core::CoreSystemBinaryOp::F32Less
                | crate::core::CoreSystemBinaryOp::F32LessEqual
                | crate::core::CoreSystemBinaryOp::F32Greater
                | crate::core::CoreSystemBinaryOp::F32GreaterEqual
                    if left_type == CoreType::F32 =>
                {
                    Ok(CoreType::Bool)
                }
                _ => Err(verify_error(format!(
                    "Core system operator {op:?} is not defined for {left_type:?}"
                ))),
            }
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
    for (schedule_index, schedule) in program.schedules.iter().enumerate() {
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
        let expected_id = dense_index(schedule_index, "schedule declaration")?;
        if schedule.id != expected_id {
            return Err(verify_error(format!(
                "schedule `{}` id {} does not match dense declaration id {expected_id}",
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
                    if system_name.as_str() != expected_name
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

    let blocks = function
        .blocks
        .iter()
        .map(|block| (block.id, block))
        .collect::<HashMap<_, _>>();
    let mut value_ids = HashSet::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Some(result) = instruction_result(instruction) {
                if !value_ids.insert(result) {
                    return Err(verify_error(format!("duplicate value {}", result.0)));
                }
            }
        }
    }
    let mut predecessors: HashMap<crate::core::BlockId, Vec<crate::core::BlockId>> = function
        .blocks
        .iter()
        .map(|block| (block.id, Vec::new()))
        .collect();
    for block in &function.blocks {
        for target in terminator_targets(&block.terminator) {
            if !blocks.contains_key(&target) {
                return Err(verify_error(format!(
                    "Core block {} targets missing block {}",
                    block.id.0, target.0
                )));
            }
            predecessors
                .get_mut(&target)
                .expect("target block has predecessor storage")
                .push(block.id);
        }
    }

    let reachable = reachable_blocks(function.entry, &blocks);
    if reachable.len() != function.blocks.len() {
        return Err(verify_error(format!(
            "Core function `{}` contains unreachable blocks",
            function.name
        )));
    }

    let all_locals = locals.keys().copied().collect::<HashSet<_>>();
    let mut initialized_in = function
        .blocks
        .iter()
        .map(|block| {
            (
                block.id,
                if block.id == function.entry {
                    HashSet::new()
                } else {
                    all_locals.clone()
                },
            )
        })
        .collect::<HashMap<_, _>>();
    loop {
        let mut changed = false;
        for block in &function.blocks {
            if block.id == function.entry {
                continue;
            }
            let incoming = predecessors[&block.id]
                .iter()
                .map(|predecessor| {
                    initialized_after_block(blocks[predecessor], &initialized_in[predecessor])
                })
                .reduce(|left, right| left.intersection(&right).copied().collect())
                .unwrap_or_default();
            if initialized_in[&block.id] != incoming {
                initialized_in.insert(block.id, incoming);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    for block in &function.blocks {
        verify_block(block, &locals, schemas, &initialized_in[&block.id])?;
    }

    Ok(())
}

fn instruction_result(instruction: &CoreInstruction) -> Option<ValueId> {
    match instruction {
        CoreInstruction::I32Const { result, .. }
        | CoreInstruction::I32Binary { result, .. }
        | CoreInstruction::I32Unary { result, .. }
        | CoreInstruction::F32Const { result, .. }
        | CoreInstruction::F32Unary { result, .. }
        | CoreInstruction::F32Binary { result, .. }
        | CoreInstruction::Compare { result, .. }
        | CoreInstruction::BoolConst { result, .. }
        | CoreInstruction::BoolNot { result, .. }
        | CoreInstruction::Equal { result, .. }
        | CoreInstruction::LocalLoad { result, .. } => Some(*result),
        CoreInstruction::InitializeResource { .. }
        | CoreInstruction::Spawn { .. }
        | CoreInstruction::RunSchedule { .. }
        | CoreInstruction::LocalStore { .. } => None,
    }
}

fn verify_block(
    block: &CoreBlock,
    locals: &HashMap<LocalId, CoreType>,
    schemas: &CoreSchemas<'_>,
    initialized_at_entry: &HashSet<LocalId>,
) -> Result<(), CoreVerifyError> {
    let mut values = HashMap::new();
    let mut initialized_locals = initialized_at_entry.clone();

    for instruction in &block.instructions {
        match instruction {
            CoreInstruction::InitializeResource {
                resource_id,
                resource_name,
                fields,
            } => verify_resource_payload(*resource_id, resource_name, fields, schemas, &values)?,
            CoreInstruction::Spawn { components } => verify_spawn(components, schemas, &values)?,
            CoreInstruction::RunSchedule {
                schedule_id,
                schedule_name,
            } => {
                resolve_schedule(schemas, *schedule_id, schedule_name)?;
            }
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
            CoreInstruction::I32Unary {
                result, operand, ..
            } => {
                require_value(&values, *operand, CoreType::I32)?;
                define_value(&mut values, *result, CoreType::I32)?;
            }
            CoreInstruction::F32Const { result, .. } => {
                define_value(&mut values, *result, CoreType::F32)?;
            }
            CoreInstruction::F32Unary {
                result, operand, ..
            } => {
                require_value(&values, *operand, CoreType::F32)?;
                define_value(&mut values, *result, CoreType::F32)?;
            }
            CoreInstruction::F32Binary {
                result,
                left,
                right,
                ..
            } => {
                require_value(&values, *left, CoreType::F32)?;
                require_value(&values, *right, CoreType::F32)?;
                define_value(&mut values, *result, CoreType::F32)?;
            }
            CoreInstruction::Compare {
                result,
                left,
                right,
                operand_type,
                ..
            } => {
                if !matches!(operand_type, CoreType::I32 | CoreType::F32) {
                    return Err(verify_error("Core comparison operands must be numeric"));
                }
                require_value(&values, *left, *operand_type)?;
                require_value(&values, *right, *operand_type)?;
                define_value(&mut values, *result, CoreType::Bool)?;
            }
            CoreInstruction::BoolConst { result, .. } => {
                define_value(&mut values, *result, CoreType::Bool)?;
            }
            CoreInstruction::BoolNot { result, operand } => {
                require_value(&values, *operand, CoreType::Bool)?;
                define_value(&mut values, *result, CoreType::Bool)?;
            }
            CoreInstruction::Equal {
                result,
                left,
                right,
                operand_type,
                ..
            } => {
                require_value(&values, *left, *operand_type)?;
                require_value(&values, *right, *operand_type)?;
                define_value(&mut values, *result, CoreType::Bool)?;
            }
            CoreInstruction::LocalStore { local, value } => {
                let local_type = require_local(locals, *local)?;
                require_value(&values, *value, local_type)?;
                initialized_locals.insert(*local);
            }
            CoreInstruction::LocalLoad { result, local } => {
                let local_type = require_local(locals, *local)?;
                if !initialized_locals.contains(local) {
                    return Err(verify_error(format!(
                        "Core local {} is not initialized before load",
                        local.0
                    )));
                }
                define_value(&mut values, *result, local_type)?;
            }
        }
    }

    match block.terminator {
        CoreTerminator::Exit { value } => require_value(&values, value, CoreType::I32),
        CoreTerminator::Jump { .. } => Ok(()),
        CoreTerminator::Branch { condition, .. } => {
            require_value(&values, condition, CoreType::Bool)
        }
    }
}

fn terminator_targets(terminator: &CoreTerminator) -> Vec<crate::core::BlockId> {
    match terminator {
        CoreTerminator::Exit { .. } => Vec::new(),
        CoreTerminator::Jump { target } => vec![*target],
        CoreTerminator::Branch {
            then_block,
            else_block,
            ..
        } => vec![*then_block, *else_block],
    }
}

fn reachable_blocks(
    entry: crate::core::BlockId,
    blocks: &HashMap<crate::core::BlockId, &CoreBlock>,
) -> HashSet<crate::core::BlockId> {
    let mut reachable = HashSet::new();
    let mut pending = vec![entry];
    while let Some(block_id) = pending.pop() {
        if !reachable.insert(block_id) {
            continue;
        }
        pending.extend(terminator_targets(&blocks[&block_id].terminator));
    }
    reachable
}

fn verify_every_block_reaches_exit(
    function: &CoreFunction,
    exit: crate::core::BlockId,
) -> Result<(), CoreVerifyError> {
    let mut predecessors: HashMap<_, Vec<_>> = function
        .blocks
        .iter()
        .map(|block| (block.id, Vec::new()))
        .collect();
    for block in &function.blocks {
        for target in terminator_targets(&block.terminator) {
            predecessors
                .get_mut(&target)
                .expect("general Core verification checked every CFG target")
                .push(block.id);
        }
    }
    let mut reaches_exit = HashSet::new();
    let mut pending = vec![exit];
    while let Some(block) = pending.pop() {
        if reaches_exit.insert(block) {
            pending.extend(predecessors[&block].iter().copied());
        }
    }
    if reaches_exit.len() != function.blocks.len() {
        return Err(verify_error(
            "every reachable executable Core startup block must reach the final exit",
        ));
    }
    Ok(())
}

fn ordered_startup_effects(
    function: &CoreFunction,
) -> Result<Vec<CoreInstructionLocation>, CoreVerifyError> {
    let blocks = function
        .blocks
        .iter()
        .map(|block| (block.id, block))
        .collect::<HashMap<_, _>>();
    let mut incoming_count = function
        .blocks
        .iter()
        .map(|block| (block.id, 0_u64))
        .collect::<HashMap<_, _>>();
    for block in &function.blocks {
        for target in terminator_targets(&block.terminator) {
            let count = incoming_count
                .get_mut(&target)
                .expect("general Core verification checked every CFG target");
            *count = count
                .checked_add(1)
                .ok_or_else(|| verify_error("Core CFG predecessor count overflow"))?;
        }
    }

    let mut ready = VecDeque::new();
    for block in &function.blocks {
        if incoming_count[&block.id] == 0 {
            ready.push_back(block.id);
        }
    }
    let mut histories = HashMap::new();
    histories.insert(function.entry, Vec::<CoreInstructionLocation>::new());
    let mut processed = 0_u64;
    while let Some(block_id) = ready.pop_front() {
        processed = processed
            .checked_add(1)
            .ok_or_else(|| verify_error("Core CFG block count overflow"))?;
        let block = blocks[&block_id];
        let mut history = histories.remove(&block_id).ok_or_else(|| {
            verify_error(format!(
                "Core startup block {} has no executable predecessor history",
                block_id.0
            ))
        })?;
        for (index, instruction) in block.instructions.iter().enumerate() {
            if is_startup_effect(instruction) {
                history.push(CoreInstructionLocation {
                    block: block_id,
                    instruction_index: u64::try_from(index).map_err(|_| {
                        verify_error("Core instruction index exceeds the u64 Core boundary")
                    })?,
                });
            }
        }
        for target in terminator_targets(&block.terminator) {
            match histories.get(&target) {
                Some(existing) if existing != &history => {
                    return Err(verify_error(format!(
                        "Core startup side effects are control-dependent before block {}",
                        target.0
                    )));
                }
                Some(_) => {}
                None => {
                    histories.insert(target, history.clone());
                }
            }
            let count = incoming_count
                .get_mut(&target)
                .expect("general Core verification checked every CFG target");
            *count = count
                .checked_sub(1)
                .expect("Core CFG incoming edge accounting is balanced");
            if *count == 0 {
                ready.push_back(target);
            }
        }
        if matches!(block.terminator, CoreTerminator::Exit { .. }) {
            histories.insert(block_id, history);
        }
    }
    let block_count = u64::try_from(function.blocks.len())
        .map_err(|_| verify_error("Core block count exceeds the u64 Core boundary"))?;
    if processed != block_count {
        return Err(verify_error("executable Core startup CFG must be acyclic"));
    }
    let exit = function
        .blocks
        .iter()
        .find(|block| matches!(block.terminator, CoreTerminator::Exit { .. }))
        .expect("executable Core startup has a unique exit");
    histories
        .remove(&exit.id)
        .ok_or_else(|| verify_error("Core startup exit has no side-effect history"))
}

fn is_startup_effect(instruction: &CoreInstruction) -> bool {
    matches!(
        instruction,
        CoreInstruction::InitializeResource { .. }
            | CoreInstruction::Spawn { .. }
            | CoreInstruction::RunSchedule { .. }
    )
}

fn verify_startup_resource_flow(
    effects: &[CoreInstructionLocation],
    startup: &CoreFunction,
    schemas: &CoreSchemas<'_>,
) -> Result<(), CoreVerifyError> {
    let blocks = startup
        .blocks
        .iter()
        .map(|block| (block.id, block))
        .collect::<HashMap<_, _>>();
    let mut initialized = HashSet::new();
    for location in effects {
        let index = usize::try_from(location.instruction_index)
            .map_err(|_| verify_error("Core instruction index exceeds host address space"))?;
        let instruction = &blocks[&location.block].instructions[index];
        match instruction {
            CoreInstruction::InitializeResource {
                resource_id,
                resource_name,
                ..
            } => {
                resolve_resource(schemas, *resource_id, resource_name)?;
                if !initialized.insert(*resource_id) {
                    return Err(verify_error(format!(
                        "Core startup initializes resource `{resource_name}` more than once"
                    )));
                }
            }
            CoreInstruction::RunSchedule {
                schedule_id,
                schedule_name,
            } => {
                let schedule = resolve_schedule(schemas, *schedule_id, schedule_name)?;
                for item in &schedule.items {
                    let crate::core::CoreScheduleItem::Run { system_id, .. } = item;
                    let system = schemas
                        .systems_by_id
                        .get(system_id)
                        .copied()
                        .expect("general Core verification checked schedule targets");
                    for param in &system.params {
                        let (resource_id, name) = match &param.kind {
                            CoreSystemParamKind::ReadResource { resource_id, name }
                            | CoreSystemParamKind::MutResource { resource_id, name } => {
                                (resource_id, name)
                            }
                            CoreSystemParamKind::Query { .. } => continue,
                        };
                        if !initialized.contains(resource_id) {
                            return Err(verify_error(format!(
                                "Core schedule `{}` reads resource `{name}` before it is initialized",
                                schedule.name
                            )));
                        }
                    }
                }
            }
            CoreInstruction::Spawn { .. } => {}
            _ => unreachable!("verified startup effect list contains only side effects"),
        }
    }
    Ok(())
}

fn initialized_after_block(
    block: &CoreBlock,
    initialized_at_entry: &HashSet<LocalId>,
) -> HashSet<LocalId> {
    let mut initialized = initialized_at_entry.clone();
    for instruction in &block.instructions {
        if let CoreInstruction::LocalStore { local, .. } = instruction {
            initialized.insert(*local);
        }
    }
    initialized
}

fn resolve_schedule<'a>(
    schemas: &CoreSchemas<'a>,
    schedule_id: u64,
    schedule_name: &str,
) -> Result<&'a CoreSchedule, CoreVerifyError> {
    let schedule = schemas
        .schedules_by_id
        .get(&schedule_id)
        .copied()
        .ok_or_else(|| {
            verify_error(format!(
                "unresolved Core schedule `{schedule_name}` id 0x{schedule_id:016x}"
            ))
        })?;
    let expected_name = format!("{}.{}", schemas.world_name, schedule.name);
    if schedule_name != expected_name
        || schemas
            .schedules_by_name
            .get(schedule.name.as_str())
            .copied()
            != Some(schedule)
    {
        return Err(verify_error(format!(
            "Core startup schedule reference `{schedule_name}` does not match id 0x{schedule_id:016x}"
        )));
    }
    Ok(schedule)
}

fn verify_resource_payload(
    resource_id: u64,
    resource_name: &str,
    fields: &[CoreResourceField],
    schemas: &CoreSchemas<'_>,
    values: &HashMap<ValueId, CoreType>,
) -> Result<(), CoreVerifyError> {
    let resource = resolve_resource(schemas, resource_id, resource_name)?;
    verify_literal_payload(
        "resource",
        resource_name,
        &resource.fields,
        fields.iter().map(|field| {
            (
                field.name.as_str(),
                field.evaluation,
                literal_type(&field.value),
            )
        }),
        values,
    )
}

fn verify_literal_payload<'a>(
    kind: &str,
    owner: &str,
    schema_fields: &[CoreField],
    fields: impl Iterator<Item = (&'a str, ValueId, CoreType)>,
    values: &HashMap<ValueId, CoreType>,
) -> Result<(), CoreVerifyError> {
    let mut seen = HashSet::new();
    let mut field_count = 0_usize;
    for (name, evaluation, value_type) in fields {
        if !seen.insert(name) {
            return Err(verify_error(format!(
                "duplicate Core {kind} field `{owner}.{name}`"
            )));
        }
        let expected_type = resolve_field(kind, owner, schema_fields, name)?;
        if value_type != expected_type {
            return Err(verify_error(format!(
                "Core {kind} field `{owner}.{name}` has the wrong type"
            )));
        }
        require_value(values, evaluation, expected_type)?;
        let expected_field = schema_fields.get(field_count).ok_or_else(|| {
            verify_error(format!(
                "Core {kind} `{owner}` contains more fields than its schema"
            ))
        })?;
        if name != expected_field.name.as_str() {
            return Err(verify_error(format!(
                "Core {kind} `{owner}` fields must follow declaration order; expected `{}`, found `{name}`",
                expected_field.name
            )));
        }
        field_count = field_count
            .checked_add(1)
            .ok_or_else(|| verify_error("Core payload field count overflow"))?;
    }
    if let Some(missing) = schema_fields.get(field_count) {
        return Err(verify_error(format!(
            "missing field `{}` in Core {kind} `{owner}`",
            missing.name
        )));
    }
    Ok(())
}

fn literal_type(value: &CoreSpawnFieldValue) -> CoreType {
    match value {
        CoreSpawnFieldValue::F32Bits(_) => CoreType::F32,
        CoreSpawnFieldValue::I32(_) => CoreType::I32,
        CoreSpawnFieldValue::Bool(_) => CoreType::Bool,
    }
}

fn verify_spawn(
    components: &[crate::core::CoreSpawnComponent],
    schemas: &CoreSchemas<'_>,
    values: &HashMap<ValueId, CoreType>,
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
        verify_literal_payload(
            "spawn component",
            &component.name,
            &schema.fields,
            component.fields.iter().map(|field| {
                (
                    field.name.as_str(),
                    field.evaluation,
                    literal_type(&field.value),
                )
            }),
            values,
        )?;
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

fn dense_offset(
    prefix: usize,
    index: usize,
    context: &'static str,
) -> Result<u64, CoreVerifyError> {
    let dense = prefix
        .checked_add(index)
        .ok_or_else(|| verify_error(format!("{context} index overflow")))?;
    dense_index(dense, context)
}

fn dense_index(index: usize, context: &'static str) -> Result<u64, CoreVerifyError> {
    u64::try_from(index).map_err(|_| verify_error(format!("{context} index exceeds u64")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        BlockId, CoreBinaryOp, CoreBlock, CoreFunction, CoreInstruction, CoreLocal, CoreProgram,
        CoreSourceMap, CoreTerminator, CoreType, CoreWorld,
    };
    use crate::{core_lower, lexer, parser};

    fn lowered(source: &str) -> CoreProgram {
        let tokens = lexer::lex(source).expect("fixture lexes");
        let ast = parser::parse_program(&tokens).expect("fixture parses");
        core_lower::lower_program_to_core(&ast).expect("fixture lowers to Core")
    }

    fn first_startup_spawn_components(
        program: &mut CoreProgram,
    ) -> &mut Vec<crate::core::CoreSpawnComponent> {
        program.functions[0]
            .blocks
            .iter_mut()
            .flat_map(|block| block.instructions.iter_mut())
            .find_map(|instruction| match instruction {
                CoreInstruction::Spawn { components } => Some(components),
                _ => None,
            })
            .expect("fixture contains a startup spawn")
    }

    #[test]
    fn core_verifier_accepts_lowered_math_and_ecs() {
        verify_core_program(&lowered(include_str!("../../../examples/math.arc")))
            .expect("lowered math Core verifies");
        verify_core_program(&lowered(include_str!("../../../examples/move_system.arc")))
            .expect("lowered ECS Core verifies");
        verify_core_program(&lowered(include_str!(
            "../../../examples/arena_recovery.arc"
        )))
        .expect("lowered mixed f32/i32 Arena Core verifies");
    }

    #[test]
    fn core_verifier_rejects_nested_tag_only_query_loops() {
        let mut program = lowered(
            "world Main\n\
             tag Enemy\n\
             system Scan(items: query[Enemy]) { for (_) in items {} }\n\
             startup { exit 0 }\n",
        );
        let nested = match &program.systems[0].body.statements[0] {
            CoreSystemStatement::QueryLoop(query) => query.clone(),
            _ => panic!("fixture must lower an outer query loop"),
        };
        let CoreSystemStatement::QueryLoop(outer) = &mut program.systems[0].body.statements[0]
        else {
            panic!("fixture must retain its outer query loop");
        };
        outer.body.push(CoreSystemStatement::QueryLoop(nested));

        let error = verify_core_program(&program)
            .expect_err("tag-only discard bindings must not bypass nested-query rejection");
        assert!(error.message.contains("nested Core query loops"));
    }

    #[test]
    fn core_verifier_rejects_invalid_value_reference() {
        let program = CoreProgram {
            world: CoreWorld {
                name: "Main".into(),
            },
            components: vec![],
            resources: vec![],
            systems: vec![],
            schedules: vec![],
            functions: vec![CoreFunction {
                name: "startup".into(),
                entry: BlockId(0),
                locals: vec![CoreLocal {
                    id: LocalId(0),
                    name: "x".into(),
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
            source_map: CoreSourceMap::default(),
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
        *field_name = "missing".into();

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
        let components = first_startup_spawn_components(&mut spawn_program);
        components[0].fields[0].name = "missing".into();
        let error = verify_core_program(&spawn_program).expect_err("unknown spawn field must fail");
        assert!(error.message.contains("unknown Core field `missing`"));
    }

    #[test]
    fn core_verifier_rejects_incomplete_spawn_literals() {
        let mut program = lowered(include_str!("../../../examples/spawn_position.arc"));
        let components = first_startup_spawn_components(&mut program);
        components[0].fields.pop();

        let error =
            verify_core_program(&program).expect_err("Core spawn must initialize every field");
        assert!(error.message.contains("missing field `y`"));
        assert!(error
            .message
            .contains("Core spawn component `Demo.Position`"));
    }

    #[test]
    fn core_verifier_rejects_schema_identity_and_schedule_target_mismatches() {
        let mut identity_program = lowered(include_str!("../../../examples/move_system.arc"));
        identity_program.components[0].id ^= 1;
        let error = verify_core_program(&identity_program).expect_err("wrong dense id must fail");
        assert!(error
            .message
            .contains("does not match dense declaration id"));

        let mut resource_program = lowered(include_str!("../../../examples/move_system.arc"));
        let CoreSystemParamKind::ReadResource { name, .. } =
            &mut resource_program.systems[0].params[0].kind
        else {
            panic!("expected resource parameter");
        };
        *name = "Demo.Missing".into();
        let error =
            verify_core_program(&resource_program).expect_err("mismatched resource name must fail");
        assert!(error.message.contains("unresolved Core resource"));

        let mut schedule_program = lowered(include_str!("../../../examples/move_system.arc"));
        let crate::core::CoreScheduleItem::Run { system_name, .. } =
            &mut schedule_program.schedules[0].items[0];
        *system_name = "Demo.Missing".into();
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

        let mut integer_spawn_program =
            lowered(include_str!("../../../examples/arena_recovery.arc"));
        let faction_field = integer_spawn_program.functions[0].blocks[0]
            .instructions
            .iter_mut()
            .find_map(|instruction| match instruction {
                CoreInstruction::Spawn { components } => components
                    .iter_mut()
                    .find(|component| component.name == "Arena.Faction")
                    .and_then(|component| component.fields.first_mut()),
                _ => None,
            })
            .expect("Arena contains an i32 faction spawn field");
        faction_field.value = CoreSpawnFieldValue::F32Bits(1.0f32.to_bits());
        let error = verify_core_program(&integer_spawn_program)
            .expect_err("f32 Core value for i32 spawn field must fail");
        assert!(error.message.contains("has the wrong type"));

        let mut float_spawn_program = lowered(include_str!("../../../examples/spawn_position.arc"));
        let components = first_startup_spawn_components(&mut float_spawn_program);
        components[0].fields[0].value = CoreSpawnFieldValue::I32(1);
        let error = verify_core_program(&float_spawn_program)
            .expect_err("i32 Core value for f32 spawn field must fail");
        assert!(error.message.contains("has the wrong type"));
    }

    #[test]
    fn core_verifier_rejects_invalid_local_reference() {
        let mut local_program = lowered(include_str!("../../../examples/math.arc"));
        let CoreInstruction::LocalStore { local, .. } =
            &mut local_program.functions[0].blocks[0].instructions[3]
        else {
            panic!("expected local store");
        };
        *local = LocalId(99);
        let error = verify_core_program(&local_program).expect_err("unknown local must fail");
        assert!(error.message.contains("undefined local 99"));
    }

    #[test]
    fn core_verifier_rejects_local_load_before_store() {
        let program = CoreProgram {
            world: CoreWorld {
                name: "Main".into(),
            },
            components: vec![],
            resources: vec![],
            systems: vec![],
            schedules: vec![],
            functions: vec![CoreFunction {
                name: "startup".into(),
                entry: BlockId(0),
                locals: vec![CoreLocal {
                    id: LocalId(0),
                    name: "status".into(),
                    ty: CoreType::I32,
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
            source_map: CoreSourceMap::default(),
        };

        let error =
            verify_core_program(&program).expect_err("uninitialized Core local load must fail");
        assert_eq!(error.message, "Core local 0 is not initialized before load");
    }

    #[test]
    fn executable_core_rejects_tampered_resource_payload_run_target_and_flow() {
        let mut payload = lowered(include_str!("../../../examples/move_system.arc"));
        let fields = payload.functions[0].blocks[0]
            .instructions
            .iter_mut()
            .find_map(|instruction| match instruction {
                CoreInstruction::InitializeResource { fields, .. } => Some(fields),
                _ => None,
            })
            .expect("movement fixture initializes Time");
        fields.clear();
        let error =
            verify_core_program(&payload).expect_err("resource payload must remain exhaustive");
        assert!(error.message.contains("missing field `delta`"));

        let mut target = lowered(include_str!("../../../examples/move_system.arc"));
        let schedule_id = target.functions[0].blocks[0]
            .instructions
            .iter_mut()
            .find_map(|instruction| match instruction {
                CoreInstruction::RunSchedule { schedule_id, .. } => Some(schedule_id),
                _ => None,
            })
            .expect("movement fixture dispatches Main");
        *schedule_id ^= 1;
        let error =
            verify_core_program(&target).expect_err("run target must resolve by id and name");
        assert!(error.message.contains("unresolved Core schedule"));

        let mut flow = lowered(include_str!("../../../examples/move_system.arc"));
        let instructions = &mut flow.functions[0].blocks[0].instructions;
        let run_index = instructions
            .iter()
            .position(|instruction| matches!(instruction, CoreInstruction::RunSchedule { .. }))
            .expect("movement fixture dispatches Main");
        let run = instructions.remove(run_index);
        instructions.insert(0, run);
        let error = verify_executable_core(flow)
            .expect_err("schedule resource flow must follow Core order");
        assert_eq!(
            error.message,
            "Core schedule `Main` reads resource `Demo.Time` before it is initialized"
        );
    }

    #[test]
    fn core_verifier_intersects_definite_local_initialization_at_branches() {
        let program = CoreProgram {
            world: CoreWorld {
                name: "Main".into(),
            },
            components: vec![],
            resources: vec![],
            systems: vec![],
            schedules: vec![],
            functions: vec![CoreFunction {
                name: "startup".into(),
                entry: BlockId(0),
                locals: vec![CoreLocal {
                    id: LocalId(0),
                    name: "status".into(),
                    ty: CoreType::I32,
                }],
                blocks: vec![
                    CoreBlock {
                        id: BlockId(0),
                        instructions: vec![CoreInstruction::BoolConst {
                            result: ValueId(0),
                            value: true,
                        }],
                        terminator: CoreTerminator::Branch {
                            condition: ValueId(0),
                            then_block: BlockId(1),
                            else_block: BlockId(2),
                        },
                    },
                    CoreBlock {
                        id: BlockId(1),
                        instructions: vec![
                            CoreInstruction::I32Const {
                                result: ValueId(1),
                                value: 47,
                            },
                            CoreInstruction::LocalStore {
                                local: LocalId(0),
                                value: ValueId(1),
                            },
                        ],
                        terminator: CoreTerminator::Jump { target: BlockId(3) },
                    },
                    CoreBlock {
                        id: BlockId(2),
                        instructions: vec![],
                        terminator: CoreTerminator::Jump { target: BlockId(3) },
                    },
                    CoreBlock {
                        id: BlockId(3),
                        instructions: vec![CoreInstruction::LocalLoad {
                            result: ValueId(2),
                            local: LocalId(0),
                        }],
                        terminator: CoreTerminator::Exit { value: ValueId(2) },
                    },
                ],
            }],
            source_map: CoreSourceMap::default(),
        };

        let error =
            verify_core_program(&program).expect_err("one-path initialization is not definite");
        assert_eq!(error.message, "Core local 0 is not initialized before load");
    }

    #[test]
    fn executable_core_preserves_startup_effect_order_across_short_circuit_cfgs() {
        let program = lowered(
            "world Demo
component Mark { value: i32 }
startup {
  spawn { Mark { value: 1 } }
  let keep: bool = true || false
  spawn { Mark { value: 2 } }
  exit 0
}",
        );
        let verified =
            verify_executable_core(program).expect("short-circuit startup Core verifies");
        let values = verified
            .startup_operations()
            .filter_map(|instruction| match instruction {
                CoreInstruction::Spawn { components } => match components[0].fields[0].value {
                    CoreSpawnFieldValue::I32(value) => Some(value),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(values, [1, 2]);
    }

    #[test]
    fn executable_core_verifier_brands_reachable_multi_block_startup_cfgs() {
        let program = lowered(include_str!("../../../examples/math.arc"));
        let verified =
            verify_executable_core(program.clone()).expect("lowered executable Core verifies");
        assert_eq!(verified.program(), &program);

        let mut missing_startup = program.clone();
        missing_startup.functions.clear();
        let error = verify_executable_core(missing_startup)
            .expect_err("executable Core without startup must fail");
        assert_eq!(
            error.message,
            "executable Core must contain exactly one `startup` function and no other functions"
        );

        let mut extra_function = program.clone();
        let mut helper = extra_function.functions[0].clone();
        helper.name = "helper".into();
        extra_function.functions.push(helper);
        let error = verify_executable_core(extra_function)
            .expect_err("the M26 executable feature set has no helper functions");
        assert_eq!(
            error.message,
            "executable Core must contain exactly one `startup` function and no other functions"
        );

        let multi_block = lowered(
            "world Main startup { let mut ready: bool = false ready = true && !false exit 0 }",
        );
        assert!(multi_block.functions[0].blocks.len() > 1);
        verify_executable_core(multi_block)
            .expect("reachable short-circuit startup CFG must verify");
    }
}
