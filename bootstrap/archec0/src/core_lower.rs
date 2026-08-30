use std::collections::HashMap;
use std::io;

use crate::core::{
    BlockId, CoreBinaryOp, CoreBlock, CoreComparisonOp, CoreComponent, CoreComponentKind,
    CoreField, CoreFunction, CoreInstruction, CoreLiteralValue, CoreLocal, CoreProgram,
    CoreQueryAccess, CoreQueryLoop, CoreQueryLoopBinding, CoreQueryTerm, CoreResource,
    CoreResourceField, CoreSchedule, CoreScheduleItem, CoreSourceMap, CoreSourceMapEntry,
    CoreSourceSubject, CoreSpawnComponent, CoreSpawnField, CoreSystem, CoreSystemBinaryOp,
    CoreSystemBody, CoreSystemExpression, CoreSystemParam, CoreSystemParamKind, CoreSystemPlace,
    CoreSystemStatement, CoreSystemUnaryOp, CoreTerminator, CoreType, CoreUnaryOp, CoreWorld,
    LocalId, ValueId,
};
use crate::identifier::{Identifier, IdentifierInterner};
use crate::parser::{
    AddAssignStatement, BinaryOperator, ComponentDecl, ComponentLiteralValue, Expression, Program,
    QueryAccess as ParserQueryAccess, ResourceDecl, ScheduleDecl, ScheduleItem,
    SpawnComponentLiteral, SpawnStatement, StartupBlock, Statement, SystemBodyStatement,
    SystemDecl, SystemParam, SystemParamKind, SystemQueryLoopStatement, TagDecl, UnaryOperator,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreLowerError {
    pub message: String,
}

struct CoreCanonicalNames {
    startup: Identifier,
    components: Vec<Identifier>,
    resource_id_base: u64,
    resources: Vec<Identifier>,
    systems: Vec<Identifier>,
    schedules: Vec<Identifier>,
}

impl CoreCanonicalNames {
    fn new(program: &Program) -> Result<Self, CoreLowerError> {
        let mut interner = IdentifierInterner::default();
        let startup = interner
            .intern_str("startup")
            .map_err(|error| allocation_lower_error("startup name", error))?;

        let component_count = program
            .components
            .len()
            .checked_add(program.tags.len())
            .ok_or_else(|| lower_error("component and tag name count overflow"))?;
        let resource_id_base = u64::try_from(component_count)
            .map_err(|_| lower_error("resource id base exceeds u64"))?;
        let mut components = reserved_names(component_count, "component names")?;
        for component in &program.components {
            components.push(intern_qualified(
                &mut interner,
                &program.world.name,
                &component.name,
            )?);
        }
        for tag in &program.tags {
            components.push(intern_qualified(
                &mut interner,
                &program.world.name,
                &tag.name,
            )?);
        }

        let mut resources = reserved_names(program.resources.len(), "resource names")?;
        for resource in &program.resources {
            resources.push(intern_qualified(
                &mut interner,
                &program.world.name,
                &resource.name,
            )?);
        }

        let mut systems = reserved_names(program.systems.len(), "system names")?;
        for system in &program.systems {
            systems.push(intern_qualified(
                &mut interner,
                &program.world.name,
                &system.name,
            )?);
        }

        let mut schedules = reserved_names(program.schedules.len(), "schedule names")?;
        for schedule in &program.schedules {
            schedules.push(intern_qualified(
                &mut interner,
                &program.world.name,
                &schedule.name,
            )?);
        }

        Ok(Self {
            startup,
            components,
            resource_id_base,
            resources,
            systems,
            schedules,
        })
    }

    fn component(&self, id: u64) -> Result<Identifier, CoreLowerError> {
        canonical_name(&self.components, id, "component")
    }

    fn resource(&self, id: u64) -> Result<Identifier, CoreLowerError> {
        let local_id = id
            .checked_sub(self.resource_id_base)
            .ok_or_else(|| lower_error(format!("unknown resource id {id}")))?;
        canonical_name(&self.resources, local_id, "resource")
    }

    fn system(&self, id: u64) -> Result<Identifier, CoreLowerError> {
        canonical_name(&self.systems, id, "system")
    }

    fn schedule(&self, id: u64) -> Result<Identifier, CoreLowerError> {
        canonical_name(&self.schedules, id, "schedule")
    }
}

fn reserved_names(count: usize, context: &'static str) -> Result<Vec<Identifier>, CoreLowerError> {
    let mut names = Vec::new();
    names
        .try_reserve_exact(count)
        .map_err(|error| lower_error(format!("could not allocate {context}: {error}")))?;
    Ok(names)
}

fn intern_qualified(
    interner: &mut IdentifierInterner,
    world: &Identifier,
    local: &Identifier,
) -> Result<Identifier, CoreLowerError> {
    let byte_len = world
        .len()
        .checked_add(1)
        .and_then(|len| len.checked_add(local.len()))
        .ok_or_else(|| lower_error("qualified identifier byte length overflow"))?;
    let mut text = String::new();
    text.try_reserve_exact(byte_len)
        .map_err(|error| lower_error(format!("could not allocate qualified name: {error}")))?;
    text.push_str(world);
    text.push('.');
    text.push_str(local);
    interner
        .intern(text)
        .map_err(|error| allocation_lower_error("qualified name", error))
}

fn canonical_name(
    names: &[Identifier],
    id: u64,
    context: &'static str,
) -> Result<Identifier, CoreLowerError> {
    let index = usize::try_from(id)
        .map_err(|_| lower_error(format!("{context} id exceeds host address space")))?;
    names
        .get(index)
        .cloned()
        .ok_or_else(|| lower_error(format!("unknown {context} id {id}")))
}

fn allocation_lower_error(context: &'static str, error: io::Error) -> CoreLowerError {
    lower_error(format!("could not allocate {context}: {error}"))
}

pub fn lower_program_to_core(program: &Program) -> Result<CoreProgram, CoreLowerError> {
    crate::scalar_v2::initialize_floating_point_environment();
    let canonical_names = CoreCanonicalNames::new(program)?;
    let startup = executable_startup(program)?;
    let components = lower_components(program, &canonical_names)?;
    let resources = lower_resources(program, &canonical_names)?;
    let systems = lower_systems(program, &canonical_names)?;
    let schedules = lower_schedules(program, &canonical_names)?;
    let lowered_startup = StartupLowerer::new(program, &canonical_names).lower_startup(startup)?;
    let source_map = build_source_map(program, lowered_startup.source_entries)?;

    Ok(CoreProgram {
        world: CoreWorld {
            name: program.world.name.clone(),
        },
        components,
        resources,
        systems,
        schedules,
        functions: vec![CoreFunction {
            name: canonical_names.startup,
            entry: BlockId(0),
            locals: lowered_startup.locals,
            blocks: lowered_startup.blocks,
        }],
        source_map,
    })
}

fn build_source_map(
    program: &Program,
    mut entries: Vec<CoreSourceMapEntry>,
) -> Result<CoreSourceMap, CoreLowerError> {
    entries.push(CoreSourceMapEntry {
        subject: CoreSourceSubject::Program,
        span: program.span,
    });
    entries.push(CoreSourceMapEntry {
        subject: CoreSourceSubject::World,
        span: program.world.span,
    });
    let startup = executable_startup(program)?;
    entries.push(CoreSourceMapEntry {
        subject: CoreSourceSubject::Startup,
        span: startup.span,
    });

    for component in &program.components {
        let component_id =
            component_declaration_id(&program.components, &program.tags, &component.name)?;
        entries.push(CoreSourceMapEntry {
            subject: CoreSourceSubject::Component { component_id },
            span: component.span,
        });
        for (field_index, field) in component.fields.iter().enumerate() {
            entries.push(CoreSourceMapEntry {
                subject: CoreSourceSubject::ComponentField {
                    component_id,
                    field_index: u64::try_from(field_index)
                        .map_err(|_| lower_error("component field index exceeds u64"))?,
                },
                span: field.span,
            });
        }
    }
    for tag in &program.tags {
        let component_id = component_declaration_id(&program.components, &program.tags, &tag.name)?;
        entries.push(CoreSourceMapEntry {
            subject: CoreSourceSubject::Component { component_id },
            span: tag.span,
        });
    }
    for resource in &program.resources {
        let resource_id = resource_declaration_id(
            &program.components,
            &program.tags,
            &program.resources,
            &resource.name,
        )?;
        entries.push(CoreSourceMapEntry {
            subject: CoreSourceSubject::Resource { resource_id },
            span: resource.span,
        });
        for (field_index, field) in resource.fields.iter().enumerate() {
            entries.push(CoreSourceMapEntry {
                subject: CoreSourceSubject::ResourceField {
                    resource_id,
                    field_index: u64::try_from(field_index)
                        .map_err(|_| lower_error("resource field index exceeds u64"))?,
                },
                span: field.span,
            });
        }
    }
    for system in &program.systems {
        let system_id = system_declaration_id(&program.systems, &system.name)?;
        entries.push(CoreSourceMapEntry {
            subject: CoreSourceSubject::System { system_id },
            span: system.span,
        });
        for (param_index, param) in system.params.iter().enumerate() {
            let param_index = u64::try_from(param_index)
                .map_err(|_| lower_error("system parameter index exceeds u64"))?;
            entries.push(CoreSourceMapEntry {
                subject: CoreSourceSubject::SystemParam {
                    system_id,
                    param_index,
                },
                span: param.span,
            });
            if let SystemParamKind::Query { terms } = &param.kind {
                for (term_index, term) in terms.iter().enumerate() {
                    entries.push(CoreSourceMapEntry {
                        subject: CoreSourceSubject::QueryTerm {
                            system_id,
                            param_index,
                            term_index: u64::try_from(term_index)
                                .map_err(|_| lower_error("query term index exceeds u64"))?,
                        },
                        span: term.span,
                    });
                }
            }
        }
        let mut statement_ordinal = 0;
        let mut expression_ordinal = 0;
        let mut place_ordinal = 0;
        record_system_statements(
            system_id,
            &system.body.statements,
            &mut statement_ordinal,
            &mut expression_ordinal,
            &mut place_ordinal,
            &mut entries,
        )?;
    }
    for schedule in &program.schedules {
        let schedule_id = schedule_declaration_id(&program.schedules, &schedule.name)?;
        entries.push(CoreSourceMapEntry {
            subject: CoreSourceSubject::Schedule { schedule_id },
            span: schedule.span,
        });
        for (item_index, item) in schedule.items.iter().enumerate() {
            let span = match item {
                ScheduleItem::Run { span, .. } => *span,
            };
            entries.push(CoreSourceMapEntry {
                subject: CoreSourceSubject::ScheduleItem {
                    schedule_id,
                    item_index: u64::try_from(item_index)
                        .map_err(|_| lower_error("schedule item index exceeds u64"))?,
                },
                span,
            });
        }
    }
    Ok(CoreSourceMap { entries })
}

fn executable_startup(program: &Program) -> Result<&StartupBlock, CoreLowerError> {
    let [startup] = program.startups.as_slice() else {
        return Err(lower_error("expected exactly one startup block"));
    };
    Ok(startup)
}

fn record_system_statements(
    system_id: u64,
    statements: &[SystemBodyStatement],
    statement_ordinal: &mut u64,
    expression_ordinal: &mut u64,
    place_ordinal: &mut u64,
    entries: &mut Vec<CoreSourceMapEntry>,
) -> Result<(), CoreLowerError> {
    for statement in statements {
        let ordinal = *statement_ordinal;
        *statement_ordinal = statement_ordinal
            .checked_add(1)
            .ok_or_else(|| lower_error("system statement ordinal space exhausted"))?;
        entries.push(CoreSourceMapEntry {
            subject: CoreSourceSubject::SystemStatement {
                system_id,
                statement_ordinal: ordinal,
            },
            span: statement.span(),
        });
        match statement {
            SystemBodyStatement::Expression(expression) => {
                record_system_expression(system_id, expression, expression_ordinal, entries)?;
            }
            SystemBodyStatement::Let(statement) => {
                record_system_expression(
                    system_id,
                    &statement.initializer,
                    expression_ordinal,
                    entries,
                )?;
            }
            SystemBodyStatement::Assign(statement) => {
                record_system_place(system_id, &statement.target, place_ordinal, entries)?;
                record_system_expression(system_id, &statement.value, expression_ordinal, entries)?;
            }
            SystemBodyStatement::AddAssign(statement) => {
                record_system_place(system_id, &statement.target, place_ordinal, entries)?;
                record_system_expression(system_id, &statement.value, expression_ordinal, entries)?;
            }
            SystemBodyStatement::QueryLoop(statement) => record_system_statements(
                system_id,
                &statement.body,
                statement_ordinal,
                expression_ordinal,
                place_ordinal,
                entries,
            )?,
            SystemBodyStatement::Block(statement) => record_system_statements(
                system_id,
                &statement.statements,
                statement_ordinal,
                expression_ordinal,
                place_ordinal,
                entries,
            )?,
            SystemBodyStatement::If(statement) => {
                record_system_expression(
                    system_id,
                    &statement.condition,
                    expression_ordinal,
                    entries,
                )?;
                record_system_statements(
                    system_id,
                    &statement.then_block.statements,
                    statement_ordinal,
                    expression_ordinal,
                    place_ordinal,
                    entries,
                )?;
                if let Some(block) = &statement.else_block {
                    record_system_statements(
                        system_id,
                        &block.statements,
                        statement_ordinal,
                        expression_ordinal,
                        place_ordinal,
                        entries,
                    )?;
                }
            }
            SystemBodyStatement::While(statement) => {
                record_system_expression(
                    system_id,
                    &statement.condition,
                    expression_ordinal,
                    entries,
                )?;
                record_system_statements(
                    system_id,
                    &statement.body.statements,
                    statement_ordinal,
                    expression_ordinal,
                    place_ordinal,
                    entries,
                )?;
            }
        }
    }
    Ok(())
}

fn record_system_place(
    system_id: u64,
    expression: &Expression,
    ordinal: &mut u64,
    entries: &mut Vec<CoreSourceMapEntry>,
) -> Result<(), CoreLowerError> {
    let current = *ordinal;
    *ordinal = ordinal
        .checked_add(1)
        .ok_or_else(|| lower_error("system place ordinal space exhausted"))?;
    entries.push(CoreSourceMapEntry {
        subject: CoreSourceSubject::SystemPlace {
            system_id,
            place_ordinal: current,
        },
        span: expression.span(),
    });
    Ok(())
}

fn record_system_expression(
    system_id: u64,
    expression: &Expression,
    ordinal: &mut u64,
    entries: &mut Vec<CoreSourceMapEntry>,
) -> Result<(), CoreLowerError> {
    if let Expression::Parenthesized {
        expression: inner,
        span,
    } = expression
    {
        return record_system_expression_with_span(system_id, inner, *span, ordinal, entries);
    }
    record_system_expression_with_span(system_id, expression, expression.span(), ordinal, entries)
}

fn record_system_expression_with_span(
    system_id: u64,
    expression: &Expression,
    span: crate::lexer::SourceSpan,
    ordinal: &mut u64,
    entries: &mut Vec<CoreSourceMapEntry>,
) -> Result<(), CoreLowerError> {
    let expression = unparenthesized_expression(expression);
    let current = *ordinal;
    *ordinal = ordinal
        .checked_add(1)
        .ok_or_else(|| lower_error("system expression ordinal space exhausted"))?;
    entries.push(CoreSourceMapEntry {
        subject: CoreSourceSubject::SystemExpression {
            system_id,
            expression_ordinal: current,
        },
        span,
    });
    match expression {
        Expression::FieldAccess { .. } => {}
        Expression::Unary(unary) => {
            if !is_folded_i32_min_literal(unary.operator, &unary.operand) {
                record_system_expression(system_id, &unary.operand, ordinal, entries)?;
            }
        }
        Expression::Binary(binary) => {
            record_system_expression(system_id, &binary.left, ordinal, entries)?;
            record_system_expression(system_id, &binary.right, ordinal, entries)?;
        }
        Expression::Parenthesized { .. } => unreachable!("parentheses were removed"),
        Expression::Integer(_)
        | Expression::Float { .. }
        | Expression::Bool { .. }
        | Expression::Identifier { .. } => {}
    }
    Ok(())
}

fn lower_components(
    program: &Program,
    canonical_names: &CoreCanonicalNames,
) -> Result<Vec<CoreComponent>, CoreLowerError> {
    let components = program.components.iter().map(|component| {
        let id = component_declaration_id(&program.components, &program.tags, &component.name)?;
        Ok(CoreComponent {
            id,
            name: canonical_names.component(id)?,
            kind: CoreComponentKind::Component,
            fields: lower_fields(
                component
                    .fields
                    .iter()
                    .map(|field| (&field.name, &field.type_name.name)),
            )?,
        })
    });
    let tags = program.tags.iter().map(|tag| {
        let id = component_declaration_id(&program.components, &program.tags, &tag.name)?;
        Ok(CoreComponent {
            id,
            name: canonical_names.component(id)?,
            kind: CoreComponentKind::Tag,
            fields: vec![],
        })
    });
    components.chain(tags).collect()
}

fn lower_resources(
    program: &Program,
    canonical_names: &CoreCanonicalNames,
) -> Result<Vec<CoreResource>, CoreLowerError> {
    program
        .resources
        .iter()
        .map(|resource| {
            let id = resource_declaration_id(
                &program.components,
                &program.tags,
                &program.resources,
                &resource.name,
            )?;
            Ok(CoreResource {
                id,
                name: canonical_names.resource(id)?,
                fields: lower_fields(
                    resource
                        .fields
                        .iter()
                        .map(|field| (&field.name, &field.type_name.name)),
                )?,
            })
        })
        .collect()
}

fn lower_fields<'a>(
    fields: impl Iterator<Item = (&'a Identifier, &'a Identifier)>,
) -> Result<Vec<CoreField>, CoreLowerError> {
    fields
        .map(|(name, type_name)| {
            Ok(CoreField {
                name: name.clone(),
                ty: lower_core_type(type_name)?,
            })
        })
        .collect()
}

fn lower_core_type(type_name: &str) -> Result<CoreType, CoreLowerError> {
    match type_name {
        "i32" => Ok(CoreType::I32),
        "f32" => Ok(CoreType::F32),
        "bool" => Ok(CoreType::Bool),
        _ => Err(lower_error(format!(
            "unknown primitive type `{type_name}` while lowering Core"
        ))),
    }
}

fn lower_schedules(
    program: &Program,
    canonical_names: &CoreCanonicalNames,
) -> Result<Vec<CoreSchedule>, CoreLowerError> {
    program
        .schedules
        .iter()
        .map(|schedule| lower_schedule(program, canonical_names, schedule))
        .collect()
}

fn lower_schedule(
    program: &Program,
    canonical_names: &CoreCanonicalNames,
    schedule: &ScheduleDecl,
) -> Result<CoreSchedule, CoreLowerError> {
    let items = schedule
        .items
        .iter()
        .map(|item| lower_schedule_item(program, canonical_names, item))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CoreSchedule {
        id: schedule_declaration_id(&program.schedules, &schedule.name)?,
        name: schedule.name.clone(),
        items,
    })
}

fn lower_schedule_item(
    program: &Program,
    canonical_names: &CoreCanonicalNames,
    item: &ScheduleItem,
) -> Result<CoreScheduleItem, CoreLowerError> {
    match item {
        ScheduleItem::Run { system_name, .. } => {
            resolve_system(&program.systems, system_name)?;
            let system_id = system_declaration_id(&program.systems, system_name)?;
            Ok(CoreScheduleItem::Run {
                system_id,
                system_name: canonical_names.system(system_id)?,
            })
        }
    }
}

fn lower_systems(
    program: &Program,
    canonical_names: &CoreCanonicalNames,
) -> Result<Vec<CoreSystem>, CoreLowerError> {
    program
        .systems
        .iter()
        .map(|system| lower_system(program, canonical_names, system))
        .collect()
}

fn lower_system(
    program: &Program,
    canonical_names: &CoreCanonicalNames,
    system: &SystemDecl,
) -> Result<CoreSystem, CoreLowerError> {
    let params = system
        .params
        .iter()
        .map(|param| lower_system_param(program, canonical_names, param))
        .collect::<Result<Vec<_>, _>>()?;
    let body = lower_system_body(program, &params, system)?;

    Ok(CoreSystem {
        id: system_declaration_id(&program.systems, &system.name)?,
        name: system.name.clone(),
        params,
        body,
    })
}

fn lower_system_body(
    program: &Program,
    params: &[CoreSystemParam],
    system: &SystemDecl,
) -> Result<CoreSystemBody, CoreLowerError> {
    let mut statements = Vec::new();
    let mut locals = HashMap::new();

    for statement in &system.body.statements {
        match statement {
            SystemBodyStatement::Expression(expression) => {
                statements.push(CoreSystemStatement::Expression(lower_system_expression(
                    program,
                    params,
                    &[],
                    &locals,
                    expression,
                )?));
            }
            SystemBodyStatement::Let(let_statement) => {
                let ty = lower_core_type(&let_statement.type_name.name)?;
                let value = lower_system_expression(
                    program,
                    params,
                    &[],
                    &locals,
                    &let_statement.initializer,
                )?;
                locals.insert(let_statement.name.clone(), (ty, let_statement.mutable));
                statements.push(CoreSystemStatement::Let {
                    name: let_statement.name.clone(),
                    ty,
                    mutable: let_statement.mutable,
                    value,
                });
            }
            SystemBodyStatement::Assign(assignment) => {
                statements.push(CoreSystemStatement::Assign {
                    target: lower_system_assignment_place(
                        params,
                        &[],
                        &locals,
                        &assignment.target,
                    )?,
                    value: lower_system_expression(
                        program,
                        params,
                        &[],
                        &locals,
                        &assignment.value,
                    )?,
                });
            }
            SystemBodyStatement::AddAssign(add_assign) => {
                statements.push(CoreSystemStatement::AddAssign {
                    target: lower_system_assignment_place(
                        params,
                        &[],
                        &locals,
                        &add_assign.target,
                    )?,
                    value: lower_system_expression(
                        program,
                        params,
                        &[],
                        &locals,
                        &add_assign.value,
                    )?,
                });
            }
            SystemBodyStatement::QueryLoop(query_loop) => {
                statements.push(CoreSystemStatement::QueryLoop(lower_system_query_loop(
                    program, params, &locals, query_loop,
                )?));
            }
            SystemBodyStatement::Block(block) => {
                let mut scoped = locals.clone();
                statements.push(CoreSystemStatement::Block(lower_system_statements(
                    program,
                    params,
                    &[],
                    &mut scoped,
                    &block.statements,
                )?));
            }
            SystemBodyStatement::If(statement) => {
                let condition =
                    lower_system_expression(program, params, &[], &locals, &statement.condition)?;
                let mut then_locals = locals.clone();
                let then_body = lower_system_statements(
                    program,
                    params,
                    &[],
                    &mut then_locals,
                    &statement.then_block.statements,
                )?;
                let mut else_locals = locals.clone();
                let else_body = statement.else_block.as_ref().map_or_else(
                    || Ok(Vec::new()),
                    |block| {
                        lower_system_statements(
                            program,
                            params,
                            &[],
                            &mut else_locals,
                            &block.statements,
                        )
                    },
                )?;
                statements.push(CoreSystemStatement::If {
                    condition,
                    then_body,
                    else_body,
                });
            }
            SystemBodyStatement::While(statement) => {
                let condition =
                    lower_system_expression(program, params, &[], &locals, &statement.condition)?;
                let mut body_locals = locals.clone();
                let body = lower_system_statements(
                    program,
                    params,
                    &[],
                    &mut body_locals,
                    &statement.body.statements,
                )?;
                statements.push(CoreSystemStatement::While { condition, body });
            }
        }
    }

    Ok(CoreSystemBody { statements })
}

fn lower_system_query_loop(
    program: &Program,
    params: &[CoreSystemParam],
    outer_locals: &HashMap<Identifier, (CoreType, bool)>,
    query_loop: &SystemQueryLoopStatement,
) -> Result<CoreQueryLoop, CoreLowerError> {
    let param = params
        .iter()
        .find(|param| param.name == query_loop.query_param)
        .ok_or_else(|| {
            lower_error(format!(
                "unknown query parameter `{}`",
                query_loop.query_param
            ))
        })?;
    let CoreSystemParamKind::Query { terms } = &param.kind else {
        return Err(lower_error(format!(
            "query loop target `{}` is not a query parameter",
            query_loop.query_param
        )));
    };

    let required_terms = terms
        .iter()
        .filter(|term| term.access != CoreQueryAccess::Exclude)
        .collect::<Vec<_>>();
    if query_loop.bindings.len() != required_terms.len() {
        return Err(lower_error(format!(
            "query loop binding count {} does not match query term count {}",
            query_loop.bindings.len(),
            required_terms.len()
        )));
    }

    let bindings: Vec<CoreQueryLoopBinding> = query_loop
        .bindings
        .iter()
        .zip(required_terms)
        .map(|(binding, term)| CoreQueryLoopBinding {
            name: binding.name.clone(),
            component_id: term.component_id,
            component_name: term.name.clone(),
            access: term.access,
        })
        .collect();
    let body =
        lower_system_query_loop_body(program, params, &bindings, outer_locals, &query_loop.body)?;

    Ok(CoreQueryLoop {
        query_param: query_loop.query_param.clone(),
        bindings,
        body,
    })
}

fn lower_system_query_loop_body(
    program: &Program,
    params: &[CoreSystemParam],
    bindings: &[CoreQueryLoopBinding],
    outer_locals: &HashMap<Identifier, (CoreType, bool)>,
    statements: &[SystemBodyStatement],
) -> Result<Vec<CoreSystemStatement>, CoreLowerError> {
    let mut lowered = Vec::new();
    let mut locals = outer_locals.clone();

    for statement in statements {
        match statement {
            SystemBodyStatement::Expression(expression) => {
                lowered.push(CoreSystemStatement::Expression(lower_system_expression(
                    program, params, bindings, &locals, expression,
                )?));
            }
            SystemBodyStatement::AddAssign(add_assign) => {
                lowered.push(CoreSystemStatement::AddAssign {
                    target: lower_system_assignment_place(
                        params,
                        bindings,
                        &locals,
                        &add_assign.target,
                    )?,
                    value: lower_system_expression(
                        program,
                        params,
                        bindings,
                        &locals,
                        &add_assign.value,
                    )?,
                });
            }
            SystemBodyStatement::Let(let_statement) => {
                let ty = lower_core_type(&let_statement.type_name.name)?;
                let value = lower_system_expression(
                    program,
                    params,
                    bindings,
                    &locals,
                    &let_statement.initializer,
                )?;
                locals.insert(let_statement.name.clone(), (ty, let_statement.mutable));
                lowered.push(CoreSystemStatement::Let {
                    name: let_statement.name.clone(),
                    ty,
                    mutable: let_statement.mutable,
                    value,
                });
            }
            SystemBodyStatement::Assign(assignment) => {
                lowered.push(CoreSystemStatement::Assign {
                    target: lower_system_assignment_place(
                        params,
                        bindings,
                        &locals,
                        &assignment.target,
                    )?,
                    value: lower_system_expression(
                        program,
                        params,
                        bindings,
                        &locals,
                        &assignment.value,
                    )?,
                });
            }
            SystemBodyStatement::QueryLoop(query_loop) => {
                if !bindings.is_empty() {
                    return Err(lower_error("nested query loop lowering is not supported"));
                }
                lowered.push(CoreSystemStatement::QueryLoop(lower_system_query_loop(
                    program, params, &locals, query_loop,
                )?));
            }
            SystemBodyStatement::Block(block) => {
                let mut scoped = locals.clone();
                lowered.push(CoreSystemStatement::Block(lower_system_statements(
                    program,
                    params,
                    bindings,
                    &mut scoped,
                    &block.statements,
                )?));
            }
            SystemBodyStatement::If(statement) => {
                let condition = lower_system_expression(
                    program,
                    params,
                    bindings,
                    &locals,
                    &statement.condition,
                )?;
                let mut then_locals = locals.clone();
                let then_body = lower_system_statements(
                    program,
                    params,
                    bindings,
                    &mut then_locals,
                    &statement.then_block.statements,
                )?;
                let mut else_locals = locals.clone();
                let else_body = statement.else_block.as_ref().map_or_else(
                    || Ok(Vec::new()),
                    |block| {
                        lower_system_statements(
                            program,
                            params,
                            bindings,
                            &mut else_locals,
                            &block.statements,
                        )
                    },
                )?;
                lowered.push(CoreSystemStatement::If {
                    condition,
                    then_body,
                    else_body,
                });
            }
            SystemBodyStatement::While(statement) => {
                let condition = lower_system_expression(
                    program,
                    params,
                    bindings,
                    &locals,
                    &statement.condition,
                )?;
                let mut body_locals = locals.clone();
                let body = lower_system_statements(
                    program,
                    params,
                    bindings,
                    &mut body_locals,
                    &statement.body.statements,
                )?;
                lowered.push(CoreSystemStatement::While { condition, body });
            }
        }
    }

    Ok(lowered)
}

fn lower_system_statements(
    program: &Program,
    params: &[CoreSystemParam],
    bindings: &[CoreQueryLoopBinding],
    locals: &mut HashMap<Identifier, (CoreType, bool)>,
    statements: &[SystemBodyStatement],
) -> Result<Vec<CoreSystemStatement>, CoreLowerError> {
    lower_system_query_loop_body(program, params, bindings, locals, statements)
}

fn lower_system_expression(
    program: &Program,
    params: &[CoreSystemParam],
    bindings: &[CoreQueryLoopBinding],
    locals: &HashMap<Identifier, (CoreType, bool)>,
    expression: &Expression,
) -> Result<CoreSystemExpression, CoreLowerError> {
    match expression {
        Expression::FieldAccess {
            target, field_name, ..
        } => lower_system_field_access(params, bindings, target, field_name),
        Expression::Binary(binary) => {
            let operand_type =
                system_expression_type(program, params, bindings, locals, &binary.left)?;
            let op = match (binary.operator, operand_type) {
                (BinaryOperator::Add, CoreType::I32) => CoreSystemBinaryOp::I32Add,
                (BinaryOperator::Subtract, CoreType::I32) => CoreSystemBinaryOp::I32Subtract,
                (BinaryOperator::Multiply, CoreType::I32) => CoreSystemBinaryOp::I32Multiply,
                (BinaryOperator::Divide, CoreType::I32) => CoreSystemBinaryOp::I32Divide,
                (BinaryOperator::Remainder, CoreType::I32) => CoreSystemBinaryOp::I32Remainder,
                (BinaryOperator::ShiftLeft, CoreType::I32) => CoreSystemBinaryOp::I32ShiftLeft,
                (BinaryOperator::ShiftRight, CoreType::I32) => CoreSystemBinaryOp::I32ShiftRight,
                (BinaryOperator::BitAnd, CoreType::I32) => CoreSystemBinaryOp::I32BitAnd,
                (BinaryOperator::BitXor, CoreType::I32) => CoreSystemBinaryOp::I32BitXor,
                (BinaryOperator::BitOr, CoreType::I32) => CoreSystemBinaryOp::I32BitOr,
                (BinaryOperator::Add, CoreType::F32) => CoreSystemBinaryOp::F32Add,
                (BinaryOperator::Subtract, CoreType::F32) => CoreSystemBinaryOp::F32Subtract,
                (BinaryOperator::Multiply, CoreType::F32) => CoreSystemBinaryOp::F32Multiply,
                (BinaryOperator::Divide, CoreType::F32) => CoreSystemBinaryOp::F32Divide,
                (BinaryOperator::Less, CoreType::I32) => CoreSystemBinaryOp::I32Less,
                (BinaryOperator::LessEqual, CoreType::I32) => CoreSystemBinaryOp::I32LessEqual,
                (BinaryOperator::Greater, CoreType::I32) => CoreSystemBinaryOp::I32Greater,
                (BinaryOperator::GreaterEqual, CoreType::I32) => {
                    CoreSystemBinaryOp::I32GreaterEqual
                }
                (BinaryOperator::Less, CoreType::F32) => CoreSystemBinaryOp::F32Less,
                (BinaryOperator::LessEqual, CoreType::F32) => CoreSystemBinaryOp::F32LessEqual,
                (BinaryOperator::Greater, CoreType::F32) => CoreSystemBinaryOp::F32Greater,
                (BinaryOperator::GreaterEqual, CoreType::F32) => {
                    CoreSystemBinaryOp::F32GreaterEqual
                }
                (BinaryOperator::Equal, _) => CoreSystemBinaryOp::Equal,
                (BinaryOperator::NotEqual, _) => CoreSystemBinaryOp::NotEqual,
                (BinaryOperator::LogicalAnd, _) => CoreSystemBinaryOp::LogicalAnd,
                (BinaryOperator::LogicalOr, _) => CoreSystemBinaryOp::LogicalOr,
                _ => {
                    return Err(lower_error(format!(
                        "system body operator `{}` is invalid for {operand_type:?}",
                        binary.operator
                    )));
                }
            };

            Ok(CoreSystemExpression::Binary {
                op,
                left: Box::new(lower_system_expression(
                    program,
                    params,
                    bindings,
                    locals,
                    &binary.left,
                )?),
                right: Box::new(lower_system_expression(
                    program,
                    params,
                    bindings,
                    locals,
                    &binary.right,
                )?),
            })
        }
        Expression::Unary(unary) => {
            if is_folded_i32_min_literal(unary.operator, &unary.operand) {
                return Ok(CoreSystemExpression::I32Const(i32::MIN));
            }
            let ty = system_expression_type(program, params, bindings, locals, &unary.operand)?;
            let op = match (unary.operator, ty) {
                (UnaryOperator::Not, CoreType::Bool) => CoreSystemUnaryOp::BoolNot,
                (UnaryOperator::Negate, CoreType::I32) => CoreSystemUnaryOp::I32Negate,
                (UnaryOperator::Negate, CoreType::F32) => CoreSystemUnaryOp::F32Negate,
                (UnaryOperator::BitNot, CoreType::I32) => CoreSystemUnaryOp::I32BitNot,
                _ => return Err(lower_error("invalid typed unary expression")),
            };
            Ok(CoreSystemExpression::Unary {
                op,
                operand: Box::new(lower_system_expression(
                    program,
                    params,
                    bindings,
                    locals,
                    &unary.operand,
                )?),
            })
        }
        Expression::Identifier { name, .. } => locals
            .get(name)
            .map(|(ty, _)| CoreSystemExpression::Local {
                name: name.clone(),
                ty: *ty,
            })
            .ok_or_else(|| lower_error(format!("unknown system local `{name}`"))),
        Expression::Integer(integer) => i32::try_from(integer.value)
            .map(CoreSystemExpression::I32Const)
            .map_err(|_| lower_error("integer literal does not fit i32")),
        Expression::Float { text, .. } => text
            .parse::<f32>()
            .map(|value| CoreSystemExpression::F32Const(value.to_bits()))
            .map_err(|_| lower_error(format!("invalid f32 literal `{text}`"))),
        Expression::Bool { value, .. } => Ok(CoreSystemExpression::BoolConst(*value)),
        Expression::Parenthesized { expression, .. } => {
            lower_system_expression(program, params, bindings, locals, expression)
        }
    }
}

fn is_folded_i32_min_literal(operator: UnaryOperator, operand: &Expression) -> bool {
    operator == UnaryOperator::Negate
        && matches!(
            unparenthesized_expression(operand),
            Expression::Integer(integer) if integer.value == i32::MAX as u64 + 1
        )
}

fn unparenthesized_expression(mut expression: &Expression) -> &Expression {
    while let Expression::Parenthesized {
        expression: inner, ..
    } = expression
    {
        expression = inner;
    }
    expression
}

fn system_expression_type(
    program: &Program,
    params: &[CoreSystemParam],
    bindings: &[CoreQueryLoopBinding],
    locals: &HashMap<Identifier, (CoreType, bool)>,
    expression: &Expression,
) -> Result<CoreType, CoreLowerError> {
    match expression {
        Expression::Integer(_) => Ok(CoreType::I32),
        Expression::Float { .. } => Ok(CoreType::F32),
        Expression::Bool { .. } => Ok(CoreType::Bool),
        Expression::Identifier { name, .. } => locals
            .get(name)
            .map(|(ty, _)| *ty)
            .ok_or_else(|| lower_error(format!("unknown system local `{name}`"))),
        Expression::Parenthesized { expression, .. } => {
            system_expression_type(program, params, bindings, locals, expression)
        }
        Expression::Unary(unary) => match unary.operator {
            UnaryOperator::Not => Ok(CoreType::Bool),
            UnaryOperator::Negate | UnaryOperator::BitNot => {
                system_expression_type(program, params, bindings, locals, &unary.operand)
            }
        },
        Expression::Binary(binary) => match binary.operator {
            BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual
            | BinaryOperator::LogicalAnd
            | BinaryOperator::LogicalOr => Ok(CoreType::Bool),
            _ => system_expression_type(program, params, bindings, locals, &binary.left),
        },
        Expression::FieldAccess {
            target, field_name, ..
        } => {
            let Expression::Identifier { name, .. } = unparenthesized_expression(target) else {
                return Err(lower_error("field target must be a direct binding"));
            };
            if let Some(binding) = bindings.iter().find(|binding| binding.name == *name) {
                let local_name = binding
                    .component_name
                    .rsplit_once('.')
                    .map_or(binding.component_name.as_str(), |(_, name)| name);
                let component = resolve_component(&program.components, local_name)?;
                return component
                    .fields
                    .iter()
                    .find(|field| field.name == *field_name)
                    .ok_or_else(|| lower_error(format!("unknown component field `{field_name}`")))
                    .and_then(|field| lower_core_type(&field.type_name.name));
            }
            if let Some(param) = params.iter().find(|param| param.name == *name) {
                let resource_name = match &param.kind {
                    CoreSystemParamKind::ReadResource { name, .. }
                    | CoreSystemParamKind::MutResource { name, .. } => name,
                    CoreSystemParamKind::Query { .. } => {
                        return Err(lower_error("query parameter has no direct fields"));
                    }
                };
                let local_name = resource_name
                    .rsplit_once('.')
                    .map_or(resource_name.as_str(), |(_, name)| name);
                let resource = resolve_resource(&program.resources, local_name)?;
                return resource
                    .fields
                    .iter()
                    .find(|field| field.name == *field_name)
                    .ok_or_else(|| lower_error(format!("unknown resource field `{field_name}`")))
                    .and_then(|field| lower_core_type(&field.type_name.name));
            }
            Err(lower_error(format!("unknown field target `{name}`")))
        }
    }
}

fn lower_system_assignment_place(
    params: &[CoreSystemParam],
    bindings: &[CoreQueryLoopBinding],
    locals: &HashMap<Identifier, (CoreType, bool)>,
    expression: &Expression,
) -> Result<CoreSystemPlace, CoreLowerError> {
    if let Expression::Identifier { name, .. } = expression {
        let (ty, mutable) = locals
            .get(name)
            .copied()
            .ok_or_else(|| lower_error(format!("unknown system local `{name}`")))?;
        return Ok(CoreSystemPlace::Local {
            name: name.clone(),
            ty,
            mutable,
        });
    }

    lower_system_place(params, bindings, expression)
}

fn lower_system_field_access(
    params: &[CoreSystemParam],
    bindings: &[CoreQueryLoopBinding],
    target: &Expression,
    field_name: &Identifier,
) -> Result<CoreSystemExpression, CoreLowerError> {
    let Expression::Identifier { name, .. } = target else {
        return Err(lower_error(
            "nested system body field access is not lowerable yet",
        ));
    };

    if let Some(binding) = bindings.iter().find(|binding| binding.name == *name) {
        return Ok(CoreSystemExpression::ComponentField {
            binding: binding.name.clone(),
            component_id: binding.component_id,
            component_name: binding.component_name.clone(),
            field_name: field_name.clone(),
        });
    }

    if let Some(param) = params.iter().find(|param| param.name == *name) {
        let (resource_id, name) = match &param.kind {
            CoreSystemParamKind::ReadResource { resource_id, name }
            | CoreSystemParamKind::MutResource { resource_id, name } => (resource_id, name),
            CoreSystemParamKind::Query { .. } => {
                return Err(lower_error(format!(
                    "system body parameter `{}` is not a resource",
                    param.name
                )));
            }
        };

        return Ok(CoreSystemExpression::ResourceField {
            param: param.name.clone(),
            resource_id: *resource_id,
            resource_name: name.clone(),
            field_name: field_name.clone(),
        });
    }

    Err(lower_error(format!(
        "unknown system body field target `{name}`"
    )))
}

fn lower_system_place(
    params: &[CoreSystemParam],
    bindings: &[CoreQueryLoopBinding],
    expression: &Expression,
) -> Result<CoreSystemPlace, CoreLowerError> {
    let Expression::FieldAccess {
        target, field_name, ..
    } = expression
    else {
        return Err(lower_error(
            "assignment target must be a mutable local or direct mutable field",
        ));
    };
    let Expression::Identifier { name, .. } = &**target else {
        return Err(lower_error(
            "assignment target must be a direct binding or resource field",
        ));
    };

    if let Some(binding) = bindings.iter().find(|binding| binding.name == *name) {
        if binding.access != CoreQueryAccess::Mut {
            return Err(lower_error(format!(
                "assignment target `{name}` is not mutable"
            )));
        }
        return Ok(CoreSystemPlace::ComponentField {
            binding: binding.name.clone(),
            component_id: binding.component_id,
            component_name: binding.component_name.clone(),
            field_name: field_name.clone(),
        });
    }

    if let Some(param) = params.iter().find(|param| param.name == *name) {
        let CoreSystemParamKind::MutResource { resource_id, name } = &param.kind else {
            return Err(lower_error(format!(
                "resource target `{name}` is not mutable"
            )));
        };
        return Ok(CoreSystemPlace::ResourceField {
            param: param.name.clone(),
            resource_id: *resource_id,
            resource_name: name.clone(),
            field_name: field_name.clone(),
        });
    }

    Err(lower_error(format!("unknown assignment target `{name}`")))
}

fn lower_system_param(
    program: &Program,
    canonical_names: &CoreCanonicalNames,
    param: &SystemParam,
) -> Result<CoreSystemParam, CoreLowerError> {
    let kind = match &param.kind {
        SystemParamKind::ReadResource { resource_name, .. }
        | SystemParamKind::MutResource { resource_name, .. } => {
            resolve_resource(&program.resources, resource_name)?;
            let resource_id = resource_declaration_id(
                &program.components,
                &program.tags,
                &program.resources,
                resource_name,
            )?;
            let name = canonical_names.resource(resource_id)?;
            if matches!(&param.kind, SystemParamKind::MutResource { .. }) {
                CoreSystemParamKind::MutResource { resource_id, name }
            } else {
                CoreSystemParamKind::ReadResource { resource_id, name }
            }
        }
        SystemParamKind::Query { terms } => {
            let terms = terms
                .iter()
                .map(|term| {
                    resolve_queryable_schema(program, &term.component_name)?;
                    let component_id = component_declaration_id(
                        &program.components,
                        &program.tags,
                        &term.component_name,
                    )?;
                    Ok(CoreQueryTerm {
                        access: lower_query_access(term.access),
                        component_id,
                        name: canonical_names.component(component_id)?,
                    })
                })
                .collect::<Result<Vec<_>, CoreLowerError>>()?;
            CoreSystemParamKind::Query { terms }
        }
    };

    Ok(CoreSystemParam {
        name: param.name.clone(),
        kind,
    })
}

fn resolve_resource<'a>(
    resources: &'a [ResourceDecl],
    name: &str,
) -> Result<&'a ResourceDecl, CoreLowerError> {
    resources
        .iter()
        .find(|resource| resource.name == name)
        .ok_or_else(|| lower_error(format!("unknown resource `{name}`")))
}

fn resolve_component<'a>(
    components: &'a [ComponentDecl],
    name: &str,
) -> Result<&'a ComponentDecl, CoreLowerError> {
    components
        .iter()
        .find(|component| component.name == name)
        .ok_or_else(|| lower_error(format!("unknown component `{name}`")))
}

fn resolve_tag<'a>(tags: &'a [TagDecl], name: &str) -> Result<&'a TagDecl, CoreLowerError> {
    tags.iter()
        .find(|tag| tag.name == name)
        .ok_or_else(|| lower_error(format!("unknown tag `{name}`")))
}

fn resolve_queryable_schema(program: &Program, name: &str) -> Result<(), CoreLowerError> {
    if resolve_component(&program.components, name).is_ok()
        || resolve_tag(&program.tags, name).is_ok()
    {
        Ok(())
    } else {
        Err(lower_error(format!("unknown component or tag `{name}`")))
    }
}

fn resolve_system<'a>(
    systems: &'a [SystemDecl],
    name: &str,
) -> Result<&'a SystemDecl, CoreLowerError> {
    systems
        .iter()
        .find(|system| system.name == name)
        .ok_or_else(|| lower_error(format!("unknown system `{name}`")))
}

fn lower_query_access(access: ParserQueryAccess) -> CoreQueryAccess {
    match access {
        ParserQueryAccess::Read => CoreQueryAccess::Read,
        ParserQueryAccess::Mut => CoreQueryAccess::Mut,
        ParserQueryAccess::Exclude => CoreQueryAccess::Exclude,
    }
}

fn component_declaration_id(
    components: &[ComponentDecl],
    tags: &[TagDecl],
    name: &str,
) -> Result<u64, CoreLowerError> {
    if let Some(index) = components
        .iter()
        .position(|component| component.name == name)
    {
        return declaration_index(index, "component declaration");
    }
    let tag_index = tags
        .iter()
        .position(|tag| tag.name == name)
        .ok_or_else(|| lower_error(format!("unknown component or tag `{name}`")))?;
    checked_declaration_offset(components.len(), tag_index, "component and tag declaration")
}

fn resource_declaration_id(
    components: &[ComponentDecl],
    tags: &[TagDecl],
    resources: &[ResourceDecl],
    name: &str,
) -> Result<u64, CoreLowerError> {
    let resource_index = resources
        .iter()
        .position(|resource| resource.name == name)
        .ok_or_else(|| lower_error(format!("unknown resource `{name}`")))?;
    let schema_prefix = components
        .len()
        .checked_add(tags.len())
        .ok_or_else(|| lower_error("schema declaration index overflow"))?;
    checked_declaration_offset(schema_prefix, resource_index, "resource declaration")
}

fn system_declaration_id(systems: &[SystemDecl], name: &str) -> Result<u64, CoreLowerError> {
    let index = systems
        .iter()
        .position(|system| system.name == name)
        .ok_or_else(|| lower_error(format!("unknown system `{name}`")))?;
    declaration_index(index, "system declaration")
}

fn schedule_declaration_id(schedules: &[ScheduleDecl], name: &str) -> Result<u64, CoreLowerError> {
    let index = schedules
        .iter()
        .position(|schedule| schedule.name == name)
        .ok_or_else(|| lower_error(format!("unknown schedule `{name}`")))?;
    declaration_index(index, "schedule declaration")
}

fn checked_declaration_offset(
    prefix: usize,
    index: usize,
    context: &'static str,
) -> Result<u64, CoreLowerError> {
    let dense = prefix
        .checked_add(index)
        .ok_or_else(|| lower_error(format!("{context} index overflow")))?;
    declaration_index(dense, context)
}

fn declaration_index(index: usize, context: &'static str) -> Result<u64, CoreLowerError> {
    u64::try_from(index).map_err(|_| lower_error(format!("{context} index exceeds u64")))
}

struct StartupLowerer<'a> {
    components: &'a [ComponentDecl],
    tags: &'a [TagDecl],
    resources: &'a [ResourceDecl],
    schedules: &'a [ScheduleDecl],
    canonical_names: &'a CoreCanonicalNames,
    locals: Vec<CoreLocal>,
    local_by_name: HashMap<Identifier, LocalId>,
    generated_names: IdentifierInterner,
    blocks: Vec<PendingCoreBlock>,
    current_block: BlockId,
    next_block: u64,
    next_local: u64,
    next_value: u64,
    source_entries: Vec<CoreSourceMapEntry>,
    deterministic_locals: HashMap<LocalId, CoreLiteralValue>,
    deterministic_reachable: bool,
}

struct PendingCoreBlock {
    id: BlockId,
    instructions: Vec<CoreInstruction>,
    terminator: Option<CoreTerminator>,
}

struct LoweredStartup {
    locals: Vec<CoreLocal>,
    blocks: Vec<CoreBlock>,
    source_entries: Vec<CoreSourceMapEntry>,
}

struct PendingPayloadField {
    name: Identifier,
    local: LocalId,
    value: CoreLiteralValue,
    span: crate::lexer::SourceSpan,
}

enum DeterministicEvaluation {
    Value(CoreLiteralValue),
    Trap,
    Unreachable,
}

impl<'a> StartupLowerer<'a> {
    fn new(program: &'a Program, canonical_names: &'a CoreCanonicalNames) -> Self {
        Self {
            components: &program.components,
            tags: &program.tags,
            resources: &program.resources,
            schedules: &program.schedules,
            canonical_names,
            locals: Vec::new(),
            local_by_name: HashMap::new(),
            generated_names: IdentifierInterner::default(),
            blocks: vec![PendingCoreBlock {
                id: BlockId(0),
                instructions: Vec::new(),
                terminator: None,
            }],
            current_block: BlockId(0),
            next_block: 1,
            next_local: 0,
            next_value: 0,
            source_entries: Vec::new(),
            deterministic_locals: HashMap::new(),
            deterministic_reachable: true,
        }
    }

    fn lower_startup(mut self, startup: &StartupBlock) -> Result<LoweredStartup, CoreLowerError> {
        let mut exited = false;
        for statement in &startup.statements {
            if exited {
                return Err(lower_error("statement after startup exit"));
            }

            match statement {
                Statement::Let(let_statement) => {
                    let ty = lower_core_type(&let_statement.type_name.name)?;
                    let deterministic =
                        self.evaluate_deterministic_expression(&let_statement.initializer)?;
                    let local = self.allocate_local(let_statement.name.clone(), ty)?;
                    let value = self.lower_expression(&let_statement.initializer)?;
                    self.emit(
                        CoreInstruction::LocalStore { local, value },
                        let_statement.span,
                    )?;
                    self.update_deterministic_local(local, ty, deterministic)?;
                }
                Statement::Assign(assignment) => {
                    let Expression::Identifier { name, .. } = &assignment.target else {
                        return Err(lower_error(
                            "startup assignment target must be a local variable",
                        ));
                    };
                    let local = self
                        .local_by_name
                        .get(name)
                        .copied()
                        .ok_or_else(|| lower_error(format!("unknown local `{name}`")))?;
                    let local_type = self.local_type(local)?;
                    let deterministic =
                        self.evaluate_deterministic_expression(&assignment.value)?;
                    let value = self.lower_expression(&assignment.value)?;
                    self.emit(
                        CoreInstruction::LocalStore { local, value },
                        assignment.span,
                    )?;
                    self.update_deterministic_local(local, local_type, deterministic)?;
                }
                Statement::AddAssign(add_assign) => {
                    self.lower_startup_add_assign(add_assign)?;
                }
                Statement::Exit(exit) => {
                    let value = self.lower_expression(&exit.expression)?;
                    self.terminate(CoreTerminator::Exit { value }, exit.span)?;
                    exited = true;
                }
                Statement::Run(run) => {
                    self.lower_run_statement(run)?;
                }
                Statement::Spawn(spawn) => {
                    self.lower_spawn_statement(spawn)?;
                }
                Statement::Resource(resource) => {
                    self.lower_resource_statement(resource)?;
                }
            }
        }

        if !exited {
            return Err(lower_error("expected startup exit"));
        }
        let blocks = self
            .blocks
            .into_iter()
            .map(|block| {
                Ok(CoreBlock {
                    id: block.id,
                    instructions: block.instructions,
                    terminator: block.terminator.ok_or_else(|| {
                        lower_error(format!("Core block {} has no terminator", block.id.0))
                    })?,
                })
            })
            .collect::<Result<Vec<_>, CoreLowerError>>()?;
        Ok(LoweredStartup {
            locals: self.locals,
            blocks,
            source_entries: self.source_entries,
        })
    }

    fn lower_startup_add_assign(
        &mut self,
        add_assign: &AddAssignStatement,
    ) -> Result<(), CoreLowerError> {
        let Expression::Identifier { name, .. } = &add_assign.target else {
            return Err(lower_error(
                "startup assignment target must be a local variable",
            ));
        };
        let local = self
            .local_by_name
            .get(name)
            .copied()
            .ok_or_else(|| lower_error(format!("unknown local `{name}`")))?;
        let local_type = self.local_type(local)?;
        let deterministic = self.evaluate_deterministic_add_assign(local, &add_assign.value)?;

        let left = self.allocate_value()?;
        self.emit(
            CoreInstruction::LocalLoad {
                result: left,
                local,
            },
            add_assign.target.span(),
        )?;
        let right = self.lower_expression(&add_assign.value)?;
        let result = self.allocate_value()?;
        match local_type {
            CoreType::I32 => self.emit(
                CoreInstruction::I32Binary {
                    result,
                    op: CoreBinaryOp::Add,
                    left,
                    right,
                },
                add_assign.span,
            )?,
            CoreType::F32 => self.emit(
                CoreInstruction::F32Binary {
                    result,
                    op: CoreBinaryOp::Add,
                    left,
                    right,
                },
                add_assign.span,
            )?,
            CoreType::Bool => {
                return Err(lower_error("add-assign target must have numeric type"));
            }
        }
        self.emit(
            CoreInstruction::LocalStore {
                local,
                value: result,
            },
            add_assign.span,
        )?;
        self.update_deterministic_local(local, local_type, deterministic)
    }

    fn lower_spawn_statement(&mut self, spawn: &SpawnStatement) -> Result<(), CoreLowerError> {
        let mut components = Vec::new();
        for component in &spawn.components {
            components.push(self.lower_spawn_component(component)?);
        }

        self.emit(CoreInstruction::Spawn { components }, spawn.span)?;
        Ok(())
    }

    fn lower_resource_statement(
        &mut self,
        resource: &crate::parser::ResourceStatement,
    ) -> Result<(), CoreLowerError> {
        let declaration = self
            .resources
            .iter()
            .find(|declaration| declaration.name == resource.name)
            .cloned()
            .ok_or_else(|| lower_error(format!("unknown resource `{}`", resource.name)))?;
        let mut pending = HashMap::new();
        for field in &resource.fields {
            let declaration_field = declaration
                .fields
                .iter()
                .find(|candidate| candidate.name == field.name)
                .ok_or_else(|| {
                    lower_error(format!(
                        "unknown field `{}` for resource `{}`",
                        field.name, resource.name
                    ))
                })?;
            let lowered = self.lower_payload_field(
                &declaration_field.type_name.name,
                &field.value,
                "resource",
                &resource.name,
                &field.name,
            )?;
            if pending.insert(field.name.clone(), lowered).is_some() {
                return Err(lower_error(format!(
                    "duplicate field `{}` in resource `{}`",
                    field.name, resource.name
                )));
            }
        }
        let mut fields = Vec::new();
        for declaration_field in &declaration.fields {
            let field = pending.remove(&declaration_field.name).ok_or_else(|| {
                lower_error(format!(
                    "missing field `{}` in resource `{}`",
                    declaration_field.name, resource.name
                ))
            })?;
            let evaluation = self.load_payload_field(&field)?;
            fields.push(CoreResourceField {
                name: field.name,
                evaluation,
                value: field.value,
            });
        }
        let resource_id =
            resource_declaration_id(self.components, self.tags, self.resources, &resource.name)?;
        let resource_name = self.canonical_names.resource(resource_id)?;
        self.emit(
            CoreInstruction::InitializeResource {
                resource_id,
                resource_name,
                fields,
            },
            resource.span,
        )?;
        Ok(())
    }

    fn lower_run_statement(
        &mut self,
        run: &crate::parser::RunStatement,
    ) -> Result<(), CoreLowerError> {
        let schedule = self
            .schedules
            .iter()
            .find(|schedule| schedule.name == run.schedule_name)
            .ok_or_else(|| lower_error(format!("unknown schedule `{}`", run.schedule_name)))?;
        let schedule_id = schedule_declaration_id(self.schedules, &schedule.name)?;
        self.emit(
            CoreInstruction::RunSchedule {
                schedule_id,
                schedule_name: self.canonical_names.schedule(schedule_id)?,
            },
            run.span,
        )?;
        Ok(())
    }

    fn lower_spawn_component(
        &mut self,
        component: &SpawnComponentLiteral,
    ) -> Result<CoreSpawnComponent, CoreLowerError> {
        let fields = if let Some(declaration) = self
            .components
            .iter()
            .find(|declaration| declaration.name == component.name)
            .cloned()
        {
            let mut pending = HashMap::new();
            for field in &component.fields {
                let declaration_field = declaration
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == field.name)
                    .ok_or_else(|| {
                        lower_error(format!(
                            "unknown field `{}` for component `{}`",
                            field.name, component.name
                        ))
                    })?;
                let lowered = self.lower_payload_field(
                    &declaration_field.type_name.name,
                    &field.value,
                    "component",
                    &component.name,
                    &field.name,
                )?;
                if pending.insert(field.name.clone(), lowered).is_some() {
                    return Err(lower_error(format!(
                        "duplicate field `{}` in component `{}`",
                        field.name, component.name
                    )));
                }
            }
            let mut fields = Vec::new();
            for declaration_field in &declaration.fields {
                let field = pending.remove(&declaration_field.name).ok_or_else(|| {
                    lower_error(format!(
                        "missing field `{}` in component `{}`",
                        declaration_field.name, component.name
                    ))
                })?;
                let evaluation = self.load_payload_field(&field)?;
                fields.push(CoreSpawnField {
                    name: field.name,
                    evaluation,
                    value: field.value,
                });
            }
            fields
        } else if self
            .tags
            .iter()
            .any(|declaration| declaration.name == component.name)
        {
            if !component.fields.is_empty() {
                return Err(lower_error(format!(
                    "tag literal `{}` cannot contain fields",
                    component.name
                )));
            }
            vec![]
        } else {
            return Err(lower_error(format!(
                "unknown component or tag `{}`",
                component.name
            )));
        };

        let component_id = component_declaration_id(self.components, self.tags, &component.name)?;
        Ok(CoreSpawnComponent {
            component_id,
            name: self.canonical_names.component(component_id)?,
            fields,
        })
    }

    fn lower_payload_field(
        &mut self,
        type_name: &Identifier,
        value: &ComponentLiteralValue,
        owner_kind: &str,
        owner_name: &Identifier,
        field_name: &Identifier,
    ) -> Result<PendingPayloadField, CoreLowerError> {
        let ty = lower_core_type(type_name)?;
        let (evaluation, literal) =
            self.lower_payload_value(ty, value, owner_kind, owner_name, field_name)?;
        let local_name = format!("$payload_{}", self.next_local);
        let local = self.allocate_generated_local(local_name, ty)?;
        self.emit(
            CoreInstruction::LocalStore {
                local,
                value: evaluation,
            },
            value.span(),
        )?;
        Ok(PendingPayloadField {
            name: field_name.clone(),
            local,
            value: literal,
            span: value.span(),
        })
    }

    fn load_payload_field(
        &mut self,
        field: &PendingPayloadField,
    ) -> Result<ValueId, CoreLowerError> {
        let result = self.allocate_value()?;
        self.emit(
            CoreInstruction::LocalLoad {
                result,
                local: field.local,
            },
            field.span,
        )?;
        Ok(result)
    }

    fn lower_payload_value(
        &mut self,
        expected_type: CoreType,
        value: &ComponentLiteralValue,
        owner_kind: &str,
        owner_name: &str,
        field_name: &str,
    ) -> Result<(ValueId, CoreLiteralValue), CoreLowerError> {
        let expected_name = core_type_name(expected_type);
        match value {
            ComponentLiteralValue::Float { text, span } => {
                if expected_type != CoreType::F32 {
                    return Err(lower_error(format!(
                        "float literal cannot initialize {expected_name} {owner_kind} field `{owner_name}.{field_name}`"
                    )));
                }
                let parsed = text.parse::<f32>().map_err(|_| {
                    lower_error(format!(
                        "invalid f32 literal `{text}` for {owner_kind} field `{owner_name}.{field_name}`"
                    ))
                })?;
                let result = self.allocate_value()?;
                let bits = parsed.to_bits();
                self.emit(CoreInstruction::F32Const { result, bits }, *span)?;
                Ok((result, CoreLiteralValue::F32Bits(bits)))
            }
            ComponentLiteralValue::Integer { value, span } => {
                if expected_type != CoreType::I32 {
                    return Err(lower_error(format!(
                        "integer literal cannot initialize {expected_name} {owner_kind} field `{owner_name}.{field_name}`"
                    )));
                }
                let parsed = i32::try_from(*value).map_err(|_| {
                    lower_error(format!(
                        "integer literal does not fit i32 for {owner_kind} field `{owner_name}.{field_name}`"
                    ))
                })?;
                let result = self.allocate_value()?;
                self.emit(
                    CoreInstruction::I32Const {
                        result,
                        value: parsed,
                    },
                    *span,
                )?;
                Ok((result, CoreLiteralValue::I32(parsed)))
            }
            ComponentLiteralValue::Bool { value, span } => {
                if expected_type != CoreType::Bool {
                    return Err(lower_error(format!(
                        "bool literal cannot initialize {expected_name} {owner_kind} field `{owner_name}.{field_name}`"
                    )));
                }
                let result = self.allocate_value()?;
                self.emit(
                    CoreInstruction::BoolConst {
                        result,
                        value: *value,
                    },
                    *span,
                )?;
                Ok((result, CoreLiteralValue::Bool(*value)))
            }
            ComponentLiteralValue::Expression { expression, .. } => {
                let deterministic = self.evaluate_deterministic_expression(expression)?;
                let result = self.lower_expression(expression)?;
                let literal = match deterministic {
                    DeterministicEvaluation::Value(literal)
                        if literal_type(&literal) == expected_type =>
                    {
                        literal
                    }
                    DeterministicEvaluation::Value(_) => {
                        return Err(lower_error(format!(
                            "expression type cannot initialize {expected_name} {owner_kind} field `{owner_name}.{field_name}`"
                        )));
                    }
                    DeterministicEvaluation::Trap => {
                        self.deterministic_reachable = false;
                        placeholder_literal(expected_type)
                    }
                    DeterministicEvaluation::Unreachable => placeholder_literal(expected_type),
                };
                Ok((result, literal))
            }
        }
    }

    fn evaluate_deterministic_expression(
        &self,
        expression: &Expression,
    ) -> Result<DeterministicEvaluation, CoreLowerError> {
        if !self.deterministic_reachable {
            return Ok(DeterministicEvaluation::Unreachable);
        }
        evaluate_deterministic_expression(
            expression,
            &self.local_by_name,
            &self.deterministic_locals,
        )
    }

    fn evaluate_deterministic_add_assign(
        &self,
        local: LocalId,
        value: &Expression,
    ) -> Result<DeterministicEvaluation, CoreLowerError> {
        if !self.deterministic_reachable {
            return Ok(DeterministicEvaluation::Unreachable);
        }
        let left = self
            .deterministic_locals
            .get(&local)
            .cloned()
            .ok_or_else(|| {
                lower_error(format!("local `{}` has no deterministic value", local.0))
            })?;
        let right = self.evaluate_deterministic_expression(value)?;
        let DeterministicEvaluation::Value(right) = right else {
            return Ok(right);
        };
        match (left, right) {
            (CoreLiteralValue::I32(left), CoreLiteralValue::I32(right)) => {
                evaluate_deterministic_i32(BinaryOperator::Add, left, right)
            }
            (CoreLiteralValue::F32Bits(left), CoreLiteralValue::F32Bits(right)) => {
                evaluate_deterministic_f32(BinaryOperator::Add, left, right)
            }
            _ => Err(lower_error(
                "deterministic add-assign operand types do not match",
            )),
        }
    }

    fn update_deterministic_local(
        &mut self,
        local: LocalId,
        expected_type: CoreType,
        evaluation: DeterministicEvaluation,
    ) -> Result<(), CoreLowerError> {
        match evaluation {
            DeterministicEvaluation::Value(value) => {
                if literal_type(&value) != expected_type {
                    return Err(lower_error("deterministic local value has the wrong type"));
                }
                self.deterministic_locals.insert(local, value);
            }
            DeterministicEvaluation::Trap => {
                self.deterministic_reachable = false;
                self.deterministic_locals.remove(&local);
            }
            DeterministicEvaluation::Unreachable => {
                self.deterministic_locals.remove(&local);
            }
        }
        Ok(())
    }

    fn local_type(&self, local: LocalId) -> Result<CoreType, CoreLowerError> {
        let index = usize::try_from(local.0)
            .map_err(|_| lower_error("Core local index exceeds host address space"))?;
        self.locals
            .get(index)
            .map(|local| local.ty)
            .ok_or_else(|| lower_error(format!("unknown Core local {}", local.0)))
    }

    fn lower_expression(&mut self, expression: &Expression) -> Result<ValueId, CoreLowerError> {
        match expression {
            Expression::Integer(integer) => {
                let value = if integer.value <= i32::MAX as u64 {
                    integer.value as i32
                } else {
                    return Err(lower_error("integer literal does not fit i32"));
                };
                let result = self.allocate_value()?;
                self.emit(
                    CoreInstruction::I32Const { result, value },
                    expression.span(),
                )?;
                Ok(result)
            }
            Expression::Bool { value, .. } => {
                let result = self.allocate_value()?;
                self.emit(
                    CoreInstruction::BoolConst {
                        result,
                        value: *value,
                    },
                    expression.span(),
                )?;
                Ok(result)
            }
            Expression::Float { text, .. } => {
                let value = text
                    .parse::<f32>()
                    .map_err(|_| lower_error(format!("invalid f32 literal `{text}`")))?;
                let result = self.allocate_value()?;
                self.emit(
                    CoreInstruction::F32Const {
                        result,
                        bits: value.to_bits(),
                    },
                    expression.span(),
                )?;
                Ok(result)
            }
            Expression::Identifier { name, .. } => {
                let local = self
                    .local_by_name
                    .get(name)
                    .copied()
                    .ok_or_else(|| lower_error(format!("unknown local `{name}`")))?;
                let result = self.allocate_value()?;
                self.emit(
                    CoreInstruction::LocalLoad { result, local },
                    expression.span(),
                )?;
                Ok(result)
            }
            Expression::FieldAccess { field_name, .. } => Err(lower_error(format!(
                "field access `{field_name}` is not lowerable yet"
            ))),
            Expression::Binary(binary) => {
                if matches!(
                    binary.operator,
                    BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
                ) {
                    return self.lower_short_circuit(binary);
                }
                let left = self.lower_expression(&binary.left)?;
                let right = self.lower_expression(&binary.right)?;
                let result = self.allocate_value()?;
                let operand_type = self.expression_core_type(&binary.left)?;
                match binary.operator {
                    BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
                        if operand_type == CoreType::F32 =>
                    {
                        self.emit(
                            CoreInstruction::F32Binary {
                                result,
                                op: lower_binary_operator(binary.operator)?,
                                left,
                                right,
                            },
                            expression.span(),
                        )?;
                    }
                    BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
                    | BinaryOperator::Remainder
                    | BinaryOperator::ShiftLeft
                    | BinaryOperator::ShiftRight
                    | BinaryOperator::BitAnd
                    | BinaryOperator::BitXor
                    | BinaryOperator::BitOr => {
                        self.emit(
                            CoreInstruction::I32Binary {
                                result,
                                op: lower_binary_operator(binary.operator)?,
                                left,
                                right,
                            },
                            expression.span(),
                        )?;
                    }
                    BinaryOperator::Equal | BinaryOperator::NotEqual => {
                        let operand_type = self.expression_core_type(&binary.left)?;
                        self.emit(
                            CoreInstruction::Equal {
                                result,
                                left,
                                right,
                                operand_type,
                                negate: binary.operator == BinaryOperator::NotEqual,
                            },
                            expression.span(),
                        )?;
                    }
                    BinaryOperator::Less
                    | BinaryOperator::LessEqual
                    | BinaryOperator::Greater
                    | BinaryOperator::GreaterEqual => {
                        self.emit(
                            CoreInstruction::Compare {
                                result,
                                op: lower_comparison_operator(binary.operator)?,
                                left,
                                right,
                                operand_type,
                            },
                            expression.span(),
                        )?;
                    }
                    BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr => unreachable!(),
                }
                Ok(result)
            }
            Expression::Unary(unary) => {
                if unary.operator == UnaryOperator::Negate {
                    if let Expression::Integer(integer) = unparenthesized_expression(&unary.operand)
                    {
                        if integer.value == i32::MAX as u64 + 1 {
                            let result = self.allocate_value()?;
                            self.emit(
                                CoreInstruction::I32Const {
                                    result,
                                    value: i32::MIN,
                                },
                                expression.span(),
                            )?;
                            return Ok(result);
                        }
                    }
                }
                let operand = self.lower_expression(&unary.operand)?;
                let result = self.allocate_value()?;
                let operand_type = self.expression_core_type(&unary.operand)?;
                match (unary.operator, operand_type) {
                    (UnaryOperator::Not, CoreType::Bool) => {
                        self.emit(
                            CoreInstruction::BoolNot { result, operand },
                            expression.span(),
                        )?;
                    }
                    (UnaryOperator::Negate, CoreType::I32) => {
                        self.emit(
                            CoreInstruction::I32Unary {
                                result,
                                op: CoreUnaryOp::Negate,
                                operand,
                            },
                            expression.span(),
                        )?;
                    }
                    (UnaryOperator::Negate, CoreType::F32) => {
                        self.emit(
                            CoreInstruction::F32Unary {
                                result,
                                op: CoreUnaryOp::Negate,
                                operand,
                            },
                            expression.span(),
                        )?;
                    }
                    (UnaryOperator::BitNot, CoreType::I32) => {
                        self.emit(
                            CoreInstruction::I32Unary {
                                result,
                                op: CoreUnaryOp::BitNot,
                                operand,
                            },
                            expression.span(),
                        )?;
                    }
                    _ => return Err(lower_error("invalid typed unary expression")),
                }
                Ok(result)
            }
            Expression::Parenthesized { expression, .. } => self.lower_expression(expression),
        }
    }

    fn lower_short_circuit(
        &mut self,
        binary: &crate::parser::BinaryExpression,
    ) -> Result<ValueId, CoreLowerError> {
        let left = self.lower_expression(&binary.left)?;
        let result_local_name = format!("$short_circuit_{}", self.next_local);
        let result_local = self.allocate_generated_local(result_local_name, CoreType::Bool)?;
        let rhs_block = self.allocate_block()?;
        let short_block = self.allocate_block()?;
        let merge_block = self.allocate_block()?;
        let (then_block, else_block, short_value) = match binary.operator {
            BinaryOperator::LogicalAnd => (rhs_block, short_block, false),
            BinaryOperator::LogicalOr => (short_block, rhs_block, true),
            _ => unreachable!(),
        };
        self.terminate(
            CoreTerminator::Branch {
                condition: left,
                then_block,
                else_block,
            },
            binary.span,
        )?;

        self.switch_to(short_block);
        let short = self.allocate_value()?;
        self.emit(
            CoreInstruction::BoolConst {
                result: short,
                value: short_value,
            },
            binary.operator_span,
        )?;
        self.emit(
            CoreInstruction::LocalStore {
                local: result_local,
                value: short,
            },
            binary.span,
        )?;
        self.terminate(
            CoreTerminator::Jump {
                target: merge_block,
            },
            binary.span,
        )?;

        self.switch_to(rhs_block);
        let right = self.lower_expression(&binary.right)?;
        self.emit(
            CoreInstruction::LocalStore {
                local: result_local,
                value: right,
            },
            binary.right.span(),
        )?;
        self.terminate(
            CoreTerminator::Jump {
                target: merge_block,
            },
            binary.span,
        )?;

        self.switch_to(merge_block);
        let result = self.allocate_value()?;
        self.emit(
            CoreInstruction::LocalLoad {
                result,
                local: result_local,
            },
            binary.span,
        )?;
        Ok(result)
    }

    fn expression_core_type(&self, expression: &Expression) -> Result<CoreType, CoreLowerError> {
        match expression {
            Expression::Integer(_) => Ok(CoreType::I32),
            Expression::Float { .. } => Ok(CoreType::F32),
            Expression::Bool { .. } => Ok(CoreType::Bool),
            Expression::Unary(unary) => match unary.operator {
                UnaryOperator::Not => Ok(CoreType::Bool),
                UnaryOperator::Negate | UnaryOperator::BitNot => {
                    self.expression_core_type(&unary.operand)
                }
            },
            Expression::Identifier { name, .. } => {
                let local = self
                    .local_by_name
                    .get(name)
                    .copied()
                    .ok_or_else(|| lower_error(format!("unknown local `{name}`")))?;
                let index = usize::try_from(local.0)
                    .map_err(|_| lower_error("Core local index exceeds host address space"))?;
                Ok(self.locals[index].ty)
            }
            Expression::Binary(binary) => match binary.operator {
                BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::LogicalAnd
                | BinaryOperator::LogicalOr => Ok(CoreType::Bool),
                BinaryOperator::Less
                | BinaryOperator::LessEqual
                | BinaryOperator::Greater
                | BinaryOperator::GreaterEqual => Ok(CoreType::Bool),
                BinaryOperator::Add
                | BinaryOperator::Subtract
                | BinaryOperator::Multiply
                | BinaryOperator::Divide
                | BinaryOperator::Remainder
                | BinaryOperator::ShiftLeft
                | BinaryOperator::ShiftRight
                | BinaryOperator::BitAnd
                | BinaryOperator::BitXor
                | BinaryOperator::BitOr => self.expression_core_type(&binary.left),
            },
            Expression::FieldAccess { .. } => {
                Err(lower_error("field access is not lowerable in startup"))
            }
            Expression::Parenthesized { expression, .. } => self.expression_core_type(expression),
        }
    }

    fn allocate_local(
        &mut self,
        name: Identifier,
        ty: CoreType,
    ) -> Result<LocalId, CoreLowerError> {
        if self.local_by_name.contains_key(&name) {
            return Err(lower_error(format!("duplicate local `{name}`")));
        }

        let id = LocalId(self.next_local);
        self.next_local = self
            .next_local
            .checked_add(1)
            .ok_or_else(|| lower_error("Core local id space is exhausted"))?;
        self.local_by_name.insert(name.clone(), id);
        self.locals.push(CoreLocal { id, name, ty });
        Ok(id)
    }

    fn allocate_generated_local(
        &mut self,
        name: String,
        ty: CoreType,
    ) -> Result<LocalId, CoreLowerError> {
        let name = self
            .generated_names
            .intern(name)
            .map_err(|error| allocation_lower_error("generated local name", error))?;
        self.allocate_local(name, ty)
    }

    fn allocate_value(&mut self) -> Result<ValueId, CoreLowerError> {
        let id = ValueId(self.next_value);
        self.next_value = self
            .next_value
            .checked_add(1)
            .ok_or_else(|| lower_error("Core value id space is exhausted"))?;
        Ok(id)
    }

    fn allocate_block(&mut self) -> Result<BlockId, CoreLowerError> {
        let id = BlockId(self.next_block);
        self.next_block = self
            .next_block
            .checked_add(1)
            .ok_or_else(|| lower_error("Core block id space is exhausted"))?;
        self.blocks.push(PendingCoreBlock {
            id,
            instructions: Vec::new(),
            terminator: None,
        });
        Ok(id)
    }

    fn switch_to(&mut self, block: BlockId) {
        self.current_block = block;
    }

    fn emit(
        &mut self,
        instruction: CoreInstruction,
        span: crate::lexer::SourceSpan,
    ) -> Result<(), CoreLowerError> {
        let block = self.current_block;
        let instruction_index = u64::try_from(self.current_block_mut().instructions.len())
            .map_err(|_| lower_error("Core instruction index exceeds u64"))?;
        self.current_block_mut().instructions.push(instruction);
        self.source_entries.push(CoreSourceMapEntry {
            subject: CoreSourceSubject::StartupInstruction {
                block,
                instruction_index,
            },
            span,
        });
        Ok(())
    }

    fn terminate(
        &mut self,
        terminator: CoreTerminator,
        span: crate::lexer::SourceSpan,
    ) -> Result<(), CoreLowerError> {
        let block_id = self.current_block;
        let block = self.current_block_mut();
        if block.terminator.replace(terminator).is_some() {
            return Err(lower_error(format!(
                "Core block {} already has a terminator",
                block.id.0
            )));
        }
        self.source_entries.push(CoreSourceMapEntry {
            subject: CoreSourceSubject::StartupTerminator { block: block_id },
            span,
        });
        Ok(())
    }

    fn current_block_mut(&mut self) -> &mut PendingCoreBlock {
        self.blocks
            .iter_mut()
            .find(|block| block.id == self.current_block)
            .expect("allocated current Core block exists")
    }
}

fn lower_binary_operator(operator: BinaryOperator) -> Result<CoreBinaryOp, CoreLowerError> {
    match operator {
        BinaryOperator::Add => Ok(CoreBinaryOp::Add),
        BinaryOperator::Subtract => Ok(CoreBinaryOp::Subtract),
        BinaryOperator::Multiply => Ok(CoreBinaryOp::Multiply),
        BinaryOperator::Divide => Ok(CoreBinaryOp::Divide),
        BinaryOperator::Remainder => Ok(CoreBinaryOp::Remainder),
        BinaryOperator::ShiftLeft => Ok(CoreBinaryOp::ShiftLeft),
        BinaryOperator::ShiftRight => Ok(CoreBinaryOp::ShiftRight),
        BinaryOperator::BitAnd => Ok(CoreBinaryOp::BitAnd),
        BinaryOperator::BitXor => Ok(CoreBinaryOp::BitXor),
        BinaryOperator::BitOr => Ok(CoreBinaryOp::BitOr),
        BinaryOperator::Equal
        | BinaryOperator::NotEqual
        | BinaryOperator::Less
        | BinaryOperator::LessEqual
        | BinaryOperator::Greater
        | BinaryOperator::GreaterEqual
        | BinaryOperator::LogicalAnd
        | BinaryOperator::LogicalOr => Err(lower_error(format!(
            "operator `{operator}` is not an i32 arithmetic operator"
        ))),
    }
}

fn lower_comparison_operator(operator: BinaryOperator) -> Result<CoreComparisonOp, CoreLowerError> {
    match operator {
        BinaryOperator::Less => Ok(CoreComparisonOp::Less),
        BinaryOperator::LessEqual => Ok(CoreComparisonOp::LessEqual),
        BinaryOperator::Greater => Ok(CoreComparisonOp::Greater),
        BinaryOperator::GreaterEqual => Ok(CoreComparisonOp::GreaterEqual),
        _ => Err(lower_error(format!(
            "operator `{operator}` is not a comparison"
        ))),
    }
}

fn core_type_name(ty: CoreType) -> &'static str {
    match ty {
        CoreType::I32 => "i32",
        CoreType::F32 => "f32",
        CoreType::Bool => "bool",
    }
}

fn literal_type(value: &CoreLiteralValue) -> CoreType {
    match value {
        CoreLiteralValue::I32(_) => CoreType::I32,
        CoreLiteralValue::F32Bits(_) => CoreType::F32,
        CoreLiteralValue::Bool(_) => CoreType::Bool,
    }
}

fn placeholder_literal(ty: CoreType) -> CoreLiteralValue {
    match ty {
        CoreType::I32 => CoreLiteralValue::I32(0),
        CoreType::F32 => CoreLiteralValue::F32Bits(0),
        CoreType::Bool => CoreLiteralValue::Bool(false),
    }
}

fn evaluate_deterministic_expression(
    expression: &Expression,
    local_by_name: &HashMap<Identifier, LocalId>,
    locals: &HashMap<LocalId, CoreLiteralValue>,
) -> Result<DeterministicEvaluation, CoreLowerError> {
    match expression {
        Expression::Integer(integer) => i32::try_from(integer.value)
            .map(|value| DeterministicEvaluation::Value(CoreLiteralValue::I32(value)))
            .map_err(|_| lower_error("integer literal does not fit i32")),
        Expression::Float { text, .. } => text
            .parse::<f32>()
            .map(|value| {
                DeterministicEvaluation::Value(CoreLiteralValue::F32Bits(
                    crate::scalar_v2::canonicalize_nan(value.to_bits()),
                ))
            })
            .map_err(|_| lower_error(format!("invalid f32 literal `{text}`"))),
        Expression::Bool { value, .. } => Ok(DeterministicEvaluation::Value(
            CoreLiteralValue::Bool(*value),
        )),
        Expression::Parenthesized { expression, .. } => {
            evaluate_deterministic_expression(expression, local_by_name, locals)
        }
        Expression::Identifier { name, .. } => {
            let local = local_by_name
                .get(name)
                .copied()
                .ok_or_else(|| lower_error(format!("unknown local `{name}`")))?;
            locals
                .get(&local)
                .cloned()
                .map(DeterministicEvaluation::Value)
                .ok_or_else(|| lower_error(format!("local `{name}` has no deterministic value")))
        }
        Expression::FieldAccess { .. } => {
            Err(lower_error("field access is not lowerable in startup"))
        }
        Expression::Unary(unary) => {
            if unary.operator == UnaryOperator::Negate {
                if let Expression::Integer(integer) = unparenthesized_expression(&unary.operand) {
                    if integer.value == i32::MAX as u64 + 1 {
                        return Ok(DeterministicEvaluation::Value(CoreLiteralValue::I32(
                            i32::MIN,
                        )));
                    }
                }
            }
            match evaluate_deterministic_expression(&unary.operand, local_by_name, locals)? {
                DeterministicEvaluation::Value(CoreLiteralValue::Bool(value))
                    if unary.operator == UnaryOperator::Not =>
                {
                    Ok(DeterministicEvaluation::Value(CoreLiteralValue::Bool(
                        !value,
                    )))
                }
                DeterministicEvaluation::Value(CoreLiteralValue::I32(value))
                    if unary.operator == UnaryOperator::Negate =>
                {
                    Ok(DeterministicEvaluation::Value(CoreLiteralValue::I32(
                        value.wrapping_neg(),
                    )))
                }
                DeterministicEvaluation::Value(CoreLiteralValue::F32Bits(bits))
                    if unary.operator == UnaryOperator::Negate =>
                {
                    Ok(DeterministicEvaluation::Value(CoreLiteralValue::F32Bits(
                        crate::scalar_v2::f32_negate(bits),
                    )))
                }
                DeterministicEvaluation::Value(CoreLiteralValue::I32(value))
                    if unary.operator == UnaryOperator::BitNot =>
                {
                    Ok(DeterministicEvaluation::Value(CoreLiteralValue::I32(
                        !value,
                    )))
                }
                DeterministicEvaluation::Value(_) => {
                    Err(lower_error("invalid typed deterministic unary expression"))
                }
                DeterministicEvaluation::Trap => Ok(DeterministicEvaluation::Trap),
                DeterministicEvaluation::Unreachable => Ok(DeterministicEvaluation::Unreachable),
            }
        }
        Expression::Binary(binary) => evaluate_deterministic_binary(binary, local_by_name, locals),
    }
}

fn evaluate_deterministic_binary(
    binary: &crate::parser::BinaryExpression,
    local_by_name: &HashMap<Identifier, LocalId>,
    locals: &HashMap<LocalId, CoreLiteralValue>,
) -> Result<DeterministicEvaluation, CoreLowerError> {
    let left = evaluate_deterministic_expression(&binary.left, local_by_name, locals)?;
    let DeterministicEvaluation::Value(left) = left else {
        return Ok(left);
    };
    if binary.operator == BinaryOperator::LogicalAnd && left == CoreLiteralValue::Bool(false) {
        return Ok(DeterministicEvaluation::Value(CoreLiteralValue::Bool(
            false,
        )));
    }
    if binary.operator == BinaryOperator::LogicalOr && left == CoreLiteralValue::Bool(true) {
        return Ok(DeterministicEvaluation::Value(CoreLiteralValue::Bool(true)));
    }
    let right = evaluate_deterministic_expression(&binary.right, local_by_name, locals)?;
    let DeterministicEvaluation::Value(right) = right else {
        return Ok(right);
    };
    match (left, right) {
        (CoreLiteralValue::I32(left), CoreLiteralValue::I32(right)) => {
            evaluate_deterministic_i32(binary.operator, left, right)
        }
        (CoreLiteralValue::F32Bits(left), CoreLiteralValue::F32Bits(right)) => {
            evaluate_deterministic_f32(binary.operator, left, right)
        }
        (CoreLiteralValue::Bool(left), CoreLiteralValue::Bool(right)) => {
            let value = match binary.operator {
                BinaryOperator::Equal => left == right,
                BinaryOperator::NotEqual => left != right,
                BinaryOperator::LogicalAnd => left && right,
                BinaryOperator::LogicalOr => left || right,
                _ => return Err(lower_error("invalid bool deterministic binary expression")),
            };
            Ok(DeterministicEvaluation::Value(CoreLiteralValue::Bool(
                value,
            )))
        }
        _ => Err(lower_error(
            "deterministic expression operand types do not match",
        )),
    }
}

fn evaluate_deterministic_i32(
    operator: BinaryOperator,
    left: i32,
    right: i32,
) -> Result<DeterministicEvaluation, CoreLowerError> {
    let value = match operator {
        BinaryOperator::Add => CoreLiteralValue::I32(left.wrapping_add(right)),
        BinaryOperator::Subtract => CoreLiteralValue::I32(left.wrapping_sub(right)),
        BinaryOperator::Multiply => CoreLiteralValue::I32(left.wrapping_mul(right)),
        BinaryOperator::Divide if right == 0 || (left == i32::MIN && right == -1) => {
            return Ok(DeterministicEvaluation::Trap);
        }
        BinaryOperator::Divide => CoreLiteralValue::I32(left / right),
        BinaryOperator::Remainder if right == 0 || (left == i32::MIN && right == -1) => {
            return Ok(DeterministicEvaluation::Trap);
        }
        BinaryOperator::Remainder => CoreLiteralValue::I32(left % right),
        BinaryOperator::ShiftLeft => CoreLiteralValue::I32(left.wrapping_shl((right as u32) & 31)),
        BinaryOperator::ShiftRight => CoreLiteralValue::I32(left.wrapping_shr((right as u32) & 31)),
        BinaryOperator::BitAnd => CoreLiteralValue::I32(left & right),
        BinaryOperator::BitXor => CoreLiteralValue::I32(left ^ right),
        BinaryOperator::BitOr => CoreLiteralValue::I32(left | right),
        BinaryOperator::Equal => CoreLiteralValue::Bool(left == right),
        BinaryOperator::NotEqual => CoreLiteralValue::Bool(left != right),
        BinaryOperator::Less => CoreLiteralValue::Bool(left < right),
        BinaryOperator::LessEqual => CoreLiteralValue::Bool(left <= right),
        BinaryOperator::Greater => CoreLiteralValue::Bool(left > right),
        BinaryOperator::GreaterEqual => CoreLiteralValue::Bool(left >= right),
        _ => return Err(lower_error("invalid i32 deterministic binary expression")),
    };
    Ok(DeterministicEvaluation::Value(value))
}

fn evaluate_deterministic_f32(
    operator: BinaryOperator,
    left_bits: u32,
    right_bits: u32,
) -> Result<DeterministicEvaluation, CoreLowerError> {
    let value = match operator {
        BinaryOperator::Add => CoreLiteralValue::F32Bits(crate::scalar_v2::f32_binary(
            crate::scalar_v2::F32BinaryOp::Add,
            left_bits,
            right_bits,
        )),
        BinaryOperator::Subtract => CoreLiteralValue::F32Bits(crate::scalar_v2::f32_binary(
            crate::scalar_v2::F32BinaryOp::Subtract,
            left_bits,
            right_bits,
        )),
        BinaryOperator::Multiply => CoreLiteralValue::F32Bits(crate::scalar_v2::f32_binary(
            crate::scalar_v2::F32BinaryOp::Multiply,
            left_bits,
            right_bits,
        )),
        BinaryOperator::Divide => CoreLiteralValue::F32Bits(crate::scalar_v2::f32_binary(
            crate::scalar_v2::F32BinaryOp::Divide,
            left_bits,
            right_bits,
        )),
        BinaryOperator::Equal => CoreLiteralValue::Bool(crate::scalar_v2::f32_compare(
            crate::scalar_v2::ComparisonOp::Equal,
            left_bits,
            right_bits,
        )),
        BinaryOperator::NotEqual => CoreLiteralValue::Bool(crate::scalar_v2::f32_compare(
            crate::scalar_v2::ComparisonOp::NotEqual,
            left_bits,
            right_bits,
        )),
        BinaryOperator::Less => CoreLiteralValue::Bool(crate::scalar_v2::f32_compare(
            crate::scalar_v2::ComparisonOp::Less,
            left_bits,
            right_bits,
        )),
        BinaryOperator::LessEqual => CoreLiteralValue::Bool(crate::scalar_v2::f32_compare(
            crate::scalar_v2::ComparisonOp::LessEqual,
            left_bits,
            right_bits,
        )),
        BinaryOperator::Greater => CoreLiteralValue::Bool(crate::scalar_v2::f32_compare(
            crate::scalar_v2::ComparisonOp::Greater,
            left_bits,
            right_bits,
        )),
        BinaryOperator::GreaterEqual => CoreLiteralValue::Bool(crate::scalar_v2::f32_compare(
            crate::scalar_v2::ComparisonOp::GreaterEqual,
            left_bits,
            right_bits,
        )),
        _ => return Err(lower_error("invalid f32 deterministic binary expression")),
    };
    Ok(DeterministicEvaluation::Value(value))
}

fn lower_error(message: impl Into<String>) -> CoreLowerError {
    CoreLowerError {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        CoreQueryAccess, CoreQueryLoopBinding, CoreQueryTerm, CoreSchedule, CoreScheduleItem,
        CoreSourceSubject, CoreSpawnFieldValue, CoreSystemBinaryOp, CoreSystemExpression,
        CoreSystemParam, CoreSystemParamKind, CoreSystemPlace, CoreSystemStatement,
    };
    use crate::lexer;
    use crate::parser;

    #[test]
    fn lowers_math_ast_to_core() {
        let source = include_str!("../../../examples/math.arc");
        let tokens = lexer::lex(source).expect("math.arc lexes");
        let ast = parser::parse_program(&tokens).expect("math.arc parses");
        let mut actual = lower_program_to_core(&ast).expect("math.arc lowers to Core");
        actual.source_map = CoreSourceMap::default();

        let expected = CoreProgram {
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
                        CoreInstruction::I32Const {
                            result: ValueId(1),
                            value: 2,
                        },
                        CoreInstruction::I32Binary {
                            result: ValueId(2),
                            op: CoreBinaryOp::Add,
                            left: ValueId(0),
                            right: ValueId(1),
                        },
                        CoreInstruction::LocalStore {
                            local: LocalId(0),
                            value: ValueId(2),
                        },
                        CoreInstruction::LocalLoad {
                            result: ValueId(3),
                            local: LocalId(0),
                        },
                    ],
                    terminator: CoreTerminator::Exit { value: ValueId(3) },
                }],
            }],
            source_map: CoreSourceMap::default(),
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn lowers_bool_short_circuit_and_reassignment_to_typed_core() {
        let source = "world Main startup {
            let mut ready: bool = false
            ready = true && !false
            exit 47
        }";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let ast = parser::parse_program(&tokens).expect("fixture parses");
        let core = lower_program_to_core(&ast).expect("fixture lowers");
        let startup = &core.functions[0];

        assert_eq!(startup.locals[0].ty, CoreType::Bool);
        assert_eq!(
            startup
                .blocks
                .iter()
                .flat_map(|block| block.instructions.iter())
                .filter(|instruction| {
                    matches!(
                        instruction,
                        CoreInstruction::LocalStore {
                            local: LocalId(0),
                            ..
                        }
                    )
                })
                .count(),
            2
        );
        assert!(
            startup.blocks.len() > 1,
            "short-circuiting requires control flow"
        );
        crate::core_verify::verify_executable_core(core)
            .expect("short-circuit CFG passes executable Core verification");
    }

    #[test]
    fn lowers_startup_add_assign_to_typed_load_add_store_core() {
        let source = "world StartupAdd
resource Result { integer: i32 scalar: f32 }
startup {
  let mut integer: i32 = 2147483647
  integer += 2
  let mut scalar: f32 = 0.5
  scalar += 0.25
  resource Result { integer: integer, scalar: scalar }
  exit integer
}";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let ast = parser::parse_program(&tokens).expect("fixture parses");
        crate::checker::check_program(&ast).expect("fixture checks");
        let core = lower_program_to_core(&ast).expect("fixture lowers");
        let startup = &core.functions[0];
        let instructions = startup
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .collect::<Vec<_>>();

        let (integer_result, integer_left) = instructions
            .iter()
            .find_map(|instruction| match instruction {
                CoreInstruction::I32Binary {
                    result,
                    op: CoreBinaryOp::Add,
                    left,
                    ..
                } => Some((*result, *left)),
                _ => None,
            })
            .expect("startup i32 add-assign emits typed addition");
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            CoreInstruction::LocalLoad {
                result,
                local: LocalId(0)
            } if *result == integer_left
        )));
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            CoreInstruction::LocalStore {
                local: LocalId(0),
                value
            } if *value == integer_result
        )));

        let (scalar_result, scalar_left) = instructions
            .iter()
            .find_map(|instruction| match instruction {
                CoreInstruction::F32Binary {
                    result,
                    op: CoreBinaryOp::Add,
                    left,
                    ..
                } => Some((*result, *left)),
                _ => None,
            })
            .expect("startup f32 add-assign emits typed addition");
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            CoreInstruction::LocalLoad {
                result,
                local: LocalId(1)
            } if *result == scalar_left
        )));
        assert!(instructions.iter().any(|instruction| matches!(
            instruction,
            CoreInstruction::LocalStore {
                local: LocalId(1),
                value
            } if *value == scalar_result
        )));
        crate::core_verify::verify_executable_core(core)
            .expect("startup add-assign Core passes executable verification");
    }

    #[test]
    fn lowering_counters_fail_at_the_u64_core_id_boundary() {
        let tokens = lexer::lex("world Main startup { exit 0 }").expect("fixture lexes");
        let program = parser::parse_program(&tokens).expect("fixture parses");
        let canonical_names = CoreCanonicalNames::new(&program).unwrap();

        let mut values = StartupLowerer::new(&program, &canonical_names);
        values.next_value = u64::MAX;
        assert_eq!(
            values
                .allocate_value()
                .expect_err("value id overflow fails")
                .message,
            "Core value id space is exhausted"
        );

        let mut locals = StartupLowerer::new(&program, &canonical_names);
        locals.next_local = u64::MAX;
        assert_eq!(
            locals
                .allocate_local("overflow".into(), CoreType::I32)
                .expect_err("local id overflow fails")
                .message,
            "Core local id space is exhausted"
        );

        let mut blocks = StartupLowerer::new(&program, &canonical_names);
        blocks.next_block = u64::MAX;
        assert_eq!(
            blocks
                .allocate_block()
                .expect_err("block id overflow fails")
                .message,
            "Core block id space is exhausted"
        );
    }

    #[test]
    fn lowers_system_bool_locals_and_assignment_to_verified_core() {
        let source = "world Main
system Toggle() { let mut ready: bool = true ready = !ready && false || true }
startup { exit 0 }";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let ast = parser::parse_program(&tokens).expect("fixture parses");
        crate::checker::check_program(&ast).expect("fixture checks");
        let core = lower_program_to_core(&ast).expect("fixture lowers");

        assert!(matches!(
            core.systems[0].body.statements.as_slice(),
            [
                CoreSystemStatement::Let { .. },
                CoreSystemStatement::Assign { .. }
            ]
        ));
        crate::core_verify::verify_executable_core(core).expect("system bool local Core verifies");
    }

    #[test]
    fn lowers_spawn_position_to_core() {
        let source = include_str!("../../../examples/spawn_position.arc");
        let tokens = lexer::lex(source).expect("spawn_position.arc lexes");
        let ast = parser::parse_program(&tokens).expect("spawn_position.arc parses");
        let actual = lower_program_to_core(&ast).expect("spawn_position.arc lowers to Core");
        crate::core_verify::verify_executable_core(actual.clone())
            .expect("spawn_position Core verifies");
        assert_eq!(actual.components[0].id, 0);
        assert_eq!(actual.components[0].name, "Demo.Position");
        let spawn = actual.functions[0].blocks[0]
            .instructions
            .iter()
            .find_map(|instruction| match instruction {
                CoreInstruction::Spawn { components } => Some(&components[0]),
                _ => None,
            })
            .expect("Position spawn exists");
        assert_eq!(spawn.component_id, 0);
        assert_eq!(
            spawn
                .fields
                .iter()
                .map(|field| (field.name.as_str(), field.value.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("x", CoreSpawnFieldValue::F32Bits(1.0_f32.to_bits())),
                ("y", CoreSpawnFieldValue::F32Bits(2.0_f32.to_bits())),
            ]
        );
    }

    #[test]
    fn lowers_typed_i32_spawn_values_to_core() {
        let source = include_str!("../../../examples/arena_recovery.arc");
        let tokens = lexer::lex(source).expect("arena_recovery.arc lexes");
        let ast = parser::parse_program(&tokens).expect("arena_recovery.arc parses");
        let actual = lower_program_to_core(&ast).expect("arena_recovery.arc lowers to Core");

        let faction_values = actual.functions[0].blocks[0]
            .instructions
            .iter()
            .filter_map(|instruction| match instruction {
                CoreInstruction::Spawn { components } => components
                    .iter()
                    .find(|component| component.name == "Arena.Faction")
                    .and_then(|component| component.fields.first())
                    .map(|field| field.value.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            faction_values,
            vec![
                CoreSpawnFieldValue::I32(1),
                CoreSpawnFieldValue::I32(2),
                CoreSpawnFieldValue::I32(3),
                CoreSpawnFieldValue::I32(4),
                CoreSpawnFieldValue::I32(5),
            ]
        );
        let formatted = crate::core_format::format_core_program(&actual);
        assert!(formatted.contains("field id = i32 1"));
        assert!(formatted.contains("field id = i32 5"));
        assert!(formatted.contains("field current = f32.bits 0x41200000"));
    }

    #[test]
    fn lowers_tags_and_zero_data_spawns_to_verifiable_core() {
        let source = "world Demo
tag Enemy
component Empty {}
system Find(q: query[Enemy, Empty]) { for (_, _) in q {} }
startup {
  spawn {}
  spawn { Enemy {} Empty {} }
  exit 0
}";
        let tokens = lexer::lex(source).expect("zero-data fixture lexes");
        let program = parser::parse_program(&tokens).expect("zero-data fixture parses");
        crate::checker::check_program(&program).expect("zero-data fixture checks");
        let core = lower_program_to_core(&program).expect("zero-data fixture lowers");
        crate::core_verify::verify_core_program(&core).expect("zero-data Core verifies");

        assert_eq!(core.components.len(), 2);
        assert_eq!(
            core.components
                .iter()
                .find(|component| component.name == "Demo.Enemy")
                .unwrap()
                .kind,
            CoreComponentKind::Tag
        );
        assert!(core
            .components
            .iter()
            .all(|component| component.fields.is_empty()));
        assert!(matches!(
            core.functions[0].blocks[0].instructions.as_slice(),
            [
                CoreInstruction::Spawn { components: empty },
                CoreInstruction::Spawn { components: tagged },
                CoreInstruction::I32Const { .. }
            ] if empty.is_empty()
                && tagged.len() == 2
                && tagged.iter().all(|component| component.fields.is_empty())
        ));
        let formatted = crate::core_format::format_core_program(&core);
        assert!(formatted.contains("tag Demo.Enemy"));
    }

    #[test]
    fn core_verifier_rejects_mutable_tag_terms_independently_of_source_checking() {
        let source = "world Demo
tag Enemy
system Find(q: query[Enemy]) { for (_) in q {} }
startup { exit 0 }";
        let tokens = lexer::lex(source).expect("tag query lexes");
        let program = parser::parse_program(&tokens).expect("tag query parses");
        crate::checker::check_program(&program).expect("read-only tag query checks");
        let mut core = lower_program_to_core(&program).expect("tag query lowers");
        let CoreSystemParamKind::Query { terms } = &mut core.systems[0].params[0].kind else {
            panic!("expected query parameter");
        };
        terms[0].access = CoreQueryAccess::Mut;
        let error = crate::core_verify::verify_core_program(&core)
            .expect_err("Core verification must reject mutable tags");
        assert!(error.message.contains("mutable Core tag"));
    }

    #[test]
    fn lowers_move_system_to_core_metadata() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let ast = parser::parse_program(&tokens).expect("move_system.arc parses");
        let actual = lower_program_to_core(&ast).expect("move_system.arc lowers to Core");
        crate::core_verify::verify_executable_core(actual.clone())
            .expect("move_system Core verifies");
        assert_eq!(
            actual
                .components
                .iter()
                .map(|component| (component.id, component.name.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, "Demo.Position"), (1, "Demo.Velocity")]
        );
        assert_eq!(actual.resources[0].id, 2);
        assert_eq!(actual.resources[0].name, "Demo.Time");
        assert_eq!(actual.systems[0].id, 0);
        assert_eq!(actual.systems[0].name, "Move");
    }

    #[test]
    fn lowers_schedule_to_core_metadata() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let ast = parser::parse_program(&tokens).expect("move_system.arc parses");
        let actual = lower_program_to_core(&ast).expect("move_system.arc lowers to Core");
        assert_eq!(
            actual.schedules,
            vec![CoreSchedule {
                id: 0,
                name: "Main".into(),
                items: vec![CoreScheduleItem::Run {
                    system_id: 0,
                    system_name: "Demo.Move".into(),
                }],
            }]
        );
    }

    #[test]
    fn lowers_query_loop_skeleton_to_core_body() {
        let source = r#"
world Demo

component Position {
    x: f32
    y: f32
}

component Velocity {
    x: f32
    y: f32
}

resource Time {
    delta: f32
}

system Move(
    time: read Time,
    movers: query[mut Position, Velocity]
) {
    for (pos, vel) in movers {
    }
}

startup {
    exit 0
}
"#;
        let tokens = lexer::lex(source).expect("query-loop fixture lexes");
        let ast = parser::parse_program(&tokens).expect("query-loop fixture parses");
        let actual = lower_program_to_core(&ast).expect("query-loop fixture lowers to Core");

        assert_eq!(actual.systems.len(), 1);
        let system = &actual.systems[0];
        assert_eq!(system.name, "Move");
        assert_eq!(
            system.params,
            vec![
                CoreSystemParam {
                    name: "time".into(),
                    kind: CoreSystemParamKind::ReadResource {
                        resource_id: 2,
                        name: "Demo.Time".into(),
                    },
                },
                CoreSystemParam {
                    name: "movers".into(),
                    kind: CoreSystemParamKind::Query {
                        terms: vec![
                            CoreQueryTerm {
                                access: CoreQueryAccess::Mut,
                                component_id: 0,
                                name: "Demo.Position".into(),
                            },
                            CoreQueryTerm {
                                access: CoreQueryAccess::Read,
                                component_id: 1,
                                name: "Demo.Velocity".into(),
                            },
                        ],
                    },
                },
            ]
        );

        assert_eq!(system.body.statements.len(), 1);
        let CoreSystemStatement::QueryLoop(query_loop) = &system.body.statements[0] else {
            panic!("expected query loop skeleton");
        };
        assert_eq!(query_loop.query_param, "movers");
        assert_eq!(
            query_loop.bindings,
            vec![
                CoreQueryLoopBinding {
                    name: "pos".into(),
                    component_id: 0,
                    component_name: "Demo.Position".into(),
                    access: CoreQueryAccess::Mut,
                },
                CoreQueryLoopBinding {
                    name: "vel".into(),
                    component_id: 1,
                    component_name: "Demo.Velocity".into(),
                    access: CoreQueryAccess::Read,
                },
            ]
        );
        assert!(query_loop.body.is_empty());

        let startup = &actual.functions[0].blocks[0];
        assert_eq!(
            startup.instructions,
            vec![CoreInstruction::I32Const {
                result: ValueId(0),
                value: 0,
            }]
        );
        assert_eq!(
            startup.terminator,
            CoreTerminator::Exit { value: ValueId(0) }
        );
    }

    #[test]
    fn lowers_query_loop_field_expressions_to_core_body() {
        let source = r#"
world Demo

component Position {
    x: f32
    y: f32
}

component Velocity {
    x: f32
    y: f32
}

resource Time {
    delta: f32
}

system Move(
    time: read Time,
    movers: query[mut Position, Velocity]
) {
    for (pos, vel) in movers {
        vel.x * time.delta
        vel.y * time.delta
    }
}

startup {
    exit 0
}
"#;
        let tokens = lexer::lex(source).expect("query-loop expression fixture lexes");
        let ast = parser::parse_program(&tokens).expect("query-loop expression fixture parses");
        let actual =
            lower_program_to_core(&ast).expect("query-loop expression fixture lowers to Core");

        assert_eq!(actual.systems.len(), 1);
        let system = &actual.systems[0];
        assert_eq!(system.body.statements.len(), 1);
        let CoreSystemStatement::QueryLoop(query_loop) = &system.body.statements[0] else {
            panic!("expected query loop");
        };

        assert_eq!(query_loop.query_param, "movers");
        assert_eq!(
            query_loop.bindings,
            vec![
                CoreQueryLoopBinding {
                    name: "pos".into(),
                    component_id: 0,
                    component_name: "Demo.Position".into(),
                    access: CoreQueryAccess::Mut,
                },
                CoreQueryLoopBinding {
                    name: "vel".into(),
                    component_id: 1,
                    component_name: "Demo.Velocity".into(),
                    access: CoreQueryAccess::Read,
                },
            ]
        );
        assert_eq!(
            query_loop.body,
            vec![
                CoreSystemStatement::Expression(move_velocity_delta_expression("x")),
                CoreSystemStatement::Expression(move_velocity_delta_expression("y")),
            ]
        );

        let startup = &actual.functions[0].blocks[0];
        assert_eq!(
            startup.instructions,
            vec![CoreInstruction::I32Const {
                result: ValueId(0),
                value: 0,
            }]
        );
        assert_eq!(
            startup.terminator,
            CoreTerminator::Exit { value: ValueId(0) }
        );
    }

    #[test]
    fn lowers_query_loop_add_assign_to_core_body() {
        let source = r#"
world Demo

component Position {
    x: f32
    y: f32
}

component Velocity {
    x: f32
    y: f32
}

resource Time {
    delta: f32
}

system Move(
    time: read Time,
    movers: query[mut Position, Velocity]
) {
    for (pos, vel) in movers {
        pos.x += vel.x * time.delta
        pos.y += vel.y * time.delta
    }
}

startup {
    exit 0
}
"#;
        let tokens = lexer::lex(source).expect("query-loop update fixture lexes");
        let ast = parser::parse_program(&tokens).expect("query-loop update fixture parses");
        let actual = lower_program_to_core(&ast).expect("query-loop update fixture lowers to Core");

        assert_eq!(actual.systems.len(), 1);
        let system = &actual.systems[0];
        assert_eq!(system.body.statements.len(), 1);
        let CoreSystemStatement::QueryLoop(query_loop) = &system.body.statements[0] else {
            panic!("expected query loop");
        };

        assert_eq!(query_loop.query_param, "movers");
        assert_eq!(
            query_loop.bindings,
            vec![
                CoreQueryLoopBinding {
                    name: "pos".into(),
                    component_id: 0,
                    component_name: "Demo.Position".into(),
                    access: CoreQueryAccess::Mut,
                },
                CoreQueryLoopBinding {
                    name: "vel".into(),
                    component_id: 1,
                    component_name: "Demo.Velocity".into(),
                    access: CoreQueryAccess::Read,
                },
            ]
        );
        assert_eq!(
            query_loop.body,
            vec![
                move_position_add_assign("x", "x"),
                move_position_add_assign("y", "y"),
            ]
        );

        let startup = &actual.functions[0].blocks[0];
        assert_eq!(
            startup.instructions,
            vec![CoreInstruction::I32Const {
                result: ValueId(0),
                value: 0,
            }]
        );
        assert_eq!(
            startup.terminator,
            CoreTerminator::Exit { value: ValueId(0) }
        );
    }

    #[test]
    fn lowers_precedence_and_parentheses_to_verified_operation_order() {
        let source = "world Main startup {
            let value: i32 = (1 + 2) * 3 - 4 * 2
            exit value
        }";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let ast = parser::parse_program(&tokens).expect("fixture parses");
        let core = lower_program_to_core(&ast).expect("fixture lowers");
        let entry = &core.functions[0].blocks[0];

        assert_eq!(
            entry.instructions,
            vec![
                CoreInstruction::I32Const {
                    result: ValueId(0),
                    value: 1,
                },
                CoreInstruction::I32Const {
                    result: ValueId(1),
                    value: 2,
                },
                CoreInstruction::I32Binary {
                    result: ValueId(2),
                    op: CoreBinaryOp::Add,
                    left: ValueId(0),
                    right: ValueId(1),
                },
                CoreInstruction::I32Const {
                    result: ValueId(3),
                    value: 3,
                },
                CoreInstruction::I32Binary {
                    result: ValueId(4),
                    op: CoreBinaryOp::Multiply,
                    left: ValueId(2),
                    right: ValueId(3),
                },
                CoreInstruction::I32Const {
                    result: ValueId(5),
                    value: 4,
                },
                CoreInstruction::I32Const {
                    result: ValueId(6),
                    value: 2,
                },
                CoreInstruction::I32Binary {
                    result: ValueId(7),
                    op: CoreBinaryOp::Multiply,
                    left: ValueId(5),
                    right: ValueId(6),
                },
                CoreInstruction::I32Binary {
                    result: ValueId(8),
                    op: CoreBinaryOp::Subtract,
                    left: ValueId(4),
                    right: ValueId(7),
                },
                CoreInstruction::LocalStore {
                    local: LocalId(0),
                    value: ValueId(8),
                },
                CoreInstruction::LocalLoad {
                    result: ValueId(9),
                    local: LocalId(0),
                },
            ]
        );
        assert_eq!(entry.terminator, CoreTerminator::Exit { value: ValueId(9) });
        crate::core_verify::verify_executable_core(core)
            .expect("precedence-aware Core should verify");
    }

    #[test]
    fn rich_m26_payloads_use_locals_source_evaluation_and_declaration_layout_order() {
        let source = include_str!("../../../examples/m26_closure.arc");
        let tokens = lexer::lex(source).expect("M26 closure fixture lexes");
        let ast = parser::parse_program(&tokens).expect("M26 closure fixture parses");
        crate::checker::check_program(&ast).expect("M26 closure fixture checks");
        let core = lower_program_to_core(&ast).expect("M26 closure fixture lowers");
        crate::core_verify::verify_executable_core(core.clone())
            .expect("M26 closure Core verifies");

        assert_eq!(
            core.components
                .iter()
                .map(|component| component.id)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
        assert_eq!(
            core.resources
                .iter()
                .map(|resource| resource.id)
                .collect::<Vec<_>>(),
            vec![5, 6, 7, 8, 9]
        );
        assert_eq!(
            core.systems
                .iter()
                .map(|system| system.id)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(
            core.schedules
                .iter()
                .map(|schedule| schedule.id)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );

        let startup = &core.functions[0];
        let entry = startup
            .blocks
            .iter()
            .find(|block| block.id == startup.entry)
            .expect("startup entry exists");
        let config_index = entry
            .instructions
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    CoreInstruction::InitializeResource { resource_name, .. }
                        if resource_name == "M26Closure.Config"
                )
            })
            .expect("Config initialization exists");
        let CoreInstruction::InitializeResource { fields, .. } = &entry.instructions[config_index]
        else {
            unreachable!();
        };
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            vec!["step", "scale", "enabled"]
        );
        assert_eq!(
            fields
                .iter()
                .map(|field| field.value.clone())
                .collect::<Vec<_>>(),
            vec![
                CoreSpawnFieldValue::I32(2),
                CoreSpawnFieldValue::F32Bits(1.0_f32.to_bits()),
                CoreSpawnFieldValue::Bool(true),
            ]
        );

        let root_instruction_index = |text: &str| {
            (0..config_index)
                .find(|instruction_index| {
                    let span = core
                        .source_map
                        .span(&CoreSourceSubject::StartupInstruction {
                            block: entry.id,
                            instruction_index: u64::try_from(*instruction_index)
                                .expect("test instruction index fits u64"),
                        })
                        .expect("startup scalar instruction is mapped");
                    span_text(source, span) == text
                })
                .expect("payload root expression has an instruction")
        };
        let enabled = root_instruction_index("!false");
        let scale = root_instruction_index("0.25 + 0.75");
        let step = root_instruction_index("base - 2");
        assert!(enabled < scale && scale < step);

        let first_spawn = entry
            .instructions
            .iter()
            .find_map(|instruction| match instruction {
                CoreInstruction::Spawn { components } if !components.is_empty() => Some(components),
                _ => None,
            })
            .expect("first populated spawn exists");
        let position = first_spawn
            .iter()
            .find(|component| component.name == "M26Closure.Position")
            .expect("Position payload exists");
        assert_eq!(
            position
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            vec!["x", "weight", "active"]
        );
        assert_eq!(
            position
                .fields
                .iter()
                .map(|field| field.value.clone())
                .collect::<Vec<_>>(),
            vec![
                CoreSpawnFieldValue::I32(5),
                CoreSpawnFieldValue::F32Bits(1.5_f32.to_bits()),
                CoreSpawnFieldValue::Bool(true),
            ]
        );
    }

    #[test]
    fn lowers_add_assign_for_mutable_local_resource_and_query_component_places() {
        let source = "world Main
component Position { value: i32 }
resource Counter { value: i32 }
system Update(counter: mut Counter, positions: query[mut Position]) {
  let mut local: i32 = 1
  local += 2
  counter.value += local
  for (position) in positions { position.value += counter.value }
}
startup {
  resource Counter { value: 0 }
  spawn { Position { value: 0 } }
  exit 0
}";
        let tokens = lexer::lex(source).expect("+= fixture lexes");
        let ast = parser::parse_program(&tokens).expect("+= fixture parses");
        crate::checker::check_program(&ast).expect("+= fixture checks");
        let core = lower_program_to_core(&ast).expect("+= fixture lowers");
        crate::core_verify::verify_executable_core(core.clone()).expect("+= Core verifies");

        let statements = &core.systems[0].body.statements;
        assert!(matches!(
            &statements[1],
            CoreSystemStatement::AddAssign {
                target: CoreSystemPlace::Local {
                    name,
                    ty: CoreType::I32,
                    mutable: true,
                },
                ..
            } if name == "local"
        ));
        assert!(matches!(
            &statements[2],
            CoreSystemStatement::AddAssign {
                target: CoreSystemPlace::ResourceField {
                    param,
                    resource_name,
                    field_name,
                    ..
                },
                ..
            } if param == "counter"
                && resource_name == "Main.Counter"
                && field_name == "value"
        ));
        let CoreSystemStatement::QueryLoop(query_loop) = &statements[3] else {
            panic!("fourth statement must remain a query loop");
        };
        assert!(matches!(
            &query_loop.body[0],
            CoreSystemStatement::AddAssign {
                target: CoreSystemPlace::ComponentField {
                    binding,
                    component_name,
                    field_name,
                    ..
                },
                ..
            } if binding == "position"
                && component_name == "Main.Position"
                && field_name == "value"
        ));
    }

    #[test]
    fn trapping_payload_keeps_prior_effect_and_uses_unreachable_typed_placeholder() {
        let source = "world Trap
component Item { first: i32 second: i32 }
resource Seen { value: i32 }
startup {
  resource Seen { value: 7 }
  let mut zero: i32 = 0
  spawn { Item { second: 9, first: 1 / zero } }
  exit 0
}";
        let tokens = lexer::lex(source).expect("trap payload fixture lexes");
        let ast = parser::parse_program(&tokens).expect("trap payload fixture parses");
        crate::checker::check_program(&ast).expect("trap payload fixture checks");
        let core = lower_program_to_core(&ast).expect("trap payload fixture lowers");
        crate::core_verify::verify_executable_core(core.clone())
            .expect("trapping payload remains valid executable Core");

        let entry = &core.functions[0].blocks[0];
        let resource_index = entry
            .instructions
            .iter()
            .position(|instruction| {
                matches!(instruction, CoreInstruction::InitializeResource { .. })
            })
            .expect("prior committed resource effect exists");
        let trap_index = entry
            .instructions
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    CoreInstruction::I32Binary {
                        op: CoreBinaryOp::Divide,
                        ..
                    }
                )
            })
            .expect("trapping divide instruction exists");
        let spawn_index = entry
            .instructions
            .iter()
            .position(|instruction| matches!(instruction, CoreInstruction::Spawn { .. }))
            .expect("unreachable spawn effect exists");
        assert!(resource_index < trap_index && trap_index < spawn_index);

        let CoreInstruction::Spawn { components } = &entry.instructions[spawn_index] else {
            unreachable!();
        };
        assert_eq!(
            components[0]
                .fields
                .iter()
                .map(|field| (field.name.as_str(), field.value.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("first", CoreSpawnFieldValue::I32(0)),
                ("second", CoreSpawnFieldValue::I32(9)),
            ]
        );
        let trap_span = core
            .source_map
            .span(&CoreSourceSubject::StartupInstruction {
                block: entry.id,
                instruction_index: u64::try_from(trap_index)
                    .expect("test instruction index fits u64"),
            })
            .expect("trapping instruction source span exists");
        assert_eq!(span_text(source, trap_span), "1 / zero");
    }

    #[test]
    fn payload_folding_uses_m26_subnormal_signed_zero_and_nan_rules() {
        let source = "world FloatBits
resource Values { tiny: f32 negative_zero: f32 nan: f32 }
startup {
  resource Values {
    nan: 0.0 / 0.0,
    tiny: 0.000000000000000000000000000000000000000000001 * 1.0,
    negative_zero: -0.0
  }
  exit 0
}";
        let tokens = lexer::lex(source).expect("f32 payload fixture lexes");
        let ast = parser::parse_program(&tokens).expect("f32 payload fixture parses");
        crate::checker::check_program(&ast).expect("f32 payload fixture checks");
        let core = lower_program_to_core(&ast).expect("f32 payload fixture lowers");
        crate::core_verify::verify_executable_core(core.clone())
            .expect("f32 payload fixture verifies");
        let fields = core.functions[0].blocks[0]
            .instructions
            .iter()
            .find_map(|instruction| match instruction {
                CoreInstruction::InitializeResource { fields, .. } => Some(fields),
                _ => None,
            })
            .expect("resource payload exists");
        assert_eq!(
            fields
                .iter()
                .map(|field| (field.name.as_str(), field.value.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("tiny", CoreSpawnFieldValue::F32Bits(1)),
                (
                    "negative_zero",
                    CoreSpawnFieldValue::F32Bits((-0.0_f32).to_bits()),
                ),
                (
                    "nan",
                    CoreSpawnFieldValue::F32Bits(crate::scalar_v2::CANONICAL_NAN_BITS),
                ),
            ]
        );
    }

    #[test]
    fn every_integer_payload_trap_lowers_to_core_instead_of_a_build_error() {
        for expression in ["1 / 0", "1 % 0", "-2147483648 / -1", "-2147483648 % -1"] {
            let source = format!(
                "world Trap component Item {{ value: i32 }} startup {{ spawn {{ Item {{ value: {expression} }} }} exit 0 }}"
            );
            let tokens = lexer::lex(&source).expect("integer trap fixture lexes");
            let ast = parser::parse_program(&tokens).expect("integer trap fixture parses");
            crate::checker::check_program(&ast).expect("integer trap fixture checks");
            let core = lower_program_to_core(&ast).expect("integer trap fixture lowers");
            crate::core_verify::verify_executable_core(core.clone())
                .expect("integer trap fixture verifies");
            let field = core.functions[0].blocks[0]
                .instructions
                .iter()
                .find_map(|instruction| match instruction {
                    CoreInstruction::Spawn { components } => Some(&components[0].fields[0]),
                    _ => None,
                })
                .expect("spawn payload exists");
            assert_eq!(field.value, CoreSpawnFieldValue::I32(0));
        }
    }

    #[test]
    fn folded_i32_min_literal_keeps_system_expression_source_map_canonical() {
        let source = "world MinFold
resource Value { current: i32 }
system NegateTwice(value: mut Value) {
    value.current = -(-2147483648)
}
schedule Main { run NegateTwice }
startup {
    resource Value { current: 0 }
    run Main
    exit 0
}";
        let tokens = lexer::lex(source).expect("minimum-literal fixture lexes");
        let ast = parser::parse_program(&tokens).expect("minimum-literal fixture parses");
        crate::checker::check_program(&ast).expect("minimum-literal fixture checks");
        let core = lower_program_to_core(&ast).expect("minimum-literal fixture lowers");

        crate::core_verify::verify_executable_core(core)
            .expect("folded minimum literal retains an exact Core source map");
    }

    #[test]
    fn source_map_is_complete_exact_and_rejected_when_tampered() {
        let source = r#"world Demo
component Position { x: i32 }
tag Enemy
resource Counter { value: i32 }
system Step(counter: mut Counter, q: query[mut Position, !Enemy]) {
    let mut n: i32 = 1
    { n = (n << 2) | 3 }
    if n > 0 {
        for (p) in q {
            p.x += n / 1
            counter.value = counter.value + 1
        }
    } else {
        n = 0
    }
    while false { n = n + 1 }
}
schedule Main { run Step }
startup {
    resource Counter { value: 0 }
    spawn { Position { x: 1 + 2 } }
    run Main
    exit 47
}"#;
        let tokens = lexer::lex(source).expect("source-map fixture lexes");
        let ast = parser::parse_program(&tokens).expect("source-map fixture parses");
        crate::checker::check_program(&ast).expect("source-map fixture checks");
        let core = lower_program_to_core(&ast).expect("source-map fixture lowers");
        crate::core_verify::verify_executable_core(core.clone())
            .expect("complete source map verifies");

        let system_id = core.systems[0].id;
        let division = core
            .source_map
            .entries
            .iter()
            .find(|entry| {
                matches!(
                    entry.subject,
                    CoreSourceSubject::SystemExpression {
                        system_id: id,
                        ..
                    } if id == system_id
                ) && span_text(source, entry.span) == "n / 1"
            })
            .expect("division expression has its exact source span");
        assert!(matches!(
            division.subject,
            CoreSourceSubject::SystemExpression { .. }
        ));

        let exclusion_span = core
            .source_map
            .span(&CoreSourceSubject::QueryTerm {
                system_id,
                param_index: 1,
                term_index: 1,
            })
            .expect("exclusion term is mapped");
        assert_eq!(span_text(source, exclusion_span), "!Enemy");

        let exit_span = core
            .source_map
            .span(&CoreSourceSubject::StartupTerminator { block: BlockId(0) })
            .expect("startup exit terminator is mapped");
        assert_eq!(span_text(source, exit_span), "exit 47");
        assert!(core.source_map.entries.iter().any(|entry| {
            matches!(entry.subject, CoreSourceSubject::StartupInstruction { .. })
                && span_text(source, entry.span) == "run Main"
        }));

        let mut missing = core.clone();
        missing
            .source_map
            .entries
            .retain(|entry| entry.subject != CoreSourceSubject::Program);
        let error = crate::core_verify::verify_executable_core(missing)
            .expect_err("missing source-map subjects are rejected");
        assert!(error.message.contains("incomplete or non-canonical"));

        let mut malformed = core;
        let program_entry = malformed
            .source_map
            .entries
            .iter_mut()
            .find(|entry| entry.subject == CoreSourceSubject::Program)
            .expect("program source entry exists");
        program_entry.span.end.byte = program_entry.span.start.byte;
        let error = crate::core_verify::verify_executable_core(malformed)
            .expect_err("byte-empty spans with distinct line/column endpoints are rejected");
        assert!(
            error.message.contains("invalid Core source span"),
            "unexpected verifier error: {}",
            error.message
        );
    }

    #[test]
    fn core_names_share_canonical_storage_and_outlive_the_ast() {
        let source = "world Demo
component Position { x: i32 }
system Move(items: query[mut Position]) {
  for (pos) in items { pos.x += 1 }
}
startup { spawn { Position { x: 0 } } exit 0 }";
        let tokens = lexer::lex(source).expect("fixture lexes");
        let ast = parser::parse_program(&tokens).expect("fixture parses");
        crate::checker::check_program(&ast).expect("fixture checks");
        let ast_field = ast.components[0].fields[0].name.clone();
        let core = lower_program_to_core(&ast).expect("fixture lowers");

        let CoreSystemParamKind::Query { terms } = &core.systems[0].params[0].kind else {
            panic!("Core parameter is a query");
        };
        let CoreSystemStatement::QueryLoop(query_loop) = &core.systems[0].body.statements[0] else {
            panic!("Core body starts with a query loop");
        };
        let CoreSystemStatement::AddAssign { target, .. } = &query_loop.body[0] else {
            panic!("Core query body contains +=");
        };
        let CoreSystemPlace::ComponentField { field_name, .. } = target else {
            panic!("Core += target is a component field");
        };
        let spawn = core.functions[0]
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .find_map(|instruction| match instruction {
                CoreInstruction::Spawn { components } => components.first(),
                _ => None,
            })
            .expect("Core startup contains a spawn");

        assert!(core.components[0].name.shares_storage_with(&terms[0].name));
        assert!(core.components[0]
            .name
            .shares_storage_with(&query_loop.bindings[0].component_name));
        assert!(core.components[0].name.shares_storage_with(&spawn.name));
        assert!(ast_field.shares_storage_with(&core.components[0].fields[0].name));
        assert!(ast_field.shares_storage_with(field_name));
        assert!(ast_field.shares_storage_with(&spawn.fields[0].name));

        drop(ast_field);
        drop(ast);
        drop(tokens);
        assert_eq!(core.components[0].name.as_str(), "Demo.Position");
        assert_eq!(core.components[0].fields[0].name.as_str(), "x");
    }

    fn span_text(source: &str, span: crate::lexer::SourceSpan) -> &str {
        let start = usize::try_from(span.start.byte).expect("test source offset fits usize");
        let end = usize::try_from(span.end.byte).expect("test source offset fits usize");
        &source[start..end]
    }

    fn move_position_add_assign(position_field: &str, velocity_field: &str) -> CoreSystemStatement {
        CoreSystemStatement::AddAssign {
            target: CoreSystemPlace::ComponentField {
                binding: "pos".into(),
                component_id: 0,
                component_name: "Demo.Position".into(),
                field_name: position_field.into(),
            },
            value: move_velocity_delta_expression(velocity_field),
        }
    }

    fn move_velocity_delta_expression(velocity_field: &str) -> CoreSystemExpression {
        CoreSystemExpression::Binary {
            op: CoreSystemBinaryOp::F32Multiply,
            left: Box::new(CoreSystemExpression::ComponentField {
                binding: "vel".into(),
                component_id: 1,
                component_name: "Demo.Velocity".into(),
                field_name: velocity_field.into(),
            }),
            right: Box::new(CoreSystemExpression::ResourceField {
                param: "time".into(),
                resource_id: 2,
                resource_name: "Demo.Time".into(),
                field_name: "delta".into(),
            }),
        }
    }
}
