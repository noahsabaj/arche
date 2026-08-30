//! Place path prefix trie, definite initialization, and move tracking for M27-C3.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use arche_frontend::Span;

use crate::mir::def::{
    BasicBlockId, LocalId, MirBody, Operand, Place, ProjectionElem, Rvalue, StatementKind,
    TerminatorKind,
};

/// Initialization state of a place or sub-path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InitState {
    /// Fully uninitialized (storage allocated but no value written).
    Uninitialized,
    /// Fully initialized.
    Initialized,
    /// Partially moved into or initialized at sub-projections.
    PartiallyMoved(BTreeMap<ProjectionElem, PathTrie>),
}

/// A hierarchical prefix trie tracking initialization and move states of a place.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathTrie {
    state: InitState,
}

impl Default for PathTrie {
    fn default() -> Self {
        Self {
            state: InitState::Uninitialized,
        }
    }
}

/// Error indicating an attempt to move out of an uninitialized path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UninitializedPathError;

/// Move or initialization tracking error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MoveError {
    UseOfUninitialized { place: Place, span: Option<Span> },
    UseOfMovedValue { place: Place, span: Option<Span> },
    CannotMoveOutOfBorrowedContext { place: Place, span: Option<Span> },
}

impl fmt::Display for MoveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UseOfUninitialized { place, .. } => {
                write!(
                    formatter,
                    "use of possibly uninitialized variable {:?}",
                    place
                )
            }
            Self::UseOfMovedValue { place, .. } => {
                write!(formatter, "use of moved value {:?}", place)
            }
            Self::CannotMoveOutOfBorrowedContext { place, .. } => {
                write!(formatter, "cannot move out of borrowed context {:?}", place)
            }
        }
    }
}

impl std::error::Error for MoveError {}

impl PathTrie {
    /// Constructs a fully uninitialized path trie.
    #[must_use]
    pub fn uninitialized() -> Self {
        Self {
            state: InitState::Uninitialized,
        }
    }

    /// Constructs a fully initialized path trie.
    #[must_use]
    pub fn initialized() -> Self {
        Self {
            state: InitState::Initialized,
        }
    }

    /// Checks if the path (or all its subpaths) is fully initialized.
    #[must_use]
    pub fn is_fully_initialized(&self, projection: &[ProjectionElem]) -> bool {
        if projection.is_empty() {
            return matches!(self.state, InitState::Initialized);
        }
        match &self.state {
            InitState::Uninitialized => false,
            InitState::Initialized => true,
            InitState::PartiallyMoved(children) => {
                if let Some(child) = children.get(&projection[0]) {
                    child.is_fully_initialized(&projection[1..])
                } else {
                    true
                }
            }
        }
    }

    /// Checks if the path is at least partially initialized.
    #[must_use]
    pub fn is_available(&self, projection: &[ProjectionElem]) -> bool {
        if projection.is_empty() {
            return !matches!(self.state, InitState::Uninitialized);
        }
        match &self.state {
            InitState::Uninitialized => false,
            InitState::Initialized => true,
            InitState::PartiallyMoved(children) => {
                if let Some(child) = children.get(&projection[0]) {
                    child.is_available(&projection[1..])
                } else {
                    true
                }
            }
        }
    }

    /// Marks the given projection as fully initialized.
    pub fn mark_init(&mut self, projection: &[ProjectionElem]) {
        if projection.is_empty() {
            self.state = InitState::Initialized;
            return;
        }
        match &mut self.state {
            InitState::Uninitialized => {
                let mut children = BTreeMap::new();
                let mut child = PathTrie::uninitialized();
                child.mark_init(&projection[1..]);
                children.insert(projection[0].clone(), child);
                self.state = InitState::PartiallyMoved(children);
            }
            InitState::Initialized => {
                // If it was already fully initialized, sub-initializing keeps it initialized.
            }
            InitState::PartiallyMoved(children) => {
                let child = children
                    .entry(projection[0].clone())
                    .or_insert_with(PathTrie::uninitialized);
                child.mark_init(&projection[1..]);
            }
        }
    }

    /// Marks the given projection as moved.
    pub fn mark_move(
        &mut self,
        projection: &[ProjectionElem],
    ) -> Result<(), UninitializedPathError> {
        if projection.is_empty() {
            if self.state == InitState::Uninitialized {
                return Err(UninitializedPathError);
            }
            self.state = InitState::Uninitialized;
            return Ok(());
        }
        match &mut self.state {
            InitState::Uninitialized => Err(UninitializedPathError),
            InitState::Initialized => {
                let mut children = BTreeMap::new();
                let mut child = PathTrie::initialized();
                child.mark_move(&projection[1..])?;
                children.insert(projection[0].clone(), child);
                self.state = InitState::PartiallyMoved(children);
                Ok(())
            }
            InitState::PartiallyMoved(children) => {
                let child = children
                    .entry(projection[0].clone())
                    .or_insert_with(PathTrie::initialized);
                child.mark_move(&projection[1..])
            }
        }
    }

    /// Computes the intersection of initialization state with another trie at a CFG join.
    pub fn intersect(&mut self, other: &Self) {
        match (&self.state, &other.state) {
            (InitState::Uninitialized, _) | (_, InitState::Uninitialized) => {
                self.state = InitState::Uninitialized;
            }
            (InitState::Initialized, InitState::Initialized) => {
                self.state = InitState::Initialized;
            }
            (InitState::PartiallyMoved(c1), InitState::PartiallyMoved(c2)) => {
                let mut merged = c1.clone();
                for (elem, child) in c2 {
                    if let Some(existing) = merged.get_mut(elem) {
                        existing.intersect(child);
                    } else {
                        merged.insert(elem.clone(), child.clone());
                    }
                }
                self.state = InitState::PartiallyMoved(merged);
            }
            (InitState::Initialized, InitState::PartiallyMoved(c2)) => {
                self.state = InitState::PartiallyMoved(c2.clone());
            }
            (InitState::PartiallyMoved(_), InitState::Initialized) => {
                // Keep partial move state (intersection with fully initialized).
            }
        }
    }
}

/// A snapshot of initialization state for all locals at a point in the CFG.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentInitState {
    pub locals: BTreeMap<LocalId, PathTrie>,
}

impl EnvironmentInitState {
    /// Creates an initial environment where arguments are initialized and locals uninitialized.
    #[must_use]
    pub fn initial_for_body(body: &MirBody) -> Self {
        let mut locals = BTreeMap::new();
        for (idx, decl) in body.locals.iter().enumerate() {
            let id = LocalId(idx as u32);
            let trie = match decl.kind {
                crate::mir::def::LocalKind::Arg => PathTrie::initialized(),
                _ => PathTrie::uninitialized(),
            };
            locals.insert(id, trie);
        }
        Self { locals }
    }

    /// Intersects this state with another incoming state at a CFG block entry.
    pub fn intersect(&mut self, other: &Self) {
        for (id, trie) in &mut self.locals {
            if let Some(other_trie) = other.locals.get(id) {
                trie.intersect(other_trie);
            } else {
                *trie = PathTrie::uninitialized();
            }
        }
    }

    /// Reads a place operand, ensuring it is initialized and recording a move if needed.
    pub fn read_operand(&mut self, operand: &Operand, span: Option<Span>) -> Result<(), MoveError> {
        match operand {
            Operand::Copy(place) => {
                if let Some(trie) = self.locals.get(&place.local) {
                    if !trie.is_fully_initialized(&place.projection) {
                        return Err(MoveError::UseOfUninitialized {
                            place: place.clone(),
                            span,
                        });
                    }
                }
                Ok(())
            }
            Operand::Move(place) => {
                if let Some(trie) = self.locals.get_mut(&place.local) {
                    if !trie.is_fully_initialized(&place.projection) {
                        return Err(MoveError::UseOfMovedValue {
                            place: place.clone(),
                            span,
                        });
                    }
                    if trie.mark_move(&place.projection).is_err() {
                        return Err(MoveError::UseOfMovedValue {
                            place: place.clone(),
                            span,
                        });
                    }
                }
                Ok(())
            }
            Operand::Constant(_) => Ok(()),
        }
    }

    /// Writes to a place, marking it initialized.
    pub fn write_place(&mut self, place: &Place) {
        if let Some(trie) = self.locals.get_mut(&place.local) {
            trie.mark_init(&place.projection);
        }
    }
}

/// Runs definite initialization and move checking across all basic blocks in a MIR body.
pub fn check_definite_initialization(body: &MirBody) -> Result<(), MoveError> {
    if body.basic_blocks.is_empty() {
        return Ok(());
    }

    let mut block_entry_states: BTreeMap<BasicBlockId, EnvironmentInitState> = BTreeMap::new();
    let mut worklist: Vec<BasicBlockId> = Vec::new();

    let entry_block = BasicBlockId(0);
    block_entry_states.insert(entry_block, EnvironmentInitState::initial_for_body(body));
    worklist.push(entry_block);

    let mut visited = BTreeSet::new();

    while let Some(block_id) = worklist.pop() {
        visited.insert(block_id);
        let Some(block) = body.block(block_id) else {
            continue;
        };

        let mut current_state = block_entry_states
            .get(&block_id)
            .cloned()
            .unwrap_or_else(|| EnvironmentInitState::initial_for_body(body));

        for statement in &block.statements {
            match &statement.kind {
                StatementKind::Assign(place, rvalue) => {
                    match rvalue.as_ref() {
                        Rvalue::Use(operand)
                        | Rvalue::UnaryOp(_, operand)
                        | Rvalue::Cast { operand, .. } => {
                            current_state.read_operand(operand, statement.span)?;
                        }
                        Rvalue::BinaryOp(_, left, right) => {
                            current_state.read_operand(left, statement.span)?;
                            current_state.read_operand(right, statement.span)?;
                        }
                        Rvalue::Ref(_, place) => {
                            if let Some(trie) = current_state.locals.get(&place.local) {
                                if !trie.is_fully_initialized(&place.projection) {
                                    return Err(MoveError::UseOfUninitialized {
                                        place: place.clone(),
                                        span: statement.span,
                                    });
                                }
                            }
                        }
                        Rvalue::Aggregate { operands, .. } => {
                            for op in operands {
                                current_state.read_operand(op, statement.span)?;
                            }
                        }
                        Rvalue::CheckedIndex { proof } => {
                            if let Some(trie) = current_state.locals.get(&proof.container.local) {
                                if !trie.is_fully_initialized(&proof.container.projection) {
                                    return Err(MoveError::UseOfUninitialized {
                                        place: proof.container.clone(),
                                        span: statement.span,
                                    });
                                }
                            }
                        }
                        Rvalue::Discriminant(place) => {
                            if let Some(trie) = current_state.locals.get(&place.local) {
                                if !trie.is_fully_initialized(&place.projection) {
                                    return Err(MoveError::UseOfUninitialized {
                                        place: place.clone(),
                                        span: statement.span,
                                    });
                                }
                            }
                        }
                    }
                    current_state.write_place(place);
                }
                StatementKind::StorageLive(local) => {
                    if let Some(trie) = current_state.locals.get_mut(local) {
                        *trie = PathTrie::uninitialized();
                    }
                }
                StatementKind::StorageDead(local) => {
                    if let Some(trie) = current_state.locals.get_mut(local) {
                        *trie = PathTrie::uninitialized();
                    }
                }
                StatementKind::Drop(place) => {
                    if let Some(trie) = current_state.locals.get_mut(&place.local) {
                        let _ = trie.mark_move(&place.projection);
                    }
                }
                StatementKind::Nop => {}
            }
        }

        if let Some(terminator) = &block.terminator {
            let successors: Vec<BasicBlockId> = match &terminator.kind {
                TerminatorKind::Goto { target } => vec![*target],
                TerminatorKind::SwitchInt {
                    discr,
                    targets,
                    otherwise,
                } => {
                    current_state.read_operand(discr, terminator.span)?;
                    let mut succs: Vec<BasicBlockId> = targets.values().copied().collect();
                    succs.push(*otherwise);
                    succs
                }
                TerminatorKind::Call {
                    callee,
                    args,
                    destination,
                    target,
                    cleanup,
                } => {
                    current_state.read_operand(callee, terminator.span)?;
                    for arg in args {
                        current_state.read_operand(arg, terminator.span)?;
                    }
                    current_state.write_place(destination);
                    let mut succs = Vec::new();
                    if let Some(t) = target {
                        succs.push(*t);
                    }
                    if let Some(c) = cleanup {
                        succs.push(*c);
                    }
                    succs
                }
                TerminatorKind::Return => {
                    // Check that return place _0 is initialized if not unit
                    if body.return_type != arche_frontend::SymbolicType::Unit {
                        let ret_place = Place::return_place();
                        if let Some(trie) = current_state.locals.get(&ret_place.local) {
                            if !trie.is_fully_initialized(&[]) {
                                return Err(MoveError::UseOfUninitialized {
                                    place: ret_place,
                                    span: terminator.span,
                                });
                            }
                        }
                    }
                    Vec::new()
                }
                TerminatorKind::Abort | TerminatorKind::Resume => Vec::new(),
            };

            for succ in successors {
                if let Some(existing) = block_entry_states.get_mut(&succ) {
                    existing.intersect(&current_state);
                } else {
                    block_entry_states.insert(succ, current_state.clone());
                    worklist.push(succ);
                }
            }
        }
    }

    Ok(())
}
