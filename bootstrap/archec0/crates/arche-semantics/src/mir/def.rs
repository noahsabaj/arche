//! Core Mid-Level Intermediate Representation (MIR) data definitions for M27-C3.

use std::collections::BTreeMap;
use std::fmt;

use arche_frontend::{Span, SymbolicType};

/// A strongly typed identifier for a basic block.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BasicBlockId(pub u32);

impl fmt::Display for BasicBlockId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "bb{}", self.0)
    }
}

/// A strongly typed identifier for a local variable or temporary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LocalId(pub u32);

impl fmt::Display for LocalId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "_{}", self.0)
    }
}

/// Mutability qualifier of a place or local.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Mutability {
    Immutable,
    Mutable,
}

/// Kind of local variable in a MIR body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalKind {
    /// Return place (`_0`).
    ReturnPlace,
    /// Function argument (`_1`, `_2`, ...).
    Arg,
    /// User-declared local variable.
    UserVar,
    /// Compiler-generated temporary.
    Temp,
    /// Compiler-generated drop flag boolean.
    DropFlag,
}

/// Declaration of a local variable in MIR.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalDecl {
    pub kind: LocalKind,
    pub ty: SymbolicType,
    pub mutability: Mutability,
    pub span: Option<Span>,
}

/// A projection element indexing into or traversing a place.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ProjectionElem {
    /// Dereference `*place`.
    Deref,
    /// Field of a struct/record `place.field`.
    Field(u32),
    /// Tuple field `place.0`.
    TupleField(u32),
    /// Dynamic element indexing `place[index_local]`.
    Index(LocalId),
    /// Constant index `place[offset]`.
    ConstantIndex {
        offset: u64,
        min_length: u64,
        from_end: bool,
    },
    /// Subslice `place[from..to]`.
    Subslice { from: u64, to: u64, from_end: bool },
    /// Downcast to an enum variant.
    Downcast(u32),
}

/// A memory place composed of a root local and a series of projections.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Place {
    pub local: LocalId,
    pub projection: Vec<ProjectionElem>,
}

impl Place {
    /// Constructs a direct local place without projections.
    #[must_use]
    pub fn from_local(local: LocalId) -> Self {
        Self {
            local,
            projection: Vec::new(),
        }
    }

    /// Constructs the return place `_0`.
    #[must_use]
    pub fn return_place() -> Self {
        Self::from_local(LocalId(0))
    }

    /// Appends a field projection.
    #[must_use]
    pub fn project_field(mut self, field: u32) -> Self {
        self.projection.push(ProjectionElem::Field(field));
        self
    }

    /// Appends a tuple field projection.
    #[must_use]
    pub fn project_tuple_field(mut self, field: u32) -> Self {
        self.projection.push(ProjectionElem::TupleField(field));
        self
    }

    /// Appends a dereference projection.
    #[must_use]
    pub fn deref(mut self) -> Self {
        self.projection.push(ProjectionElem::Deref);
        self
    }

    /// Appends a dynamic index projection.
    #[must_use]
    pub fn project_index(mut self, index: LocalId) -> Self {
        self.projection.push(ProjectionElem::Index(index));
        self
    }

    /// Appends a downcast projection.
    #[must_use]
    pub fn downcast(mut self, variant: u32) -> Self {
        self.projection.push(ProjectionElem::Downcast(variant));
        self
    }
}

/// Borrow kind for a place.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BorrowKind {
    Shared,
    Mutable,
}

/// Unary operator in MIR.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MirUnOp {
    Not,
    Neg,
}

/// Binary operator in MIR.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MirBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitXor,
    BitAnd,
    BitOr,
    Shl,
    Shr,
    Eq,
    Lt,
    Le,
    Ne,
    Ge,
    Gt,
    Offset,
}

/// A constant literal value in MIR.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MirConstant {
    Bool(bool),
    Int(i128, SymbolicType),
    Float(u64, SymbolicType),
    Char(char),
    Str(String),
    Unit,
}

/// An operand consumed by an rvalue or call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operand {
    /// Copies a place that implements `Copy`.
    Copy(Place),
    /// Moves a place, invalidating its source location.
    Move(Place),
    /// Immediate constant literal value.
    Constant(MirConstant),
}

impl Operand {
    /// Returns the place referenced by this operand, if any.
    #[must_use]
    pub fn place(&self) -> Option<&Place> {
        match self {
            Self::Copy(place) | Self::Move(place) => Some(place),
            Self::Constant(_) => None,
        }
    }
}

/// An authorization token proving dynamic indexing safety.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedIndexProof {
    pub container: Place,
    pub index_local: LocalId,
    pub element_type: SymbolicType,
}

/// An rvalue producing a value assigned to a place.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Rvalue {
    /// Use of an operand.
    Use(Operand),
    /// Borrow of a place (`&place` or `&mut place`).
    Ref(BorrowKind, Place),
    /// Unary operation.
    UnaryOp(MirUnOp, Operand),
    /// Binary operation.
    BinaryOp(MirBinOp, Operand, Operand),
    /// Checked dynamic index read authorized by proof.
    CheckedIndex { proof: CheckedIndexProof },
    /// Aggregate constructor (tuple, array, struct, enum variant).
    Aggregate {
        ty: SymbolicType,
        operands: Vec<Operand>,
    },
    /// Discriminant readout of an enum.
    Discriminant(Place),
    /// Cast of an operand to a target type.
    Cast {
        operand: Operand,
        target: SymbolicType,
    },
}

/// A non-branching statement inside a basic block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StatementKind {
    /// Assigns the result of an rvalue to a place (`place = rvalue`).
    Assign(Place, Box<Rvalue>),
    /// Declares a local variable storage live.
    StorageLive(LocalId),
    /// Declares a local variable storage dead.
    StorageDead(LocalId),
    /// Explicit drop of a place.
    Drop(Place),
    /// No-operation statement.
    Nop,
}

/// A statement paired with optional source span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: Option<Span>,
}

/// A basic block terminator specifying control flow out of the block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminatorKind {
    /// Unconditional jump to target block.
    Goto { target: BasicBlockId },
    /// Switch on integer/enum discriminant.
    SwitchInt {
        discr: Operand,
        targets: BTreeMap<u128, BasicBlockId>,
        otherwise: BasicBlockId,
    },
    /// Normal return from the current body.
    Return,
    /// Abort execution due to trap or panic without recovery.
    Abort,
    /// Resume unwinding to caller.
    Resume,
    /// Function or method call.
    Call {
        callee: Operand,
        args: Vec<Operand>,
        destination: Place,
        target: Option<BasicBlockId>,
        cleanup: Option<BasicBlockId>,
    },
}

/// A terminator paired with optional source span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Terminator {
    pub kind: TerminatorKind,
    pub span: Option<Span>,
}

/// A basic block in a MIR body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BasicBlock {
    pub statements: Vec<Statement>,
    pub terminator: Option<Terminator>,
    pub is_cleanup: bool,
}

impl BasicBlock {
    /// Constructs a new empty basic block.
    #[must_use]
    pub fn new(is_cleanup: bool) -> Self {
        Self {
            statements: Vec::new(),
            terminator: None,
            is_cleanup,
        }
    }
}

/// A typed Mid-Level Intermediate Representation body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MirBody {
    pub basic_blocks: Vec<BasicBlock>,
    pub locals: Vec<LocalDecl>,
    pub arg_count: usize,
    pub return_type: SymbolicType,
}

impl MirBody {
    /// Creates a new empty MIR body with return place `_0`.
    #[must_use]
    pub fn new(return_type: SymbolicType) -> Self {
        let return_local = LocalDecl {
            kind: LocalKind::ReturnPlace,
            ty: return_type.clone(),
            mutability: Mutability::Mutable,
            span: None,
        };
        Self {
            basic_blocks: Vec::new(),
            locals: vec![return_local],
            arg_count: 0,
            return_type,
        }
    }

    /// Allocates a new local variable and returns its ID.
    pub fn alloc_local(
        &mut self,
        kind: LocalKind,
        ty: SymbolicType,
        mutability: Mutability,
        span: Option<Span>,
    ) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(LocalDecl {
            kind,
            ty,
            mutability,
            span,
        });
        if kind == LocalKind::Arg {
            self.arg_count += 1;
        }
        id
    }

    /// Allocates a new basic block and returns its ID.
    pub fn alloc_basic_block(&mut self, is_cleanup: bool) -> BasicBlockId {
        let id = BasicBlockId(self.basic_blocks.len() as u32);
        self.basic_blocks.push(BasicBlock::new(is_cleanup));
        id
    }

    /// Returns a reference to a basic block.
    #[must_use]
    pub fn block(&self, id: BasicBlockId) -> Option<&BasicBlock> {
        self.basic_blocks.get(id.0 as usize)
    }

    /// Returns a mutable reference to a basic block.
    pub fn block_mut(&mut self, id: BasicBlockId) -> Option<&mut BasicBlock> {
        self.basic_blocks.get_mut(id.0 as usize)
    }

    /// Pushes a statement to a basic block.
    pub fn push_statement(&mut self, block: BasicBlockId, kind: StatementKind, span: Option<Span>) {
        if let Some(bb) = self.block_mut(block) {
            bb.statements.push(Statement { kind, span });
        }
    }

    /// Sets the terminator of a basic block.
    pub fn set_terminator(
        &mut self,
        block: BasicBlockId,
        kind: TerminatorKind,
        span: Option<Span>,
    ) {
        if let Some(bb) = self.block_mut(block) {
            bb.terminator = Some(Terminator { kind, span });
        }
    }
}
