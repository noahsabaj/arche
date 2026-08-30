//! Hermetic CTFE interpreter and arithmetic evaluator for M27-C5.

use std::collections::BTreeMap;

use arche_frontend::Span;

use crate::ctfe::def::{AllocId, ArithmeticTrap, CtfeError, CtfeScalar};
use crate::mir::MirBinOp;

/// Evaluation context and virtual machine state for CTFE.
pub struct CtfeContext {
    pub step_budget: u64,
    pub steps_used: u64,
    pub max_depth: usize,
    pub current_depth: usize,
    pub max_heap_bytes: usize,
    pub current_heap_bytes: usize,
    pub next_alloc_id: u64,
    pub virtual_heap: BTreeMap<AllocId, Vec<u8>>,
    pub event_log: Vec<Vec<u8>>,
}

impl CtfeContext {
    /// Creates a new CTFE execution context with default deterministic limits.
    #[must_use]
    pub fn new(step_budget: u64, max_depth: usize, max_heap_bytes: usize) -> Self {
        Self {
            step_budget,
            steps_used: 0,
            max_depth,
            current_depth: 0,
            max_heap_bytes,
            current_heap_bytes: 0,
            next_alloc_id: 1,
            virtual_heap: BTreeMap::new(),
            event_log: Vec::new(),
        }
    }

    /// Consumes a single execution step, returning `CTFE002` if budget is exceeded.
    pub fn consume_step(&mut self, span: Option<Span>) -> Result<(), CtfeError> {
        if self.steps_used >= self.step_budget {
            return Err(CtfeError::BudgetExceeded {
                limit: self.step_budget,
                span,
            });
        }
        self.steps_used += 1;
        self.log_event(b"step");
        Ok(())
    }

    /// Records entering a call frame.
    pub fn enter_depth(&mut self, span: Option<Span>) -> Result<(), CtfeError> {
        if self.current_depth >= self.max_depth {
            return Err(CtfeError::RecursionLimitExceeded {
                depth: self.max_depth,
                span,
            });
        }
        self.current_depth += 1;
        self.log_event(b"depth_enter");
        Ok(())
    }

    /// Records exiting a call frame.
    pub fn exit_depth(&mut self) {
        self.current_depth = self.current_depth.saturating_sub(1);
        self.log_event(b"depth_exit");
    }

    /// Allocates a block of memory on the virtual heap.
    pub fn alloc_bytes(&mut self, data: Vec<u8>, span: Option<Span>) -> Result<AllocId, CtfeError> {
        let size = data.len();
        if self.current_heap_bytes + size > self.max_heap_bytes {
            return Err(CtfeError::ResourceExhausted {
                reason: format!(
                    "heap memory limit of {} bytes exceeded",
                    self.max_heap_bytes
                ),
                span,
            });
        }
        let id = AllocId(self.next_alloc_id);
        self.next_alloc_id += 1;
        self.current_heap_bytes += size;
        self.virtual_heap.insert(id, data);
        self.log_event(b"alloc");
        Ok(id)
    }

    /// Frees an allocation from the virtual heap.
    pub fn free_alloc(&mut self, id: AllocId) {
        if let Some(data) = self.virtual_heap.remove(&id) {
            self.current_heap_bytes = self.current_heap_bytes.saturating_sub(data.len());
            self.log_event(b"free");
        }
    }

    /// Appends an event to the execution audit log.
    pub fn log_event(&mut self, event: &[u8]) {
        self.event_log.push(event.to_vec());
    }

    /// Finalizes evaluation, verifying that no leaked allocations remain.
    pub fn finish(self) -> Result<Vec<Vec<u8>>, CtfeError> {
        if !self.virtual_heap.is_empty() {
            return Err(CtfeError::MemoryLeakDetected {
                active_allocations: self.virtual_heap.len(),
            });
        }
        Ok(self.event_log)
    }
}

/// Evaluates an exact binary arithmetic operation with overflow and divide-by-zero traps.
pub fn eval_binary_arithmetic(
    op: MirBinOp,
    left: &CtfeScalar,
    right: &CtfeScalar,
    span: Option<Span>,
) -> Result<CtfeScalar, CtfeError> {
    match (op, left, right) {
        // i32 arithmetic
        (MirBinOp::Add, CtfeScalar::I32(a), CtfeScalar::I32(b)) => a
            .checked_add(*b)
            .map(CtfeScalar::I32)
            .ok_or(CtfeError::Trap {
                kind: ArithmeticTrap::IntegerSignedOverflow,
                span,
            }),
        (MirBinOp::Sub, CtfeScalar::I32(a), CtfeScalar::I32(b)) => a
            .checked_sub(*b)
            .map(CtfeScalar::I32)
            .ok_or(CtfeError::Trap {
                kind: ArithmeticTrap::IntegerSignedOverflow,
                span,
            }),
        (MirBinOp::Mul, CtfeScalar::I32(a), CtfeScalar::I32(b)) => a
            .checked_mul(*b)
            .map(CtfeScalar::I32)
            .ok_or(CtfeError::Trap {
                kind: ArithmeticTrap::IntegerSignedOverflow,
                span,
            }),
        (MirBinOp::Div, CtfeScalar::I32(a), CtfeScalar::I32(b)) => {
            if *b == 0 {
                return Err(CtfeError::Trap {
                    kind: ArithmeticTrap::IntegerDivideByZero,
                    span,
                });
            }
            if *a == i32::MIN && *b == -1 {
                return Err(CtfeError::Trap {
                    kind: ArithmeticTrap::IntegerSignedOverflow,
                    span,
                });
            }
            Ok(CtfeScalar::I32(a / b))
        }
        (MirBinOp::Rem, CtfeScalar::I32(a), CtfeScalar::I32(b)) => {
            if *b == 0 {
                return Err(CtfeError::Trap {
                    kind: ArithmeticTrap::IntegerRemainderByZero,
                    span,
                });
            }
            if *a == i32::MIN && *b == -1 {
                return Err(CtfeError::Trap {
                    kind: ArithmeticTrap::IntegerSignedOverflow,
                    span,
                });
            }
            Ok(CtfeScalar::I32(a % b))
        }
        (MirBinOp::Shl, CtfeScalar::I32(a), CtfeScalar::I32(b)) => {
            let shift = (b & 31) as u32;
            Ok(CtfeScalar::I32(a.wrapping_shl(shift)))
        }
        (MirBinOp::Shr, CtfeScalar::I32(a), CtfeScalar::I32(b)) => {
            let shift = (b & 31) as u32;
            Ok(CtfeScalar::I32(a.wrapping_shr(shift)))
        }
        (MirBinOp::Eq, CtfeScalar::I32(a), CtfeScalar::I32(b)) => Ok(CtfeScalar::Bool(a == b)),
        (MirBinOp::Lt, CtfeScalar::I32(a), CtfeScalar::I32(b)) => Ok(CtfeScalar::Bool(a < b)),
        (MirBinOp::Le, CtfeScalar::I32(a), CtfeScalar::I32(b)) => Ok(CtfeScalar::Bool(a <= b)),
        (MirBinOp::Gt, CtfeScalar::I32(a), CtfeScalar::I32(b)) => Ok(CtfeScalar::Bool(a > b)),
        (MirBinOp::Ge, CtfeScalar::I32(a), CtfeScalar::I32(b)) => Ok(CtfeScalar::Bool(a >= b)),

        // u32 arithmetic
        (MirBinOp::Add, CtfeScalar::U32(a), CtfeScalar::U32(b)) => a
            .checked_add(*b)
            .map(CtfeScalar::U32)
            .ok_or(CtfeError::Trap {
                kind: ArithmeticTrap::IntegerSignedOverflow,
                span,
            }),
        (MirBinOp::Div, CtfeScalar::U32(a), CtfeScalar::U32(b)) => {
            if *b == 0 {
                return Err(CtfeError::Trap {
                    kind: ArithmeticTrap::IntegerDivideByZero,
                    span,
                });
            }
            Ok(CtfeScalar::U32(a / b))
        }
        (MirBinOp::Rem, CtfeScalar::U32(a), CtfeScalar::U32(b)) => {
            if *b == 0 {
                return Err(CtfeError::Trap {
                    kind: ArithmeticTrap::IntegerRemainderByZero,
                    span,
                });
            }
            Ok(CtfeScalar::U32(a % b))
        }

        _ => Err(CtfeError::ResourceExhausted {
            reason: format!(
                "unsupported binary operation {:?} on {:?}, {:?}",
                op, left, right
            ),
            span,
        }),
    }
}
