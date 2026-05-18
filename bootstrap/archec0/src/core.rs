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
    pub functions: Vec<CoreFunction>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWorld {
    pub name: String,
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
}
