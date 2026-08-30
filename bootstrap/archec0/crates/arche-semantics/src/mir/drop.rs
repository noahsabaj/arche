//! Drop elaboration and cleanup ladders for M27-C3.

use arche_frontend::SymbolicType;

use crate::mir::def::{LocalId, LocalKind, MirBody, Place, StatementKind, TerminatorKind};

/// Elaborates drops in a MIR body, inserting drop statements and cleanup blocks.
pub fn elaborate_drops(body: &mut MirBody) {
    if body.basic_blocks.is_empty() {
        return;
    }

    // Identify locals that require drop (non-Copy nominals or aggregates).
    let mut droppable_locals = Vec::new();
    for (idx, decl) in body.locals.iter().enumerate() {
        let local_id = LocalId(idx as u32);
        if decl.kind == LocalKind::DropFlag || decl.kind == LocalKind::ReturnPlace {
            continue;
        }
        // In full pipeline, check type has Drop or is non-Copy aggregate
        if matches!(decl.ty, SymbolicType::NominalPath { .. }) {
            droppable_locals.push(local_id);
        }
    }

    // For return blocks, insert drops for all live droppable locals in reverse allocation order.
    for bb in &mut body.basic_blocks {
        if let Some(terminator) = &bb.terminator {
            if matches!(terminator.kind, TerminatorKind::Return) {
                let mut drop_stmts = Vec::new();
                for &local in droppable_locals.iter().rev() {
                    drop_stmts.push(crate::mir::def::Statement {
                        kind: StatementKind::Drop(Place::from_local(local)),
                        span: terminator.span,
                    });
                }
                drop_stmts.append(&mut bb.statements);
                bb.statements = drop_stmts;
            }
        }
    }
}
