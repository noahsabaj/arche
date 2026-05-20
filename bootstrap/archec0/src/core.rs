#![allow(dead_code)]

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ValueId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LocalId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BlockId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreProgram {
    pub world: CoreWorld,
    pub systems: Vec<CoreSystem>,
    pub schedules: Vec<CoreSchedule>,
    pub functions: Vec<CoreFunction>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWorld {
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreSystem {
    pub name: String,
    pub params: Vec<CoreSystemParam>,
    pub body: CoreSystemBody,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreSystemParam {
    pub name: String,
    pub kind: CoreSystemParamKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreSystemParamKind {
    ReadResource { resource_id: u64, name: String },
    Query { terms: Vec<CoreQueryTerm> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreQueryTerm {
    pub access: CoreQueryAccess,
    pub component_id: u64,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreQueryAccess {
    Read,
    Mut,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreSystemBody {
    pub statements: Vec<CoreSystemStatement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreSystemStatement {
    QueryLoop(CoreQueryLoop),
    AddAssign {
        target: CoreSystemPlace,
        value: CoreSystemExpression,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreQueryLoop {
    pub query_param: String,
    pub bindings: Vec<CoreQueryLoopBinding>,
    pub body: Vec<CoreSystemStatement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreQueryLoopBinding {
    pub name: String,
    pub component_id: u64,
    pub component_name: String,
    pub access: CoreQueryAccess,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreSystemPlace {
    ComponentField {
        binding: String,
        component_id: u64,
        component_name: String,
        field_name: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreSystemExpression {
    ResourceField {
        param: String,
        resource_id: u64,
        resource_name: String,
        field_name: String,
    },
    ComponentField {
        binding: String,
        component_id: u64,
        component_name: String,
        field_name: String,
    },
    Binary {
        op: CoreSystemBinaryOp,
        left: Box<CoreSystemExpression>,
        right: Box<CoreSystemExpression>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreSystemBinaryOp {
    F32Multiply,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreSchedule {
    pub name: String,
    pub items: Vec<CoreScheduleItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreScheduleItem {
    Run { system_id: u64, system_name: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreFunction {
    pub name: String,
    pub entry: BlockId,
    pub locals: Vec<CoreLocal>,
    pub blocks: Vec<CoreBlock>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreBlock {
    pub id: BlockId,
    pub instructions: Vec<CoreInstruction>,
    pub terminator: CoreTerminator,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreType {
    I32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreLocal {
    pub id: LocalId,
    pub name: String,
    pub ty: CoreType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreInstruction {
    Spawn {
        components: Vec<CoreSpawnComponent>,
    },
    I32Const {
        result: ValueId,
        value: i32,
    },
    I32Binary {
        result: ValueId,
        op: CoreBinaryOp,
        left: ValueId,
        right: ValueId,
    },
    LocalStore {
        local: LocalId,
        value: ValueId,
    },
    LocalLoad {
        result: ValueId,
        local: LocalId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreSpawnComponent {
    pub component_id: u64,
    pub name: String,
    pub fields: Vec<CoreSpawnField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreSpawnField {
    pub name: String,
    pub value: CoreSpawnFieldValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreSpawnFieldValue {
    F32Bits(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreBinaryOp {
    Add,
    Subtract,
    Multiply,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreTerminator {
    Exit { value: ValueId },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_represents_math_startup() {
        let program = CoreProgram {
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

        assert_eq!(program.world.name, "Main");
        assert_eq!(program.functions.len(), 1);

        let startup = &program.functions[0];
        assert_eq!(startup.name, "startup");
        assert_eq!(startup.entry, BlockId(0));
        assert_eq!(
            startup.locals,
            vec![CoreLocal {
                id: LocalId(0),
                name: "x".to_string(),
                ty: CoreType::I32,
            }]
        );
        assert_eq!(startup.blocks.len(), 1);

        let entry = &startup.blocks[0];
        assert_eq!(entry.id, BlockId(0));
        assert_eq!(
            entry.instructions,
            vec![
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
            ]
        );
        assert_eq!(entry.terminator, CoreTerminator::Exit { value: ValueId(3) });
    }

    #[test]
    fn core_represents_move_system_body_model() {
        let system = CoreSystem {
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
            body: CoreSystemBody {
                statements: vec![CoreSystemStatement::QueryLoop(CoreQueryLoop {
                    query_param: "movers".to_string(),
                    bindings: vec![
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
                    ],
                    body: vec![move_add_assign("x", "x"), move_add_assign("y", "y")],
                })],
            },
        };

        assert_eq!(system.name, "Move");
        assert_eq!(system.params.len(), 2);
        assert_eq!(system.body.statements.len(), 1);
        let CoreSystemStatement::QueryLoop(query_loop) = &system.body.statements[0] else {
            panic!("expected a query loop statement");
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
            vec![move_add_assign("x", "x"), move_add_assign("y", "y")]
        );
    }

    fn move_add_assign(position_field: &str, velocity_field: &str) -> CoreSystemStatement {
        CoreSystemStatement::AddAssign {
            target: CoreSystemPlace::ComponentField {
                binding: "pos".to_string(),
                component_id: 0x002202c6aeb4f27b,
                component_name: "Demo.Position".to_string(),
                field_name: position_field.to_string(),
            },
            value: CoreSystemExpression::Binary {
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
            },
        }
    }
}
