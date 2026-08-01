use crate::identifier::Identifier;
use crate::lexer::SourceSpan;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ValueId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LocalId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BlockId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreProgram {
    pub world: CoreWorld,
    pub components: Vec<CoreComponent>,
    pub resources: Vec<CoreResource>,
    pub systems: Vec<CoreSystem>,
    pub schedules: Vec<CoreSchedule>,
    pub functions: Vec<CoreFunction>,
    pub source_map: CoreSourceMap,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoreSourceMap {
    pub entries: Vec<CoreSourceMapEntry>,
}

impl CoreSourceMap {
    pub fn span(&self, subject: &CoreSourceSubject) -> Option<SourceSpan> {
        self.entries
            .iter()
            .find(|entry| &entry.subject == subject)
            .map(|entry| entry.span)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreSourceMapEntry {
    pub subject: CoreSourceSubject,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum CoreSourceSubject {
    Program,
    World,
    Component {
        component_id: u64,
    },
    ComponentField {
        component_id: u64,
        field_index: u64,
    },
    Resource {
        resource_id: u64,
    },
    ResourceField {
        resource_id: u64,
        field_index: u64,
    },
    System {
        system_id: u64,
    },
    SystemParam {
        system_id: u64,
        param_index: u64,
    },
    QueryTerm {
        system_id: u64,
        param_index: u64,
        term_index: u64,
    },
    SystemStatement {
        system_id: u64,
        statement_ordinal: u64,
    },
    SystemExpression {
        system_id: u64,
        expression_ordinal: u64,
    },
    SystemPlace {
        system_id: u64,
        place_ordinal: u64,
    },
    Schedule {
        schedule_id: u64,
    },
    ScheduleItem {
        schedule_id: u64,
        item_index: u64,
    },
    Startup,
    StartupInstruction {
        block: BlockId,
        instruction_index: u64,
    },
    StartupTerminator {
        block: BlockId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWorld {
    pub name: Identifier,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreComponent {
    pub id: u64,
    pub name: Identifier,
    pub kind: CoreComponentKind,
    pub fields: Vec<CoreField>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreComponentKind {
    Component,
    Tag,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreResource {
    pub id: u64,
    pub name: Identifier,
    pub fields: Vec<CoreField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreField {
    pub name: Identifier,
    pub ty: CoreType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreSystem {
    pub id: u64,
    pub name: Identifier,
    pub params: Vec<CoreSystemParam>,
    pub body: CoreSystemBody,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreSystemParam {
    pub name: Identifier,
    pub kind: CoreSystemParamKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreSystemParamKind {
    ReadResource { resource_id: u64, name: Identifier },
    MutResource { resource_id: u64, name: Identifier },
    Query { terms: Vec<CoreQueryTerm> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreQueryTerm {
    pub access: CoreQueryAccess,
    pub component_id: u64,
    pub name: Identifier,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreQueryAccess {
    Read,
    Mut,
    Exclude,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreSystemBody {
    pub statements: Vec<CoreSystemStatement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreSystemStatement {
    QueryLoop(CoreQueryLoop),
    Expression(CoreSystemExpression),
    Let {
        name: Identifier,
        ty: CoreType,
        mutable: bool,
        value: CoreSystemExpression,
    },
    Assign {
        target: CoreSystemPlace,
        value: CoreSystemExpression,
    },
    AddAssign {
        target: CoreSystemPlace,
        value: CoreSystemExpression,
    },
    Block(Vec<CoreSystemStatement>),
    If {
        condition: CoreSystemExpression,
        then_body: Vec<CoreSystemStatement>,
        else_body: Vec<CoreSystemStatement>,
    },
    While {
        condition: CoreSystemExpression,
        body: Vec<CoreSystemStatement>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreQueryLoop {
    pub query_param: Identifier,
    pub bindings: Vec<CoreQueryLoopBinding>,
    pub body: Vec<CoreSystemStatement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreQueryLoopBinding {
    pub name: Identifier,
    pub component_id: u64,
    pub component_name: Identifier,
    pub access: CoreQueryAccess,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreSystemPlace {
    Local {
        name: Identifier,
        ty: CoreType,
        mutable: bool,
    },
    ComponentField {
        binding: Identifier,
        component_id: u64,
        component_name: Identifier,
        field_name: Identifier,
    },
    ResourceField {
        param: Identifier,
        resource_id: u64,
        resource_name: Identifier,
        field_name: Identifier,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreSystemExpression {
    I32Const(i32),
    F32Const(u32),
    BoolConst(bool),
    Local {
        name: Identifier,
        ty: CoreType,
    },
    ResourceField {
        param: Identifier,
        resource_id: u64,
        resource_name: Identifier,
        field_name: Identifier,
    },
    ComponentField {
        binding: Identifier,
        component_id: u64,
        component_name: Identifier,
        field_name: Identifier,
    },
    BoolNot(Box<CoreSystemExpression>),
    Unary {
        op: CoreSystemUnaryOp,
        operand: Box<CoreSystemExpression>,
    },
    Binary {
        op: CoreSystemBinaryOp,
        left: Box<CoreSystemExpression>,
        right: Box<CoreSystemExpression>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreSystemUnaryOp {
    I32Negate,
    F32Negate,
    I32BitNot,
    BoolNot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreSystemBinaryOp {
    I32Add,
    I32Subtract,
    I32Multiply,
    I32Divide,
    I32Remainder,
    I32ShiftLeft,
    I32ShiftRight,
    I32BitAnd,
    I32BitXor,
    I32BitOr,
    F32Add,
    F32Subtract,
    F32Multiply,
    F32Divide,
    I32Less,
    I32LessEqual,
    I32Greater,
    I32GreaterEqual,
    F32Less,
    F32LessEqual,
    F32Greater,
    F32GreaterEqual,
    Equal,
    NotEqual,
    LogicalAnd,
    LogicalOr,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreSchedule {
    pub id: u64,
    pub name: Identifier,
    pub items: Vec<CoreScheduleItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreScheduleItem {
    Run {
        system_id: u64,
        system_name: Identifier,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreFunction {
    pub name: Identifier,
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
    F32,
    Bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreLocal {
    pub id: LocalId,
    pub name: Identifier,
    pub ty: CoreType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreInstruction {
    InitializeResource {
        resource_id: u64,
        resource_name: Identifier,
        fields: Vec<CoreResourceField>,
    },
    Spawn {
        components: Vec<CoreSpawnComponent>,
    },
    RunSchedule {
        schedule_id: u64,
        schedule_name: Identifier,
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
    I32Unary {
        result: ValueId,
        op: CoreUnaryOp,
        operand: ValueId,
    },
    F32Const {
        result: ValueId,
        bits: u32,
    },
    F32Unary {
        result: ValueId,
        op: CoreUnaryOp,
        operand: ValueId,
    },
    F32Binary {
        result: ValueId,
        op: CoreBinaryOp,
        left: ValueId,
        right: ValueId,
    },
    Compare {
        result: ValueId,
        op: CoreComparisonOp,
        left: ValueId,
        right: ValueId,
        operand_type: CoreType,
    },
    BoolConst {
        result: ValueId,
        value: bool,
    },
    BoolNot {
        result: ValueId,
        operand: ValueId,
    },
    Equal {
        result: ValueId,
        left: ValueId,
        right: ValueId,
        operand_type: CoreType,
        negate: bool,
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
pub struct CoreResourceField {
    pub name: Identifier,
    pub evaluation: ValueId,
    pub value: CoreLiteralValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreSpawnComponent {
    pub component_id: u64,
    pub name: Identifier,
    pub fields: Vec<CoreSpawnField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreSpawnField {
    pub name: Identifier,
    pub evaluation: ValueId,
    pub value: CoreLiteralValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreLiteralValue {
    F32Bits(u32),
    I32(i32),
    Bool(bool),
}

pub type CoreSpawnFieldValue = CoreLiteralValue;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreBinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    ShiftLeft,
    ShiftRight,
    BitAnd,
    BitXor,
    BitOr,
}

impl CoreBinaryOp {
    pub fn trap_kind(self) -> Option<CoreTrapKind> {
        match self {
            Self::Divide => Some(CoreTrapKind::IntegerDivide),
            Self::Remainder => Some(CoreTrapKind::IntegerRemainder),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreUnaryOp {
    Negate,
    BitNot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreComparisonOp {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreTrapKind {
    IntegerDivide,
    IntegerRemainder,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreTerminator {
    Exit {
        value: ValueId,
    },
    Jump {
        target: BlockId,
    },
    Branch {
        condition: ValueId,
        then_block: BlockId,
        else_block: BlockId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_represents_math_startup() {
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

        assert_eq!(program.world.name, "Main");
        assert_eq!(program.functions.len(), 1);

        let startup = &program.functions[0];
        assert_eq!(startup.name, "startup");
        assert_eq!(startup.entry, BlockId(0));
        assert_eq!(
            startup.locals,
            vec![CoreLocal {
                id: LocalId(0),
                name: "x".into(),
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
            id: 0x723b6b52df270ed5,
            name: "Move".into(),
            params: vec![
                CoreSystemParam {
                    name: "time".into(),
                    kind: CoreSystemParamKind::ReadResource {
                        resource_id: 0x7924ce11db524521,
                        name: "Demo.Time".into(),
                    },
                },
                CoreSystemParam {
                    name: "movers".into(),
                    kind: CoreSystemParamKind::Query {
                        terms: vec![
                            CoreQueryTerm {
                                access: CoreQueryAccess::Mut,
                                component_id: 0x002202c6aeb4f27b,
                                name: "Demo.Position".into(),
                            },
                            CoreQueryTerm {
                                access: CoreQueryAccess::Read,
                                component_id: 0x2cf8a68bcb7f913b,
                                name: "Demo.Velocity".into(),
                            },
                        ],
                    },
                },
            ],
            body: CoreSystemBody {
                statements: vec![CoreSystemStatement::QueryLoop(CoreQueryLoop {
                    query_param: "movers".into(),
                    bindings: vec![
                        CoreQueryLoopBinding {
                            name: "pos".into(),
                            component_id: 0x002202c6aeb4f27b,
                            component_name: "Demo.Position".into(),
                            access: CoreQueryAccess::Mut,
                        },
                        CoreQueryLoopBinding {
                            name: "vel".into(),
                            component_id: 0x2cf8a68bcb7f913b,
                            component_name: "Demo.Velocity".into(),
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
                    name: "pos".into(),
                    component_id: 0x002202c6aeb4f27b,
                    component_name: "Demo.Position".into(),
                    access: CoreQueryAccess::Mut,
                },
                CoreQueryLoopBinding {
                    name: "vel".into(),
                    component_id: 0x2cf8a68bcb7f913b,
                    component_name: "Demo.Velocity".into(),
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
                binding: "pos".into(),
                component_id: 0x002202c6aeb4f27b,
                component_name: "Demo.Position".into(),
                field_name: position_field.into(),
            },
            value: CoreSystemExpression::Binary {
                op: CoreSystemBinaryOp::F32Multiply,
                left: Box::new(CoreSystemExpression::ComponentField {
                    binding: "vel".into(),
                    component_id: 0x2cf8a68bcb7f913b,
                    component_name: "Demo.Velocity".into(),
                    field_name: velocity_field.into(),
                }),
                right: Box::new(CoreSystemExpression::ResourceField {
                    param: "time".into(),
                    resource_id: 0x7924ce11db524521,
                    resource_name: "Demo.Time".into(),
                    field_name: "delta".into(),
                }),
            },
        }
    }
}
