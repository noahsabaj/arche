//! MIR lowering from checked bodies for M27-C3.

use std::collections::BTreeMap;

use arche_frontend::{Span, SymbolicType};

use crate::mir::def::{
    BasicBlockId, LocalId, LocalKind, MirBody, Mutability, Operand, Place, Rvalue, StatementKind,
    TerminatorKind,
};

/// Builder context for lowering expressions and statements into a MIR body.
pub struct MirBuilder {
    body: MirBody,
    current_block: BasicBlockId,
}

impl MirBuilder {
    /// Creates a new MIR builder for a function returning the given type.
    #[must_use]
    pub fn new(return_type: SymbolicType) -> Self {
        let mut body = MirBody::new(return_type);
        let entry_block = body.alloc_basic_block(false);
        Self {
            body,
            current_block: entry_block,
        }
    }

    /// Allocates an argument local.
    pub fn add_arg(&mut self, ty: SymbolicType, span: Option<Span>) -> LocalId {
        self.body
            .alloc_local(LocalKind::Arg, ty, Mutability::Immutable, span)
    }

    /// Allocates a user-declared local variable.
    pub fn add_user_var(
        &mut self,
        ty: SymbolicType,
        mutability: Mutability,
        span: Option<Span>,
    ) -> LocalId {
        self.body
            .alloc_local(LocalKind::UserVar, ty, mutability, span)
    }

    /// Allocates a temporary variable.
    pub fn add_temp(&mut self, ty: SymbolicType, span: Option<Span>) -> LocalId {
        self.body
            .alloc_local(LocalKind::Temp, ty, Mutability::Mutable, span)
    }

    /// Returns the current basic block.
    #[must_use]
    pub fn current_block(&self) -> BasicBlockId {
        self.current_block
    }

    /// Sets the current basic block.
    pub fn set_current_block(&mut self, block: BasicBlockId) {
        self.current_block = block;
    }

    /// Pushes an assignment statement to the current block.
    pub fn push_assign(&mut self, place: Place, rvalue: Rvalue, span: Option<Span>) {
        self.body.push_statement(
            self.current_block,
            StatementKind::Assign(place, Box::new(rvalue)),
            span,
        );
    }

    /// Pushes a StorageLive statement to the current block.
    pub fn push_storage_live(&mut self, local: LocalId, span: Option<Span>) {
        self.body
            .push_statement(self.current_block, StatementKind::StorageLive(local), span);
    }

    /// Pushes a StorageDead statement to the current block.
    pub fn push_storage_dead(&mut self, local: LocalId, span: Option<Span>) {
        self.body
            .push_statement(self.current_block, StatementKind::StorageDead(local), span);
    }

    /// Terminates the current block with a Goto.
    pub fn terminate_goto(&mut self, target: BasicBlockId, span: Option<Span>) {
        self.body
            .set_terminator(self.current_block, TerminatorKind::Goto { target }, span);
    }

    /// Terminates the current block with a Return.
    pub fn terminate_return(&mut self, span: Option<Span>) {
        self.body
            .set_terminator(self.current_block, TerminatorKind::Return, span);
    }

    /// Terminates the current block with a SwitchInt.
    pub fn terminate_switch_int(
        &mut self,
        discr: Operand,
        targets: BTreeMap<u128, BasicBlockId>,
        otherwise: BasicBlockId,
        span: Option<Span>,
    ) {
        self.body.set_terminator(
            self.current_block,
            TerminatorKind::SwitchInt {
                discr,
                targets,
                otherwise,
            },
            span,
        );
    }

    /// Finishes building and returns the completed MIR body.
    #[must_use]
    pub fn finish(self) -> MirBody {
        self.body
    }
}
