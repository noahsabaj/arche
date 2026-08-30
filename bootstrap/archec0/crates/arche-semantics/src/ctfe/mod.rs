//! Compile-Time Function Execution (CTFE) evaluator and receipts for M27-C5.

pub mod def;
pub mod eval;
pub mod receipt;

pub use def::{AllocId, ArithmeticTrap, CtfeError, CtfeScalar, CtfeValue};
pub use eval::{eval_binary_arithmetic, CtfeContext};
pub use receipt::{compute_ctfe_receipt, serialize_ctfe_value, CtfeReceipt};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mir::MirBinOp;

    #[test]
    fn divide_by_zero_produces_exact_trap() {
        let err = eval_binary_arithmetic(
            MirBinOp::Div,
            &CtfeScalar::I32(10),
            &CtfeScalar::I32(0),
            None,
        )
        .unwrap_err();
        assert_eq!(
            err,
            CtfeError::Trap {
                kind: ArithmeticTrap::IntegerDivideByZero,
                span: None,
            }
        );
    }

    #[test]
    fn signed_min_divided_by_negative_one_overflows() {
        let err = eval_binary_arithmetic(
            MirBinOp::Div,
            &CtfeScalar::I32(i32::MIN),
            &CtfeScalar::I32(-1),
            None,
        )
        .unwrap_err();
        assert_eq!(
            err,
            CtfeError::Trap {
                kind: ArithmeticTrap::IntegerSignedOverflow,
                span: None,
            }
        );
    }

    #[test]
    fn step_budget_exhaustion_produces_ctfe002() {
        let mut ctx = CtfeContext::new(2, 16, 1024);
        assert!(ctx.consume_step(None).is_ok());
        assert!(ctx.consume_step(None).is_ok());
        let err = ctx.consume_step(None).unwrap_err();
        assert_eq!(
            err,
            CtfeError::BudgetExceeded {
                limit: 2,
                span: None
            }
        );
    }

    #[test]
    fn heap_allocation_and_zero_leak_verification() {
        let mut ctx = CtfeContext::new(100, 16, 1024);
        let alloc = ctx.alloc_bytes(vec![1, 2, 3, 4], None).unwrap();
        assert_eq!(ctx.current_heap_bytes, 4);

        // If we don't free, finish() detects memory leak!
        let leak_ctx = CtfeContext {
            step_budget: ctx.step_budget,
            steps_used: ctx.steps_used,
            max_depth: ctx.max_depth,
            current_depth: ctx.current_depth,
            max_heap_bytes: ctx.max_heap_bytes,
            current_heap_bytes: ctx.current_heap_bytes,
            next_alloc_id: ctx.next_alloc_id,
            virtual_heap: ctx.virtual_heap.clone(),
            event_log: ctx.event_log.clone(),
        };
        assert!(leak_ctx.finish().is_err());

        // Freeing restores clean zero-allocation heap
        ctx.free_alloc(alloc);
        assert_eq!(ctx.current_heap_bytes, 0);
        let events = ctx.finish().unwrap();
        assert!(!events.is_empty());
    }

    #[test]
    fn cryptographic_receipts_are_deterministic() {
        let val = CtfeValue::Tuple(vec![
            CtfeValue::Scalar(CtfeScalar::I32(42)),
            CtfeValue::String("hello".into()),
        ]);
        let events = vec![b"step".to_vec(), b"alloc".to_vec(), b"free".to_vec()];

        let r1 = compute_ctfe_receipt(&val, &events, 10);
        let r2 = compute_ctfe_receipt(&val, &events, 10);
        assert_eq!(r1, r2);
        assert_ne!(r1.result_digest, [0u8; 16]);
        assert_ne!(r1.trace_digest, [0u8; 16]);
    }
}
