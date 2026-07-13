use std::collections::{HashMap, HashSet};

use crate::layout;
use crate::lexer::Span;
use crate::parser::{
    BinaryOperator, ComponentDecl, ComponentLiteralValue, Expression, Program, QueryAccess,
    QueryTerm, ResourceDecl, ScheduleItem, Statement, SystemBodyStatement, SystemParam,
    SystemParamKind, SystemQueryLoopStatement,
};
use crate::runtime;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckError {
    pub span: Span,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Type {
    I32,
    F32,
}

impl Type {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "i32" => Some(Self::I32),
            "f32" => Some(Self::F32),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::I32 => "i32",
            Self::F32 => "f32",
        }
    }
}

struct SemanticTables<'a> {
    components: HashMap<&'a str, &'a ComponentDecl>,
    resources: HashMap<&'a str, &'a ResourceDecl>,
}

/// Runs the authoritative source semantic pass.
///
/// The pass first validates and indexes declarations, then resolves every use.
/// Keeping those phases together prevents individual compiler modes from
/// accidentally accepting a different language subset.
pub fn check_program(program: &Program) -> Result<(), CheckError> {
    let tables = build_semantic_tables(program)?;
    check_systems(program, &tables)?;
    check_schedules(program)?;
    check_startup(program, &tables)
}

fn build_semantic_tables(program: &Program) -> Result<SemanticTables<'_>, CheckError> {
    let mut components = HashMap::new();
    let mut component_ids = HashMap::new();
    for component in &program.components {
        if components
            .insert(component.name.as_str(), component)
            .is_some()
        {
            return Err(check_error(
                component.name_span,
                format!("duplicate component declaration `{}`", component.name),
            ));
        }

        let id = layout::stable_component_id(&program.world.name, &component.name).0;
        if let Some(previous) = component_ids.insert(id, component.name.as_str()) {
            return Err(check_error(
                component.name_span,
                format!(
                    "component `{}` has the same stable id as `{previous}`",
                    component.name
                ),
            ));
        }

        check_declared_fields(
            "component",
            &component.name,
            component.fields.iter().map(|field| {
                (
                    field.name.as_str(),
                    field.name_span,
                    field.type_name.name.as_str(),
                    field.type_name.span,
                )
            }),
        )?;
    }

    let mut resources = HashMap::new();
    let mut resource_ids = HashMap::new();
    for resource in &program.resources {
        if resources.insert(resource.name.as_str(), resource).is_some() {
            return Err(check_error(
                resource.name_span,
                format!("duplicate resource declaration `{}`", resource.name),
            ));
        }

        let id = runtime::stable_resource_id(&program.world.name, &resource.name).0;
        if let Some(previous) = resource_ids.insert(id, resource.name.as_str()) {
            return Err(check_error(
                resource.name_span,
                format!(
                    "resource `{}` has the same stable id as `{previous}`",
                    resource.name
                ),
            ));
        }

        check_declared_fields(
            "resource",
            &resource.name,
            resource.fields.iter().map(|field| {
                (
                    field.name.as_str(),
                    field.name_span,
                    field.type_name.name.as_str(),
                    field.type_name.span,
                )
            }),
        )?;
    }

    check_unique_systems(program)?;
    check_unique_schedules(program)?;

    Ok(SemanticTables {
        components,
        resources,
    })
}

fn check_declared_fields<'a>(
    kind: &str,
    owner: &str,
    fields: impl Iterator<Item = (&'a str, Span, &'a str, Span)>,
) -> Result<(), CheckError> {
    let mut names = HashSet::new();
    for (name, name_span, type_name, type_span) in fields {
        if !names.insert(name) {
            return Err(check_error(
                name_span,
                format!("duplicate field `{name}` in {kind} `{owner}`"),
            ));
        }
        if Type::from_name(type_name).is_none() {
            return Err(check_error(
                type_span,
                format!("unknown primitive type `{type_name}` for {kind} field `{owner}.{name}`"),
            ));
        }
    }
    Ok(())
}

fn check_unique_systems(program: &Program) -> Result<(), CheckError> {
    let mut names = HashSet::new();
    let mut ids = HashMap::new();
    for system in &program.systems {
        if !names.insert(system.name.as_str()) {
            return Err(check_error(
                system.name_span,
                format!("duplicate system declaration `{}`", system.name),
            ));
        }
        let id = runtime::stable_system_id(&program.world.name, &system.name).0;
        if let Some(previous) = ids.insert(id, system.name.as_str()) {
            return Err(check_error(
                system.name_span,
                format!(
                    "system `{}` has the same stable id as `{previous}`",
                    system.name
                ),
            ));
        }
    }
    Ok(())
}

fn check_unique_schedules(program: &Program) -> Result<(), CheckError> {
    let mut names = HashSet::new();
    let mut ids = HashMap::new();
    for schedule in &program.schedules {
        if !names.insert(schedule.name.as_str()) {
            return Err(check_error(
                schedule.name_span,
                format!("duplicate schedule declaration `{}`", schedule.name),
            ));
        }
        let id = runtime::stable_schedule_id(&program.world.name, &schedule.name).0;
        if let Some(previous) = ids.insert(id, schedule.name.as_str()) {
            return Err(check_error(
                schedule.name_span,
                format!(
                    "schedule `{}` has the same stable id as `{previous}`",
                    schedule.name
                ),
            ));
        }
    }
    Ok(())
}

fn check_systems(program: &Program, tables: &SemanticTables<'_>) -> Result<(), CheckError> {
    for system in &program.systems {
        let mut params = HashMap::new();
        let mut query_accesses = HashMap::new();

        for param in &system.params {
            if params.insert(param.name.as_str(), param).is_some() {
                return Err(check_error(
                    param.name_span,
                    format!(
                        "duplicate parameter `{}` in system `{}`",
                        param.name, system.name
                    ),
                ));
            }

            match &param.kind {
                SystemParamKind::ReadResource {
                    resource_name,
                    resource_span,
                } => {
                    if !tables.resources.contains_key(resource_name.as_str()) {
                        return Err(check_error(
                            *resource_span,
                            format!("unknown resource `{resource_name}` in system parameter"),
                        ));
                    }
                }
                SystemParamKind::Query { terms } => {
                    for term in terms {
                        if !tables.components.contains_key(term.component_name.as_str()) {
                            return Err(check_error(
                                term.component_span,
                                format!("unknown component `{}` in query", term.component_name),
                            ));
                        }

                        if let Some(previous_access) =
                            query_accesses.get(term.component_name.as_str())
                        {
                            if *previous_access == QueryAccess::Mut
                                || term.access == QueryAccess::Mut
                            {
                                return Err(check_error(
                                    term.component_span,
                                    format!(
                                        "conflicting query access for component `{}`",
                                        term.component_name
                                    ),
                                ));
                            }
                        } else {
                            query_accesses.insert(term.component_name.as_str(), term.access);
                        }
                    }
                }
            }
        }

        for statement in &system.body.statements {
            match statement {
                SystemBodyStatement::QueryLoop(query_loop) => {
                    check_query_loop(query_loop, tables, &params)?;
                }
                SystemBodyStatement::Expression(expression) => {
                    return Err(check_error(
                        expression_span(expression),
                        "top-level system expressions are not lowerable; use a query loop",
                    ));
                }
                SystemBodyStatement::AddAssign(add_assign) => {
                    return Err(check_error(
                        expression_span(&add_assign.target),
                        "top-level system updates are not lowerable; use a query loop",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn check_query_loop<'a>(
    query_loop: &'a SystemQueryLoopStatement,
    tables: &SemanticTables<'a>,
    params: &HashMap<&'a str, &'a SystemParam>,
) -> Result<(), CheckError> {
    let Some(param) = params.get(query_loop.query_param.as_str()).copied() else {
        return Err(check_error(
            query_loop.query_span,
            format!("unknown query parameter `{}`", query_loop.query_param),
        ));
    };
    let SystemParamKind::Query { terms } = &param.kind else {
        return Err(check_error(
            query_loop.query_span,
            format!(
                "query loop target `{}` is not a query parameter",
                query_loop.query_param
            ),
        ));
    };

    if query_loop.bindings.len() != terms.len() {
        let span = query_loop
            .bindings
            .get(terms.len())
            .map_or(query_loop.query_span, |binding| binding.span);
        return Err(check_error(
            span,
            format!(
                "query loop binding count {} does not match query term count {}",
                query_loop.bindings.len(),
                terms.len()
            ),
        ));
    }

    let mut bindings = HashMap::new();
    for (binding, term) in query_loop.bindings.iter().zip(terms) {
        if bindings.insert(binding.name.as_str(), term).is_some() {
            return Err(check_error(
                binding.span,
                format!("duplicate query loop binding `{}`", binding.name),
            ));
        }
    }

    for statement in &query_loop.body {
        match statement {
            SystemBodyStatement::Expression(expression) => {
                check_system_expression(expression, tables, params, &bindings)?;
            }
            SystemBodyStatement::AddAssign(add_assign) => {
                let target_type = check_system_place(&add_assign.target, tables, &bindings)?;
                let value_type =
                    check_system_expression(&add_assign.value, tables, params, &bindings)?;
                if target_type != Type::F32 {
                    return Err(check_error(
                        expression_span(&add_assign.target),
                        "only f32 component fields can be updated in system bodies",
                    ));
                }
                if target_type != value_type {
                    return Err(check_error(
                        expression_span(&add_assign.value),
                        format!(
                            "cannot add {} expression to {} component field",
                            value_type.name(),
                            target_type.name()
                        ),
                    ));
                }
            }
            SystemBodyStatement::QueryLoop(nested) => {
                return Err(check_error(
                    nested.query_span,
                    "nested query loops are not lowerable yet",
                ));
            }
        }
    }

    Ok(())
}

fn check_system_place(
    expression: &Expression,
    tables: &SemanticTables<'_>,
    bindings: &HashMap<&str, &QueryTerm>,
) -> Result<Type, CheckError> {
    let Expression::FieldAccess {
        target,
        field_name,
        field_span,
    } = expression
    else {
        return Err(check_error(
            expression_span(expression),
            "add-assign target must be a component field",
        ));
    };
    let Expression::Identifier { name, span } = &**target else {
        return Err(check_error(
            expression_span(target),
            "add-assign target must be a direct component binding field",
        ));
    };
    let Some(term) = bindings.get(name.as_str()).copied() else {
        return Err(check_error(
            *span,
            format!("unknown component binding `{name}` in add-assign target"),
        ));
    };
    if term.access != QueryAccess::Mut {
        return Err(check_error(
            *span,
            format!("add-assign target `{name}` is not mutable"),
        ));
    }

    let component = tables.components[term.component_name.as_str()];
    component_field_type(component, field_name, *field_span)
}

fn check_system_expression(
    expression: &Expression,
    tables: &SemanticTables<'_>,
    params: &HashMap<&str, &SystemParam>,
    bindings: &HashMap<&str, &QueryTerm>,
) -> Result<Type, CheckError> {
    match expression {
        Expression::FieldAccess {
            target,
            field_name,
            field_span,
        } => {
            let Expression::Identifier { name, span } = &**target else {
                return Err(check_error(
                    expression_span(target),
                    "nested system field access is not lowerable yet",
                ));
            };

            if let Some(term) = bindings.get(name.as_str()).copied() {
                let component = tables.components[term.component_name.as_str()];
                return component_field_type(component, field_name, *field_span);
            }

            if let Some(param) = params.get(name.as_str()).copied() {
                let SystemParamKind::ReadResource { resource_name, .. } = &param.kind else {
                    return Err(check_error(
                        *span,
                        format!("system parameter `{name}` is not a read resource"),
                    ));
                };
                let resource = tables.resources[resource_name.as_str()];
                return resource_field_type(resource, field_name, *field_span);
            }

            Err(check_error(
                *span,
                format!("unknown system body field target `{name}`"),
            ))
        }
        Expression::Binary(binary) => {
            if binary.operator != BinaryOperator::Multiply {
                return Err(check_error(
                    expression_span(expression),
                    format!(
                        "system body operator `{}` is not lowerable yet",
                        binary.operator
                    ),
                ));
            }
            let left = check_system_expression(&binary.left, tables, params, bindings)?;
            let right = check_system_expression(&binary.right, tables, params, bindings)?;
            if left != Type::F32 || right != Type::F32 {
                return Err(check_error(
                    expression_span(expression),
                    "system multiplication requires f32 operands",
                ));
            }
            Ok(Type::F32)
        }
        Expression::Identifier { name, span } => Err(check_error(
            *span,
            format!("system body identifier `{name}` requires a field access"),
        )),
        Expression::Integer(integer) => Err(check_error(
            integer.span,
            "integer literals are not lowerable in system bodies yet",
        )),
    }
}

fn component_field_type(
    component: &ComponentDecl,
    field_name: &str,
    field_span: Span,
) -> Result<Type, CheckError> {
    component
        .fields
        .iter()
        .find(|field| field.name == field_name)
        .and_then(|field| Type::from_name(&field.type_name.name))
        .ok_or_else(|| {
            check_error(
                field_span,
                format!(
                    "unknown field `{field_name}` for component `{}`",
                    component.name
                ),
            )
        })
}

fn resource_field_type(
    resource: &ResourceDecl,
    field_name: &str,
    field_span: Span,
) -> Result<Type, CheckError> {
    resource
        .fields
        .iter()
        .find(|field| field.name == field_name)
        .and_then(|field| Type::from_name(&field.type_name.name))
        .ok_or_else(|| {
            check_error(
                field_span,
                format!(
                    "unknown field `{field_name}` for resource `{}`",
                    resource.name
                ),
            )
        })
}

fn check_schedules(program: &Program) -> Result<(), CheckError> {
    let systems = program
        .systems
        .iter()
        .map(|system| system.name.as_str())
        .collect::<HashSet<_>>();

    for schedule in &program.schedules {
        for item in &schedule.items {
            match item {
                ScheduleItem::Run {
                    system_name,
                    system_span,
                } if !systems.contains(system_name.as_str()) => {
                    return Err(check_error(
                        *system_span,
                        format!("unknown system `{system_name}` in schedule"),
                    ));
                }
                ScheduleItem::Run { .. } => {}
            }
        }
    }
    Ok(())
}

fn check_startup(program: &Program, tables: &SemanticTables<'_>) -> Result<(), CheckError> {
    let Some(startup) = &program.startup else {
        return Ok(());
    };

    let mut bindings = HashMap::new();
    let mut initialized_resources = HashSet::new();
    let mut exited = false;
    for statement in &startup.statements {
        if exited {
            return Err(check_error(
                statement_span(statement, program.world.name_span),
                "statement after startup exit",
            ));
        }

        match statement {
            Statement::Let(let_statement) => {
                if bindings.contains_key(let_statement.name.as_str()) {
                    return Err(check_error(
                        let_statement.name_span,
                        format!("duplicate local `{}`", let_statement.name),
                    ));
                }
                let Some(declared_type) = Type::from_name(&let_statement.type_name.name) else {
                    return Err(check_error(
                        let_statement.type_name.span,
                        format!("unknown local type `{}`", let_statement.type_name.name),
                    ));
                };
                if declared_type != Type::I32 {
                    return Err(check_error(
                        let_statement.type_name.span,
                        "only i32 startup locals are lowerable",
                    ));
                }
                let initializer_type =
                    check_startup_expression(&let_statement.initializer, &bindings)?;
                if initializer_type != declared_type {
                    return Err(check_error(
                        expression_span(&let_statement.initializer),
                        format!(
                            "cannot initialize {} local with {} expression",
                            declared_type.name(),
                            initializer_type.name()
                        ),
                    ));
                }
                bindings.insert(let_statement.name.as_str(), declared_type);
            }
            Statement::Exit(exit) => {
                let exit_type = check_startup_expression(&exit.expression, &bindings)?;
                if exit_type != Type::I32 {
                    return Err(check_error(
                        expression_span(&exit.expression),
                        "startup exit requires an i32 expression",
                    ));
                }
                if let Expression::Integer(integer) = &exit.expression {
                    if integer.value > u64::from(u8::MAX) {
                        return Err(check_error(
                            integer.span,
                            "literal process exit status must be in the range 0..=255",
                        ));
                    }
                }
                exited = true;
            }
            Statement::Run(run) => {
                if !program
                    .schedules
                    .iter()
                    .any(|schedule| schedule.name == run.schedule_name)
                {
                    return Err(check_error(
                        run.schedule_span,
                        format!("unknown schedule `{}` in startup", run.schedule_name),
                    ));
                }
            }
            Statement::Spawn(spawn) => check_spawn(spawn, tables)?,
            Statement::Resource(resource) => {
                if !initialized_resources.insert(resource.name.as_str()) {
                    return Err(check_error(
                        resource.name_span,
                        format!("duplicate startup resource `{}`", resource.name),
                    ));
                }
                check_resource_literal(resource, tables)?;
            }
        }
    }
    Ok(())
}

fn check_spawn(
    spawn: &crate::parser::SpawnStatement,
    tables: &SemanticTables<'_>,
) -> Result<(), CheckError> {
    let mut components = HashSet::new();
    for literal in &spawn.components {
        if !components.insert(literal.name.as_str()) {
            return Err(check_error(
                literal.name_span,
                format!("duplicate component `{}` in spawn", literal.name),
            ));
        }
        let Some(component) = tables.components.get(literal.name.as_str()).copied() else {
            return Err(check_error(
                literal.name_span,
                format!("unknown component `{}` in spawn", literal.name),
            ));
        };

        let mut fields = HashSet::new();
        for field in &literal.fields {
            if !fields.insert(field.name.as_str()) {
                return Err(check_error(
                    field.name_span,
                    format!(
                        "duplicate field `{}` in component literal `{}`",
                        field.name, literal.name
                    ),
                ));
            }
            let field_type = component_field_type(component, &field.name, field.name_span)?;
            check_literal_value(
                &field.value,
                field_type,
                &format!("component field `{}.{}`", literal.name, field.name),
            )?;
        }
    }
    Ok(())
}

fn check_resource_literal(
    literal: &crate::parser::ResourceStatement,
    tables: &SemanticTables<'_>,
) -> Result<(), CheckError> {
    let Some(resource) = tables.resources.get(literal.name.as_str()).copied() else {
        return Err(check_error(
            literal.name_span,
            format!("unknown resource `{}` in startup", literal.name),
        ));
    };

    let mut fields = HashSet::new();
    for field in &literal.fields {
        if !fields.insert(field.name.as_str()) {
            return Err(check_error(
                field.name_span,
                format!(
                    "duplicate field `{}` in resource literal `{}`",
                    field.name, literal.name
                ),
            ));
        }
        let field_type = resource_field_type(resource, &field.name, field.name_span)?;
        check_literal_value(
            &field.value,
            field_type,
            &format!("resource field `{}.{}`", literal.name, field.name),
        )?;
    }
    Ok(())
}

fn check_literal_value(
    value: &ComponentLiteralValue,
    expected: Type,
    label: &str,
) -> Result<(), CheckError> {
    match value {
        ComponentLiteralValue::Float { text, span } => {
            if expected != Type::F32 {
                return Err(check_error(
                    *span,
                    format!(
                        "float literal cannot initialize {expected_name} {label}",
                        expected_name = expected.name()
                    ),
                ));
            }
            text.parse::<f32>().map_err(|_| {
                check_error(*span, format!("invalid f32 literal `{text}` for {label}"))
            })?;
        }
    }
    Ok(())
}

fn check_startup_expression(
    expression: &Expression,
    bindings: &HashMap<&str, Type>,
) -> Result<Type, CheckError> {
    match expression {
        Expression::Integer(integer) => {
            if integer.value > i32::MAX as u64 {
                Err(check_error(
                    integer.span,
                    "integer literal does not fit i32",
                ))
            } else {
                Ok(Type::I32)
            }
        }
        Expression::Identifier { name, span } => bindings
            .get(name.as_str())
            .copied()
            .ok_or_else(|| check_error(*span, format!("unknown local variable `{name}`"))),
        Expression::FieldAccess { field_span, .. } => Err(check_error(
            *field_span,
            "field access is only supported inside system query loops",
        )),
        Expression::Binary(binary) => {
            let left_type = check_startup_expression(&binary.left, bindings)?;
            let right_type = check_startup_expression(&binary.right, bindings)?;
            if left_type == Type::I32 && right_type == Type::I32 {
                Ok(Type::I32)
            } else {
                Err(check_error(
                    expression_span(expression),
                    "expected i32 operands for arithmetic expression",
                ))
            }
        }
    }
}

fn statement_span(statement: &Statement, fallback: Span) -> Span {
    match statement {
        Statement::Let(statement) => statement.name_span,
        Statement::Run(statement) => statement.schedule_span,
        Statement::Spawn(statement) => statement
            .components
            .first()
            .map_or(fallback, |component| component.name_span),
        Statement::Resource(statement) => statement.name_span,
        Statement::Exit(statement) => expression_span(&statement.expression),
    }
}

fn expression_span(expression: &Expression) -> Span {
    match expression {
        Expression::Integer(integer) => integer.span,
        Expression::Identifier { span, .. } => *span,
        Expression::FieldAccess { target, .. } => expression_span(target),
        Expression::Binary(binary) => expression_span(&binary.left),
    }
}

fn check_error(span: Span, message: impl Into<String>) -> CheckError {
    CheckError {
        span,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser};

    fn check(source: &str) -> Result<(), CheckError> {
        let tokens = lexer::lex(source).expect("fixture lexes");
        let program = parser::parse_program(&tokens).expect("fixture parses");
        check_program(&program)
    }

    #[test]
    fn accepts_supported_ecs_program() {
        check(include_str!("../../../examples/move_system.arc"))
            .expect("supported ECS source checks");
    }

    #[test]
    fn rejects_duplicate_declaration_at_second_name() {
        let source = "world Demo\ncomponent Position { x: f32 }\ncomponent Position { y: f32 }\nstartup { exit 0 }\n";
        let second = source.rfind("Position").unwrap();
        let error = check(source).expect_err("duplicate declaration must fail");
        assert_eq!(
            error.span,
            Span {
                start: second,
                end: second + "Position".len()
            }
        );
        assert!(error.message.contains("duplicate component"));
    }

    #[test]
    fn rejects_duplicate_literal_field_at_second_name() {
        let source = "world Demo\ncomponent Position { x: f32 }\nstartup { spawn { Position { x: 1.0, x: 2.0 } } exit 0 }\n";
        let second = source.rfind("x:").unwrap();
        let error = check(source).expect_err("duplicate literal field must fail");
        assert_eq!(
            error.span,
            Span {
                start: second,
                end: second + 1
            }
        );
        assert!(error.message.contains("duplicate field"));
    }

    #[test]
    fn rejects_unknown_system_field_at_field_span() {
        let source =
            include_str!("../../../examples/move_system.arc").replace("time.delta", "time.missing");
        let field = source.find("missing").unwrap();
        let error = check(&source).expect_err("unknown field must fail");
        assert_eq!(
            error.span,
            Span {
                start: field,
                end: field + "missing".len()
            }
        );
        assert!(error.message.contains("unknown field `missing`"));
    }

    #[test]
    fn rejects_i32_and_direct_exit_ranges_at_literal_spans() {
        let too_large_i32 = "world Demo startup { let x: i32 = 2147483648 exit x }";
        let error = check(too_large_i32).expect_err("out-of-range i32 must fail");
        assert_eq!(error.span.start, too_large_i32.find("2147483648").unwrap());
        assert!(error.message.contains("does not fit i32"));

        let too_large_exit = "world Demo startup { exit 256 }";
        let error = check(too_large_exit).expect_err("out-of-range process status must fail");
        assert_eq!(error.span.start, too_large_exit.find("256").unwrap());
        assert!(error.message.contains("0..=255"));
    }

    #[test]
    fn permits_repeated_read_only_query_terms_but_rejects_mutable_aliases() {
        let read_only = "world Demo component Position { x: f32 } system ReadBoth(q: query[Position, Position]) { for (a, b) in q { a.x b.x } } startup { exit 0 }";
        check(read_only).expect("repeated read-only terms are legal");

        let mutable =
            read_only.replace("query[Position, Position]", "query[mut Position, Position]");
        let error = check(&mutable).expect_err("mutable aliases must fail");
        assert!(error.message.contains("conflicting query access"));
    }

    #[test]
    fn rejects_every_duplicate_scope() {
        let cases = [
            (
                "world Demo resource Time { delta: f32 } resource Time { delta: f32 } startup { exit 0 }",
                "duplicate resource declaration",
            ),
            (
                "world Demo system Tick() {} system Tick() {} startup { exit 0 }",
                "duplicate system declaration",
            ),
            (
                "world Demo schedule Main {} schedule Main {} startup { exit 0 }",
                "duplicate schedule declaration",
            ),
            (
                "world Demo component Position { x: f32 x: f32 } startup { exit 0 }",
                "duplicate field `x` in component",
            ),
            (
                "world Demo resource Time { delta: f32 delta: f32 } startup { exit 0 }",
                "duplicate field `delta` in resource",
            ),
            (
                "world Demo resource Time { delta: f32 } system Tick(time: read Time, time: read Time) {} startup { exit 0 }",
                "duplicate parameter `time`",
            ),
            (
                "world Demo component Position { x: f32 } system Tick(q: query[Position, Position]) { for (item, item) in q { item.x } } startup { exit 0 }",
                "duplicate query loop binding `item`",
            ),
            (
                "world Demo startup { let x: i32 = 1 let x: i32 = 2 exit 0 }",
                "duplicate local `x`",
            ),
            (
                "world Demo component Position { x: f32 } startup { spawn { Position { x: 1.0 } Position { x: 2.0 } } exit 0 }",
                "duplicate component `Position` in spawn",
            ),
            (
                "world Demo resource Time { delta: f32 } startup { resource Time { delta: 1.0, delta: 2.0 } exit 0 }",
                "duplicate field `delta` in resource literal",
            ),
            (
                "world Demo resource Time { delta: f32 } startup { resource Time { delta: 1.0 } resource Time { delta: 2.0 } exit 0 }",
                "duplicate startup resource `Time`",
            ),
        ];

        for (source, expected) in cases {
            let error = check(source).expect_err(expected);
            assert!(
                error.message.contains(expected),
                "expected `{expected}`, got `{}`",
                error.message
            );
            assert_ne!(error.span, Span { start: 0, end: 0 });
        }
    }

    #[test]
    fn permits_cross_kind_name_reuse_and_repeated_schedule_items() {
        let source = "world Demo
component Shared { value: f32 }
resource Shared { value: f32 }
system Shared() {}
schedule Shared { run Shared run Shared }
startup {
  resource Shared { value: 1.0 }
  spawn { Shared { value: 2.0 } }
  run Shared
  exit 0
}";

        check(source).expect("separate namespaces and repeated schedule runs are legal");
    }

    #[test]
    fn accepts_integer_and_process_status_boundaries() {
        check("world Demo startup { let max: i32 = 2147483647 exit 255 }")
            .expect("i32::MAX and exit status 255 are accepted");
    }

    #[test]
    fn rejects_unknown_startup_references_and_fields() {
        let cases = [
            (
                "world Demo startup { run Missing exit 0 }",
                "unknown schedule `Missing`",
            ),
            (
                "world Demo startup { resource Missing { value: 1.0 } exit 0 }",
                "unknown resource `Missing`",
            ),
            (
                "world Demo startup { spawn { Missing { value: 1.0 } } exit 0 }",
                "unknown component `Missing`",
            ),
            (
                "world Demo component Position { x: f32 } startup { spawn { Position { missing: 1.0 } } exit 0 }",
                "unknown field `missing` for component `Position`",
            ),
            (
                "world Demo resource Time { delta: f32 } startup { resource Time { missing: 1.0 } exit 0 }",
                "unknown field `missing` for resource `Time`",
            ),
        ];

        for (source, expected) in cases {
            let error = check(source).expect_err(expected);
            assert!(
                error.message.contains(expected),
                "expected `{expected}`, got `{}`",
                error.message
            );
        }
    }

    #[test]
    fn rejects_query_binding_count_mutability_and_type_mismatch() {
        let binding_count = "world Demo component Position { x: f32 } system Tick(q: query[Position, Position]) { for (item) in q { item.x } } startup { exit 0 }";
        let error = check(binding_count).expect_err("binding count mismatch must fail");
        assert!(error.message.contains("binding count 1"));

        let immutable = "world Demo component Position { x: f32 } system Tick(q: query[Position]) { for (pos) in q { pos.x += pos.x } } startup { exit 0 }";
        let error = check(immutable).expect_err("immutable update must fail");
        assert!(error.message.contains("is not mutable"));

        let type_mismatch = "world Demo component Position { x: f32 } resource Count { value: i32 } system Tick(count: read Count, q: query[mut Position]) { for (pos) in q { pos.x += count.value } } startup { exit 0 }";
        let error = check(type_mismatch).expect_err("add-assign type mismatch must fail");
        assert!(error.message.contains("cannot add i32 expression"));
    }

    #[test]
    fn rejects_statement_after_exit_at_the_next_statement_span() {
        let source = "world Demo startup { exit 0 let later: i32 = 1 }";
        let later = source.find("later").unwrap();
        let error = check(source).expect_err("statement after exit must fail before Core lowering");
        assert_eq!(
            error.span,
            Span {
                start: later,
                end: later + "later".len(),
            }
        );
        assert!(error.message.contains("statement after startup exit"));
    }
}
