use std::collections::{HashMap, HashSet};

use crate::lexer::SourceSpan as Span;
use crate::parser::{
    BinaryOperator, ComponentDecl, ComponentLiteralValue, Expression, Program, QueryAccess,
    QueryTerm, ResourceDecl, ScheduleItem, Statement, SystemBodyStatement, SystemParam,
    SystemParamKind, SystemQueryLoopStatement, TagDecl, UnaryOperator,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckError {
    pub span: Span,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Type {
    I32,
    F32,
    Bool,
}

impl Type {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "i32" => Some(Self::I32),
            "f32" => Some(Self::F32),
            "bool" => Some(Self::Bool),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::I32 => "i32",
            Self::F32 => "f32",
            Self::Bool => "bool",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LocalBinding {
    ty: Type,
    mutable: bool,
}

#[derive(Clone, Copy)]
enum QueryableSchema<'a> {
    Component(&'a ComponentDecl),
    Tag(&'a TagDecl),
}

impl<'a> QueryableSchema<'a> {
    fn name(self) -> &'a str {
        match self {
            Self::Component(component) => &component.name,
            Self::Tag(tag) => &tag.name,
        }
    }

    fn fields(self) -> &'a [crate::parser::ComponentField] {
        match self {
            Self::Component(component) => &component.fields,
            Self::Tag(_) => &[],
        }
    }

    fn is_tag(self) -> bool {
        matches!(self, Self::Tag(_))
    }
}

struct SemanticTables<'a> {
    components: HashMap<&'a str, QueryableSchema<'a>>,
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

/// Validates declarations without requiring an executable startup program.
///
/// This is intentionally narrower than [`check_program`] and exists for
/// declaration inspection. Compiler modes that lower or execute a program must
/// use the authoritative executable pass instead.
pub fn check_declarations(program: &Program) -> Result<(), CheckError> {
    build_semantic_tables(program).map(|_| ())
}

fn build_semantic_tables(program: &Program) -> Result<SemanticTables<'_>, CheckError> {
    let mut components = HashMap::new();
    for component in &program.components {
        if components
            .insert(
                component.name.as_str(),
                QueryableSchema::Component(component),
            )
            .is_some()
        {
            return Err(check_error(
                component.name_span,
                format!("duplicate component declaration `{}`", component.name),
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
    for tag in &program.tags {
        if components
            .insert(tag.name.as_str(), QueryableSchema::Tag(tag))
            .is_some()
        {
            return Err(check_error(
                tag.name_span,
                format!("duplicate queryable schema declaration `{}`", tag.name),
            ));
        }
    }

    let mut resources = HashMap::new();
    for resource in &program.resources {
        if resources.insert(resource.name.as_str(), resource).is_some() {
            return Err(check_error(
                resource.name_span,
                format!("duplicate resource declaration `{}`", resource.name),
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
    for system in &program.systems {
        if !names.insert(system.name.as_str()) {
            return Err(check_error(
                system.name_span,
                format!("duplicate system declaration `{}`", system.name),
            ));
        }

        let mut parameters = HashSet::new();
        for parameter in &system.params {
            if !parameters.insert(parameter.name.as_str()) {
                return Err(check_error(
                    parameter.name_span,
                    format!(
                        "duplicate parameter `{}` in system `{}`",
                        parameter.name, system.name
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn check_unique_schedules(program: &Program) -> Result<(), CheckError> {
    let mut names = HashSet::new();
    for schedule in &program.schedules {
        if !names.insert(schedule.name.as_str()) {
            return Err(check_error(
                schedule.name_span,
                format!("duplicate schedule declaration `{}`", schedule.name),
            ));
        }
    }
    Ok(())
}

fn check_systems(program: &Program, tables: &SemanticTables<'_>) -> Result<(), CheckError> {
    for system in &program.systems {
        let mut params = HashMap::new();
        let mut query_accesses = HashMap::new();
        let mut resource_accesses = HashMap::new();

        for param in &system.params {
            params.insert(param.name.as_str(), param);

            match &param.kind {
                SystemParamKind::ReadResource {
                    resource_name,
                    resource_span,
                }
                | SystemParamKind::MutResource {
                    resource_name,
                    resource_span,
                } => {
                    if !tables.resources.contains_key(resource_name.as_str()) {
                        return Err(check_error(
                            *resource_span,
                            format!("unknown resource `{resource_name}` in system parameter"),
                        ));
                    }
                    let mutable = matches!(&param.kind, SystemParamKind::MutResource { .. });
                    if let Some(previous_mutable) =
                        resource_accesses.insert(resource_name.as_str(), mutable)
                    {
                        if previous_mutable || mutable {
                            return Err(check_error(
                                *resource_span,
                                format!(
                                    "conflicting resource access for `{resource_name}` in system `{}`",
                                    system.name
                                ),
                            ));
                        }
                    }
                }
                SystemParamKind::Query { terms } => {
                    for term in terms {
                        let Some(schema) =
                            tables.components.get(term.component_name.as_str()).copied()
                        else {
                            return Err(check_error(
                                term.component_span,
                                format!("unknown component `{}` in query", term.component_name),
                            ));
                        };
                        if schema.is_tag() && term.access == QueryAccess::Mut {
                            return Err(check_error(
                                term.component_span,
                                format!(
                                    "mutable tag query term `{}` is invalid",
                                    term.component_name
                                ),
                            ));
                        }

                        if let Some(previous_access) =
                            query_accesses.get(term.component_name.as_str()).copied()
                        {
                            if (previous_access == QueryAccess::Exclude)
                                != (term.access == QueryAccess::Exclude)
                            {
                                return Err(check_error(
                                    term.component_span,
                                    format!(
                                        "query cannot both include and exclude component `{}`",
                                        term.component_name
                                    ),
                                ));
                            }
                            if previous_access == QueryAccess::Mut
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

        let mut locals = HashMap::new();
        let bindings = HashMap::new();
        check_system_statements(
            &system.body.statements,
            tables,
            &params,
            &bindings,
            &mut locals,
            false,
        )?;
    }
    Ok(())
}

fn check_query_loop<'a>(
    query_loop: &'a SystemQueryLoopStatement,
    tables: &SemanticTables<'a>,
    params: &HashMap<&'a str, &'a SystemParam>,
    outer_locals: &HashMap<&'a str, LocalBinding>,
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

    let required_terms = terms
        .iter()
        .filter(|term| term.access != QueryAccess::Exclude)
        .collect::<Vec<_>>();
    if query_loop.bindings.len() != required_terms.len() {
        let span = query_loop
            .bindings
            .get(required_terms.len())
            .map_or(query_loop.query_span, |binding| binding.span);
        return Err(check_error(
            span,
            format!(
                "query loop binding count {} does not match query term count {}",
                query_loop.bindings.len(),
                required_terms.len()
            ),
        ));
    }

    let mut bindings = HashMap::new();
    for (binding, term) in query_loop.bindings.iter().zip(required_terms) {
        let schema = tables.components[term.component_name.as_str()];
        if schema.fields().is_empty() && binding.name != "_" {
            return Err(check_error(
                binding.span,
                format!(
                    "zero-sized query term `{}` must bind to `_`",
                    term.component_name
                ),
            ));
        }
        if binding.name == "_" {
            continue;
        }
        if params.contains_key(binding.name.as_str())
            || outer_locals.contains_key(binding.name.as_str())
        {
            return Err(check_error(
                binding.span,
                format!("duplicate active binding `{}`", binding.name),
            ));
        }
        if bindings.insert(binding.name.as_str(), term).is_some() {
            return Err(check_error(
                binding.span,
                format!("duplicate query loop binding `{}`", binding.name),
            ));
        }
    }

    let mut locals = outer_locals.clone();
    check_system_statements(
        &query_loop.body,
        tables,
        params,
        &bindings,
        &mut locals,
        true,
    )
}

fn check_system_statements<'a>(
    statements: &'a [SystemBodyStatement],
    tables: &SemanticTables<'a>,
    params: &HashMap<&'a str, &'a SystemParam>,
    bindings: &HashMap<&'a str, &'a QueryTerm>,
    locals: &mut HashMap<&'a str, LocalBinding>,
    inside_query: bool,
) -> Result<(), CheckError> {
    for statement in statements {
        match statement {
            SystemBodyStatement::Expression(expression) => {
                check_system_expression(expression, tables, params, bindings, locals)?;
            }
            SystemBodyStatement::AddAssign(add_assign) => {
                let target_type = check_system_assignment_place(
                    &add_assign.target,
                    tables,
                    params,
                    bindings,
                    locals,
                )?;
                let value_type =
                    check_system_expression(&add_assign.value, tables, params, bindings, locals)?;
                check_add_assign_types(
                    target_type,
                    value_type,
                    expression_span(&add_assign.target),
                    expression_span(&add_assign.value),
                )?;
            }
            SystemBodyStatement::Let(let_statement) => {
                if params.contains_key(let_statement.name.as_str())
                    || bindings.contains_key(let_statement.name.as_str())
                    || locals.contains_key(let_statement.name.as_str())
                {
                    return Err(check_error(
                        let_statement.name_span,
                        format!("duplicate active binding `{}`", let_statement.name),
                    ));
                }
                let declared_type =
                    Type::from_name(&let_statement.type_name.name).ok_or_else(|| {
                        check_error(
                            let_statement.type_name.span,
                            format!("unknown local type `{}`", let_statement.type_name.name),
                        )
                    })?;
                let initializer_type = check_system_expression(
                    &let_statement.initializer,
                    tables,
                    params,
                    bindings,
                    locals,
                )?;
                if declared_type != initializer_type {
                    return Err(check_error(
                        expression_span(&let_statement.initializer),
                        format!(
                            "cannot initialize {} local with {} expression",
                            declared_type.name(),
                            initializer_type.name()
                        ),
                    ));
                }
                locals.insert(
                    let_statement.name.as_str(),
                    LocalBinding {
                        ty: declared_type,
                        mutable: let_statement.mutable,
                    },
                );
            }
            SystemBodyStatement::Assign(assignment) => {
                let target_type = check_system_assignment_place(
                    &assignment.target,
                    tables,
                    params,
                    bindings,
                    locals,
                )?;
                let value_type =
                    check_system_expression(&assignment.value, tables, params, bindings, locals)?;
                if target_type != value_type {
                    return Err(check_error(
                        expression_span(&assignment.value),
                        format!(
                            "cannot assign {} expression to {} place",
                            value_type.name(),
                            target_type.name()
                        ),
                    ));
                }
            }
            SystemBodyStatement::QueryLoop(nested) => {
                if inside_query {
                    return Err(check_error(
                        nested.query_span,
                        "nested query loops are not lowerable yet",
                    ));
                }
                check_query_loop(nested, tables, params, locals)?;
            }
            SystemBodyStatement::Block(block) => {
                let mut block_locals = locals.clone();
                check_system_statements(
                    &block.statements,
                    tables,
                    params,
                    bindings,
                    &mut block_locals,
                    inside_query,
                )?;
            }
            SystemBodyStatement::If(statement) => {
                let condition = check_system_expression(
                    &statement.condition,
                    tables,
                    params,
                    bindings,
                    locals,
                )?;
                if condition != Type::Bool {
                    return Err(check_error(
                        statement.condition.span(),
                        "system `if` condition must have type bool",
                    ));
                }
                let mut then_locals = locals.clone();
                check_system_statements(
                    &statement.then_block.statements,
                    tables,
                    params,
                    bindings,
                    &mut then_locals,
                    inside_query,
                )?;
                if let Some(block) = &statement.else_block {
                    let mut else_locals = locals.clone();
                    check_system_statements(
                        &block.statements,
                        tables,
                        params,
                        bindings,
                        &mut else_locals,
                        inside_query,
                    )?;
                }
            }
            SystemBodyStatement::While(statement) => {
                let condition = check_system_expression(
                    &statement.condition,
                    tables,
                    params,
                    bindings,
                    locals,
                )?;
                if condition != Type::Bool {
                    return Err(check_error(
                        statement.condition.span(),
                        "system `while` condition must have type bool",
                    ));
                }
                let mut body_locals = locals.clone();
                check_system_statements(
                    &statement.body.statements,
                    tables,
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

fn check_system_assignment_place(
    expression: &Expression,
    tables: &SemanticTables<'_>,
    params: &HashMap<&str, &SystemParam>,
    bindings: &HashMap<&str, &QueryTerm>,
    locals: &HashMap<&str, LocalBinding>,
) -> Result<Type, CheckError> {
    if let Expression::Identifier { name, span } = expression {
        let local = locals
            .get(name.as_str())
            .copied()
            .ok_or_else(|| check_error(*span, format!("unknown local variable `{name}`")))?;
        if !local.mutable {
            return Err(check_error(*span, format!("local `{name}` is not mutable")));
        }
        return Ok(local.ty);
    }

    check_system_place(expression, tables, params, bindings)
}

fn check_add_assign_types(
    target_type: Type,
    value_type: Type,
    target_span: Span,
    value_span: Span,
) -> Result<(), CheckError> {
    if !matches!(target_type, Type::I32 | Type::F32) {
        return Err(check_error(
            target_span,
            "add-assign target must have numeric type",
        ));
    }
    if target_type != value_type {
        return Err(check_error(
            value_span,
            format!(
                "cannot add {} expression to {} assignment target",
                value_type.name(),
                target_type.name()
            ),
        ));
    }
    Ok(())
}

fn check_system_place(
    expression: &Expression,
    tables: &SemanticTables<'_>,
    params: &HashMap<&str, &SystemParam>,
    bindings: &HashMap<&str, &QueryTerm>,
) -> Result<Type, CheckError> {
    let Expression::FieldAccess {
        target,
        field_name,
        field_span,
        ..
    } = expression
    else {
        return Err(check_error(
            expression_span(expression),
            "assignment target must be a mutable local or direct mutable field",
        ));
    };
    let Expression::Identifier { name, span } = &**target else {
        return Err(check_error(
            expression_span(target),
            "assignment target must be a direct binding or resource field",
        ));
    };
    if let Some(term) = bindings.get(name.as_str()).copied() {
        if term.access != QueryAccess::Mut {
            return Err(check_error(
                *span,
                format!("assignment target `{name}` is not mutable"),
            ));
        }
        let component = tables.components[term.component_name.as_str()];
        return component_field_type(component, field_name, *field_span);
    }

    if let Some(param) = params.get(name.as_str()).copied() {
        let SystemParamKind::MutResource { resource_name, .. } = &param.kind else {
            return Err(check_error(
                *span,
                format!("resource parameter `{name}` is not mutable"),
            ));
        };
        return resource_field_type(
            tables.resources[resource_name.as_str()],
            field_name,
            *field_span,
        );
    }

    Err(check_error(
        *span,
        format!("unknown assignment target `{name}`"),
    ))
}

fn check_system_expression(
    expression: &Expression,
    tables: &SemanticTables<'_>,
    params: &HashMap<&str, &SystemParam>,
    bindings: &HashMap<&str, &QueryTerm>,
    locals: &HashMap<&str, LocalBinding>,
) -> Result<Type, CheckError> {
    match expression {
        Expression::FieldAccess {
            target,
            field_name,
            field_span,
            ..
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
                let resource_name = match &param.kind {
                    SystemParamKind::ReadResource { resource_name, .. }
                    | SystemParamKind::MutResource { resource_name, .. } => resource_name,
                    SystemParamKind::Query { .. } => {
                        return Err(check_error(
                            *span,
                            format!("system parameter `{name}` is not a resource"),
                        ));
                    }
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
            let left = check_system_expression(&binary.left, tables, params, bindings, locals)?;
            let right = check_system_expression(&binary.right, tables, params, bindings, locals)?;
            check_binary_types(binary.operator, left, right, expression_span(expression))
        }
        Expression::Unary(unary) => {
            if unary.operator == UnaryOperator::Negate {
                if let Expression::Integer(integer) = unparenthesized(&unary.operand) {
                    if integer.value == i32::MAX as u64 + 1 {
                        return Ok(Type::I32);
                    }
                }
            }
            let operand =
                check_system_expression(&unary.operand, tables, params, bindings, locals)?;
            check_unary_type(unary.operator, operand, &unary.operand, unary.operator_span)
        }
        Expression::Identifier { name, span } => locals
            .get(name.as_str())
            .map(|local| local.ty)
            .ok_or_else(|| check_error(*span, format!("unknown local variable `{name}`"))),
        Expression::Integer(integer) => {
            if integer.value <= i32::MAX as u64 {
                Ok(Type::I32)
            } else {
                Err(check_error(
                    integer.span,
                    "integer literal does not fit i32",
                ))
            }
        }
        Expression::Float { text, span } => text
            .parse::<f32>()
            .map(|_| Type::F32)
            .map_err(|_| check_error(*span, format!("invalid f32 literal `{text}`"))),
        Expression::Bool { .. } => Ok(Type::Bool),
        Expression::Parenthesized { expression, .. } => {
            check_system_expression(expression, tables, params, bindings, locals)
        }
    }
}

fn check_binary_types(
    operator: BinaryOperator,
    left: Type,
    right: Type,
    span: Span,
) -> Result<Type, CheckError> {
    if left != right {
        return Err(check_error(
            span,
            format!(
                "operator `{operator}` requires matching operand types, found {} and {}",
                left.name(),
                right.name()
            ),
        ));
    }

    match operator {
        BinaryOperator::Add
        | BinaryOperator::Subtract
        | BinaryOperator::Multiply
        | BinaryOperator::Divide
            if matches!(left, Type::I32 | Type::F32) =>
        {
            Ok(left)
        }
        BinaryOperator::Equal | BinaryOperator::NotEqual => Ok(Type::Bool),
        BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr if left == Type::Bool => {
            Ok(Type::Bool)
        }
        BinaryOperator::Remainder
        | BinaryOperator::ShiftLeft
        | BinaryOperator::ShiftRight
        | BinaryOperator::BitAnd
        | BinaryOperator::BitXor
        | BinaryOperator::BitOr
            if left == Type::I32 =>
        {
            Ok(Type::I32)
        }
        BinaryOperator::Less
        | BinaryOperator::LessEqual
        | BinaryOperator::Greater
        | BinaryOperator::GreaterEqual
            if matches!(left, Type::I32 | Type::F32) =>
        {
            Ok(Type::Bool)
        }
        _ => Err(check_error(
            span,
            format!(
                "operator `{operator}` is not defined for {} operands",
                left.name()
            ),
        )),
    }
}

fn check_unary_type(
    operator: UnaryOperator,
    operand_type: Type,
    operand: &Expression,
    span: Span,
) -> Result<Type, CheckError> {
    match operator {
        UnaryOperator::Not if operand_type == Type::Bool => Ok(Type::Bool),
        UnaryOperator::Negate if matches!(operand_type, Type::I32 | Type::F32) => {
            if let Expression::Integer(integer) = unparenthesized(operand) {
                if integer.value > i32::MAX as u64 + 1 {
                    return Err(check_error(
                        integer.span,
                        "integer literal does not fit i32",
                    ));
                }
            }
            Ok(operand_type)
        }
        UnaryOperator::BitNot if operand_type == Type::I32 => Ok(Type::I32),
        _ => Err(check_error(
            span,
            format!(
                "operator `{operator}` is not defined for {} operand",
                operand_type.name()
            ),
        )),
    }
}

fn unparenthesized(mut expression: &Expression) -> &Expression {
    while let Expression::Parenthesized {
        expression: inner, ..
    } = expression
    {
        expression = inner;
    }
    expression
}

fn component_field_type(
    component: QueryableSchema<'_>,
    field_name: &str,
    field_span: Span,
) -> Result<Type, CheckError> {
    component
        .fields()
        .iter()
        .find(|field| field.name == field_name)
        .and_then(|field| Type::from_name(&field.type_name.name))
        .ok_or_else(|| {
            check_error(
                field_span,
                format!(
                    "unknown field `{field_name}` for component `{}`",
                    component.name()
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
                    ..
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
    let startup = match program.startups.as_slice() {
        [] => {
            return Err(check_error(
                program.eof_span,
                "executable program requires a `startup` block",
            ));
        }
        [startup] => startup,
        [_, second, ..] => {
            return Err(check_error(
                second.keyword_span,
                "multiple `startup` blocks are not allowed",
            ));
        }
    };

    let mut bindings: HashMap<&str, LocalBinding> = HashMap::new();
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
                bindings.insert(
                    let_statement.name.as_str(),
                    LocalBinding {
                        ty: declared_type,
                        mutable: let_statement.mutable,
                    },
                );
            }
            Statement::Assign(assignment) => {
                let local = check_startup_assignment_place(&assignment.target, &bindings)?;
                let value_type = check_startup_expression(&assignment.value, &bindings)?;
                if value_type != local.ty {
                    return Err(check_error(
                        expression_span(&assignment.value),
                        format!(
                            "cannot assign {} expression to {} local",
                            value_type.name(),
                            local.ty.name()
                        ),
                    ));
                }
            }
            Statement::AddAssign(add_assign) => {
                let local = check_startup_assignment_place(&add_assign.target, &bindings)?;
                let value_type = check_startup_expression(&add_assign.value, &bindings)?;
                check_add_assign_types(
                    local.ty,
                    value_type,
                    expression_span(&add_assign.target),
                    expression_span(&add_assign.value),
                )?;
            }
            Statement::Exit(exit) => {
                let exit_type = check_startup_expression(&exit.expression, &bindings)?;
                if exit_type != Type::I32 {
                    return Err(check_error(
                        expression_span(&exit.expression),
                        "startup exit requires an i32 expression",
                    ));
                }
                exited = true;
            }
            Statement::Run(run) => {
                let Some(schedule) = program
                    .schedules
                    .iter()
                    .find(|schedule| schedule.name == run.schedule_name)
                else {
                    return Err(check_error(
                        run.schedule_span,
                        format!("unknown schedule `{}` in startup", run.schedule_name),
                    ));
                };
                check_schedule_resources_initialized(
                    schedule,
                    program,
                    &initialized_resources,
                    run.schedule_span,
                )?;
            }
            Statement::Spawn(spawn) => check_spawn(spawn, tables, &bindings)?,
            Statement::Resource(resource) => {
                if !initialized_resources.insert(resource.name.as_str()) {
                    return Err(check_error(
                        resource.name_span,
                        format!("duplicate startup resource `{}`", resource.name),
                    ));
                }
                check_resource_literal(resource, tables, &bindings)?;
            }
        }
    }

    if exited {
        Ok(())
    } else {
        Err(check_error(
            startup.close_span,
            "`startup` block must terminate with `exit`",
        ))
    }
}

fn check_schedule_resources_initialized(
    schedule: &crate::parser::ScheduleDecl,
    program: &Program,
    initialized_resources: &HashSet<&str>,
    run_span: Span,
) -> Result<(), CheckError> {
    for item in &schedule.items {
        let ScheduleItem::Run { system_name, .. } = item;
        let Some(system) = program
            .systems
            .iter()
            .find(|system| system.name == *system_name)
        else {
            continue;
        };

        for param in &system.params {
            let resource_name = match &param.kind {
                SystemParamKind::ReadResource { resource_name, .. }
                | SystemParamKind::MutResource { resource_name, .. } => resource_name,
                SystemParamKind::Query { .. } => continue,
            };
            if !initialized_resources.contains(resource_name.as_str()) {
                return Err(check_error(
                    run_span,
                    format!(
                        "schedule `{}` reads resource `{resource_name}` before it is initialized",
                        schedule.name
                    ),
                ));
            }
        }
    }

    Ok(())
}

fn check_spawn(
    spawn: &crate::parser::SpawnStatement,
    tables: &SemanticTables<'_>,
    bindings: &HashMap<&str, LocalBinding>,
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
                bindings,
            )?;
        }

        if let Some(missing) = component
            .fields()
            .iter()
            .find(|field| !fields.contains(field.name.as_str()))
        {
            return Err(check_error(
                literal.name_span,
                format!(
                    "missing field `{}` in component literal `{}`",
                    missing.name, literal.name
                ),
            ));
        }
    }
    Ok(())
}

fn check_resource_literal(
    literal: &crate::parser::ResourceStatement,
    tables: &SemanticTables<'_>,
    bindings: &HashMap<&str, LocalBinding>,
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
            bindings,
        )?;
    }

    if let Some(missing) = resource
        .fields
        .iter()
        .find(|field| !fields.contains(field.name.as_str()))
    {
        return Err(check_error(
            literal.name_span,
            format!(
                "missing field `{}` in resource literal `{}`",
                missing.name, literal.name
            ),
        ));
    }
    Ok(())
}

fn check_literal_value(
    value: &ComponentLiteralValue,
    expected: Type,
    label: &str,
    bindings: &HashMap<&str, LocalBinding>,
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
        ComponentLiteralValue::Integer { value, span } => {
            if expected != Type::I32 {
                return Err(check_error(
                    *span,
                    format!(
                        "integer literal cannot initialize {expected_name} {label}",
                        expected_name = expected.name()
                    ),
                ));
            }
            if *value > i32::MAX as u64 {
                return Err(check_error(*span, "integer literal does not fit i32"));
            }
        }
        ComponentLiteralValue::Bool { span, .. } => {
            if expected != Type::Bool {
                return Err(check_error(
                    *span,
                    format!(
                        "bool literal cannot initialize {expected_name} {label}",
                        expected_name = expected.name()
                    ),
                ));
            }
        }
        ComponentLiteralValue::Expression { expression, .. } => {
            let actual = check_startup_expression(expression, bindings)?;
            if actual != expected {
                return Err(check_error(
                    expression.span(),
                    format!(
                        "{} expression cannot initialize {} {label}",
                        actual.name(),
                        expected.name()
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn check_startup_expression(
    expression: &Expression,
    bindings: &HashMap<&str, LocalBinding>,
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
        Expression::Float { text, span } => text
            .parse::<f32>()
            .map(|_| Type::F32)
            .map_err(|_| check_error(*span, format!("invalid f32 literal `{text}`"))),
        Expression::Identifier { name, span } => bindings
            .get(name.as_str())
            .map(|binding| binding.ty)
            .ok_or_else(|| check_error(*span, format!("unknown local variable `{name}`"))),
        Expression::FieldAccess { field_span, .. } => Err(check_error(
            *field_span,
            "field access is only supported inside system query loops",
        )),
        Expression::Binary(binary) => {
            let left_type = check_startup_expression(&binary.left, bindings)?;
            let right_type = check_startup_expression(&binary.right, bindings)?;
            check_binary_types(
                binary.operator,
                left_type,
                right_type,
                expression_span(expression),
            )
        }
        Expression::Unary(unary) => {
            if unary.operator == UnaryOperator::Negate {
                if let Expression::Integer(integer) = unparenthesized(&unary.operand) {
                    if integer.value == i32::MAX as u64 + 1 {
                        return Ok(Type::I32);
                    }
                }
            }
            let operand = check_startup_expression(&unary.operand, bindings)?;
            check_unary_type(unary.operator, operand, &unary.operand, unary.operator_span)
        }
        Expression::Bool { .. } => Ok(Type::Bool),
        Expression::Parenthesized { expression, .. } => {
            check_startup_expression(expression, bindings)
        }
    }
}

fn check_startup_assignment_place(
    expression: &Expression,
    bindings: &HashMap<&str, LocalBinding>,
) -> Result<LocalBinding, CheckError> {
    let Expression::Identifier { name, span } = expression else {
        return Err(check_error(
            expression_span(expression),
            "startup assignment target must be a local variable",
        ));
    };
    let local = bindings
        .get(name.as_str())
        .copied()
        .ok_or_else(|| check_error(*span, format!("unknown local variable `{name}`")))?;
    if !local.mutable {
        return Err(check_error(*span, format!("local `{name}` is not mutable")));
    }
    Ok(local)
}

fn statement_span(statement: &Statement, _fallback: Span) -> Span {
    match statement {
        Statement::Let(statement) => statement.name_span,
        Statement::Assign(statement) => statement.target.span(),
        Statement::AddAssign(statement) => statement.target.span(),
        Statement::Run(statement) => statement.schedule_span,
        Statement::Spawn(statement) => statement
            .components
            .first()
            .map_or(statement.span, |component| component.name_span),
        Statement::Resource(statement) => statement.name_span,
        Statement::Exit(statement) => statement.expression.span(),
    }
}

fn expression_span(expression: &Expression) -> Span {
    expression.span()
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

    fn check_declarations_only(source: &str) -> Result<(), CheckError> {
        let tokens = lexer::lex(source).expect("fixture lexes");
        let program = parser::parse_program(&tokens).expect("fixture parses");
        check_declarations(&program)
    }

    fn assert_span_bytes(span: Span, start: usize, end: usize) {
        assert_eq!(
            (span.start.byte, span.end.byte),
            (
                u64::try_from(start).expect("test byte offset fits u64"),
                u64::try_from(end).expect("test byte offset fits u64"),
            )
        );
    }

    #[test]
    fn accepts_supported_ecs_program() {
        check(include_str!("../../../examples/move_system.arc"))
            .expect("supported ECS source checks");
    }

    #[test]
    fn accepts_typed_bool_short_circuit_and_mutable_assignment() {
        check(
            "world Main
system Toggle() { let mut ready: bool = true ready = !ready && false || true }
startup { let mut ready: bool = false ready = true == !false exit 0 }",
        )
        .expect("typed boolean locals and mutable assignments check");
    }

    #[test]
    fn accepts_add_assign_for_mutable_numeric_local_resource_and_query_field() {
        check(
            "world Main
component Position { value: i32 }
resource Counter { value: i32 }
system Update(counter: mut Counter, positions: query[mut Position]) {
  let mut local: i32 = 1
  local += 2
  counter.value += local
  for (position) in positions { position.value += counter.value }
}
startup {
  let mut initial: i32 = 1
  initial += 2
  resource Counter { value: initial }
  spawn { Position { value: 0 } }
  exit 0
}",
        )
        .expect("numeric += remains supported for every mutable place kind");
    }

    #[test]
    fn rejects_add_assign_for_immutable_places_and_bool_targets() {
        let cases = [
            (
                "world Main system Update() { let local: i32 = 1 local += 1 } startup { exit 0 }",
                "local `local` is not mutable",
            ),
            (
                "world Main resource Counter { value: i32 } system Update(counter: read Counter) { counter.value += 1 } startup { resource Counter { value: 0 } exit 0 }",
                "resource parameter `counter` is not mutable",
            ),
            (
                "world Main component Position { value: i32 } system Update(positions: query[Position]) { for (position) in positions { position.value += 1 } } startup { exit 0 }",
                "assignment target `position` is not mutable",
            ),
            (
                "world Main system Update() { let mut flag: bool = false flag += true } startup { exit 0 }",
                "add-assign target must have numeric type",
            ),
            (
                "world Main startup { let value: i32 = 1 value += 2 exit 0 }",
                "local `value` is not mutable",
            ),
            (
                "world Main startup { let mut flag: bool = false flag += true exit 0 }",
                "add-assign target must have numeric type",
            ),
            (
                "world Main startup { let mut value: i32 = 1 value += 2.0 exit 0 }",
                "cannot add f32 expression to i32 assignment target",
            ),
        ];

        for (source, expected) in cases {
            let error = check(source).expect_err(expected);
            assert_eq!(error.message, expected);
        }
    }

    #[test]
    fn rejects_assignment_to_immutable_local_and_implicit_bool_numeric_conversion() {
        let immutable = check("world Main startup { let ready: bool = false ready = true exit 0 }")
            .expect_err("immutable assignment must fail");
        assert_eq!(immutable.message, "local `ready` is not mutable");

        let conversion = check("world Main startup { let ready: bool = 1 exit 0 }")
            .expect_err("implicit i32-to-bool conversion must fail");
        assert_eq!(
            conversion.message,
            "cannot initialize bool local with i32 expression"
        );
    }

    #[test]
    fn rejects_executable_without_startup() {
        let source = "world Demo component Position { x: f32 }";
        let error = check(source).expect_err("an executable program must have startup");
        assert_span_bytes(error.span, source.len(), source.len());
        assert_eq!(
            (error.span.start.line, error.span.start.column),
            (1, u64::try_from(source.len()).unwrap() + 1)
        );
        assert_eq!(
            error.message,
            "executable program requires a `startup` block"
        );
    }

    #[test]
    fn executable_rejects_multiple_startups_at_second_keyword_only() {
        let source = "world Demo startup { exit 0 }\ncomponent Later {}\nstartup { exit 1 }";
        check_declarations_only(source)
            .expect("declaration-only checking ignores executable startup cardinality");

        let error = check(source).expect_err("an executable program requires one startup");
        assert_eq!(error.message, "multiple `startup` blocks are not allowed");
        let second = source.rfind("startup").unwrap();
        assert_span_bytes(error.span, second, second + "startup".len());
        assert_eq!((error.span.start.line, error.span.start.column), (3, 1));
    }

    #[test]
    fn rejects_startup_without_final_exit() {
        let source = "world Demo startup { let value: i32 = 1 }";
        let error = check(source).expect_err("startup must terminate with exit");
        let close = source.rfind('}').expect("fixture has closing brace");
        assert_span_bytes(error.span, close, close + 1);
        assert_eq!(
            (error.span.start.line, error.span.start.column),
            (1, u64::try_from(close).unwrap() + 1)
        );
        assert_eq!(error.message, "`startup` block must terminate with `exit`");
    }

    #[test]
    fn declaration_check_does_not_require_executable_semantics() {
        let source = "world Demo
component Position { x: f32 }
system Broken(missing: read Missing) {}";
        check_declarations_only(source)
            .expect("declaration inspection does not resolve executable uses");
        check(source).expect_err("the executable checker still resolves system parameters");
    }

    #[test]
    fn declaration_check_rejects_duplicate_system_parameters() {
        let source = "world Demo
resource Time { delta: f32 }
system Tick(time: read Time, time: read Time) {}";
        let error = check_declarations_only(source)
            .expect_err("declaration inspection must reject duplicate parameter names");
        assert_eq!(error.message, "duplicate parameter `time` in system `Tick`");
        let second = source.rfind("time: read Time").unwrap();
        assert_eq!(error.span.start.byte, u64::try_from(second).unwrap());
    }

    #[test]
    fn rejects_incomplete_component_and_resource_literals() {
        let component = "world Demo component Position { x: f32 y: f32 } startup { spawn { Position { x: 1.0 } } exit 0 }";
        let error = check(component).expect_err("component literals must initialize every field");
        assert_eq!(
            error.message,
            "missing field `y` in component literal `Position`"
        );
        let position = component.rfind("Position").unwrap();
        assert_span_bytes(error.span, position, position + "Position".len());

        let resource = "world Demo resource Time { delta: f32 scale: f32 } startup { resource Time { delta: 1.0 } exit 0 }";
        let error = check(resource).expect_err("resource literals must initialize every field");
        assert_eq!(
            error.message,
            "missing field `scale` in resource literal `Time`"
        );
        let time = resource.rfind("Time").unwrap();
        assert_span_bytes(error.span, time, time + "Time".len());
    }

    #[test]
    fn rejects_schedule_run_before_required_resource_initialization() {
        let source = "world Demo
component Position { x: f32 }
resource Time { delta: f32 }
system Tick(time: read Time, items: query[mut Position]) {
  for (item) in items { item.x += time.delta }
}
schedule Main { run Tick }
startup {
  spawn { Position { x: 1.0 } }
  run Main
  resource Time { delta: 1.0 }
  exit 0
}";
        let error = check(source).expect_err("a schedule cannot read an uninitialized resource");
        assert_eq!(
            error.message,
            "schedule `Main` reads resource `Time` before it is initialized"
        );
        let run_target = source.rfind("Main").unwrap();
        assert_span_bytes(error.span, run_target, run_target + "Main".len());
    }

    #[test]
    fn rejects_duplicate_declaration_at_second_name() {
        let source = "world Demo\ncomponent Position { x: f32 }\ncomponent Position { y: f32 }\nstartup { exit 0 }\n";
        let second = source.rfind("Position").unwrap();
        let error = check(source).expect_err("duplicate declaration must fail");
        assert_span_bytes(error.span, second, second + "Position".len());
        assert_eq!(error.message, "duplicate component declaration `Position`");
    }

    #[test]
    fn rejects_duplicate_literal_field_at_second_name() {
        let source = "world Demo\ncomponent Position { x: f32 }\nstartup { spawn { Position { x: 1.0, x: 2.0 } } exit 0 }\n";
        let second = source.rfind("x:").unwrap();
        let error = check(source).expect_err("duplicate literal field must fail");
        assert_span_bytes(error.span, second, second + 1);
        assert_eq!(
            error.message,
            "duplicate field `x` in component literal `Position`"
        );
    }

    #[test]
    fn rejects_unknown_system_field_at_field_span() {
        let source =
            include_str!("../../../examples/move_system.arc").replace("time.delta", "time.missing");
        let field = source.find("missing").unwrap();
        let error = check(&source).expect_err("unknown field must fail");
        assert_span_bytes(error.span, field, field + "missing".len());
        assert_eq!(error.message, "unknown field `missing` for resource `Time`");
    }

    #[test]
    fn rejects_i32_range_and_accepts_low_eight_bit_exit_semantics() {
        let too_large_i32 = "world Demo startup { let x: i32 = 2147483648 exit x }";
        let error = check(too_large_i32).expect_err("out-of-range i32 must fail");
        assert_eq!(
            error.span.start.byte,
            u64::try_from(too_large_i32.find("2147483648").unwrap()).unwrap()
        );
        assert!(error.message.contains("does not fit i32"));

        let too_large_exit = "world Demo startup { exit 256 }";
        check(too_large_exit).expect("source exit uses the low eight bits of any i32 value");
        check("world Demo startup { exit -1 }")
            .expect("negative source exits also use their low eight bits");
    }

    #[test]
    fn permits_repeated_read_only_query_terms_but_rejects_mutable_aliases() {
        let read_only = "world Demo component Position { x: f32 } system ReadBoth(q: query[Position, Position]) { for (a, b) in q { a.x b.x } } startup { exit 0 }";
        check(read_only).expect("repeated read-only terms are legal");

        let mutable =
            read_only.replace("query[Position, Position]", "query[mut Position, Position]");
        let error = check(&mutable).expect_err("mutable aliases must fail");
        assert_eq!(
            error.message,
            "conflicting query access for component `Position`"
        );
        let second = mutable.rfind("Position").unwrap();
        assert_span_bytes(error.span, second, second + "Position".len());
    }

    #[test]
    fn rejects_every_duplicate_scope() {
        let cases = [
            (
                "world Demo resource Time { delta: f32 } resource Time { delta: f32 } startup { exit 0 }",
                "duplicate resource declaration `Time`",
                "Time",
                "Time".len(),
            ),
            (
                "world Demo system Tick() {} system Tick() {} startup { exit 0 }",
                "duplicate system declaration `Tick`",
                "Tick",
                "Tick".len(),
            ),
            (
                "world Demo schedule Main {} schedule Main {} startup { exit 0 }",
                "duplicate schedule declaration `Main`",
                "Main",
                "Main".len(),
            ),
            (
                "world Demo component Position { x: f32 x: f32 } startup { exit 0 }",
                "duplicate field `x` in component `Position`",
                "x: f32 }",
                1,
            ),
            (
                "world Demo resource Time { delta: f32 delta: f32 } startup { exit 0 }",
                "duplicate field `delta` in resource `Time`",
                "delta",
                "delta".len(),
            ),
            (
                "world Demo resource Time { delta: f32 } system Tick(time: read Time, time: read Time) {} startup { exit 0 }",
                "duplicate parameter `time` in system `Tick`",
                "time",
                "time".len(),
            ),
            (
                "world Demo component Position { x: f32 } system Tick(q: query[Position, Position]) { for (item, item) in q { item.x } } startup { exit 0 }",
                "duplicate query loop binding `item`",
                "item) in",
                "item".len(),
            ),
            (
                "world Demo startup { let x: i32 = 1 let x: i32 = 2 exit 0 }",
                "duplicate local `x`",
                "x: i32 = 2",
                1,
            ),
            (
                "world Demo component Position { x: f32 } startup { spawn { Position { x: 1.0 } Position { x: 2.0 } } exit 0 }",
                "duplicate component `Position` in spawn",
                "Position",
                "Position".len(),
            ),
            (
                "world Demo resource Time { delta: f32 } startup { resource Time { delta: 1.0, delta: 2.0 } exit 0 }",
                "duplicate field `delta` in resource literal `Time`",
                "delta",
                "delta".len(),
            ),
            (
                "world Demo resource Time { delta: f32 } startup { resource Time { delta: 1.0 } resource Time { delta: 2.0 } exit 0 }",
                "duplicate startup resource `Time`",
                "Time",
                "Time".len(),
            ),
        ];

        for (source, expected, span_marker, span_len) in cases {
            let error = check(source).expect_err(expected);
            assert_eq!(error.message, expected);
            let start = source.rfind(span_marker).unwrap();
            assert_span_bytes(error.span, start, start + span_len);
        }
    }

    #[test]
    fn rejects_active_binding_collisions_at_the_second_binding_span() {
        let cases = [
            (
                "world Demo component Position { x: i32 } system Tick(q: query[Position]) { for (q) in q { q.x } } startup { exit 0 }",
                "q",
                "q) in",
                0,
            ),
            (
                "world Demo component Position { x: i32 } system Tick(q: query[Position]) { let item: i32 = 0 for (item) in q { item.x } } startup { exit 0 }",
                "item",
                "item) in",
                0,
            ),
            (
                "world Demo component Position { x: i32 } system Tick(q: query[Position]) { for (item) in q { let item: i32 = 0 } } startup { exit 0 }",
                "item",
                "let item",
                "let ".len(),
            ),
            (
                "world Demo system Tick() { let value: i32 = 1 let value: i32 = 2 } startup { exit 0 }",
                "value",
                "value: i32 = 2",
                0,
            ),
        ];

        for (source, binding, span_marker, span_offset) in cases {
            let error = check(source).expect_err("active binding collision must fail");
            assert_eq!(
                error.message,
                format!("duplicate active binding `{binding}`")
            );
            let start = source.rfind(span_marker).unwrap() + span_offset;
            assert_span_bytes(error.span, start, start + binding.len());
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
    fn checks_typed_integer_component_and_resource_literals() {
        let boundaries = "world Bounds
component Values { zero: i32 max: i32 scalar: f32 }
resource Limits { zero: i32 max: i32 scalar: f32 }
startup {
  resource Limits { zero: 0, max: 2147483647, scalar: 1.0 }
  spawn { Values { zero: 0, max: 2147483647, scalar: 2.0 } }
  exit 0
}";
        check(boundaries).expect("typed integer and existing f32 startup literals check");

        for source in [
            "world Bounds component Values { value: i32 } startup { spawn { Values { value: 2147483648 } } exit 0 }",
            "world Bounds resource Limits { value: i32 } startup { resource Limits { value: 2147483648 } exit 0 }",
        ] {
            let literal = source.find("2147483648").expect("range fixture has literal");
            let error = check(source).expect_err("out-of-range startup i32 must fail");
            assert_span_bytes(error.span, literal, literal + "2147483648".len());
            assert!(error.message.contains("does not fit i32"));
        }

        for (source, literal, expected) in [
            (
                "world Bounds component Values { value: f32 } startup { spawn { Values { value: 1 } } exit 0 }",
                "1",
                "integer literal cannot initialize f32",
            ),
            (
                "world Bounds resource Limits { value: i32 } startup { resource Limits { value: 1.0 } exit 0 }",
                "1.0",
                "float literal cannot initialize i32",
            ),
        ] {
            let literal_start = source.rfind(literal).expect("type fixture has literal");
            let error = check(source).expect_err("typed startup literal mismatch must fail");
            assert_span_bytes(error.span, literal_start, literal_start + literal.len());
            assert!(error.message.contains(expected));
        }
    }

    #[test]
    fn rejects_unknown_startup_references_and_fields() {
        let cases = [
            (
                "world Demo startup { run Missing exit 0 }",
                "unknown schedule `Missing` in startup",
                "Missing",
            ),
            (
                "world Demo startup { resource Missing { value: 1.0 } exit 0 }",
                "unknown resource `Missing` in startup",
                "Missing",
            ),
            (
                "world Demo startup { spawn { Missing { value: 1.0 } } exit 0 }",
                "unknown component `Missing` in spawn",
                "Missing",
            ),
            (
                "world Demo component Position { x: f32 } startup { spawn { Position { missing: 1.0 } } exit 0 }",
                "unknown field `missing` for component `Position`",
                "missing",
            ),
            (
                "world Demo resource Time { delta: f32 } startup { resource Time { missing: 1.0 } exit 0 }",
                "unknown field `missing` for resource `Time`",
                "missing",
            ),
        ];

        for (source, expected, span_text) in cases {
            let error = check(source).expect_err(expected);
            assert_eq!(error.message, expected);
            let start = source.rfind(span_text).unwrap();
            assert_span_bytes(error.span, start, start + span_text.len());
        }
    }

    #[test]
    fn rejects_query_binding_count_mutability_and_type_mismatch() {
        let binding_count = "world Demo component Position { x: f32 } system Tick(q: query[Position, Position]) { for (item) in q { item.x } } startup { exit 0 }";
        let error = check(binding_count).expect_err("binding count mismatch must fail");
        assert_eq!(
            error.message,
            "query loop binding count 1 does not match query term count 2"
        );
        let query_target = binding_count.rfind('q').unwrap();
        assert_span_bytes(error.span, query_target, query_target + 1);

        let immutable = "world Demo component Position { x: f32 } system Tick(q: query[Position]) { for (pos) in q { pos.x += pos.x } } startup { exit 0 }";
        let error = check(immutable).expect_err("immutable update must fail");
        assert_eq!(error.message, "assignment target `pos` is not mutable");
        let immutable_target = immutable.find("pos.x +=").unwrap();
        assert_span_bytes(error.span, immutable_target, immutable_target + "pos".len());

        let type_mismatch = "world Demo component Position { x: f32 } resource Count { value: i32 } system Tick(count: read Count, q: query[mut Position]) { for (pos) in q { pos.x += count.value } } startup { exit 0 }";
        let error = check(type_mismatch).expect_err("add-assign type mismatch must fail");
        assert_eq!(
            error.message,
            "cannot add i32 expression to f32 assignment target"
        );
        let mismatched_value = type_mismatch.rfind("count.value").unwrap();
        assert_span_bytes(
            error.span,
            mismatched_value,
            mismatched_value + "count.value".len(),
        );
    }

    #[test]
    fn rejects_statement_after_exit_at_the_next_statement_span() {
        let source = "world Demo startup { exit 0 let later: i32 = 1 }";
        let later = source.find("later").unwrap();
        let error = check(source).expect_err("statement after exit must fail before Core lowering");
        assert_span_bytes(error.span, later, later + "later".len());
        assert_eq!(error.message, "statement after startup exit");
    }

    #[test]
    fn accepts_parenthesized_startup_math_with_standard_precedence() {
        check(
            "world Demo startup {
                let value: i32 = (1 + 2) * 3 - 4 * (5 - 2)
                exit value
            }",
        )
        .expect("parenthesized, precedence-aware startup math should check");
    }

    #[test]
    fn accepts_tags_zero_field_schemas_and_empty_archetypes() {
        check(
            "world Demo
             tag Enemy
             component Empty {}
             resource Ready {}
             system Find(q: query[Enemy, mut Empty]) { for (_, _) in q {} }
             startup {
               resource Ready {}
               spawn {}
               spawn { Enemy {} Empty {} }
               exit 0
             }",
        )
        .expect("tags, mutable zero-field components, and empty-archetype spawns should check");
    }

    #[test]
    fn rejects_mutable_tag_queries() {
        let source = "world Demo
             tag Enemy
             system Find(q: query[mut Enemy]) { for (_) in q {} }
             startup { exit 0 }";
        let error = check(source).expect_err("tags cannot be queried mutably");
        assert_eq!(error.message, "mutable tag query term `Enemy` is invalid");
        let query_enemy = source.rfind("Enemy").unwrap();
        assert_span_bytes(error.span, query_enemy, query_enemy + "Enemy".len());
    }

    #[test]
    fn requires_discard_bindings_for_zero_sized_query_terms() {
        for (source, schema, binding) in [
            (
                "world Demo tag Enemy system Find(q: query[Enemy]) { for (enemy) in q {} } startup { exit 0 }",
                "Enemy",
                "enemy",
            ),
            (
                "world Demo component Empty {} system Find(q: query[Empty]) { for (empty) in q {} } startup { exit 0 }",
                "Empty",
                "empty",
            ),
        ] {
            let error =
                check(source).expect_err("zero-sized query terms must use discard bindings");
            assert_eq!(
                error.message,
                format!("zero-sized query term `{schema}` must bind to `_`")
            );
            let binding_start = source.find(&format!("({binding})")).unwrap() + 1;
            assert_span_bytes(error.span, binding_start, binding_start + binding.len());
        }
    }

    #[test]
    fn rejects_duplicate_component_and_tag_names_in_the_queryable_namespace() {
        let source = "world Demo component Enemy {} tag Enemy startup { exit 0 }";
        let error = check(source).expect_err("components and tags share a queryable namespace");
        assert_eq!(
            error.message,
            "duplicate queryable schema declaration `Enemy`"
        );
        let tag_enemy = source.rfind("Enemy").unwrap();
        assert_span_bytes(error.span, tag_enemy, tag_enemy + "Enemy".len());
    }
}
