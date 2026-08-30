//! Core data structures and error definitions for the CTFE evaluator (M27-C5).

use std::collections::BTreeMap;
use std::fmt;

use arche_frontend::Span;

/// Strongly typed identifier for an allocation in the CTFE virtual heap.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AllocId(pub u64);

impl fmt::Display for AllocId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "alloc#{}", self.0)
    }
}

/// A scalar constant value in CTFE.
#[derive(Clone, Debug, PartialEq)]
pub enum CtfeScalar {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    Isize(i64),
    Usize(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    Char(char),
    Unit,
}

/// A value evaluated at compile time.
#[derive(Clone, Debug, PartialEq)]
pub enum CtfeValue {
    Scalar(CtfeScalar),
    String(String),
    Tuple(Vec<CtfeValue>),
    Array(Vec<CtfeValue>),
    Struct {
        name: String,
        fields: BTreeMap<String, CtfeValue>,
    },
    Enum {
        name: String,
        variant: String,
        discriminant: u32,
        payload: Option<Box<CtfeValue>>,
    },
    HeapRef(AllocId),
}

/// Arithmetic trap kinds in CTFE (CTFE001).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArithmeticTrap {
    IntegerDivideByZero,
    IntegerRemainderByZero,
    IntegerSignedOverflow,
}

impl fmt::Display for ArithmeticTrap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IntegerDivideByZero => write!(formatter, "division by zero"),
            Self::IntegerRemainderByZero => write!(formatter, "remainder by zero"),
            Self::IntegerSignedOverflow => write!(formatter, "signed integer overflow"),
        }
    }
}

/// CTFE execution errors.
#[derive(Clone, Debug, PartialEq)]
pub enum CtfeError {
    Trap {
        kind: ArithmeticTrap,
        span: Option<Span>,
    },
    BudgetExceeded {
        limit: u64,
        span: Option<Span>,
    },
    ResourceExhausted {
        reason: String,
        span: Option<Span>,
    },
    RecursionLimitExceeded {
        depth: usize,
        span: Option<Span>,
    },
    MemoryLeakDetected {
        active_allocations: usize,
    },
}

impl fmt::Display for CtfeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Trap { kind, .. } => write!(formatter, "CTFE001: arithmetic trap: {}", kind),
            Self::BudgetExceeded { limit, .. } => {
                write!(
                    formatter,
                    "CTFE002: step budget of {} steps exceeded",
                    limit
                )
            }
            Self::ResourceExhausted { reason, .. } => {
                write!(formatter, "CTFE004: resource exhausted: {}", reason)
            }
            Self::RecursionLimitExceeded { depth, .. } => {
                write!(
                    formatter,
                    "CTFE003: recursion depth limit of {} exceeded",
                    depth
                )
            }
            Self::MemoryLeakDetected { active_allocations } => {
                write!(
                    formatter,
                    "CTFE005: memory leak: {} active allocations remain at root exit",
                    active_allocations
                )
            }
        }
    }
}

impl std::error::Error for CtfeError {}
