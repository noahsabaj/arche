//! Branded, read-only C2 handoff and terminal-result model.
//!
//! Session indexes are deliberately ephemeral. Their private owner brand
//! prevents a dense offset from being reused with another C2 session, and the
//! row views returned from the tables cannot outlive the owning result.

#![allow(
    dead_code,
    reason = "crate-private terminal constructors are reserved for C2 checker modules"
)]

use std::collections::BTreeMap;
use std::sync::Arc;

use arche_foundation::identity::PackageId;
use arche_frontend::{
    Diagnostic, FrontendOutput, HirBodyId, HirItemId, ResolvedSymbolicWorkspaceHir,
    SemanticBodyKind, SourceDatabase, Span, TargetId, WorkspaceInventorySkeleton,
};

use crate::body_check::C2BodyTable;
use crate::declaration_check::CheckedDeclarationFacts;
use crate::diagnostic::{NonEmptySemanticDiagnostics, ScopedPackageBytes};

/// Dense item index owned by one C2 checking session.
///
/// The offset is useful for deterministic debugging, but is neither a stable
/// semantic identity nor sufficient to access a row without the owner brand.
#[derive(Clone, Debug)]
pub struct SessionItemIndex {
    owner: Arc<SessionOwner>,
    offset: u64,
}

impl SessionItemIndex {
    /// Returns the session-local dense offset.
    pub const fn offset(&self) -> u64 {
        self.offset
    }
}

/// Dense checked-type index owned by one C2 checking session.
#[derive(Clone, Debug)]
pub struct SessionTypeIndex {
    owner: Arc<SessionOwner>,
    offset: u64,
}

impl SessionTypeIndex {
    /// Returns the session-local dense offset.
    pub const fn offset(&self) -> u64 {
        self.offset
    }
}

/// Cloneable, opaque proof that two retained fact tables belong to the exact
/// same C2 checking session.
///
/// The brand deliberately exposes neither the session owner nor a serializable
/// value. Equality is allocation identity, so an empty fact table retains the
/// same provenance strength as a table containing owner-branded row handles.
#[derive(Clone, Debug)]
pub(crate) struct SessionBrand(Arc<SessionOwner>);

impl SessionBrand {
    /// Returns whether both brands were minted by the exact same handoff.
    pub(crate) fn same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Returns whether this session owns the supplied item handle.
    pub(crate) fn owns_item(&self, item: &SessionItemIndex) -> bool {
        Arc::ptr_eq(&self.0, &item.owner)
    }

    /// Returns whether this session owns the supplied checked-type handle.
    pub(crate) fn owns_type(&self, ty: &SessionTypeIndex) -> bool {
        Arc::ptr_eq(&self.0, &ty.owner)
    }
}

/// Borrow-tied view of one retained HIR item row.
#[derive(Clone, Copy, Debug)]
pub struct SessionItemView<'a> {
    item: &'a HirItemId,
}

impl SessionItemView<'_> {
    /// Returns the retained C1 item ID in this row.
    pub const fn id(self) -> HirItemId {
        *self.item
    }
}

/// One opaque canonical dependency on a future CTFE result.
///
/// These bytes name a pre-result obligation only. C2 neither constructs nor
/// stores any later gate's stable root identity.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NeedsCtfeObligation(Box<[u8]>);

impl NeedsCtfeObligation {
    pub(crate) fn from_canonical_bytes(bytes: Vec<u8>) -> Result<Self, CanonicalDependencyError> {
        if bytes.is_empty() {
            return Err(CanonicalDependencyError::EmptyNeedsCtfeObligation);
        }
        Ok(Self(bytes.into_boxed_slice()))
    }

    /// Returns the complete canonical pre-result obligation bytes.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Nonempty canonical set of CTFE obligations retained by C2.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NeedsCtfeObligations(Box<[NeedsCtfeObligation]>);

impl NeedsCtfeObligations {
    pub(crate) fn from_unsorted(
        mut obligations: Vec<NeedsCtfeObligation>,
    ) -> Result<Self, CanonicalDependencyError> {
        if obligations.is_empty() {
            return Err(CanonicalDependencyError::EmptyNeedsCtfeSet);
        }
        obligations.sort();
        obligations.dedup();
        Ok(Self(obligations.into_boxed_slice()))
    }

    /// Returns the sorted, exact-deduplicated, nonempty obligation set.
    pub fn as_slice(&self) -> &[NeedsCtfeObligation] {
        &self.0
    }

    /// Returns the number of distinct obligations.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns false; construction enforces the nonempty invariant.
    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// One opaque canonical dependency owned by the later C4 gate.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PendingC4Dependency(Box<[u8]>);

impl PendingC4Dependency {
    pub(crate) fn from_canonical_bytes(bytes: Vec<u8>) -> Result<Self, CanonicalDependencyError> {
        if bytes.is_empty() {
            return Err(CanonicalDependencyError::EmptyPendingC4Dependency);
        }
        Ok(Self(bytes.into_boxed_slice()))
    }

    /// Returns the complete canonical dependency bytes.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Canonical set of independent C4 dependencies retained by C2.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PendingC4Dependencies(Box<[PendingC4Dependency]>);

impl PendingC4Dependencies {
    pub(crate) fn from_unsorted(mut dependencies: Vec<PendingC4Dependency>) -> Self {
        dependencies.sort();
        dependencies.dedup();
        Self(dependencies.into_boxed_slice())
    }

    /// Returns the sorted, exact-deduplicated dependency set.
    pub fn as_slice(&self) -> &[PendingC4Dependency] {
        &self.0
    }

    /// Returns the number of distinct dependencies.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether no later C4 dependency remains.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// C2 target or checked-node resolution state.
///
/// There is intentionally no `PendingC2` variant. A branded result can be
/// complete at C2 or retain a nonempty canonical CTFE obligation set; C4
/// dependencies are recorded independently alongside this value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum C2Resolution {
    /// C2 completed all work required before C3/C4 for this row.
    Complete,
    /// A const-dependent decision must be replayed after successful CTFE.
    NeedsCtfe(NeedsCtfeObligations),
}

/// Borrow-tied view of one checked-type row.
#[derive(Clone, Copy, Debug)]
pub struct SessionTypeView<'a> {
    row: &'a CheckedTypeRow,
}

impl<'a> SessionTypeView<'a> {
    /// Returns the exact retained semantic producer whose recursive type state
    /// this row records.
    pub const fn producer(self) -> C2TypeProducer {
        self.row.producer
    }

    /// Returns the row's C2/CTFE resolution state, tied to the owning table.
    pub const fn resolution(self) -> &'a C2Resolution {
        &self.row.resolution
    }

    /// Returns the orthogonal C4 dependency set.
    pub const fn pending_c4(self) -> &'a PendingC4Dependencies {
        &self.row.pending_c4
    }
}

/// Error raised while constructing deterministic C2 session indexes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionIndexError {
    diagnostic: Diagnostic,
}

impl SessionIndexError {
    /// Returns the exact diagnostic describing the invalid C1 row sequence.
    pub const fn diagnostic(&self) -> &Diagnostic {
        &self.diagnostic
    }
}

/// Failed pre-checking handoff that retains the complete C1 authority.
#[derive(Debug)]
pub struct SessionIndexFailure {
    frontend: FrontendOutput,
    error: SessionIndexError,
}

impl SessionIndexFailure {
    /// Returns the index construction error.
    pub const fn error(&self) -> &SessionIndexError {
        &self.error
    }

    /// Returns the retained C1 authority.
    pub const fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }
}

/// Deterministic dense indexes derived from one retained C1 result.
#[derive(Debug)]
pub struct SessionIndexTables {
    owner: Arc<SessionOwner>,
    item_rows: Vec<HirItemId>,
    item_lookup: BTreeMap<HirItemId, u64>,
    checked_type_rows: Vec<CheckedTypeRow>,
    checked_type_lookup: BTreeMap<C2TypeProducer, u64>,
}

#[derive(Debug)]
struct SessionOwner;

#[derive(Debug)]
struct CheckedTypeRow {
    producer: C2TypeProducer,
    package: PackageId,
    target: TargetId,
    resolution: C2Resolution,
    pending_c4: PendingC4Dependencies,
}

/// Canonical, kind-discriminated producer of one aggregate checked-type row.
///
/// IDs are meaningful only with the owner brand on the containing session
/// table. The enum discriminant prevents a declaration and body with the same
/// numeric payload from aliasing.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum C2TypeProducer {
    /// Recursive type state of one complete checked declaration/signature.
    Declaration(HirItemId),
    /// Recursive type state of one complete checked body.
    Body(HirBodyId),
}

impl SessionIndexTables {
    fn empty() -> Self {
        Self {
            owner: Arc::new(SessionOwner),
            item_rows: Vec::new(),
            item_lookup: BTreeMap::new(),
            checked_type_rows: Vec::new(),
            checked_type_lookup: BTreeMap::new(),
        }
    }

    fn from_hir(hir: &ResolvedSymbolicWorkspaceHir) -> Result<Self, SessionIndexError> {
        let mut tables = Self::empty();
        for package in &hir.packages {
            for target in &package.targets {
                for item in &target.items {
                    let offset =
                        u64::try_from(tables.item_rows.len()).map_err(|_| SessionIndexError {
                            diagnostic: Diagnostic::path(
                                "IDENTITY001",
                                "C2 session item index space is exhausted",
                            ),
                        })?;
                    if let Some(previous) = tables.item_lookup.insert(item.id, offset) {
                        return Err(SessionIndexError {
                            diagnostic: Diagnostic::at(
                                "IDENTITY001",
                                item.span,
                                format!(
                                    "HIR item {} appears twice in the C2 session index (first at {})",
                                    item.id.0, previous
                                ),
                            ),
                        });
                    }
                    tables.item_rows.push(item.id);
                }
            }
        }
        Ok(tables)
    }

    /// Returns the number of retained C1 HIR item rows.
    pub fn item_count(&self) -> usize {
        self.item_rows.len()
    }

    /// Mints an opaque proof of this table's exact checking session.
    pub(crate) fn session_brand(&self) -> SessionBrand {
        SessionBrand(Arc::clone(&self.owner))
    }

    /// Returns the number of types that reached a branded C2 row.
    pub fn checked_type_count(&self) -> usize {
        self.checked_type_rows.len()
    }

    /// Looks up the owner-branded handle for a retained HIR item.
    pub fn item_index(&self, item: HirItemId) -> Option<SessionItemIndex> {
        self.item_lookup.get(&item).map(|&offset| SessionItemIndex {
            owner: Arc::clone(&self.owner),
            offset,
        })
    }

    /// Returns the owner-branded handle for a checked-type offset.
    pub fn checked_type_index(&self, offset: u64) -> Option<SessionTypeIndex> {
        usize::try_from(offset)
            .ok()
            .filter(|&offset| offset < self.checked_type_rows.len())
            .map(|_| SessionTypeIndex {
                owner: Arc::clone(&self.owner),
                offset,
            })
    }

    /// Looks up the owner-branded checked-type handle for an exact producer.
    pub fn checked_type_index_for(&self, producer: C2TypeProducer) -> Option<SessionTypeIndex> {
        self.checked_type_lookup
            .get(&producer)
            .copied()
            .map(|offset| SessionTypeIndex {
                owner: Arc::clone(&self.owner),
                offset,
            })
    }

    /// Resolves an item handle to a view tied to this table borrow.
    pub fn item(&self, index: &SessionItemIndex) -> Option<SessionItemView<'_>> {
        if !Arc::ptr_eq(&self.owner, &index.owner) {
            return None;
        }
        usize::try_from(index.offset)
            .ok()
            .and_then(|offset| self.item_rows.get(offset))
            .map(|item| SessionItemView { item })
    }

    /// Resolves a checked-type handle to a view tied to this table borrow.
    pub fn checked_type(&self, index: &SessionTypeIndex) -> Option<SessionTypeView<'_>> {
        if !Arc::ptr_eq(&self.owner, &index.owner) {
            return None;
        }
        usize::try_from(index.offset)
            .ok()
            .and_then(|offset| self.checked_type_rows.get(offset))
            .map(|row| SessionTypeView { row })
    }
}

/// Consuming ownership handoff from the complete C1 frontend result.
#[derive(Debug)]
pub struct C2Handoff {
    frontend: FrontendOutput,
    indexes: SessionIndexTables,
}

impl C2Handoff {
    /// Takes ownership of C1 and constructs deterministic, session-branded
    /// index tables. A pre-checking index failure retains that C1 authority.
    pub fn begin(frontend: FrontendOutput) -> Result<Self, Box<SessionIndexFailure>> {
        match SessionIndexTables::from_hir(frontend.hir()) {
            Ok(indexes) => Ok(Self { frontend, indexes }),
            Err(error) => Err(Box::new(SessionIndexFailure { frontend, error })),
        }
    }

    /// Returns the retained C1 authority.
    pub const fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }

    /// Returns the read-only session index tables.
    pub const fn indexes(&self) -> &SessionIndexTables {
        &self.indexes
    }

    /// Mints an opaque proof of this handoff's exact checking session.
    pub(crate) fn session_brand(&self) -> SessionBrand {
        self.indexes.session_brand()
    }

    /// Consumes the only complete declaration/body proof pair accepted by the
    /// C2 orchestrator and mints the terminal checked workspace. All terminal
    /// rows are independently reconstructed from those facts; callers cannot
    /// supply target gates or checked-type rows.
    pub(crate) fn aggregate_checked(
        mut self,
        declarations: CheckedDeclarationFacts,
        bodies: C2BodyTable,
    ) -> Result<C2CheckedWorkspace, Box<C2AggregationFailure>> {
        let model = match C2CheckedModel::aggregate(&self, &declarations, &bodies) {
            Ok(model) => model,
            Err(error) => {
                return Err(Box::new(C2AggregationFailure {
                    handoff: self,
                    declarations,
                    bodies,
                    error,
                }));
            }
        };
        self.indexes.checked_type_rows = model.checked_types;
        self.indexes.checked_type_lookup = self
            .indexes
            .checked_type_rows
            .iter()
            .enumerate()
            .map(|(offset, row)| {
                (
                    row.producer,
                    u64::try_from(offset).expect("validated checked-type count fits u64"),
                )
            })
            .collect();
        Ok(C2CheckedWorkspace {
            frontend: self.frontend,
            indexes: self.indexes,
            targets: model.targets,
            declarations,
            bodies,
        })
    }

    pub(crate) fn into_rejected(
        self,
        diagnostics: NonEmptySemanticDiagnostics,
    ) -> C2RejectedWorkspace {
        C2RejectedWorkspace {
            frontend: self.frontend,
            indexes: self.indexes,
            diagnostics,
        }
    }
}

/// One read-only target row in a branded C2 result.
#[derive(Clone, Copy, Debug)]
pub struct C2TargetView<'a> {
    row: &'a C2TargetRow,
}

impl<'a> C2TargetView<'a> {
    /// Returns the retained C1 package ID.
    pub const fn package(self) -> PackageId {
        self.row.package
    }

    /// Returns the canonical scoped-package bytes used for diagnostics.
    pub const fn package_scope(self) -> &'a ScopedPackageBytes {
        &self.row.package_scope
    }

    /// Returns the package-local target ID.
    pub const fn target(self) -> TargetId {
        self.row.target
    }

    /// Returns whether this target is complete or awaits CTFE.
    pub const fn resolution(self) -> &'a C2Resolution {
        &self.row.resolution
    }

    /// Returns the independent C4 dependency set.
    pub const fn pending_c4(self) -> &'a PendingC4Dependencies {
        &self.row.pending_c4
    }
}

/// Fully checked C2 result. It has no public constructor.
///
/// ```compile_fail
/// use arche_semantics::C2CheckedWorkspace;
/// let _ = C2CheckedWorkspace::new();
/// ```
#[derive(Debug)]
pub struct C2CheckedWorkspace {
    frontend: FrontendOutput,
    indexes: SessionIndexTables,
    targets: Vec<C2TargetRow>,
    declarations: CheckedDeclarationFacts,
    bodies: C2BodyTable,
}

impl C2CheckedWorkspace {
    /// Returns the retained C1 authority.
    pub const fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }

    /// Returns the branded, read-only session index tables.
    pub const fn indexes(&self) -> &SessionIndexTables {
        &self.indexes
    }

    /// Returns the number of checked target rows.
    pub fn target_count(&self) -> usize {
        self.targets.len()
    }

    /// Iterates checked targets in canonical package-scope/target order.
    pub fn targets(&self) -> impl ExactSizeIterator<Item = C2TargetView<'_>> + '_ {
        self.targets.iter().map(|row| C2TargetView { row })
    }

    /// Returns the complete checked declaration/signature facts consumed by
    /// terminal aggregation.
    pub const fn declarations(&self) -> &CheckedDeclarationFacts {
        &self.declarations
    }

    /// Returns the complete checked body facts consumed by terminal
    /// aggregation.
    pub const fn bodies(&self) -> &C2BodyTable {
        &self.bodies
    }
}

/// Rejected C2 result retaining C1, session indexes, and canonical diagnostics.
/// It has no public constructor.
///
/// ```compile_fail
/// use arche_semantics::C2RejectedWorkspace;
/// let _ = C2RejectedWorkspace::new();
/// ```
#[derive(Debug)]
pub struct C2RejectedWorkspace {
    frontend: FrontendOutput,
    indexes: SessionIndexTables,
    diagnostics: NonEmptySemanticDiagnostics,
}

impl C2RejectedWorkspace {
    /// Returns the retained C1 authority.
    pub const fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }

    /// Returns the branded session index tables owned by this rejection.
    pub const fn indexes(&self) -> &SessionIndexTables {
        &self.indexes
    }

    /// Returns the nonempty canonical diagnostic sequence.
    pub const fn diagnostics(&self) -> &NonEmptySemanticDiagnostics {
        &self.diagnostics
    }
}

/// Read-only retained frontend authority shared by all C2 wrappers.
pub trait RetainedFrontend {
    /// Returns the retained C1 frontend result.
    fn frontend(&self) -> &FrontendOutput;

    /// Returns the retained symbolic HIR.
    fn hir(&self) -> &ResolvedSymbolicWorkspaceHir {
        self.frontend().hir()
    }

    /// Returns the immutable source database.
    fn sources(&self) -> &Arc<SourceDatabase> {
        self.frontend().sources()
    }

    /// Returns the unverified C1 inventory skeleton.
    fn inventory(&self) -> &WorkspaceInventorySkeleton {
        self.frontend().inventory()
    }
}

impl RetainedFrontend for SessionIndexFailure {
    fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }
}

impl RetainedFrontend for C2Handoff {
    fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }
}

impl RetainedFrontend for C2CheckedWorkspace {
    fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }
}

impl RetainedFrontend for C2RejectedWorkspace {
    fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CanonicalDependencyError {
    EmptyNeedsCtfeObligation,
    EmptyNeedsCtfeSet,
    EmptyPendingC4Dependency,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum C2ModelError {
    EmptyPackageScope {
        package: PackageId,
    },
    DuplicateInventoryTarget {
        package: PackageId,
        target: TargetId,
    },
    UnexpectedTarget {
        package: PackageId,
        target: TargetId,
    },
    DuplicateExpectedTypeProducer(C2TypeProducer),
    ForeignDeclarationSession,
    ForeignBodySession,
    ForeignTypeProducerSession(C2TypeProducer),
    DeclarationItemWrongSession(u64),
    UnexpectedTypeProducer(C2TypeProducer),
    DuplicateTypeProducer(C2TypeProducer),
    MissingTypeProducer(C2TypeProducer),
    TypeProducerTargetMismatch(C2TypeProducer),
    DeclarationKindMismatch(HirItemId),
    BodyOwnerMismatch(HirBodyId),
    BodyKindMismatch(HirBodyId),
    BodySpanMismatch(HirBodyId),
    IncompleteTypeProducer(C2TypeProducer),
}

impl C2ModelError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::EmptyPackageScope { .. } => "empty-package-scope",
            Self::DuplicateInventoryTarget { .. } => "duplicate-inventory-target",
            Self::UnexpectedTarget { .. } => "unexpected-target",
            Self::DuplicateExpectedTypeProducer(_) => "duplicate-expected-type-producer",
            Self::ForeignDeclarationSession => "foreign-declaration-session",
            Self::ForeignBodySession => "foreign-body-session",
            Self::ForeignTypeProducerSession(_) => "foreign-type-producer-session",
            Self::DeclarationItemWrongSession(_) => "declaration-item-wrong-session",
            Self::UnexpectedTypeProducer(_) => "unexpected-type-producer",
            Self::DuplicateTypeProducer(_) => "duplicate-type-producer",
            Self::MissingTypeProducer(_) => "missing-type-producer",
            Self::TypeProducerTargetMismatch(_) => "type-producer-target-mismatch",
            Self::DeclarationKindMismatch(_) => "declaration-kind-mismatch",
            Self::BodyOwnerMismatch(_) => "body-owner-mismatch",
            Self::BodyKindMismatch(_) => "body-kind-mismatch",
            Self::BodySpanMismatch(_) => "body-span-mismatch",
            Self::IncompleteTypeProducer(_) => "incomplete-type-producer",
        }
    }
}

/// Failed exact terminal aggregation. It retains every consumed authority so a
/// caller can report a fail-closed compiler blocker without losing C1 or the
/// completed semantic facts.
#[derive(Debug)]
pub(crate) struct C2AggregationFailure {
    handoff: C2Handoff,
    declarations: CheckedDeclarationFacts,
    bodies: C2BodyTable,
    error: C2ModelError,
}

impl C2AggregationFailure {
    pub(crate) fn into_parts(
        self,
    ) -> (
        C2Handoff,
        CheckedDeclarationFacts,
        C2BodyTable,
        C2ModelError,
    ) {
        (self.handoff, self.declarations, self.bodies, self.error)
    }
}

#[derive(Debug)]
struct C2CheckedModel {
    targets: Vec<C2TargetRow>,
    checked_types: Vec<CheckedTypeRow>,
}

impl C2CheckedModel {
    fn aggregate(
        handoff: &C2Handoff,
        declarations: &CheckedDeclarationFacts,
        bodies: &C2BodyTable,
    ) -> Result<Self, C2ModelError> {
        if !handoff
            .session_brand()
            .same_session(declarations.session_brand())
        {
            return Err(C2ModelError::ForeignDeclarationSession);
        }
        if !handoff.session_brand().same_session(bodies.session_brand()) {
            return Err(C2ModelError::ForeignBodySession);
        }
        if !bodies.all_authority_complete() {
            let producer = bodies
                .attempts()
                .find(|body| !body.authority_complete())
                .map(|body| C2TypeProducer::Body(body.id()))
                .expect("an incomplete body table contains an incomplete row");
            return Err(C2ModelError::IncompleteTypeProducer(producer));
        }
        if let Some(body) = bodies.attempts().find(|body| body.has_source_diagnostics()) {
            return Err(C2ModelError::IncompleteTypeProducer(C2TypeProducer::Body(
                body.id(),
            )));
        }

        let expected_types = expected_type_producers(handoff)?;
        let session = handoff.session_brand();
        let mut inputs = Vec::with_capacity(declarations.len() + bodies.len());
        for declaration in declarations.declarations() {
            let item = handoff
                .indexes()
                .item(declaration.session_item())
                .ok_or_else(|| {
                    C2ModelError::DeclarationItemWrongSession(declaration.session_item().offset())
                })?
                .id();
            inputs.push(C2CheckedTypeInput {
                session: session.clone(),
                producer: C2TypeProducer::Declaration(item),
                package: declaration.package(),
                target: declaration.target(),
                evidence: TypeProducerEvidence::Declaration {
                    kind: declaration.kind(),
                },
                consumable: true,
                resolution: declaration.resolution().clone(),
                pending_c4: declaration.pending_c4().clone(),
            });
        }
        for body in bodies.bodies() {
            let producer = C2TypeProducer::Body(body.id());
            let expected = expected_types
                .get(&producer)
                .ok_or(C2ModelError::UnexpectedTypeProducer(producer))?;
            inputs.push(C2CheckedTypeInput {
                session: session.clone(),
                producer,
                package: expected.package,
                target: expected.target,
                evidence: TypeProducerEvidence::Body {
                    owner: body.owner(),
                    kind: body.kind(),
                    span: body.span(),
                },
                consumable: true,
                resolution: body.resolution().clone(),
                pending_c4: body.pending_c4().clone(),
            });
        }

        let checked_types = canonical_checked_types(handoff, &expected_types, inputs)?;
        let targets = derive_target_rows(handoff, &checked_types)?;
        Ok(Self {
            targets,
            checked_types,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct ExpectedTypeProducer {
    package: PackageId,
    target: TargetId,
    evidence: TypeProducerEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TypeProducerEvidence {
    Declaration {
        kind: arche_frontend::DeclarationKind,
    },
    Body {
        owner: HirItemId,
        kind: SemanticBodyKind,
        span: Span,
    },
}

#[derive(Clone, Debug)]
struct C2CheckedTypeInput {
    session: SessionBrand,
    producer: C2TypeProducer,
    package: PackageId,
    target: TargetId,
    evidence: TypeProducerEvidence,
    consumable: bool,
    resolution: C2Resolution,
    pending_c4: PendingC4Dependencies,
}

fn expected_type_producers(
    handoff: &C2Handoff,
) -> Result<BTreeMap<C2TypeProducer, ExpectedTypeProducer>, C2ModelError> {
    let mut expected = BTreeMap::new();
    for package in &handoff.hir().packages {
        for target in &package.targets {
            for item in &target.items {
                let producer = C2TypeProducer::Declaration(item.id);
                if expected
                    .insert(
                        producer,
                        ExpectedTypeProducer {
                            package: package.package,
                            target: target.id,
                            evidence: TypeProducerEvidence::Declaration { kind: item.kind },
                        },
                    )
                    .is_some()
                {
                    return Err(C2ModelError::DuplicateExpectedTypeProducer(producer));
                }
            }
            for body in &target.bodies {
                let producer = C2TypeProducer::Body(body.id);
                if expected
                    .insert(
                        producer,
                        ExpectedTypeProducer {
                            package: package.package,
                            target: target.id,
                            evidence: TypeProducerEvidence::Body {
                                owner: body.owner,
                                kind: body.kind,
                                span: body.span,
                            },
                        },
                    )
                    .is_some()
                {
                    return Err(C2ModelError::DuplicateExpectedTypeProducer(producer));
                }
            }
        }
    }
    Ok(expected)
}

fn canonical_checked_types(
    handoff: &C2Handoff,
    expected: &BTreeMap<C2TypeProducer, ExpectedTypeProducer>,
    inputs: Vec<C2CheckedTypeInput>,
) -> Result<Vec<CheckedTypeRow>, C2ModelError> {
    let session = handoff.session_brand();
    let mut rows = BTreeMap::new();
    for input in inputs {
        if !session.same_session(&input.session) {
            return Err(C2ModelError::ForeignTypeProducerSession(input.producer));
        }
        let expected = expected
            .get(&input.producer)
            .ok_or(C2ModelError::UnexpectedTypeProducer(input.producer))?;
        if (expected.package, expected.target) != (input.package, input.target) {
            return Err(C2ModelError::TypeProducerTargetMismatch(input.producer));
        }
        if !input.consumable {
            return Err(C2ModelError::IncompleteTypeProducer(input.producer));
        }
        match (expected.evidence, input.evidence) {
            (
                TypeProducerEvidence::Declaration { kind: expected },
                TypeProducerEvidence::Declaration { kind: actual },
            ) if expected != actual => {
                let C2TypeProducer::Declaration(item) = input.producer else {
                    unreachable!("evidence kind was validated against producer kind")
                };
                return Err(C2ModelError::DeclarationKindMismatch(item));
            }
            (
                TypeProducerEvidence::Body {
                    owner: expected, ..
                },
                TypeProducerEvidence::Body { owner: actual, .. },
            ) if expected != actual => {
                let C2TypeProducer::Body(body) = input.producer else {
                    unreachable!("evidence kind was validated against producer kind")
                };
                return Err(C2ModelError::BodyOwnerMismatch(body));
            }
            (
                TypeProducerEvidence::Body { kind: expected, .. },
                TypeProducerEvidence::Body { kind: actual, .. },
            ) if expected != actual => {
                let C2TypeProducer::Body(body) = input.producer else {
                    unreachable!("evidence kind was validated against producer kind")
                };
                return Err(C2ModelError::BodyKindMismatch(body));
            }
            (
                TypeProducerEvidence::Body { span: expected, .. },
                TypeProducerEvidence::Body { span: actual, .. },
            ) if expected != actual => {
                let C2TypeProducer::Body(body) = input.producer else {
                    unreachable!("evidence kind was validated against producer kind")
                };
                return Err(C2ModelError::BodySpanMismatch(body));
            }
            (TypeProducerEvidence::Declaration { .. }, TypeProducerEvidence::Body { .. })
            | (TypeProducerEvidence::Body { .. }, TypeProducerEvidence::Declaration { .. }) => {
                return Err(C2ModelError::UnexpectedTypeProducer(input.producer));
            }
            _ => {}
        }
        let producer = input.producer;
        if rows
            .insert(
                producer,
                CheckedTypeRow {
                    producer,
                    package: input.package,
                    target: input.target,
                    resolution: input.resolution,
                    pending_c4: input.pending_c4,
                },
            )
            .is_some()
        {
            return Err(C2ModelError::DuplicateTypeProducer(producer));
        }
    }
    if let Some((&producer, _)) = expected
        .iter()
        .find(|(producer, _)| !rows.contains_key(producer))
    {
        return Err(C2ModelError::MissingTypeProducer(producer));
    }
    Ok(rows.into_values().collect())
}

fn derive_target_rows(
    handoff: &C2Handoff,
    checked_types: &[CheckedTypeRow],
) -> Result<Vec<C2TargetRow>, C2ModelError> {
    let mut targets = BTreeMap::new();
    for package in &handoff.inventory().packages {
        let package_scope = ScopedPackageBytes::from_canonical_name(
            &package.provenance.scoped_name,
        )
        .ok_or(C2ModelError::EmptyPackageScope {
            package: package.package,
        })?;
        for target in &package.targets {
            let key = (package.package, target.target_id);
            if targets
                .insert(
                    key,
                    TargetAccumulator {
                        package_scope: package_scope.clone(),
                        ctfe: Vec::new(),
                        pending_c4: Vec::new(),
                    },
                )
                .is_some()
            {
                return Err(C2ModelError::DuplicateInventoryTarget {
                    package: package.package,
                    target: target.target_id,
                });
            }
        }
    }

    for row in checked_types {
        let accumulator =
            targets
                .get_mut(&(row.package, row.target))
                .ok_or(C2ModelError::UnexpectedTarget {
                    package: row.package,
                    target: row.target,
                })?;
        if let C2Resolution::NeedsCtfe(obligations) = &row.resolution {
            accumulator.ctfe.extend_from_slice(obligations.as_slice());
        }
        accumulator
            .pending_c4
            .extend_from_slice(row.pending_c4.as_slice());
    }

    let mut rows = targets
        .into_iter()
        .map(|((package, target), accumulator)| {
            let resolution = if accumulator.ctfe.is_empty() {
                C2Resolution::Complete
            } else {
                C2Resolution::NeedsCtfe(
                    NeedsCtfeObligations::from_unsorted(accumulator.ctfe)
                        .expect("nonempty target CTFE accumulator validates"),
                )
            };
            C2TargetRow {
                package,
                package_scope: accumulator.package_scope,
                target,
                resolution,
                pending_c4: PendingC4Dependencies::from_unsorted(accumulator.pending_c4),
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        left.package_scope
            .cmp(&right.package_scope)
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| left.package.cmp(&right.package))
    });
    Ok(rows)
}

struct TargetAccumulator {
    package_scope: ScopedPackageBytes,
    ctfe: Vec<NeedsCtfeObligation>,
    pending_c4: Vec<PendingC4Dependency>,
}

#[derive(Debug)]
struct C2TargetRow {
    package: PackageId,
    package_scope: ScopedPackageBytes,
    target: TargetId,
    resolution: C2Resolution,
    pending_c4: PendingC4Dependencies,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use arche_frontend::{check_workspace_c1, Diagnostic};
    use arche_package::{load_workspace, resolve, ManifestRequest, PortablePath, RegistrySnapshot};

    use crate::body_check::check_workspace_bodies_c2;
    use crate::declaration_check::check_declarations_c2;
    use crate::declarations::DeclarationTable;
    use crate::diagnostic::{CompilationPhase, SemanticDiagnostic};

    use super::*;

    fn corpus_frontend(name: &str) -> FrontendOutput {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../tests/m27c1")
            .join(name);
        let workspace = load_workspace(&ManifestRequest::discover_from(&root)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        check_workspace_c1(&workspace, &graph, &[]).unwrap()
    }

    fn complete_type_inputs(
        handoff: &C2Handoff,
    ) -> (
        BTreeMap<C2TypeProducer, ExpectedTypeProducer>,
        Vec<C2CheckedTypeInput>,
    ) {
        let expected = expected_type_producers(handoff).unwrap();
        let session = handoff.session_brand();
        let inputs = expected
            .iter()
            .map(|(&producer, expected)| C2CheckedTypeInput {
                session: session.clone(),
                producer,
                package: expected.package,
                target: expected.target,
                evidence: expected.evidence,
                consumable: true,
                resolution: C2Resolution::Complete,
                pending_c4: PendingC4Dependencies::default(),
            })
            .collect();
        (expected, inputs)
    }

    fn install_checked_types(handoff: &mut C2Handoff, rows: Vec<CheckedTypeRow>) {
        handoff.indexes.checked_type_lookup = rows
            .iter()
            .enumerate()
            .map(|(offset, row)| {
                (
                    row.producer,
                    u64::try_from(offset).expect("test checked-type offset fits u64"),
                )
            })
            .collect();
        handoff.indexes.checked_type_rows = rows;
    }

    fn obligation(bytes: &[u8]) -> NeedsCtfeObligation {
        NeedsCtfeObligation::from_canonical_bytes(bytes.to_vec()).unwrap()
    }

    #[test]
    fn handoff_builds_deterministic_item_indexes_and_borrowed_views() {
        let handoff = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let expected = handoff
            .hir()
            .packages
            .iter()
            .flat_map(|package| &package.targets)
            .flat_map(|target| &target.items)
            .map(|item| item.id)
            .collect::<Vec<_>>();

        assert_eq!(handoff.indexes().item_count(), expected.len());
        assert_eq!(handoff.indexes().checked_type_count(), 0);
        for (offset, item) in expected.into_iter().enumerate() {
            let index = handoff.indexes().item_index(item).unwrap();
            assert_eq!(index.offset(), u64::try_from(offset).unwrap());
            assert_eq!(handoff.indexes().item(&index).unwrap().id(), item);
        }
    }

    #[test]
    fn checked_type_producer_kind_prevents_numeric_id_aliasing() {
        assert_ne!(
            C2TypeProducer::Declaration(HirItemId(7)),
            C2TypeProducer::Body(HirBodyId(7))
        );
    }

    #[test]
    fn item_and_type_handles_reject_cross_session_swaps_at_equal_offsets() {
        let mut game = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let mut environment = C2Handoff::begin(corpus_frontend("language-environment")).unwrap();

        let game_item_id = game.indexes.item_rows[0];
        let environment_item_id = environment.indexes.item_rows[0];
        let game_item = game.indexes().item_index(game_item_id).unwrap();
        let environment_item = environment
            .indexes()
            .item_index(environment_item_id)
            .unwrap();
        assert_eq!(game_item.offset(), environment_item.offset());
        assert!(game.indexes().item(&environment_item).is_none());
        assert!(environment.indexes().item(&game_item).is_none());

        let (game_expected, game_inputs) = complete_type_inputs(&game);
        let game_rows = canonical_checked_types(&game, &game_expected, game_inputs).unwrap();
        let (environment_expected, environment_inputs) = complete_type_inputs(&environment);
        let environment_rows =
            canonical_checked_types(&environment, &environment_expected, environment_inputs)
                .unwrap();
        install_checked_types(&mut game, game_rows);
        install_checked_types(&mut environment, environment_rows);
        let game_type = game.indexes().checked_type_index(0).unwrap();
        let environment_type = environment.indexes().checked_type_index(0).unwrap();
        assert_eq!(game_type.offset(), environment_type.offset());
        assert!(game.indexes().checked_type(&environment_type).is_none());
        assert!(environment.indexes().checked_type(&game_type).is_none());
        assert_eq!(
            game.indexes()
                .checked_type(&game_type)
                .unwrap()
                .resolution(),
            &C2Resolution::Complete
        );
        let producer = game.indexes().checked_type(&game_type).unwrap().producer();
        assert_eq!(
            game.indexes()
                .checked_type_index_for(producer)
                .unwrap()
                .offset(),
            game_type.offset()
        );
    }

    #[test]
    fn needs_ctfe_is_nonempty_sorted_and_exact_deduplicated() {
        assert_eq!(
            NeedsCtfeObligation::from_canonical_bytes(Vec::new()).unwrap_err(),
            CanonicalDependencyError::EmptyNeedsCtfeObligation
        );
        assert_eq!(
            NeedsCtfeObligations::from_unsorted(Vec::new()).unwrap_err(),
            CanonicalDependencyError::EmptyNeedsCtfeSet
        );
        assert_eq!(
            PendingC4Dependency::from_canonical_bytes(Vec::new()).unwrap_err(),
            CanonicalDependencyError::EmptyPendingC4Dependency
        );
        let obligations = NeedsCtfeObligations::from_unsorted(vec![
            obligation(b"zeta"),
            obligation(b"alpha"),
            obligation(b"zeta"),
        ])
        .unwrap();
        assert_eq!(obligations.len(), 2);
        assert_eq!(obligations.as_slice()[0].canonical_bytes(), b"alpha");
        assert_eq!(obligations.as_slice()[1].canonical_bytes(), b"zeta");
    }

    #[test]
    fn target_may_need_ctfe_and_independently_retain_c4_dependencies() {
        let handoff = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let (expected, mut inputs) = complete_type_inputs(&handoff);
        let obligations =
            NeedsCtfeObligations::from_unsorted(vec![obligation(b"const:WIDTH")]).unwrap();
        let pending_c4 = PendingC4Dependencies::from_unsorted(vec![
            PendingC4Dependency::from_canonical_bytes(b"identity:type".to_vec()).unwrap(),
            PendingC4Dependency::from_canonical_bytes(b"effect:draw".to_vec()).unwrap(),
            PendingC4Dependency::from_canonical_bytes(b"identity:type".to_vec()).unwrap(),
        ]);
        inputs[0].resolution = C2Resolution::NeedsCtfe(obligations);
        inputs[0].pending_c4 = pending_c4;
        let checked_types = canonical_checked_types(&handoff, &expected, inputs).unwrap();
        let targets = derive_target_rows(&handoff, &checked_types).unwrap();
        let target = targets
            .iter()
            .find(|target| matches!(target.resolution, C2Resolution::NeedsCtfe(_)))
            .unwrap();
        match &target.resolution {
            C2Resolution::Complete => panic!("selected target must retain CTFE"),
            C2Resolution::NeedsCtfe(obligations) => {
                assert_eq!(obligations.len(), 1);
            }
        }
        assert_eq!(target.pending_c4.len(), 2);
        assert_eq!(
            target.pending_c4.as_slice()[0].canonical_bytes(),
            b"effect:draw"
        );
        assert_eq!(
            target.pending_c4.as_slice()[1].canonical_bytes(),
            b"identity:type"
        );
    }

    #[test]
    fn rejected_terminal_construction_consumes_and_retains_the_handoff() {
        let rejected_handoff = C2Handoff::begin(corpus_frontend("language-environment")).unwrap();
        let package = &rejected_handoff.inventory().packages[0];
        let target = package.targets[0].target_id;
        let diagnostic = SemanticDiagnostic::new(
            CompilationPhase::BodyCallOperatorPattern,
            ScopedPackageBytes::from_canonical_name(&package.provenance.scoped_name).unwrap(),
            target,
            PortablePath::new("src/main.arc").unwrap(),
            Diagnostic::path("TYPE001", "deliberate rejection"),
            Vec::new(),
        )
        .unwrap();
        let diagnostics = NonEmptySemanticDiagnostics::from_unsorted(vec![diagnostic]).unwrap();
        let rejected = rejected_handoff.into_rejected(diagnostics);
        assert_eq!(rejected.diagnostics().len(), 1);
        assert!(!rejected.hir().packages.is_empty());
    }

    #[test]
    fn checked_type_universe_rejects_missing_duplicate_unexpected_and_foreign_rows() {
        let game = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let same_shape_other_session = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let (expected, inputs) = complete_type_inputs(&game);

        let canonical = canonical_checked_types(&game, &expected, inputs.clone()).unwrap();
        let mut reversed = inputs.clone();
        reversed.reverse();
        let reversed = canonical_checked_types(&game, &expected, reversed).unwrap();
        assert_eq!(
            canonical.iter().map(|row| row.producer).collect::<Vec<_>>(),
            reversed.iter().map(|row| row.producer).collect::<Vec<_>>()
        );

        let mut missing = inputs.clone();
        let missing_producer = missing.pop().unwrap().producer;
        assert_eq!(
            canonical_checked_types(&game, &expected, missing).unwrap_err(),
            C2ModelError::MissingTypeProducer(missing_producer)
        );

        let mut duplicate = inputs.clone();
        let duplicate_producer = duplicate[0].producer;
        duplicate.push(duplicate[0].clone());
        assert_eq!(
            canonical_checked_types(&game, &expected, duplicate).unwrap_err(),
            C2ModelError::DuplicateTypeProducer(duplicate_producer)
        );

        let mut unexpected = inputs.clone();
        let unexpected_producer = C2TypeProducer::Body(HirBodyId(u64::MAX));
        unexpected[0].producer = unexpected_producer;
        assert_eq!(
            canonical_checked_types(&game, &expected, unexpected).unwrap_err(),
            C2ModelError::UnexpectedTypeProducer(unexpected_producer)
        );

        let mut foreign = inputs;
        let foreign_producer = foreign[0].producer;
        foreign[0].session = same_shape_other_session.session_brand();
        assert_eq!(
            canonical_checked_types(&game, &expected, foreign).unwrap_err(),
            C2ModelError::ForeignTypeProducerSession(foreign_producer)
        );

        let (_, mut incomplete) = complete_type_inputs(&game);
        let incomplete_producer = incomplete[0].producer;
        incomplete[0].consumable = false;
        assert_eq!(
            canonical_checked_types(&game, &expected, incomplete).unwrap_err(),
            C2ModelError::IncompleteTypeProducer(incomplete_producer)
        );
    }

    #[test]
    fn terminal_aggregator_rejects_equal_key_fact_tables_from_another_session() {
        let first = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let second = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let first_table = DeclarationTable::build(&first).unwrap();
        let second_table = DeclarationTable::build(&second).unwrap();
        let first_declarations = check_declarations_c2(&first, &first_table);
        let second_declarations = check_declarations_c2(&second, &second_table);
        let first_facts = match &first_declarations {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let second_facts = match &second_declarations {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let first_bodies = check_workspace_bodies_c2(&first, &first_table, first_facts);
        let second_bodies = check_workspace_bodies_c2(&second, &second_table, second_facts);
        let first_body_table = match &first_bodies {
            Ok(bodies) => bodies,
            Err(failure) => failure.partial(),
        };
        let second_body_table = match &second_bodies {
            Ok(bodies) => bodies,
            Err(failure) => failure.partial(),
        };

        assert_eq!(
            C2CheckedModel::aggregate(&first, second_facts, first_body_table).unwrap_err(),
            C2ModelError::ForeignDeclarationSession
        );
        assert_eq!(
            C2CheckedModel::aggregate(&first, first_facts, second_body_table).unwrap_err(),
            C2ModelError::ForeignBodySession
        );
    }
}
