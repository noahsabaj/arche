//! Mid-Level Intermediate Representation (MIR) and ownership system for M27-C3.

pub mod call_graph;
pub mod dataflow;
pub mod def;
pub mod drop;
pub mod lower;
pub mod nll;

pub use call_graph::{check_call_graph_recursion, CallEdge, CallGraphError};
pub use dataflow::{
    check_definite_initialization, InitState, MoveError, PathTrie, UninitializedPathError,
};
pub use def::{
    BasicBlock, BasicBlockId, BorrowKind, CheckedIndexProof, LocalDecl, LocalId, LocalKind,
    MirBinOp, MirBody, MirConstant, MirUnOp, Mutability, Operand, Place, ProjectionElem, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};
pub use drop::elaborate_drops;
pub use lower::MirBuilder;
pub use nll::{check_borrows, ActiveLoan, BorrowError, LoanId};

#[cfg(test)]
mod tests {
    use super::*;
    use arche_frontend::{
        DeclarationKind, Mutability as FrontendMutability, SemanticDeclarationPath,
        SymbolicLifetime, SymbolicType, TargetRoot,
    };

    fn sample_nominal() -> SymbolicType {
        SymbolicType::NominalPath {
            declaration: SemanticDeclarationPath {
                registry_origin: "workspace".to_owned(),
                package_name: "test".to_owned(),
                target: TargetRoot::Library,
                modules: vec!["types".to_owned()],
                kind: DeclarationKind::Struct,
                name: "Holder".to_owned(),
            },
            arguments: Vec::new(),
        }
    }

    #[test]
    fn mir_body_allocates_locals_and_blocks() {
        let mut body = MirBody::new(SymbolicType::I32);
        assert_eq!(body.locals.len(), 1); // _0 return place
        assert_eq!(body.return_type, SymbolicType::I32);

        let arg = body.alloc_local(
            LocalKind::Arg,
            SymbolicType::I32,
            Mutability::Immutable,
            None,
        );
        assert_eq!(arg, LocalId(1));
        assert_eq!(body.arg_count, 1);

        let bb0 = body.alloc_basic_block(false);
        assert_eq!(bb0, BasicBlockId(0));
        assert!(!body.block(bb0).unwrap().is_cleanup);

        body.push_statement(
            bb0,
            StatementKind::Assign(
                Place::return_place(),
                Box::new(Rvalue::Use(Operand::Copy(Place::from_local(arg)))),
            ),
            None,
        );
        body.set_terminator(bb0, TerminatorKind::Return, None);

        assert_eq!(body.block(bb0).unwrap().statements.len(), 1);
        assert!(matches!(
            body.block(bb0).unwrap().terminator.as_ref().unwrap().kind,
            TerminatorKind::Return
        ));
    }

    #[test]
    fn definite_initialization_accepts_initialized_paths_and_rejects_uninitialized() {
        let mut builder = MirBuilder::new(SymbolicType::I32);
        let arg = builder.add_arg(SymbolicType::I32, None);
        let temp = builder.add_temp(SymbolicType::I32, None);

        // temp = arg
        builder.push_assign(
            Place::from_local(temp),
            Rvalue::Use(Operand::Copy(Place::from_local(arg))),
            None,
        );
        // _0 = temp
        builder.push_assign(
            Place::return_place(),
            Rvalue::Use(Operand::Copy(Place::from_local(temp))),
            None,
        );
        builder.terminate_return(None);

        let body = builder.finish();
        assert!(check_definite_initialization(&body).is_ok());

        // Body with uninitialized read
        let mut bad_builder = MirBuilder::new(SymbolicType::I32);
        let bad_temp = bad_builder.add_temp(SymbolicType::I32, None);
        bad_builder.push_assign(
            Place::return_place(),
            Rvalue::Use(Operand::Copy(Place::from_local(bad_temp))),
            None,
        );
        bad_builder.terminate_return(None);

        let bad_body = bad_builder.finish();
        let err = check_definite_initialization(&bad_body).unwrap_err();
        assert!(matches!(err, MoveError::UseOfUninitialized { .. }));
    }

    #[test]
    fn move_tracking_rejects_use_after_move() {
        let mut builder = MirBuilder::new(SymbolicType::Unit);
        let arg = builder.add_arg(sample_nominal(), None);
        let temp1 = builder.add_temp(sample_nominal(), None);
        let temp2 = builder.add_temp(sample_nominal(), None);

        // temp1 = move arg
        builder.push_assign(
            Place::from_local(temp1),
            Rvalue::Use(Operand::Move(Place::from_local(arg))),
            None,
        );
        // temp2 = move arg (second move -> error!)
        builder.push_assign(
            Place::from_local(temp2),
            Rvalue::Use(Operand::Move(Place::from_local(arg))),
            None,
        );
        builder.terminate_return(None);

        let body = builder.finish();
        let err = check_definite_initialization(&body).unwrap_err();
        assert!(matches!(err, MoveError::UseOfMovedValue { .. }));
    }

    #[test]
    fn borrow_checker_rejects_mutable_and_shared_conflicts() {
        let mut builder = MirBuilder::new(SymbolicType::Unit);
        let arg = builder.add_arg(SymbolicType::I32, None);
        let r1 = builder.add_temp(
            SymbolicType::Reference {
                mutability: FrontendMutability::Shared,
                lifetime: SymbolicLifetime::Static,
                pointee: Box::new(SymbolicType::I32),
            },
            None,
        );
        let r2 = builder.add_temp(
            SymbolicType::Reference {
                mutability: FrontendMutability::Mutable,
                lifetime: SymbolicLifetime::Static,
                pointee: Box::new(SymbolicType::I32),
            },
            None,
        );

        // r1 = &arg
        builder.push_assign(
            Place::from_local(r1),
            Rvalue::Ref(BorrowKind::Shared, Place::from_local(arg)),
            None,
        );
        // r2 = &mut arg (conflict!)
        builder.push_assign(
            Place::from_local(r2),
            Rvalue::Ref(BorrowKind::Mutable, Place::from_local(arg)),
            None,
        );
        builder.terminate_return(None);

        let body = builder.finish();
        let err = check_borrows(&body).unwrap_err();
        assert!(matches!(
            *err,
            BorrowError::CannotBorrowMutablyWhileShared { .. }
        ));
    }

    #[test]
    fn call_graph_detects_infinite_generic_expansion() {
        let cyclic_expanding = vec![CallEdge {
            caller: "recursive_fn".into(),
            callee: "recursive_fn".into(),
            type_arguments: vec![SymbolicType::Slice(Box::new(SymbolicType::Slice(
                Box::new(SymbolicType::Slice(Box::new(SymbolicType::I32))),
            )))],
            span: None,
        }];

        let err = check_call_graph_recursion(&cyclic_expanding).unwrap_err();
        assert!(matches!(
            *err,
            CallGraphError::InfiniteGenericExpansion { .. }
        ));
    }
}
