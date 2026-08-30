//! Non-Lexical Lifetimes (NLL) region constraint graph and borrow checker for M27-C3.

use std::collections::BTreeMap;
use std::fmt;

use arche_frontend::Span;

use crate::mir::def::{BorrowKind, MirBody, Operand, Place, Rvalue, StatementKind, TerminatorKind};

/// Unique identifier for an active loan.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LoanId(pub u32);

/// An active loan in a MIR body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveLoan {
    pub id: LoanId,
    pub kind: BorrowKind,
    pub place: Place,
    pub span: Option<Span>,
}

/// Borrow checking error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BorrowError {
    CannotBorrowMutablyWhileShared {
        place: Place,
        loan_span: Option<Span>,
        conflict_span: Option<Span>,
    },
    CannotBorrowMutablyMultipleTimes {
        place: Place,
        first_loan_span: Option<Span>,
        second_loan_span: Option<Span>,
    },
    CannotMutateWhileBorrowed {
        place: Place,
        loan_span: Option<Span>,
        mutation_span: Option<Span>,
    },
    CannotMoveWhileBorrowed {
        place: Place,
        loan_span: Option<Span>,
        move_span: Option<Span>,
    },
}

impl fmt::Display for BorrowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CannotBorrowMutablyWhileShared { place, .. } => {
                write!(
                    formatter,
                    "cannot borrow {:?} as mutable because it is also borrowed as immutable",
                    place
                )
            }
            Self::CannotBorrowMutablyMultipleTimes { place, .. } => {
                write!(
                    formatter,
                    "cannot borrow {:?} as mutable more than once at a time",
                    place
                )
            }
            Self::CannotMutateWhileBorrowed { place, .. } => {
                write!(
                    formatter,
                    "cannot assign to {:?} because it is borrowed",
                    place
                )
            }
            Self::CannotMoveWhileBorrowed { place, .. } => {
                write!(
                    formatter,
                    "cannot move out of {:?} because it is borrowed",
                    place
                )
            }
        }
    }
}

impl std::error::Error for BorrowError {}

/// Checks whether two places overlap or one is a prefix of the other.
#[must_use]
pub fn places_conflict(a: &Place, b: &Place) -> bool {
    if a.local != b.local {
        return false;
    }
    let min_len = a.projection.len().min(b.projection.len());
    for i in 0..min_len {
        if a.projection[i] != b.projection[i] {
            return false;
        }
    }
    true
}

/// Runs Non-Lexical Lifetimes (NLL) borrow checking on a MIR body.
pub fn check_borrows(body: &MirBody) -> Result<(), Box<BorrowError>> {
    if body.basic_blocks.is_empty() {
        return Ok(());
    }

    let mut active_loans: BTreeMap<LoanId, ActiveLoan> = BTreeMap::new();
    let mut next_loan_id = 0u32;

    for block in &body.basic_blocks {
        for statement in &block.statements {
            match &statement.kind {
                StatementKind::Assign(place, rvalue) => {
                    // Check if assigning to a place conflicts with an active loan
                    for loan in active_loans.values() {
                        if places_conflict(place, &loan.place) {
                            return Err(Box::new(BorrowError::CannotMutateWhileBorrowed {
                                place: place.clone(),
                                loan_span: loan.span,
                                mutation_span: statement.span,
                            }));
                        }
                    }

                    // Process borrow expressions in rvalue
                    if let Rvalue::Ref(kind, borrowed_place) = rvalue.as_ref() {
                        for loan in active_loans.values() {
                            if places_conflict(borrowed_place, &loan.place) {
                                match (loan.kind, kind) {
                                    (BorrowKind::Shared, BorrowKind::Mutable) => {
                                        return Err(Box::new(
                                            BorrowError::CannotBorrowMutablyWhileShared {
                                                place: borrowed_place.clone(),
                                                loan_span: loan.span,
                                                conflict_span: statement.span,
                                            },
                                        ));
                                    }
                                    (BorrowKind::Mutable, BorrowKind::Mutable) => {
                                        return Err(Box::new(
                                            BorrowError::CannotBorrowMutablyMultipleTimes {
                                                place: borrowed_place.clone(),
                                                first_loan_span: loan.span,
                                                second_loan_span: statement.span,
                                            },
                                        ));
                                    }
                                    (BorrowKind::Mutable, BorrowKind::Shared) => {
                                        return Err(Box::new(
                                            BorrowError::CannotBorrowMutablyWhileShared {
                                                place: borrowed_place.clone(),
                                                loan_span: loan.span,
                                                conflict_span: statement.span,
                                            },
                                        ));
                                    }
                                    (BorrowKind::Shared, BorrowKind::Shared) => {
                                        // Multiple shared borrows are allowed
                                    }
                                }
                            }
                        }

                        let loan_id = LoanId(next_loan_id);
                        next_loan_id += 1;
                        active_loans.insert(
                            loan_id,
                            ActiveLoan {
                                id: loan_id,
                                kind: *kind,
                                place: borrowed_place.clone(),
                                span: statement.span,
                            },
                        );
                    }

                    // Process move operands in rvalue
                    if let Rvalue::Use(Operand::Move(moved_place)) = rvalue.as_ref() {
                        for loan in active_loans.values() {
                            if places_conflict(moved_place, &loan.place) {
                                return Err(Box::new(BorrowError::CannotMoveWhileBorrowed {
                                    place: moved_place.clone(),
                                    loan_span: loan.span,
                                    move_span: statement.span,
                                }));
                            }
                        }
                    }
                }
                StatementKind::StorageDead(local) => {
                    // Invalidate loans associated with this local
                    active_loans.retain(|_, loan| loan.place.local != *local);
                }
                StatementKind::Drop(place) => {
                    for loan in active_loans.values() {
                        if places_conflict(place, &loan.place) {
                            return Err(Box::new(BorrowError::CannotMoveWhileBorrowed {
                                place: place.clone(),
                                loan_span: loan.span,
                                move_span: statement.span,
                            }));
                        }
                    }
                }
                StatementKind::StorageLive(_) | StatementKind::Nop => {}
            }
        }

        if let Some(terminator) = &block.terminator {
            if let TerminatorKind::Call { args, .. } = &terminator.kind {
                for arg in args {
                    if let Operand::Move(moved_place) = arg {
                        for loan in active_loans.values() {
                            if places_conflict(moved_place, &loan.place) {
                                return Err(Box::new(BorrowError::CannotMoveWhileBorrowed {
                                    place: moved_place.clone(),
                                    loan_span: loan.span,
                                    move_span: terminator.span,
                                }));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
