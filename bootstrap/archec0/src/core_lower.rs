#![allow(dead_code)]

use std::collections::HashMap;

use crate::core::{
    BlockId, CoreBinaryOp, CoreBlock, CoreFunction, CoreInstruction, CoreLocal, CoreProgram,
    CoreQueryAccess, CoreQueryLoop, CoreQueryLoopBinding, CoreQueryTerm, CoreSchedule,
    CoreScheduleItem, CoreSpawnComponent, CoreSpawnField, CoreSpawnFieldValue, CoreSystem,
    CoreSystemBinaryOp, CoreSystemBody, CoreSystemExpression, CoreSystemParam, CoreSystemParamKind,
    CoreSystemStatement, CoreTerminator, CoreType, CoreWorld, LocalId, ValueId,
};
use crate::layout;
use crate::parser::{
    BinaryOperator, ComponentDecl, ComponentLiteralValue, Expression, Program,
    QueryAccess as ParserQueryAccess, ResourceDecl, ScheduleDecl, ScheduleItem,
    SpawnComponentField, SpawnComponentLiteral, SpawnStatement, StartupBlock, Statement,
    SystemBodyStatement, SystemDecl, SystemParam, SystemParamKind, SystemQueryLoopStatement,
};
use crate::runtime;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreLowerError {
    pub message: String,
}

pub fn lower_program_to_core(program: &Program) -> Result<CoreProgram, CoreLowerError> {
    let startup = program
        .startup
        .as_ref()
        .ok_or_else(|| lower_error("expected startup block"))?;
    let systems = lower_systems(program)?;
    let schedules = lower_schedules(program)?;
    let (locals, instructions, terminator) = StartupLowerer::new(program).lower_startup(startup)?;

    Ok(CoreProgram {
        world: CoreWorld {
            name: program.world.name.clone(),
        },
        systems,
        schedules,
        functions: vec![CoreFunction {
            name: "startup".to_string(),
            entry: BlockId(0),
            locals,
            blocks: vec![CoreBlock {
                id: BlockId(0),
                instructions,
                terminator,
            }],
        }],
    })
}

fn lower_schedules(program: &Program) -> Result<Vec<CoreSchedule>, CoreLowerError> {
    program
        .schedules
        .iter()
        .map(|schedule| lower_schedule(program, schedule))
        .collect()
}

fn lower_schedule(
    program: &Program,
    schedule: &ScheduleDecl,
) -> Result<CoreSchedule, CoreLowerError> {
    let items = schedule
        .items
        .iter()
        .map(|item| lower_schedule_item(program, item))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CoreSchedule {
        name: schedule.name.clone(),
        items,
    })
}

fn lower_schedule_item(
    program: &Program,
    item: &ScheduleItem,
) -> Result<CoreScheduleItem, CoreLowerError> {
    match item {
        ScheduleItem::Run { system_name, .. } => {
            resolve_system(&program.systems, system_name)?;
            Ok(CoreScheduleItem::Run {
                system_id: runtime::stable_system_id(&program.world.name, system_name).0,
                system_name: qualified_name(&program.world.name, system_name),
            })
        }
    }
}

fn lower_systems(program: &Program) -> Result<Vec<CoreSystem>, CoreLowerError> {
    program
        .systems
        .iter()
        .map(|system| lower_system(program, system))
        .collect()
}

fn lower_system(program: &Program, system: &SystemDecl) -> Result<CoreSystem, CoreLowerError> {
    let params = system
        .params
        .iter()
        .map(|param| lower_system_param(program, param))
        .collect::<Result<Vec<_>, _>>()?;
    let body = lower_system_body(&params, system)?;

    Ok(CoreSystem {
        name: system.name.clone(),
        params,
        body,
    })
}

fn lower_system_body(
    params: &[CoreSystemParam],
    system: &SystemDecl,
) -> Result<CoreSystemBody, CoreLowerError> {
    let mut statements = Vec::new();

    for statement in &system.body.statements {
        match statement {
            SystemBodyStatement::Expression(_) => {}
            SystemBodyStatement::QueryLoop(query_loop) => {
                statements.push(CoreSystemStatement::QueryLoop(lower_system_query_loop(
                    params, query_loop,
                )?));
            }
        }
    }

    Ok(CoreSystemBody { statements })
}

fn lower_system_query_loop(
    params: &[CoreSystemParam],
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

    if query_loop.bindings.len() != terms.len() {
        return Err(lower_error(format!(
            "query loop binding count {} does not match query term count {}",
            query_loop.bindings.len(),
            terms.len()
        )));
    }

    let bindings: Vec<CoreQueryLoopBinding> = query_loop
        .bindings
        .iter()
        .zip(terms.iter())
        .map(|(binding, term)| CoreQueryLoopBinding {
            name: binding.name.clone(),
            component_id: term.component_id,
            component_name: term.name.clone(),
            access: term.access,
        })
        .collect();
    let body = lower_system_query_loop_body(params, &bindings, &query_loop.body)?;

    Ok(CoreQueryLoop {
        query_param: query_loop.query_param.clone(),
        bindings,
        body,
    })
}

fn lower_system_query_loop_body(
    params: &[CoreSystemParam],
    bindings: &[CoreQueryLoopBinding],
    statements: &[SystemBodyStatement],
) -> Result<Vec<CoreSystemStatement>, CoreLowerError> {
    let mut lowered = Vec::new();

    for statement in statements {
        match statement {
            SystemBodyStatement::Expression(expression) => {
                lowered.push(CoreSystemStatement::Expression(lower_system_expression(
                    params, bindings, expression,
                )?));
            }
            SystemBodyStatement::QueryLoop(_) => {
                return Err(lower_error(
                    "nested query loop lowering is not supported yet",
                ));
            }
        }
    }

    Ok(lowered)
}

fn lower_system_expression(
    params: &[CoreSystemParam],
    bindings: &[CoreQueryLoopBinding],
    expression: &Expression,
) -> Result<CoreSystemExpression, CoreLowerError> {
    match expression {
        Expression::FieldAccess {
            target, field_name, ..
        } => lower_system_field_access(params, bindings, target, field_name),
        Expression::Binary(binary) => {
            if binary.operator != BinaryOperator::Multiply {
                return Err(lower_error(format!(
                    "system body operator `{}` is not lowerable yet",
                    binary.operator
                )));
            }

            Ok(CoreSystemExpression::Binary {
                op: CoreSystemBinaryOp::F32Multiply,
                left: Box::new(lower_system_expression(params, bindings, &binary.left)?),
                right: Box::new(lower_system_expression(params, bindings, &binary.right)?),
            })
        }
        Expression::Identifier { name, .. } => Err(lower_error(format!(
            "system body identifier `{name}` is not lowerable without a field"
        ))),
        Expression::Integer(_) => Err(lower_error(
            "integer literals are not lowerable in system bodies yet",
        )),
    }
}

fn lower_system_field_access(
    params: &[CoreSystemParam],
    bindings: &[CoreQueryLoopBinding],
    target: &Expression,
    field_name: &str,
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
            field_name: field_name.to_string(),
        });
    }

    if let Some(param) = params.iter().find(|param| param.name == *name) {
        let CoreSystemParamKind::ReadResource { resource_id, name } = &param.kind else {
            return Err(lower_error(format!(
                "system body parameter `{}` is not a read resource",
                param.name
            )));
        };

        return Ok(CoreSystemExpression::ResourceField {
            param: param.name.clone(),
            resource_id: *resource_id,
            resource_name: name.clone(),
            field_name: field_name.to_string(),
        });
    }

    Err(lower_error(format!(
        "unknown system body field target `{name}`"
    )))
}

fn lower_system_param(
    program: &Program,
    param: &SystemParam,
) -> Result<CoreSystemParam, CoreLowerError> {
    let kind = match &param.kind {
        SystemParamKind::ReadResource { resource_name, .. } => {
            resolve_resource(&program.resources, resource_name)?;
            CoreSystemParamKind::ReadResource {
                resource_id: runtime::stable_resource_id(&program.world.name, resource_name).0,
                name: qualified_name(&program.world.name, resource_name),
            }
        }
        SystemParamKind::Query { terms } => {
            let terms = terms
                .iter()
                .map(|term| {
                    resolve_component(&program.components, &term.component_name)?;
                    Ok(CoreQueryTerm {
                        access: lower_query_access(term.access),
                        component_id: layout::stable_component_id(
                            &program.world.name,
                            &term.component_name,
                        )
                        .0,
                        name: layout::component_qualified_name(
                            &program.world.name,
                            &term.component_name,
                        ),
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
    }
}

fn qualified_name(world_name: &str, item_name: &str) -> String {
    format!("{world_name}.{item_name}")
}

struct StartupLowerer<'a> {
    world_name: &'a str,
    components: &'a [ComponentDecl],
    locals: Vec<CoreLocal>,
    local_by_name: HashMap<String, LocalId>,
    instructions: Vec<CoreInstruction>,
    next_local: u32,
    next_value: u32,
}

impl<'a> StartupLowerer<'a> {
    fn new(program: &'a Program) -> Self {
        Self {
            world_name: &program.world.name,
            components: &program.components,
            locals: Vec::new(),
            local_by_name: HashMap::new(),
            instructions: Vec::new(),
            next_local: 0,
            next_value: 0,
        }
    }

    fn lower_startup(
        mut self,
        startup: &StartupBlock,
    ) -> Result<(Vec<CoreLocal>, Vec<CoreInstruction>, CoreTerminator), CoreLowerError> {
        let mut terminator = None;

        for statement in &startup.statements {
            if terminator.is_some() {
                return Err(lower_error("statement after startup exit"));
            }

            match statement {
                Statement::Let(let_statement) => {
                    if let_statement.type_name.name != "i32" {
                        return Err(lower_error("only i32 locals can be lowered to Core"));
                    }

                    let local = self.allocate_local(&let_statement.name)?;
                    let value = self.lower_expression(&let_statement.initializer)?;
                    self.instructions
                        .push(CoreInstruction::LocalStore { local, value });
                }
                Statement::Exit(exit) => {
                    let value = self.lower_expression(&exit.expression)?;
                    terminator = Some(CoreTerminator::Exit { value });
                }
                Statement::Run(_) => {}
                Statement::Spawn(spawn) => {
                    self.lower_spawn_statement(spawn)?;
                }
                Statement::Resource(_) => {}
            }
        }

        let terminator = terminator.ok_or_else(|| lower_error("expected startup exit"))?;
        Ok((self.locals, self.instructions, terminator))
    }

    fn lower_spawn_statement(&mut self, spawn: &SpawnStatement) -> Result<(), CoreLowerError> {
        let components = spawn
            .components
            .iter()
            .map(|component| self.lower_spawn_component(component))
            .collect::<Result<Vec<_>, _>>()?;

        self.instructions
            .push(CoreInstruction::Spawn { components });
        Ok(())
    }

    fn lower_spawn_component(
        &self,
        component: &SpawnComponentLiteral,
    ) -> Result<CoreSpawnComponent, CoreLowerError> {
        let declaration = self
            .components
            .iter()
            .find(|declaration| declaration.name == component.name)
            .ok_or_else(|| lower_error(format!("unknown component `{}`", component.name)))?;

        let fields = component
            .fields
            .iter()
            .map(|field| self.lower_spawn_field(declaration, field))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CoreSpawnComponent {
            component_id: layout::stable_component_id(self.world_name, &component.name).0,
            name: layout::component_qualified_name(self.world_name, &component.name),
            fields,
        })
    }

    fn lower_spawn_field(
        &self,
        component: &ComponentDecl,
        field: &SpawnComponentField,
    ) -> Result<CoreSpawnField, CoreLowerError> {
        let declaration = component
            .fields
            .iter()
            .find(|declaration| declaration.name == field.name)
            .ok_or_else(|| {
                lower_error(format!(
                    "unknown field `{}` for component `{}`",
                    field.name, component.name
                ))
            })?;

        if declaration.type_name.name != "f32" {
            return Err(lower_error(format!(
                "only f32 spawn fields can be lowered to Core: {}.{}",
                component.name, field.name
            )));
        }

        let value = match &field.value {
            ComponentLiteralValue::Float { text, .. } => {
                let parsed = text.parse::<f32>().map_err(|_| {
                    lower_error(format!(
                        "invalid f32 literal `{text}` for component field `{}.{}`",
                        component.name, field.name
                    ))
                })?;
                CoreSpawnFieldValue::F32Bits(parsed.to_bits())
            }
        };

        Ok(CoreSpawnField {
            name: field.name.clone(),
            value,
        })
    }

    fn lower_expression(&mut self, expression: &Expression) -> Result<ValueId, CoreLowerError> {
        match expression {
            Expression::Integer(integer) => {
                let value = if integer.value <= i32::MAX as u64 {
                    integer.value as i32
                } else {
                    return Err(lower_error("integer literal does not fit i32"));
                };
                let result = self.allocate_value();
                self.instructions
                    .push(CoreInstruction::I32Const { result, value });
                Ok(result)
            }
            Expression::Identifier { name, .. } => {
                let local = self
                    .local_by_name
                    .get(name)
                    .copied()
                    .ok_or_else(|| lower_error(format!("unknown local `{name}`")))?;
                let result = self.allocate_value();
                self.instructions
                    .push(CoreInstruction::LocalLoad { result, local });
                Ok(result)
            }
            Expression::FieldAccess { field_name, .. } => Err(lower_error(format!(
                "field access `{field_name}` is not lowerable yet"
            ))),
            Expression::Binary(binary) => {
                let left = self.lower_expression(&binary.left)?;
                let right = self.lower_expression(&binary.right)?;
                let result = self.allocate_value();
                self.instructions.push(CoreInstruction::I32Binary {
                    result,
                    op: lower_binary_operator(binary.operator),
                    left,
                    right,
                });
                Ok(result)
            }
        }
    }

    fn allocate_local(&mut self, name: &str) -> Result<LocalId, CoreLowerError> {
        if self.local_by_name.contains_key(name) {
            return Err(lower_error(format!("duplicate local `{name}`")));
        }

        let id = LocalId(self.next_local);
        self.next_local += 1;
        self.local_by_name.insert(name.to_string(), id);
        self.locals.push(CoreLocal {
            id,
            name: name.to_string(),
            ty: CoreType::I32,
        });
        Ok(id)
    }

    fn allocate_value(&mut self) -> ValueId {
        let id = ValueId(self.next_value);
        self.next_value += 1;
        id
    }
}

fn lower_binary_operator(operator: BinaryOperator) -> CoreBinaryOp {
    match operator {
        BinaryOperator::Add => CoreBinaryOp::Add,
        BinaryOperator::Subtract => CoreBinaryOp::Subtract,
        BinaryOperator::Multiply => CoreBinaryOp::Multiply,
    }
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
        CoreSpawnComponent, CoreSpawnField, CoreSpawnFieldValue, CoreSystem, CoreSystemBinaryOp,
        CoreSystemBody, CoreSystemExpression, CoreSystemParam, CoreSystemParamKind,
        CoreSystemStatement,
    };
    use crate::lexer;
    use crate::parser;

    #[test]
    fn lowers_math_ast_to_core() {
        let source = include_str!("../../../examples/math.arc");
        let tokens = lexer::lex(source).expect("math.arc lexes");
        let ast = parser::parse_program(&tokens).expect("math.arc parses");
        let actual = lower_program_to_core(&ast).expect("math.arc lowers to Core");

        let expected = CoreProgram {
            world: CoreWorld {
                name: "Main".to_string(),
            },
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
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn lowers_spawn_position_to_core() {
        let source = include_str!("../../../examples/spawn_position.arc");
        let tokens = lexer::lex(source).expect("spawn_position.arc lexes");
        let ast = parser::parse_program(&tokens).expect("spawn_position.arc parses");
        let actual = lower_program_to_core(&ast).expect("spawn_position.arc lowers to Core");

        let expected = CoreProgram {
            world: CoreWorld {
                name: "Demo".to_string(),
            },
            systems: vec![],
            schedules: vec![],
            functions: vec![CoreFunction {
                name: "startup".to_string(),
                entry: BlockId(0),
                locals: vec![],
                blocks: vec![CoreBlock {
                    id: BlockId(0),
                    instructions: vec![
                        CoreInstruction::Spawn {
                            components: vec![CoreSpawnComponent {
                                component_id: 0x002202c6aeb4f27b,
                                name: "Demo.Position".to_string(),
                                fields: vec![
                                    CoreSpawnField {
                                        name: "x".to_string(),
                                        value: CoreSpawnFieldValue::F32Bits(0x3f800000),
                                    },
                                    CoreSpawnField {
                                        name: "y".to_string(),
                                        value: CoreSpawnFieldValue::F32Bits(0x40000000),
                                    },
                                ],
                            }],
                        },
                        CoreInstruction::I32Const {
                            result: ValueId(0),
                            value: 0,
                        },
                    ],
                    terminator: CoreTerminator::Exit { value: ValueId(0) },
                }],
            }],
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn lowers_move_system_to_core_metadata() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let ast = parser::parse_program(&tokens).expect("move_system.arc parses");
        let actual = lower_program_to_core(&ast).expect("move_system.arc lowers to Core");

        assert_eq!(actual, expected_move_system_core());
    }

    #[test]
    fn lowers_schedule_to_core_metadata() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let ast = parser::parse_program(&tokens).expect("move_system.arc parses");
        let actual = lower_program_to_core(&ast).expect("move_system.arc lowers to Core");

        assert_eq!(actual, expected_move_system_core());
        assert_eq!(
            actual.schedules,
            vec![CoreSchedule {
                name: "Main".to_string(),
                items: vec![CoreScheduleItem::Run {
                    system_id: 0x723b6b52df270ed5,
                    system_name: "Demo.Move".to_string(),
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
                    name: "time".to_string(),
                    kind: CoreSystemParamKind::ReadResource {
                        resource_id: 0x7924ce11db524521,
                        name: "Demo.Time".to_string(),
                    },
                },
                CoreSystemParam {
                    name: "movers".to_string(),
                    kind: CoreSystemParamKind::Query {
                        terms: vec![
                            CoreQueryTerm {
                                access: CoreQueryAccess::Mut,
                                component_id: 0x002202c6aeb4f27b,
                                name: "Demo.Position".to_string(),
                            },
                            CoreQueryTerm {
                                access: CoreQueryAccess::Read,
                                component_id: 0x2cf8a68bcb7f913b,
                                name: "Demo.Velocity".to_string(),
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
                    name: "pos".to_string(),
                    component_id: 0x002202c6aeb4f27b,
                    component_name: "Demo.Position".to_string(),
                    access: CoreQueryAccess::Mut,
                },
                CoreQueryLoopBinding {
                    name: "vel".to_string(),
                    component_id: 0x2cf8a68bcb7f913b,
                    component_name: "Demo.Velocity".to_string(),
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
                    name: "pos".to_string(),
                    component_id: 0x002202c6aeb4f27b,
                    component_name: "Demo.Position".to_string(),
                    access: CoreQueryAccess::Mut,
                },
                CoreQueryLoopBinding {
                    name: "vel".to_string(),
                    component_id: 0x2cf8a68bcb7f913b,
                    component_name: "Demo.Velocity".to_string(),
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

    fn move_velocity_delta_expression(velocity_field: &str) -> CoreSystemExpression {
        CoreSystemExpression::Binary {
            op: CoreSystemBinaryOp::F32Multiply,
            left: Box::new(CoreSystemExpression::ComponentField {
                binding: "vel".to_string(),
                component_id: 0x2cf8a68bcb7f913b,
                component_name: "Demo.Velocity".to_string(),
                field_name: velocity_field.to_string(),
            }),
            right: Box::new(CoreSystemExpression::ResourceField {
                param: "time".to_string(),
                resource_id: 0x7924ce11db524521,
                resource_name: "Demo.Time".to_string(),
                field_name: "delta".to_string(),
            }),
        }
    }

    fn expected_move_system_core() -> CoreProgram {
        CoreProgram {
            world: CoreWorld {
                name: "Demo".to_string(),
            },
            systems: vec![CoreSystem {
                name: "Move".to_string(),
                params: vec![
                    CoreSystemParam {
                        name: "time".to_string(),
                        kind: CoreSystemParamKind::ReadResource {
                            resource_id: 0x7924ce11db524521,
                            name: "Demo.Time".to_string(),
                        },
                    },
                    CoreSystemParam {
                        name: "movers".to_string(),
                        kind: CoreSystemParamKind::Query {
                            terms: vec![
                                CoreQueryTerm {
                                    access: CoreQueryAccess::Mut,
                                    component_id: 0x002202c6aeb4f27b,
                                    name: "Demo.Position".to_string(),
                                },
                                CoreQueryTerm {
                                    access: CoreQueryAccess::Read,
                                    component_id: 0x2cf8a68bcb7f913b,
                                    name: "Demo.Velocity".to_string(),
                                },
                            ],
                        },
                    },
                ],
                body: CoreSystemBody { statements: vec![] },
            }],
            schedules: vec![CoreSchedule {
                name: "Main".to_string(),
                items: vec![CoreScheduleItem::Run {
                    system_id: 0x723b6b52df270ed5,
                    system_name: "Demo.Move".to_string(),
                }],
            }],
            functions: vec![CoreFunction {
                name: "startup".to_string(),
                entry: BlockId(0),
                locals: vec![],
                blocks: vec![CoreBlock {
                    id: BlockId(0),
                    instructions: vec![
                        CoreInstruction::Spawn {
                            components: vec![
                                CoreSpawnComponent {
                                    component_id: 0x002202c6aeb4f27b,
                                    name: "Demo.Position".to_string(),
                                    fields: vec![
                                        CoreSpawnField {
                                            name: "x".to_string(),
                                            value: CoreSpawnFieldValue::F32Bits(0x3f800000),
                                        },
                                        CoreSpawnField {
                                            name: "y".to_string(),
                                            value: CoreSpawnFieldValue::F32Bits(0x40000000),
                                        },
                                    ],
                                },
                                CoreSpawnComponent {
                                    component_id: 0x2cf8a68bcb7f913b,
                                    name: "Demo.Velocity".to_string(),
                                    fields: vec![
                                        CoreSpawnField {
                                            name: "x".to_string(),
                                            value: CoreSpawnFieldValue::F32Bits(0x40400000),
                                        },
                                        CoreSpawnField {
                                            name: "y".to_string(),
                                            value: CoreSpawnFieldValue::F32Bits(0x40800000),
                                        },
                                    ],
                                },
                            ],
                        },
                        CoreInstruction::I32Const {
                            result: ValueId(0),
                            value: 0,
                        },
                    ],
                    terminator: CoreTerminator::Exit { value: ValueId(0) },
                }],
            }],
        }
    }
}
