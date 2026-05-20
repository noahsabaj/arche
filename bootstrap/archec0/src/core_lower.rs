#![allow(dead_code)]

use std::collections::HashMap;

use crate::core::{
    BlockId, CoreBinaryOp, CoreBlock, CoreFunction, CoreInstruction, CoreLocal, CoreProgram,
    CoreQueryAccess, CoreQueryTerm, CoreSchedule, CoreScheduleItem, CoreSpawnComponent,
    CoreSpawnField, CoreSpawnFieldValue, CoreSystem, CoreSystemBody, CoreSystemParam,
    CoreSystemParamKind, CoreTerminator, CoreType, CoreWorld, LocalId, ValueId,
};
use crate::layout;
use crate::parser::{
    BinaryOperator, ComponentDecl, ComponentLiteralValue, Expression, Program,
    QueryAccess as ParserQueryAccess, ResourceDecl, ScheduleDecl, ScheduleItem,
    SpawnComponentField, SpawnComponentLiteral, SpawnStatement, StartupBlock, Statement,
    SystemDecl, SystemParam, SystemParamKind,
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

    Ok(CoreSystem {
        name: system.name.clone(),
        params,
        body: CoreSystemBody { statements: vec![] },
    })
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
        CoreQueryAccess, CoreQueryTerm, CoreSchedule, CoreScheduleItem, CoreSpawnComponent,
        CoreSpawnField, CoreSpawnFieldValue, CoreSystem, CoreSystemBody, CoreSystemParam,
        CoreSystemParamKind,
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
