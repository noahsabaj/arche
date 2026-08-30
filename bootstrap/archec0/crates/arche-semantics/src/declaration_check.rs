//! Consuming C2 declaration and signature checking.
//!
//! This module deliberately stops before stable semantic identity.  Its rows
//! retain the complete C1 declaration keys only as session-local traversal
//! material.  A successful result contains no declaration-level `PendingC2`
//! descendant. Contextual `Self` is closed through frontend's retained,
//! noncanonical structured template authority; every missing, ambiguous, or
//! independently pending template fails closed with an explicit blocker
//! instead of diagnosing a valid source program or duplicating lowering.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use arche_foundation::identity::PackageId;
use arche_frontend::embedded_core::{
    CompilerPrimitiveTypePattern, CompilerTraitAuthority, CompilerTraitCallablePattern,
    CompilerTraitKind, CompilerTraitReceiverMode, CompilerTraitSelfRelation,
    CompilerTraitTypePattern, UserImplPolicy, VerifiedEmbeddedCoreAuthority, VirtualNamespace,
};
use arche_frontend::{
    encode_symbolic_const, encode_symbolic_predicate, encode_symbolic_type, C2TypeTemplateBlocker,
    C2TypeTemplateInstantiationError, C2TypeTemplateLookupError, DeclarationKind, Diagnostic,
    GenericArgumentShape, GenericParameterKind, HiddenLifetimeBinderSource, HirItemId,
    HirItemSource, Mutability, PendingShapeKind, ResolvedSymbolicItem, SemanticDeclarationPath,
    SemanticDefinitionInventorySkeleton, Span, SymbolicCallableParameterMode,
    SymbolicCallableShapeSkeleton, SymbolicDeclarationPayloadSkeleton,
    SymbolicDeclarationShapeSkeleton, SymbolicDefinitionOwnerSkeleton, SymbolicEffectSetsSkeleton,
    SymbolicEffectShapeSkeleton, SymbolicPendingShape, SymbolicPredicate,
    SymbolicPredicateShapeSkeleton, SymbolicSystemAccessShapeSkeleton, SymbolicType,
    SymbolicTypeShapeSkeleton, TargetId, TargetRoot,
};
use arche_package::PortablePath;

use crate::binder::{
    validate_symbolic_predicate, validate_symbolic_type, BinderFrame, BinderStack,
};
use crate::declarations::{DeclarationTable, DeclarationView};
use crate::diagnostic::{
    CompilationPhase, NonEmptySemanticDiagnostics, ScopedPackageBytes, SemanticDiagnostic,
};
use crate::formation::{validate_generic_arguments, TraitFrameSubstitution};
use crate::model::{
    C2Handoff, C2Resolution, NeedsCtfeObligation, NeedsCtfeObligations, PendingC4Dependencies,
    PendingC4Dependency, SessionBrand, SessionItemIndex,
};
use crate::readiness::{
    audit_declaration_shape, audit_definition_owner, ShapeGateAudit, ShapeReadinessInconsistency,
};
use crate::traits::{
    OrdinaryImplCandidateSpec, SemanticPredicate, SemanticTraitKey, TraitEnvironment,
    TraitModelError, TraitPredicate,
};

/// Opaque checked-declaration handle branded by one successful checking run.
#[derive(Clone, Debug)]
pub struct CheckedDeclarationHandle {
    owner: Arc<CheckedDeclarationOwner>,
    offset: u64,
}

impl CheckedDeclarationHandle {
    /// Dense session-local offset. This is not stable semantic identity.
    pub const fn offset(&self) -> u64 {
        self.offset
    }
}

/// Borrow-tied read-only checked declaration row.
#[derive(Clone, Copy, Debug)]
pub struct CheckedDeclarationView<'a> {
    row: &'a CheckedDeclarationRow,
}

impl<'a> CheckedDeclarationView<'a> {
    pub const fn session_item(self) -> &'a SessionItemIndex {
        &self.row.session_item
    }

    pub const fn package(self) -> PackageId {
        self.row.package
    }

    pub const fn target(self) -> TargetId {
        self.row.target
    }

    pub const fn kind(self) -> DeclarationKind {
        self.row.kind
    }

    pub fn name(self) -> &'a str {
        &self.row.name
    }

    /// Complete C1 semantic-key bytes retained only for this checking session.
    pub fn session_traversal_bytes(self) -> &'a [u8] {
        &self.row.session_key
    }

    pub const fn owner_shape(self) -> &'a SymbolicDefinitionOwnerSkeleton {
        &self.row.owner_shape
    }

    /// A post-C2 declaration shape. Construction proves that its recursive
    /// readiness audit contains no `PendingC2` leaf or inconsistency.
    pub const fn declaration_shape(self) -> &'a SymbolicDeclarationShapeSkeleton {
        &self.row.declaration_shape
    }

    pub const fn resolution(self) -> &'a C2Resolution {
        &self.row.resolution
    }

    pub const fn pending_c4(self) -> &'a PendingC4Dependencies {
        &self.row.pending_c4
    }

    /// Returns the complete solver descriptor for a trait impl. Inherent impls
    /// and non-impl declarations return `None`.
    pub const fn ordinary_impl_candidate(self) -> Option<&'a OrdinaryImplCandidateSpec> {
        self.row.ordinary_impl.as_ref()
    }
}

/// Immutable checked declaration/signature facts for one C2 handoff.
#[derive(Debug)]
pub struct CheckedDeclarationFacts {
    session: SessionBrand,
    owner: Arc<CheckedDeclarationOwner>,
    rows: Vec<CheckedDeclarationRow>,
    by_item: BTreeMap<HirItemId, u64>,
    by_session_key: BTreeMap<Box<[u8]>, u64>,
}

impl CheckedDeclarationFacts {
    pub(crate) const fn session_brand(&self) -> &SessionBrand {
        &self.session
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn declarations(&self) -> impl ExactSizeIterator<Item = CheckedDeclarationView<'_>> + '_ {
        self.rows.iter().map(|row| CheckedDeclarationView { row })
    }

    pub fn handle_for_session_item(&self, item: HirItemId) -> Option<CheckedDeclarationHandle> {
        self.by_item
            .get(&item)
            .copied()
            .map(|offset| CheckedDeclarationHandle {
                owner: Arc::clone(&self.owner),
                offset,
            })
    }

    pub fn declaration(
        &self,
        handle: &CheckedDeclarationHandle,
    ) -> Option<CheckedDeclarationView<'_>> {
        if !Arc::ptr_eq(&self.owner, &handle.owner) {
            return None;
        }
        usize::try_from(handle.offset)
            .ok()
            .and_then(|offset| self.rows.get(offset))
            .map(|row| CheckedDeclarationView { row })
    }

    /// Joins an exact declaration-table row to its checked impl descriptor.
    /// The complete session key is used only within this run; it is not a
    /// semantic selection tie-break or stable identity.
    pub fn ordinary_impl_candidate_for(
        &self,
        declaration: DeclarationView<'_>,
    ) -> Option<OrdinaryImplCandidateSpec> {
        if !self.session.owns_item(declaration.session_item()) {
            return None;
        }
        let offset = self
            .by_session_key
            .get(declaration.session_traversal_bytes())?;
        self.rows
            .get(usize::try_from(*offset).ok()?)?
            .ordinary_impl
            .clone()
    }
}

/// Why declaration checking could not yet produce a semantic success result.
///
/// Blockers are compiler-contract gaps, not source diagnostics. In particular,
/// a valid contextual `Self` retained as a collapsed C1 pending row must never
/// be reported as a source `TYPE001`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DeclarationCheckBlockerReason {
    /// C1 retained only an inner span and debug spelling for an enclosing type,
    /// effect, or predicate. C2 requires the single-authority structured
    /// relowering/template bridge before it may replace this continuation.
    MissingStructuredPendingAuthority {
        domain: PendingAuthorityDomain,
        kind: PendingShapeKind,
    },
    /// Exact contextual-Self template lookup or instantiation failed closed.
    ContextualSelfTemplate {
        domain: PendingAuthorityDomain,
        failure: ContextualSelfTemplateFailure,
    },
    /// The typed Embedded Core projection identifies the trait and its shape,
    /// but does not yet furnish the exact raw stable trait `DefinitionId`
    /// required by the compiler-known `SemanticTraitKey` encoding.
    MissingFinalEmbeddedTraitIdentity(CompilerTraitKind),
    /// The named declaration-level judgment is not implemented yet. The
    /// candidate declaration fails closed instead of letting its target mint a
    /// successful `Complete` resolution by omission. Unlike authority
    /// blockers, this reason does not suppress independently collected source
    /// diagnostics: implemented checks stay trustworthy while an absent one
    /// can only prevent success.
    MissingDeclarationJudgment(UnimplementedDeclarationJudgment),
}

/// Closed inventory of C2 declaration judgments this checker does not yet
/// implement. Every candidate declaration records one explicit blocker per
/// applicable judgment, so absence of a required check can never look like
/// success. Removing a member is a reviewed API change that lands together
/// with the implementation of that judgment.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum UnimplementedDeclarationJudgment {
    /// Sizedness, direct/indirect recursive storage cycles, nonregular
    /// recursive re-entry, and bare slice/`str` fields.
    SizednessRecursion,
    /// Transparent type-alias acyclicity.
    TypeAliasCycle,
    /// Byte-identical inherent-head method-name uniqueness across blocks.
    InherentMethodUniqueness,
    /// Impl overlap, `default` containment, and coherence selection.
    ImplCoherenceOverlap,
    /// `Map<K, V>` key comparator selection (`K: Eq + Ord`).
    MapKeyComparison,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ContextualSelfTemplateFailure {
    NotContextualSelf,
    Missing,
    Ambiguous,
    AdditionalPending(PendingShapeKind),
    FrontendInvariant,
    ReservedTemplateCoordinate,
    MissingSelfAuthority,
    MissingCanonicalImplTarget,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PendingAuthorityDomain {
    DeclarationShape,
    DefinitionOwner,
}

/// One canonically scoped fail-closed blocker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationCheckBlocker {
    package: ScopedPackageBytes,
    target: TargetId,
    path: PortablePath,
    span: Span,
    item: HirItemId,
    debug_spelling: String,
    reason: DeclarationCheckBlockerReason,
}

impl Ord for DeclarationCheckBlocker {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.package
            .cmp(&other.package)
            .then_with(|| self.target.cmp(&other.target))
            .then_with(|| self.path.cmp(&other.path))
            .then_with(|| span_key(self.span).cmp(&span_key(other.span)))
            .then_with(|| self.item.cmp(&other.item))
            .then_with(|| self.debug_spelling.cmp(&other.debug_spelling))
            .then_with(|| self.reason.cmp(&other.reason))
    }
}

impl PartialOrd for DeclarationCheckBlocker {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl DeclarationCheckBlocker {
    pub const fn package(&self) -> &ScopedPackageBytes {
        &self.package
    }

    pub const fn target(&self) -> TargetId {
        self.target
    }

    pub const fn path(&self) -> &PortablePath {
        &self.path
    }

    pub const fn span(&self) -> Span {
        self.span
    }

    pub const fn item(&self) -> HirItemId {
        self.item
    }

    pub fn debug_spelling(&self) -> &str {
        &self.debug_spelling
    }

    pub const fn reason(&self) -> &DeclarationCheckBlockerReason {
        &self.reason
    }
}

/// Non-success result. Semantic source errors and internal authority blockers
/// remain distinct so a public checker cannot misdiagnose a valid program.
#[derive(Debug)]
pub struct DeclarationCheckFailure {
    diagnostics: Option<NonEmptySemanticDiagnostics>,
    blockers: Box<[DeclarationCheckBlocker]>,
    internal_error: Option<DeclarationCheckInternalError>,
    partial: Box<CheckedDeclarationFacts>,
}

impl DeclarationCheckFailure {
    pub const fn diagnostics(&self) -> Option<&NonEmptySemanticDiagnostics> {
        self.diagnostics.as_ref()
    }

    pub fn blockers(&self) -> &[DeclarationCheckBlocker] {
        &self.blockers
    }

    /// Returns a fail-closed retained-session contract failure, if one
    /// prevented source-level declaration checking from starting.
    pub const fn internal_error(&self) -> Option<&DeclarationCheckInternalError> {
        self.internal_error.as_ref()
    }

    /// Structurally closed, binder-validated post-C2 rows retained even when
    /// an independent declaration diagnostic or authority blocker prevents a
    /// complete success result. These facts are not a success certificate.
    pub fn partial(&self) -> &CheckedDeclarationFacts {
        &self.partial
    }

    pub fn is_blocked(&self) -> bool {
        !self.blockers.is_empty() || self.internal_error.is_some()
    }

    /// Returns whether this failure invalidates trusting collected source
    /// diagnostics. A missing-judgment blocker only forbids minting success;
    /// every other blocker and every internal error marks compiler authority
    /// itself as incomplete, so the session must not blame valid source.
    pub fn suppresses_source_diagnostics(&self) -> bool {
        self.internal_error.is_some()
            || self.blockers.iter().any(|blocker| {
                !matches!(
                    blocker.reason(),
                    DeclarationCheckBlockerReason::MissingDeclarationJudgment(_)
                )
            })
    }

    /// Consumes the failure and returns its partial checked facts.
    pub fn into_partial(self) -> CheckedDeclarationFacts {
        *self.partial
    }
}

/// Internal retained-input contract failure. These are deliberately not
/// represented as source diagnostics because no source program is at fault.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeclarationCheckInternalError {
    /// The declaration table and handoff did not form the complete one-to-one
    /// retained join guaranteed by `DeclarationTable::build` for one session.
    IncompleteRetainedDeclarationJoin {
        expected_declarations: usize,
        joined_declarations: usize,
    },
}

#[derive(Debug)]
struct CheckedDeclarationOwner;

#[derive(Debug)]
struct CheckedDeclarationRow {
    session_item: SessionItemIndex,
    package: PackageId,
    target: TargetId,
    kind: DeclarationKind,
    name: String,
    session_key: Box<[u8]>,
    owner_shape: SymbolicDefinitionOwnerSkeleton,
    declaration_shape: SymbolicDeclarationShapeSkeleton,
    resolution: C2Resolution,
    pending_c4: PendingC4Dependencies,
    ordinary_impl: Option<OrdinaryImplCandidateSpec>,
}

#[derive(Clone, Copy)]
struct InputRow<'a> {
    declaration: DeclarationView<'a>,
    definition: &'a SemanticDefinitionInventorySkeleton,
    item: &'a ResolvedSymbolicItem,
    package_scope: &'a str,
    target: TargetId,
    path: &'a PortablePath,
}

#[derive(Clone)]
struct CatalogRow<'a> {
    declaration: DeclarationView<'a>,
    source_formals: Vec<GenericParameterKind>,
}

struct DeclarationCatalog<'a> {
    rows: BTreeMap<SemanticDeclarationPath, CatalogRow<'a>>,
    embedded: &'a VerifiedEmbeddedCoreAuthority,
}

/// Checks the complete retained C1 declaration table without consuming it.
///
/// A successful return proves declaration-level C2 closure, binder/generic
/// validity, trait-impl conformance, and complete ordinary impl descriptors.
/// It never creates a `DefinitionId`, `TypeId`, or interface identity.
pub fn check_declarations_c2(
    handoff: &C2Handoff,
    declarations: &DeclarationTable,
) -> Result<CheckedDeclarationFacts, DeclarationCheckFailure> {
    let mut diagnostics = Vec::new();
    let mut blockers = Vec::new();
    let (inputs, internal_error) = collect_inputs(handoff, declarations);
    if let Some(internal_error) = internal_error {
        return Err(DeclarationCheckFailure {
            diagnostics: None,
            blockers: Box::new([]),
            internal_error: Some(internal_error),
            partial: Box::new(checked_facts_from_provisional(handoff, &[], Vec::new())),
        });
    }
    let catalog = build_catalog(handoff, &inputs);

    check_trait_impl_visibility(&inputs, &mut diagnostics);

    let mut provisional = Vec::with_capacity(inputs.len());
    for input in &inputs {
        let c1_definition_audit = audit_declaration_shape(&input.definition.symbolic_shape);
        let c1_owner_audit = audit_definition_owner(&input.definition.key.owner_path);
        report_inconsistencies(
            input,
            &c1_definition_audit,
            "declaration shape",
            &mut diagnostics,
        );
        report_inconsistencies(input, &c1_owner_audit, "definition owner", &mut diagnostics);

        let shape = resolve_declaration_contextual_self(
            input,
            &inputs,
            &input.definition.symbolic_shape,
            PendingAuthorityDomain::DeclarationShape,
            &mut blockers,
        );
        let owner = resolve_owner_contextual_self(input, &inputs, &mut blockers);
        record_unimplemented_judgments(input, &shape, catalog.embedded, &mut blockers);
        let definition_audit = audit_declaration_shape(&shape);
        let owner_audit = audit_definition_owner(&owner);
        collect_pending_blockers(
            input,
            &definition_audit,
            PendingAuthorityDomain::DeclarationShape,
            &mut blockers,
        );
        collect_pending_blockers(
            input,
            &owner_audit,
            PendingAuthorityDomain::DefinitionOwner,
            &mut blockers,
        );

        if !definition_audit.post_c2_is_closed() || !owner_audit.post_c2_is_closed() {
            provisional.push(None);
            continue;
        }

        if let Err(message) = validate_generic_layout(input, &shape) {
            push_diagnostic(
                input,
                "IDENTITY001",
                message,
                input.definition.key.span,
                &mut diagnostics,
            );
            provisional.push(None);
            continue;
        }
        let binders = binder_stack_for(input, &inputs, &shape);
        if let Err(message) = validate_checked_shape(&shape, &binders, &catalog) {
            push_diagnostic(
                input,
                "IDENTITY001",
                message,
                input.definition.key.span,
                &mut diagnostics,
            );
            provisional.push(None);
            continue;
        }

        let (resolution, pending_c4) =
            gate_state(input, &definition_audit, &owner_audit, &mut diagnostics);
        provisional.push(Some(CheckedDeclarationRow {
            session_item: input.declaration.session_item().clone(),
            package: input.declaration.package(),
            target: input.target,
            kind: input.declaration.kind(),
            name: input.declaration.name().to_owned(),
            session_key: input
                .declaration
                .session_traversal_bytes()
                .to_vec()
                .into_boxed_slice(),
            owner_shape: owner,
            declaration_shape: shape,
            resolution,
            pending_c4,
            ordinary_impl: None,
        }));
    }

    // Trait/header checking is meaningful only for rows whose structural C2
    // authority is already complete. Continue across independent rows to
    // accumulate all semantic diagnostics and missing compiler-key blockers.
    for index in 0..inputs.len() {
        let Some(mut row) = provisional[index].take() else {
            continue;
        };
        if row.kind == DeclarationKind::Impl {
            row.ordinary_impl = describe_impl(
                index,
                &row,
                &inputs,
                &provisional,
                &catalog,
                &mut diagnostics,
                &mut blockers,
            );
        }
        provisional[index] = Some(row);
    }

    blockers.sort();
    blockers.dedup();
    let diagnostics = NonEmptySemanticDiagnostics::from_unsorted(diagnostics).ok();
    let missing_rows = provisional.iter().any(Option::is_none);
    let facts = checked_facts_from_provisional(handoff, &inputs, provisional);
    if diagnostics.is_some() || !blockers.is_empty() || missing_rows {
        return Err(DeclarationCheckFailure {
            diagnostics,
            blockers: blockers.into_boxed_slice(),
            internal_error: None,
            partial: Box::new(facts),
        });
    }

    debug_assert_eq!(facts.len(), inputs.len());
    Ok(facts)
}

fn checked_facts_from_provisional(
    handoff: &C2Handoff,
    inputs: &[InputRow<'_>],
    provisional: Vec<Option<CheckedDeclarationRow>>,
) -> CheckedDeclarationFacts {
    let owner = Arc::new(CheckedDeclarationOwner);
    let mut rows = Vec::with_capacity(provisional.len());
    let mut by_item = BTreeMap::new();
    let mut by_session_key = BTreeMap::new();
    for (input, row) in inputs
        .iter()
        .zip(provisional)
        .filter_map(|(input, row)| row.map(|row| (input, row)))
    {
        let offset = u64::try_from(rows.len()).expect("declaration table already fits u64 offsets");
        by_item.insert(input.definition.hir_item, offset);
        by_session_key.insert(row.session_key.clone(), offset);
        rows.push(row);
    }
    CheckedDeclarationFacts {
        session: handoff.session_brand(),
        owner,
        rows,
        by_item,
        by_session_key,
    }
}

fn collect_inputs<'a>(
    handoff: &'a C2Handoff,
    declarations: &'a DeclarationTable,
) -> (Vec<InputRow<'a>>, Option<DeclarationCheckInternalError>) {
    let mut item_locations = BTreeMap::new();
    for (package_index, package) in handoff.frontend().hir().packages.iter().enumerate() {
        for (target_index, target) in package.targets.iter().enumerate() {
            for (item_index, item) in target.items.iter().enumerate() {
                item_locations.insert(item.id, (package_index, target_index, item_index));
            }
        }
    }
    let mut definitions = BTreeMap::new();
    for package in &handoff.frontend().inventory().packages {
        for definition in &package.definitions {
            definitions.insert(definition.hir_item, (package, definition));
        }
    }

    let mut inputs = Vec::with_capacity(declarations.len());
    for declaration in declarations.declarations() {
        let Some(item) = handoff.indexes().item(declaration.session_item()) else {
            continue;
        };
        let Some(&(package_index, target_index, item_index)) = item_locations.get(&item.id())
        else {
            continue;
        };
        let hir_package = &handoff.frontend().hir().packages[package_index];
        let target = &hir_package.targets[target_index];
        let hir_item = &target.items[item_index];
        let Some((inventory_package, definition)) = definitions.get(&item.id()).copied() else {
            continue;
        };
        let Some(source) = handoff.frontend().sources().file(hir_item.span.file) else {
            continue;
        };
        inputs.push(InputRow {
            declaration,
            definition,
            item: hir_item,
            package_scope: &inventory_package.provenance.scoped_name,
            target: target.id,
            path: source.portable_path(),
        });
    }

    let internal_error = (inputs.len() != declarations.len()).then_some(
        DeclarationCheckInternalError::IncompleteRetainedDeclarationJoin {
            expected_declarations: declarations.len(),
            joined_declarations: inputs.len(),
        },
    );
    (inputs, internal_error)
}

fn build_catalog<'a>(handoff: &'a C2Handoff, inputs: &[InputRow<'a>]) -> DeclarationCatalog<'a> {
    let mut rows = BTreeMap::new();
    for input in inputs {
        let package = handoff
            .frontend()
            .inventory()
            .packages
            .iter()
            .find(|package| package.package == input.declaration.package())
            .expect("input row retains its inventory package");
        rows.insert(
            SemanticDeclarationPath {
                registry_origin: package.provenance.registry_origin.clone(),
                package_name: package.provenance.scoped_name.clone(),
                target: input.definition.key.module.target.clone(),
                modules: input.definition.key.module.path.clone(),
                kind: input.definition.key.kind,
                name: input.definition.key.name.clone(),
            },
            CatalogRow {
                declaration: input.declaration,
                source_formals: input
                    .item
                    .symbolic_shape
                    .generic_parameters
                    .iter()
                    .map(|parameter| parameter.kind.clone())
                    .collect(),
            },
        );
    }
    DeclarationCatalog {
        rows,
        embedded: handoff.frontend().inventory().embedded_core.as_ref(),
    }
}

fn resolve_owner_contextual_self(
    input: &InputRow<'_>,
    inputs: &[InputRow<'_>],
    blockers: &mut Vec<DeclarationCheckBlocker>,
) -> SymbolicDefinitionOwnerSkeleton {
    let authority = input
        .item
        .owner
        .and_then(|owner| {
            inputs
                .iter()
                .find(|candidate| candidate.definition.hir_item == owner)
        })
        .unwrap_or(input);
    resolve_owner_shape_contextual_self(
        authority,
        inputs,
        &input.definition.key.owner_path,
        blockers,
    )
}

fn resolve_owner_shape_contextual_self(
    input: &InputRow<'_>,
    inputs: &[InputRow<'_>],
    owner: &SymbolicDefinitionOwnerSkeleton,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) -> SymbolicDefinitionOwnerSkeleton {
    let domain = PendingAuthorityDomain::DefinitionOwner;
    match owner {
        SymbolicDefinitionOwnerSkeleton::TopLevel => SymbolicDefinitionOwnerSkeleton::TopLevel,
        SymbolicDefinitionOwnerSkeleton::Trait { path, shape } => {
            SymbolicDefinitionOwnerSkeleton::Trait {
                path: path.clone(),
                shape: Box::new(resolve_declaration_contextual_self(
                    input, inputs, shape, domain, blockers,
                )),
            }
        }
        SymbolicDefinitionOwnerSkeleton::SystemQuery { path, shape } => {
            SymbolicDefinitionOwnerSkeleton::SystemQuery {
                path: path.clone(),
                shape: Box::new(resolve_declaration_contextual_self(
                    input, inputs, shape, domain, blockers,
                )),
            }
        }
        SymbolicDefinitionOwnerSkeleton::InherentImpl {
            target,
            generic_parameters,
            predicates,
        } => SymbolicDefinitionOwnerSkeleton::InherentImpl {
            target: resolve_contextual_self_type_shape(input, inputs, target, domain, blockers),
            generic_parameters: generic_parameters.clone(),
            predicates: predicates.clone(),
        },
        SymbolicDefinitionOwnerSkeleton::TraitImpl {
            trait_ref,
            target,
            generic_parameters,
            predicates,
            is_default,
        } => SymbolicDefinitionOwnerSkeleton::TraitImpl {
            trait_ref: resolve_contextual_self_type_shape(
                input, inputs, trait_ref, domain, blockers,
            ),
            target: resolve_contextual_self_type_shape(input, inputs, target, domain, blockers),
            generic_parameters: generic_parameters.clone(),
            predicates: predicates.clone(),
            is_default: *is_default,
        },
    }
}

fn resolve_declaration_contextual_self(
    input: &InputRow<'_>,
    inputs: &[InputRow<'_>],
    shape: &SymbolicDeclarationShapeSkeleton,
    domain: PendingAuthorityDomain,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) -> SymbolicDeclarationShapeSkeleton {
    SymbolicDeclarationShapeSkeleton {
        generic_parameters: shape.generic_parameters.clone(),
        // Predicate templates are a separate authority amendment. Retaining
        // them here keeps that independent pending path fail-closed.
        predicates: shape.predicates.clone(),
        payload: resolve_payload_contextual_self(input, inputs, &shape.payload, domain, blockers),
    }
}

fn resolve_payload_contextual_self(
    input: &InputRow<'_>,
    inputs: &[InputRow<'_>],
    payload: &SymbolicDeclarationPayloadSkeleton,
    domain: PendingAuthorityDomain,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) -> SymbolicDeclarationPayloadSkeleton {
    use arche_frontend::{
        SymbolicFieldShapeSkeleton, SymbolicImpliedCapabilityRequirementSkeleton,
        SymbolicMethodShapeSkeleton, SymbolicQueryTermShapeSkeleton, SymbolicRecordShapeSkeleton,
        SymbolicVariantShapeSkeleton,
    };
    let type_shape = |shape: &SymbolicTypeShapeSkeleton,
                      blockers: &mut Vec<DeclarationCheckBlocker>| {
        resolve_contextual_self_type_shape(input, inputs, shape, domain, blockers)
    };
    match payload {
        SymbolicDeclarationPayloadSkeleton::World => SymbolicDeclarationPayloadSkeleton::World,
        SymbolicDeclarationPayloadSkeleton::Tag => SymbolicDeclarationPayloadSkeleton::Tag,
        SymbolicDeclarationPayloadSkeleton::Record(record) => {
            SymbolicDeclarationPayloadSkeleton::Record(SymbolicRecordShapeSkeleton {
                form: record.form,
                fields: record
                    .fields
                    .iter()
                    .map(|field| SymbolicFieldShapeSkeleton {
                        name: field.name.clone(),
                        ty: type_shape(&field.ty, blockers),
                    })
                    .collect(),
            })
        }
        SymbolicDeclarationPayloadSkeleton::Enum(variants) => {
            SymbolicDeclarationPayloadSkeleton::Enum(
                variants
                    .iter()
                    .map(|variant| SymbolicVariantShapeSkeleton {
                        name: variant.name.clone(),
                        form: variant.form,
                        fields: variant
                            .fields
                            .iter()
                            .map(|field| SymbolicFieldShapeSkeleton {
                                name: field.name.clone(),
                                ty: type_shape(&field.ty, blockers),
                            })
                            .collect(),
                    })
                    .collect(),
            )
        }
        SymbolicDeclarationPayloadSkeleton::Callable(callable) => {
            SymbolicDeclarationPayloadSkeleton::Callable(Box::new(
                resolve_callable_contextual_self(input, inputs, callable, domain, blockers),
            ))
        }
        SymbolicDeclarationPayloadSkeleton::System {
            accesses,
            implied_requires,
            result,
            effects,
        } => SymbolicDeclarationPayloadSkeleton::System {
            accesses: accesses
                .iter()
                .map(|access| match access {
                    SymbolicSystemAccessShapeSkeleton::CapabilityShared(ty) => {
                        SymbolicSystemAccessShapeSkeleton::CapabilityShared(type_shape(
                            ty, blockers,
                        ))
                    }
                    SymbolicSystemAccessShapeSkeleton::CapabilityMutable(ty) => {
                        SymbolicSystemAccessShapeSkeleton::CapabilityMutable(type_shape(
                            ty, blockers,
                        ))
                    }
                    SymbolicSystemAccessShapeSkeleton::ResourceRead(ty) => {
                        SymbolicSystemAccessShapeSkeleton::ResourceRead(type_shape(ty, blockers))
                    }
                    SymbolicSystemAccessShapeSkeleton::ResourceWrite(ty) => {
                        SymbolicSystemAccessShapeSkeleton::ResourceWrite(type_shape(ty, blockers))
                    }
                    SymbolicSystemAccessShapeSkeleton::Query(terms) => {
                        SymbolicSystemAccessShapeSkeleton::Query(
                            terms
                                .iter()
                                .map(|term| SymbolicQueryTermShapeSkeleton {
                                    kind: term.kind,
                                    ty: type_shape(&term.ty, blockers),
                                })
                                .collect(),
                        )
                    }
                    SymbolicSystemAccessShapeSkeleton::Commands => {
                        SymbolicSystemAccessShapeSkeleton::Commands
                    }
                })
                .collect(),
            implied_requires: implied_requires
                .iter()
                .map(|implied| SymbolicImpliedCapabilityRequirementSkeleton {
                    parameter_ordinal: implied.parameter_ordinal,
                    parameter_span: implied.parameter_span,
                    access: implied.access,
                    referent: type_shape(&implied.referent, blockers),
                    readiness: implied.readiness,
                })
                .collect(),
            result: type_shape(result, blockers),
            effects: resolve_contextual_self_effects(input, inputs, effects, domain, blockers),
        },
        SymbolicDeclarationPayloadSkeleton::Trait { methods } => {
            SymbolicDeclarationPayloadSkeleton::Trait {
                methods: methods
                    .iter()
                    .map(|method| {
                        let child = inputs.iter().find(|candidate| {
                            candidate.item.owner == Some(input.definition.hir_item)
                                && candidate.item.name.as_deref() == Some(method.name.as_str())
                        });
                        SymbolicMethodShapeSkeleton {
                            name: method.name.clone(),
                            shape: Box::new(child.map_or_else(
                                || method.shape.as_ref().clone(),
                                |child| {
                                    resolve_declaration_contextual_self(
                                        child,
                                        inputs,
                                        &method.shape,
                                        domain,
                                        blockers,
                                    )
                                },
                            )),
                        }
                    })
                    .collect(),
            }
        }
        SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref,
            target,
            is_default,
            methods,
        } => SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref: trait_ref
                .as_ref()
                .map(|trait_ref| type_shape(trait_ref, blockers)),
            target: type_shape(target, blockers),
            is_default: *is_default,
            methods: methods
                .iter()
                .map(|method| {
                    let child = inputs.iter().find(|candidate| {
                        candidate.item.owner == Some(input.definition.hir_item)
                            && candidate.item.name.as_deref() == Some(method.name.as_str())
                    });
                    SymbolicMethodShapeSkeleton {
                        name: method.name.clone(),
                        shape: Box::new(child.map_or_else(
                            || method.shape.as_ref().clone(),
                            |child| {
                                resolve_declaration_contextual_self(
                                    child,
                                    inputs,
                                    &method.shape,
                                    domain,
                                    blockers,
                                )
                            },
                        )),
                    }
                })
                .collect(),
        },
        SymbolicDeclarationPayloadSkeleton::Alias { target } => {
            SymbolicDeclarationPayloadSkeleton::Alias {
                target: type_shape(target, blockers),
            }
        }
        SymbolicDeclarationPayloadSkeleton::Const { ty } => {
            SymbolicDeclarationPayloadSkeleton::Const {
                ty: type_shape(ty, blockers),
            }
        }
        SymbolicDeclarationPayloadSkeleton::Static { mutable, ty } => {
            SymbolicDeclarationPayloadSkeleton::Static {
                mutable: *mutable,
                ty: type_shape(ty, blockers),
            }
        }
        SymbolicDeclarationPayloadSkeleton::Query { terms } => {
            SymbolicDeclarationPayloadSkeleton::Query {
                terms: terms
                    .iter()
                    .map(|term| SymbolicQueryTermShapeSkeleton {
                        kind: term.kind,
                        ty: type_shape(&term.ty, blockers),
                    })
                    .collect(),
            }
        }
        SymbolicDeclarationPayloadSkeleton::Schedule { effects, readiness } => {
            SymbolicDeclarationPayloadSkeleton::Schedule {
                effects: resolve_contextual_self_effects(input, inputs, effects, domain, blockers),
                readiness: *readiness,
            }
        }
    }
}

fn resolve_callable_contextual_self(
    input: &InputRow<'_>,
    inputs: &[InputRow<'_>],
    callable: &SymbolicCallableShapeSkeleton,
    domain: PendingAuthorityDomain,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) -> SymbolicCallableShapeSkeleton {
    SymbolicCallableShapeSkeleton {
        kind: callable.kind,
        parameters: callable
            .parameters
            .iter()
            .map(
                |parameter| arche_frontend::SymbolicCallableParameterSkeleton {
                    mode: parameter.mode,
                    ty: resolve_contextual_self_type_shape(
                        input,
                        inputs,
                        &parameter.ty,
                        domain,
                        blockers,
                    ),
                },
            )
            .collect(),
        result: resolve_contextual_self_type_shape(
            input,
            inputs,
            &callable.result,
            domain,
            blockers,
        ),
        unsafe_: callable.unsafe_,
        resume: callable.resume.as_ref().map(|resume| {
            resolve_contextual_self_type_shape(input, inputs, resume, domain, blockers)
        }),
        yields: callable.yields.as_ref().map(|yields| {
            resolve_contextual_self_type_shape(input, inputs, yields, domain, blockers)
        }),
        effects: resolve_contextual_self_effects(
            input,
            inputs,
            &callable.effects,
            domain,
            blockers,
        ),
    }
}

fn resolve_contextual_self_effects(
    input: &InputRow<'_>,
    inputs: &[InputRow<'_>],
    effects: &SymbolicEffectSetsSkeleton,
    domain: PendingAuthorityDomain,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) -> SymbolicEffectSetsSkeleton {
    let resolve = |effect: &SymbolicEffectShapeSkeleton,
                   blockers: &mut Vec<DeclarationCheckBlocker>| {
        match effect {
            SymbolicEffectShapeSkeleton::Pending(pending)
                if pending.kind == PendingShapeKind::ContextualSelf =>
            {
                resolve_contextual_self_template(input, inputs, pending, domain, blockers)
                    .map_or_else(
                        || SymbolicEffectShapeSkeleton::Pending(pending.clone()),
                        SymbolicEffectShapeSkeleton::resolved_pending_c4,
                    )
            }
            _ => effect.clone(),
        }
    };
    SymbolicEffectSetsSkeleton {
        requires: effects
            .requires
            .iter()
            .map(|effect| resolve(effect, blockers))
            .collect(),
        throws: effects
            .throws
            .iter()
            .map(|effect| resolve(effect, blockers))
            .collect(),
    }
}

fn resolve_contextual_self_type_shape(
    input: &InputRow<'_>,
    inputs: &[InputRow<'_>],
    shape: &SymbolicTypeShapeSkeleton,
    domain: PendingAuthorityDomain,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) -> SymbolicTypeShapeSkeleton {
    let SymbolicTypeShapeSkeleton::Pending(pending) = shape else {
        return shape.clone();
    };
    if pending.kind != PendingShapeKind::ContextualSelf {
        return shape.clone();
    }
    resolve_contextual_self_template(input, inputs, pending, domain, blockers)
        .map_or_else(|| shape.clone(), SymbolicTypeShapeSkeleton::resolved)
}

fn resolve_contextual_self_template(
    input: &InputRow<'_>,
    inputs: &[InputRow<'_>],
    pending: &SymbolicPendingShape,
    domain: PendingAuthorityDomain,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) -> Option<SymbolicType> {
    let template = match input
        .item
        .symbolic_shape
        .contextual_self_type_template(pending)
    {
        Ok(template) => template,
        Err(error) => {
            push_contextual_self_blocker(input, domain, pending, error, blockers);
            return None;
        }
    };
    let self_type = match contextual_self_authority(input, inputs) {
        Ok(self_type) => self_type,
        Err(failure) => {
            push_contextual_self_failure(input, domain, pending, failure, blockers);
            return None;
        }
    };
    match template.instantiate_contextual_self(&self_type) {
        Ok(instantiated) => Some(instantiated),
        Err(C2TypeTemplateInstantiationError::ReservedTemplateCoordinate) => {
            push_contextual_self_failure(
                input,
                domain,
                pending,
                ContextualSelfTemplateFailure::ReservedTemplateCoordinate,
                blockers,
            );
            None
        }
    }
}

fn contextual_self_authority(
    input: &InputRow<'_>,
    inputs: &[InputRow<'_>],
) -> Result<SymbolicType, ContextualSelfTemplateFailure> {
    if input.item.kind == DeclarationKind::Trait {
        return Ok(SymbolicType::BoundType {
            depth: 0,
            index: u64::try_from(input.item.symbolic_shape.generic_parameters.len())
                .map_err(|_| ContextualSelfTemplateFailure::MissingSelfAuthority)?,
        });
    }
    if let Some(parent) = input.item.owner.and_then(|owner| {
        inputs
            .iter()
            .find(|candidate| candidate.definition.hir_item == owner)
    }) {
        if parent.item.kind == DeclarationKind::Trait {
            return Ok(SymbolicType::BoundType {
                depth: 1,
                index: u64::try_from(parent.item.symbolic_shape.generic_parameters.len())
                    .map_err(|_| ContextualSelfTemplateFailure::MissingSelfAuthority)?,
            });
        }
        if parent.item.kind == DeclarationKind::Impl {
            let target = canonical_impl_target(parent)?;
            return impl_method_lift(&parent.definition.symbolic_shape.generic_parameters)
                .substitute_type(&target, 0)
                .map_err(|_| ContextualSelfTemplateFailure::MissingCanonicalImplTarget);
        }
    }
    if input.item.kind == DeclarationKind::Impl {
        return canonical_impl_target(input);
    }
    Err(ContextualSelfTemplateFailure::MissingSelfAuthority)
}

fn canonical_impl_target(
    input: &InputRow<'_>,
) -> Result<SymbolicType, ContextualSelfTemplateFailure> {
    let SymbolicDeclarationPayloadSkeleton::Impl { target, .. } =
        &input.definition.symbolic_shape.payload
    else {
        return Err(ContextualSelfTemplateFailure::MissingCanonicalImplTarget);
    };
    resolved_type(target)
        .cloned()
        .ok_or(ContextualSelfTemplateFailure::MissingCanonicalImplTarget)
}

fn push_contextual_self_blocker(
    input: &InputRow<'_>,
    domain: PendingAuthorityDomain,
    pending: &SymbolicPendingShape,
    error: C2TypeTemplateLookupError,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) {
    let (span, debug_spelling, failure) = contextual_self_lookup_failure(pending, error);
    push_contextual_self_failure_at(
        input,
        domain,
        frontend_span(span),
        debug_spelling,
        failure,
        blockers,
    );
}

fn contextual_self_lookup_failure(
    pending: &SymbolicPendingShape,
    error: C2TypeTemplateLookupError,
) -> (
    arche_frontend::SymbolicSourceSpan,
    String,
    ContextualSelfTemplateFailure,
) {
    match error {
        C2TypeTemplateLookupError::NotContextualSelf => (
            pending.source_span,
            pending.debug_spelling.clone(),
            ContextualSelfTemplateFailure::NotContextualSelf,
        ),
        C2TypeTemplateLookupError::Missing => (
            pending.source_span,
            pending.debug_spelling.clone(),
            ContextualSelfTemplateFailure::Missing,
        ),
        C2TypeTemplateLookupError::Ambiguous => (
            pending.source_span,
            pending.debug_spelling.clone(),
            ContextualSelfTemplateFailure::Ambiguous,
        ),
        C2TypeTemplateLookupError::Blocked(C2TypeTemplateBlocker::AdditionalPending {
            source_span,
            kind,
            debug_spelling,
        }) => (
            source_span,
            debug_spelling,
            ContextualSelfTemplateFailure::AdditionalPending(kind),
        ),
        C2TypeTemplateLookupError::Blocked(C2TypeTemplateBlocker::FrontendInvariant {
            source_span,
            code,
            message,
        }) => (
            source_span.unwrap_or(pending.source_span),
            format!("{code}: {message}"),
            ContextualSelfTemplateFailure::FrontendInvariant,
        ),
    }
}

fn push_contextual_self_failure(
    input: &InputRow<'_>,
    domain: PendingAuthorityDomain,
    pending: &SymbolicPendingShape,
    failure: ContextualSelfTemplateFailure,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) {
    push_contextual_self_failure_at(
        input,
        domain,
        frontend_span(pending.source_span),
        pending.debug_spelling.clone(),
        failure,
        blockers,
    );
}

fn push_contextual_self_failure_at(
    input: &InputRow<'_>,
    domain: PendingAuthorityDomain,
    span: Span,
    debug_spelling: String,
    failure: ContextualSelfTemplateFailure,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) {
    blockers.push(DeclarationCheckBlocker {
        package: package_scope(input),
        target: input.target,
        path: input.path.clone(),
        span,
        item: input.definition.hir_item,
        debug_spelling,
        reason: DeclarationCheckBlockerReason::ContextualSelfTemplate { domain, failure },
    });
}

fn report_inconsistencies(
    input: &InputRow<'_>,
    audit: &ShapeGateAudit,
    label: &str,
    diagnostics: &mut Vec<SemanticDiagnostic>,
) {
    for inconsistency in audit.inconsistencies() {
        push_diagnostic(
            input,
            "IDENTITY001",
            format!(
                "C1 {label} has an inconsistent recursive readiness claim: {}",
                inconsistency_message(inconsistency)
            ),
            input.definition.key.span,
            diagnostics,
        );
    }
}

fn inconsistency_message(inconsistency: &ShapeReadinessInconsistency) -> String {
    match inconsistency {
        ShapeReadinessInconsistency::PendingLeafNotPendingC2 { kind, readiness } => {
            format!("pending {kind:?} leaf is marked {readiness:?}")
        }
        ShapeReadinessInconsistency::ResolvedLeafMarkedPendingC2 => {
            "a resolved leaf is marked PendingC2".to_owned()
        }
        ShapeReadinessInconsistency::ResolvedLeafClaimsConstIndependent => {
            "a resolved leaf claims const independence while retaining a later dependency"
                .to_owned()
        }
        ShapeReadinessInconsistency::ImpliedCapabilityNotPendingC4(readiness) => {
            format!("an implied capability row is marked {readiness:?} instead of PendingC4")
        }
        ShapeReadinessInconsistency::ScheduleNotPendingC4(readiness) => {
            format!("a schedule row is marked {readiness:?} instead of PendingC4")
        }
        ShapeReadinessInconsistency::NestedEffectSetMarkedPendingC2 => {
            "a nested effect set is marked PendingC2 instead of retaining a structured leaf"
                .to_owned()
        }
    }
}

fn collect_pending_blockers(
    input: &InputRow<'_>,
    audit: &ShapeGateAudit,
    domain: PendingAuthorityDomain,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) {
    for pending in audit.pending_c2() {
        if pending.kind == PendingShapeKind::ContextualSelf
            && blockers.iter().any(|blocker| {
                blocker.item == input.definition.hir_item
                    && blocker.span == frontend_span(pending.source_span)
                    && blocker.debug_spelling == pending.debug_spelling
                    && matches!(
                        &blocker.reason,
                        DeclarationCheckBlockerReason::ContextualSelfTemplate {
                            domain: blocker_domain,
                            ..
                        } if *blocker_domain == domain
                    )
            })
        {
            continue;
        }
        blockers.push(DeclarationCheckBlocker {
            package: package_scope(input),
            target: input.target,
            path: input.path.clone(),
            span: frontend_span(pending.source_span),
            item: input.definition.hir_item,
            debug_spelling: pending.debug_spelling.clone(),
            reason: DeclarationCheckBlockerReason::MissingStructuredPendingAuthority {
                domain,
                kind: pending.kind,
            },
        });
    }
}

fn validate_generic_layout(
    input: &InputRow<'_>,
    shape: &SymbolicDeclarationShapeSkeleton,
) -> Result<(), String> {
    let mut expected = input
        .item
        .symbolic_shape
        .generic_parameters
        .iter()
        .map(|parameter| parameter.kind.clone())
        .collect::<Vec<_>>();
    expected.extend(
        input
            .item
            .symbolic_shape
            .hidden_lifetime_binders
            .iter()
            .map(|_| GenericParameterKind::Lifetime),
    );
    if shape.generic_parameters != expected {
        return Err(format!(
            "checked declaration generic binder layout differs from retained C1 authority: expected {expected:?}, found {:?}",
            shape.generic_parameters
        ));
    }
    if input.item.kind == DeclarationKind::Trait
        && !input.item.symbolic_shape.hidden_lifetime_binders.is_empty()
    {
        return Err(
            "trait declaration cannot place a hidden lifetime binder in the implicit Self slot"
                .to_owned(),
        );
    }
    Ok(())
}

fn binder_stack_for(
    input: &InputRow<'_>,
    inputs: &[InputRow<'_>],
    shape: &SymbolicDeclarationShapeSkeleton,
) -> BinderStack {
    let current = if input.item.kind == DeclarationKind::Trait {
        BinderFrame::trait_declaration(
            input
                .item
                .symbolic_shape
                .generic_parameters
                .iter()
                .map(|parameter| parameter.kind.clone())
                .collect(),
        )
    } else {
        BinderFrame::declaration(shape.generic_parameters.clone())
    };
    let Some(parent) = input.item.owner else {
        return BinderStack::new(vec![current]);
    };
    let Some(parent) = inputs
        .iter()
        .find(|candidate| candidate.definition.hir_item == parent)
    else {
        return BinderStack::new(vec![current]);
    };
    let outer = if parent.item.kind == DeclarationKind::Trait {
        BinderFrame::trait_declaration(
            parent
                .item
                .symbolic_shape
                .generic_parameters
                .iter()
                .map(|parameter| parameter.kind.clone())
                .collect(),
        )
    } else {
        BinderFrame::declaration(parent.definition.symbolic_shape.generic_parameters.clone())
    };
    BinderStack::new(vec![current, outer])
}

fn validate_checked_shape(
    shape: &SymbolicDeclarationShapeSkeleton,
    binders: &BinderStack,
    catalog: &DeclarationCatalog<'_>,
) -> Result<(), String> {
    for predicate in &shape.predicates {
        let SymbolicPredicateShapeSkeleton::Resolved { value, .. } = predicate else {
            return Err("post-C2 declaration retains a pending predicate".to_owned());
        };
        validate_symbolic_predicate(value, binders)
            .map_err(|error| format!("invalid bound coordinate in predicate: {error:?}"))?;
        validate_predicate_formation(value, catalog)?;
    }
    match &shape.payload {
        SymbolicDeclarationPayloadSkeleton::World | SymbolicDeclarationPayloadSkeleton::Tag => {}
        SymbolicDeclarationPayloadSkeleton::Record(record) => {
            for field in &record.fields {
                validate_type_shape(&field.ty, binders, catalog)?;
            }
        }
        SymbolicDeclarationPayloadSkeleton::Enum(variants) => {
            for field in variants.iter().flat_map(|variant| &variant.fields) {
                validate_type_shape(&field.ty, binders, catalog)?;
            }
        }
        SymbolicDeclarationPayloadSkeleton::Callable(callable) => {
            validate_callable(callable, binders, catalog)?;
        }
        SymbolicDeclarationPayloadSkeleton::System {
            accesses,
            implied_requires,
            result,
            effects,
        } => {
            for access in accesses {
                match access {
                    SymbolicSystemAccessShapeSkeleton::CapabilityShared(ty)
                    | SymbolicSystemAccessShapeSkeleton::CapabilityMutable(ty)
                    | SymbolicSystemAccessShapeSkeleton::ResourceRead(ty)
                    | SymbolicSystemAccessShapeSkeleton::ResourceWrite(ty) => {
                        validate_type_shape(ty, binders, catalog)?;
                    }
                    SymbolicSystemAccessShapeSkeleton::Query(terms) => {
                        for term in terms {
                            validate_type_shape(&term.ty, binders, catalog)?;
                        }
                    }
                    SymbolicSystemAccessShapeSkeleton::Commands => {}
                }
            }
            for implied in implied_requires {
                validate_type_shape(&implied.referent, binders, catalog)?;
            }
            validate_type_shape(result, binders, catalog)?;
            validate_effects(effects, binders, catalog)?;
        }
        SymbolicDeclarationPayloadSkeleton::Trait { methods }
        | SymbolicDeclarationPayloadSkeleton::Impl { methods, .. } => {
            if let SymbolicDeclarationPayloadSkeleton::Impl {
                trait_ref, target, ..
            } = &shape.payload
            {
                if let Some(trait_ref) = trait_ref {
                    validate_type_shape(trait_ref, binders, catalog)?;
                }
                validate_type_shape(target, binders, catalog)?;
            }
            for method in methods {
                let mut method_binders = binders.clone();
                method_binders.push_innermost(BinderFrame::declaration(
                    method.shape.generic_parameters.clone(),
                ));
                validate_checked_shape(&method.shape, &method_binders, catalog)?;
            }
        }
        SymbolicDeclarationPayloadSkeleton::Alias { target } => {
            validate_type_shape(target, binders, catalog)?;
        }
        SymbolicDeclarationPayloadSkeleton::Const { ty }
        | SymbolicDeclarationPayloadSkeleton::Static { ty, .. } => {
            validate_type_shape(ty, binders, catalog)?;
        }
        SymbolicDeclarationPayloadSkeleton::Query { terms } => {
            for term in terms {
                validate_type_shape(&term.ty, binders, catalog)?;
            }
        }
        SymbolicDeclarationPayloadSkeleton::Schedule { effects, .. } => {
            validate_effects(effects, binders, catalog)?;
        }
    }
    Ok(())
}

fn validate_callable(
    callable: &SymbolicCallableShapeSkeleton,
    binders: &BinderStack,
    catalog: &DeclarationCatalog<'_>,
) -> Result<(), String> {
    for parameter in &callable.parameters {
        validate_type_shape(&parameter.ty, binders, catalog)?;
    }
    validate_type_shape(&callable.result, binders, catalog)?;
    if let Some(resume) = &callable.resume {
        validate_type_shape(resume, binders, catalog)?;
    }
    if let Some(yields) = &callable.yields {
        validate_type_shape(yields, binders, catalog)?;
    }
    validate_effects(&callable.effects, binders, catalog)
}

fn validate_effects(
    effects: &SymbolicEffectSetsSkeleton,
    binders: &BinderStack,
    catalog: &DeclarationCatalog<'_>,
) -> Result<(), String> {
    for effect in effects.requires.iter().chain(&effects.throws) {
        let SymbolicEffectShapeSkeleton::Resolved { value, .. } = effect else {
            return Err("post-C2 declaration retains a pending effect member".to_owned());
        };
        validate_symbolic_type(value, binders)
            .map_err(|error| format!("invalid bound coordinate in effect member: {error:?}"))?;
        validate_type_formation(value, catalog)?;
    }
    Ok(())
}

fn validate_type_shape(
    shape: &SymbolicTypeShapeSkeleton,
    binders: &BinderStack,
    catalog: &DeclarationCatalog<'_>,
) -> Result<(), String> {
    let SymbolicTypeShapeSkeleton::Resolved { value, .. } = shape else {
        return Err("post-C2 declaration retains a pending type".to_owned());
    };
    validate_symbolic_type(value, binders)
        .map_err(|error| format!("invalid bound coordinate in type: {error:?}"))?;
    validate_type_formation(value, catalog)
}

fn validate_predicate_formation(
    predicate: &SymbolicPredicate,
    catalog: &DeclarationCatalog<'_>,
) -> Result<(), String> {
    match predicate {
        SymbolicPredicate::Trait {
            trait_path,
            self_type,
            arguments,
        } => {
            if trait_path.kind != DeclarationKind::Trait {
                return Err(format!(
                    "trait predicate names non-trait declaration {}",
                    trait_path.name
                ));
            }
            validate_declaration_arguments(trait_path, arguments, catalog)?;
            validate_type_formation(self_type, catalog)?;
            validate_argument_formations(arguments, catalog)
        }
        SymbolicPredicate::LifetimeOutlives { .. } => Ok(()),
        SymbolicPredicate::TypeOutlives { ty, .. } => validate_type_formation(ty, catalog),
    }
}

fn validate_type_formation(
    ty: &SymbolicType,
    catalog: &DeclarationCatalog<'_>,
) -> Result<(), String> {
    match ty {
        SymbolicType::Slice(element)
        | SymbolicType::RawPointer {
            pointee: element, ..
        }
        | SymbolicType::Reference {
            pointee: element, ..
        } => validate_type_formation(element, catalog),
        SymbolicType::Array { element, length } => {
            validate_type_formation(element, catalog)?;
            validate_const_paths(length, catalog)
        }
        SymbolicType::Tuple(elements) => {
            for element in elements {
                validate_type_formation(element, catalog)?;
            }
            Ok(())
        }
        SymbolicType::NominalPath {
            declaration,
            arguments,
        } => {
            validate_declaration_arguments(declaration, arguments, catalog)?;
            validate_argument_formations(arguments, catalog)
        }
        SymbolicType::FunctionPointer {
            parameters,
            result,
            requires,
            throws,
            ..
        } => {
            validate_types(parameters, catalog)?;
            validate_type_formation(result, catalog)?;
            validate_types(requires.members(), catalog)?;
            validate_types(throws.members(), catalog)
        }
        SymbolicType::Closure {
            captures,
            parameters,
            result,
            requires,
            throws,
            arguments,
            ..
        } => {
            for capture in captures {
                validate_type_formation(&capture.ty, catalog)?;
            }
            validate_types(parameters, catalog)?;
            validate_type_formation(result, catalog)?;
            validate_types(requires.members(), catalog)?;
            validate_types(throws.members(), catalog)?;
            validate_argument_formations(arguments, catalog)
        }
        SymbolicType::Generator {
            target,
            captures,
            parameters,
            resume,
            yields,
            result,
            requires,
            throws,
            ..
        } => {
            validate_generator_target(target, catalog)?;
            for capture in captures {
                validate_type_formation(&capture.ty, catalog)?;
            }
            validate_types(parameters, catalog)?;
            validate_type_formation(resume, catalog)?;
            validate_type_formation(yields, catalog)?;
            validate_type_formation(result, catalog)?;
            validate_types(requires.members(), catalog)?;
            validate_types(throws.members(), catalog)
        }
        SymbolicType::JoinHandle { result, throws } => {
            validate_type_formation(result, catalog)?;
            validate_types(throws.members(), catalog)
        }
        SymbolicType::GeneratorFactory {
            target,
            captures,
            parameters,
            produced_generator,
            ..
        } => {
            validate_generator_target(target, catalog)?;
            for capture in captures {
                validate_type_formation(&capture.ty, catalog)?;
            }
            validate_types(parameters, catalog)?;
            validate_type_formation(produced_generator, catalog)
        }
        SymbolicType::I8
        | SymbolicType::I16
        | SymbolicType::I32
        | SymbolicType::I64
        | SymbolicType::U8
        | SymbolicType::U16
        | SymbolicType::U32
        | SymbolicType::U64
        | SymbolicType::Isize
        | SymbolicType::Usize
        | SymbolicType::F32
        | SymbolicType::F64
        | SymbolicType::Bool
        | SymbolicType::Char
        | SymbolicType::Entity
        | SymbolicType::Unit
        | SymbolicType::Never
        | SymbolicType::Str
        | SymbolicType::BoundType { .. } => Ok(()),
    }
}

fn validate_declaration_arguments(
    declaration: &SemanticDeclarationPath,
    arguments: &[GenericArgumentShape],
    catalog: &DeclarationCatalog<'_>,
) -> Result<(), String> {
    if let Some(row) = catalog.rows.get(declaration) {
        validate_generic_arguments(&row.source_formals, arguments).map_err(|error| {
            format!(
                "generic formation for {} does not match its exact declaration formals: {error:?}",
                declaration.name
            )
        })?;
        return Ok(());
    }
    if is_embedded_path(declaration, catalog.embedded) {
        // C1's branded projection proves each embedded path row and its source
        // argument partition. The current typed nominal projection does not
        // expose nominal formal kinds, so this module must not parse the
        // release-only shape string as a second authority. Compiler-trait
        // arity/designated-Self is independently checked by typed metadata.
        return Ok(());
    }
    Err(format!(
        "symbolic type references declaration path with no retained C1 row: {}/{}",
        declaration.package_name, declaration.name
    ))
}

fn validate_argument_formations(
    arguments: &[GenericArgumentShape],
    catalog: &DeclarationCatalog<'_>,
) -> Result<(), String> {
    for argument in arguments {
        match argument {
            GenericArgumentShape::Type(ty) => validate_type_formation(ty, catalog)?,
            GenericArgumentShape::Lifetime(_) => {}
            GenericArgumentShape::IntegerConst(value) => validate_const_paths(value, catalog)?,
        }
    }
    Ok(())
}

fn validate_const_paths(
    value: &arche_frontend::SymbolicConstExpression,
    catalog: &DeclarationCatalog<'_>,
) -> Result<(), String> {
    use arche_frontend::SymbolicConstNode;
    match &value.node {
        SymbolicConstNode::ConstDefinitionPath(path) => {
            if path.kind != DeclarationKind::Const {
                return Err(format!(
                    "symbolic const path {} names a non-const declaration",
                    path.name
                ));
            }
            if catalog.rows.contains_key(path) || is_embedded_path(path, catalog.embedded) {
                Ok(())
            } else {
                Err(format!(
                    "symbolic const references path with no retained C1 row: {}/{}",
                    path.package_name, path.name
                ))
            }
        }
        SymbolicConstNode::WrappingNeg(child) | SymbolicConstNode::BitNot(child) => {
            validate_const_paths(child, catalog)
        }
        SymbolicConstNode::WrappingMul(left, right)
        | SymbolicConstNode::IntegerDivide(left, right)
        | SymbolicConstNode::IntegerRemainder(left, right)
        | SymbolicConstNode::WrappingAdd(left, right)
        | SymbolicConstNode::WrappingSub(left, right)
        | SymbolicConstNode::MaskedShiftLeft(left, right)
        | SymbolicConstNode::MaskedShiftRight(left, right)
        | SymbolicConstNode::BitAnd(left, right)
        | SymbolicConstNode::BitXor(left, right)
        | SymbolicConstNode::BitOr(left, right) => {
            validate_const_paths(left, catalog)?;
            validate_const_paths(right, catalog)
        }
        SymbolicConstNode::IntegerLiteral(_) | SymbolicConstNode::Bound { .. } => Ok(()),
    }
}

fn validate_types(types: &[SymbolicType], catalog: &DeclarationCatalog<'_>) -> Result<(), String> {
    for ty in types {
        validate_type_formation(ty, catalog)?;
    }
    Ok(())
}

fn validate_generator_target(
    target: &arche_frontend::GeneratorTarget,
    catalog: &DeclarationCatalog<'_>,
) -> Result<(), String> {
    let (declaration, arguments) = match target {
        arche_frontend::GeneratorTarget::Named {
            declaration,
            arguments,
            ..
        } => (Some(declaration), arguments.as_slice()),
        arche_frontend::GeneratorTarget::Anonymous {
            owner, arguments, ..
        } => (Some(owner), arguments.as_slice()),
    };
    if let Some(declaration) = declaration {
        validate_declaration_arguments(declaration, arguments, catalog)?;
    }
    validate_argument_formations(arguments, catalog)
}

fn is_embedded_path(
    path: &SemanticDeclarationPath,
    embedded: &VerifiedEmbeddedCoreAuthority,
) -> bool {
    let projection = embedded.projection();
    path.registry_origin == projection.registry_origin()
        && path.package_name == projection.scoped_name()
        && path.target == TargetRoot::Library
        && path.modules.is_empty()
}

fn gate_state(
    input: &InputRow<'_>,
    definition: &ShapeGateAudit,
    owner: &ShapeGateAudit,
    diagnostics: &mut Vec<SemanticDiagnostic>,
) -> (C2Resolution, PendingC4Dependencies) {
    let ctfe = definition
        .ctfe_dependencies()
        .iter()
        .chain(owner.ctfe_dependencies())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut obligations = Vec::new();
    for path in ctfe {
        let value = arche_frontend::SymbolicConstExpression {
            integer_type: arche_frontend::IntegerType::Usize,
            node: arche_frontend::SymbolicConstNode::ConstDefinitionPath(path),
        };
        match encode_symbolic_const(&value)
            .ok()
            .and_then(|bytes| NeedsCtfeObligation::from_canonical_bytes(bytes).ok())
        {
            Some(obligation) => obligations.push(obligation),
            None => push_diagnostic(
                input,
                "IDENTITY001",
                "could not encode a canonical C2 CTFE obligation",
                input.definition.key.span,
                diagnostics,
            ),
        }
    }
    let resolution = if obligations.is_empty() {
        C2Resolution::Complete
    } else {
        C2Resolution::NeedsCtfe(
            NeedsCtfeObligations::from_unsorted(obligations)
                .expect("nonempty encoded CTFE dependency set"),
        )
    };

    let mut pending = Vec::new();
    for (domain, count) in [
        (0_u8, definition.pending_c4_count()),
        (1_u8, owner.pending_c4_count()),
    ] {
        for ordinal in 0..count {
            let mut bytes = b"ARCHE-C2-PENDING-C4\0".to_vec();
            bytes.push(domain);
            bytes.extend_from_slice(
                &u64::try_from(input.declaration.session_traversal_bytes().len())
                    .expect("declaration key length already fits checked encoding")
                    .to_le_bytes(),
            );
            bytes.extend_from_slice(input.declaration.session_traversal_bytes());
            bytes.extend_from_slice(&ordinal.to_le_bytes());
            pending.push(
                PendingC4Dependency::from_canonical_bytes(bytes)
                    .expect("C2 pending-C4 dependency encoding is nonempty"),
            );
        }
    }
    (resolution, PendingC4Dependencies::from_unsorted(pending))
}

fn describe_impl(
    index: usize,
    row: &CheckedDeclarationRow,
    inputs: &[InputRow<'_>],
    rows: &[Option<CheckedDeclarationRow>],
    catalog: &DeclarationCatalog<'_>,
    diagnostics: &mut Vec<SemanticDiagnostic>,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) -> Option<OrdinaryImplCandidateSpec> {
    let input = &inputs[index];
    let SymbolicDeclarationPayloadSkeleton::Impl {
        trait_ref,
        target,
        is_default,
        methods,
    } = &row.declaration_shape.payload
    else {
        push_diagnostic(
            input,
            "IDENTITY001",
            "impl declaration row does not contain an impl shape",
            input.definition.key.span,
            diagnostics,
        );
        return None;
    };
    let Some(trait_ref) = trait_ref else {
        if *is_default {
            push_diagnostic(
                input,
                "TRAIT001",
                "an inherent impl cannot be declared default",
                input.definition.key.span,
                diagnostics,
            );
        }
        return None;
    };
    let target = resolved_type(target)?;
    let Some((trait_path, arguments)) = nominal_trait_ref(trait_ref) else {
        push_diagnostic(
            input,
            "TRAIT001",
            "trait impl header does not name one resolved trait application",
            input.definition.key.span,
            diagnostics,
        );
        return None;
    };

    let trait_key = if let Some(trait_row) = catalog.rows.get(trait_path) {
        if trait_row.declaration.kind() != DeclarationKind::Trait {
            push_diagnostic(
                input,
                "TRAIT001",
                format!(
                    "impl header names non-trait declaration {}",
                    trait_path.name
                ),
                input.definition.key.span,
                diagnostics,
            );
            return None;
        }
        let Some(trait_index) = inputs.iter().position(|candidate| {
            candidate.declaration.session_traversal_bytes()
                == trait_row.declaration.session_traversal_bytes()
        }) else {
            push_diagnostic(
                input,
                "IDENTITY001",
                "ordinary trait row is absent from the checked declaration table",
                input.definition.key.span,
                diagnostics,
            );
            return None;
        };
        let trait_checked = rows.get(trait_index).and_then(Option::as_ref)?;
        if !validate_ordinary_trait_impl(
            input,
            trait_checked,
            arguments,
            target,
            methods,
            diagnostics,
        ) {
            return None;
        }
        match SemanticTraitKey::from_ordinary_declaration(trait_row.declaration) {
            Ok(key) => key,
            Err(error) => {
                push_diagnostic(
                    input,
                    "IDENTITY001",
                    format!("could not construct ordinary semantic trait key: {error:?}"),
                    input.definition.key.span,
                    diagnostics,
                );
                return None;
            }
        }
    } else if let Some(compiler) = compiler_trait_for_path(catalog.embedded, trait_path) {
        let validation = CompilerTraitImplValidation {
            input,
            inputs,
            authority: compiler,
            impl_formals: &row.declaration_shape.generic_parameters,
            arguments,
            target,
            methods,
            embedded: catalog.embedded,
        };
        if !validate_compiler_trait_impl(&validation, diagnostics) {
            return None;
        }
        match SemanticTraitKey::from_verified_embedded_core(catalog.embedded, compiler.kind()) {
            Ok(key) => key,
            Err(TraitModelError::MissingFinalEmbeddedTraitIdentity(kind)) => {
                blockers.push(compiler_identity_blocker(input, kind));
                return None;
            }
            Err(error) => {
                push_diagnostic(
                    input,
                    "IDENTITY001",
                    format!("invalid compiler semantic trait key: {error:?}"),
                    input.definition.key.span,
                    diagnostics,
                );
                return None;
            }
        }
    } else {
        push_diagnostic(
            input,
            "TRAIT001",
            format!("impl header names unknown trait path {}", trait_path.name),
            input.definition.key.span,
            diagnostics,
        );
        return None;
    };

    let head = match TraitPredicate::new(trait_key, target.clone(), arguments.to_vec()) {
        Ok(head) => head,
        Err(error) => {
            push_diagnostic(
                input,
                "TRAIT001",
                format!("trait impl designated-Self or generic formation is invalid: {error:?}"),
                input.definition.key.span,
                diagnostics,
            );
            return None;
        }
    };
    let predicates = semantic_environment(
        input,
        &row.declaration_shape.predicates,
        catalog,
        diagnostics,
        blockers,
    )?;
    Some(OrdinaryImplCandidateSpec::new(
        *is_default,
        row.declaration_shape.generic_parameters.clone(),
        head,
        predicates,
    ))
}

fn nominal_trait_ref(
    shape: &SymbolicTypeShapeSkeleton,
) -> Option<(&SemanticDeclarationPath, &[GenericArgumentShape])> {
    let SymbolicType::NominalPath {
        declaration,
        arguments,
    } = resolved_type(shape)?
    else {
        return None;
    };
    (declaration.kind == DeclarationKind::Trait).then_some((declaration, arguments))
}

fn resolved_type(shape: &SymbolicTypeShapeSkeleton) -> Option<&SymbolicType> {
    match shape {
        SymbolicTypeShapeSkeleton::Resolved { value, .. } => Some(value),
        SymbolicTypeShapeSkeleton::Pending(_) => None,
    }
}

fn semantic_environment(
    input: &InputRow<'_>,
    predicates: &[SymbolicPredicateShapeSkeleton],
    catalog: &DeclarationCatalog<'_>,
    diagnostics: &mut Vec<SemanticDiagnostic>,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) -> Option<TraitEnvironment> {
    let mut semantic = Vec::with_capacity(predicates.len());
    for predicate in predicates {
        let SymbolicPredicateShapeSkeleton::Resolved { value, .. } = predicate else {
            return None;
        };
        semantic.push(semantic_predicate(
            input,
            value,
            catalog,
            diagnostics,
            blockers,
        )?);
    }
    match TraitEnvironment::new(semantic) {
        Ok(environment) => Some(environment),
        Err(error) => {
            push_diagnostic(
                input,
                "TRAIT001",
                format!("impl predicate set is not canonical: {error:?}"),
                input.definition.key.span,
                diagnostics,
            );
            None
        }
    }
}

fn semantic_predicate(
    input: &InputRow<'_>,
    predicate: &SymbolicPredicate,
    catalog: &DeclarationCatalog<'_>,
    diagnostics: &mut Vec<SemanticDiagnostic>,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) -> Option<SemanticPredicate> {
    match predicate {
        SymbolicPredicate::Trait {
            trait_path,
            self_type,
            arguments,
        } => {
            let key = if let Some(row) = catalog.rows.get(trait_path) {
                match SemanticTraitKey::from_ordinary_declaration(row.declaration) {
                    Ok(key) => key,
                    Err(error) => {
                        push_diagnostic(
                            input,
                            "IDENTITY001",
                            format!("invalid ordinary trait predicate key: {error:?}"),
                            input.definition.key.span,
                            diagnostics,
                        );
                        return None;
                    }
                }
            } else if let Some(compiler) = compiler_trait_for_path(catalog.embedded, trait_path) {
                match SemanticTraitKey::from_verified_embedded_core(
                    catalog.embedded,
                    compiler.kind(),
                ) {
                    Ok(key) => key,
                    Err(TraitModelError::MissingFinalEmbeddedTraitIdentity(kind)) => {
                        blockers.push(compiler_identity_blocker(input, kind));
                        return None;
                    }
                    Err(error) => {
                        push_diagnostic(
                            input,
                            "IDENTITY001",
                            format!("invalid compiler trait predicate key: {error:?}"),
                            input.definition.key.span,
                            diagnostics,
                        );
                        return None;
                    }
                }
            } else {
                push_diagnostic(
                    input,
                    "TRAIT001",
                    format!("predicate names unknown trait path {}", trait_path.name),
                    input.definition.key.span,
                    diagnostics,
                );
                return None;
            };
            TraitPredicate::new(key, self_type.clone(), arguments.clone())
                .map(SemanticPredicate::trait_bound)
                .map_err(|error| {
                    push_diagnostic(
                        input,
                        "TRAIT001",
                        format!("trait predicate designated-Self is invalid: {error:?}"),
                        input.definition.key.span,
                        diagnostics,
                    );
                })
                .ok()
        }
        SymbolicPredicate::LifetimeOutlives { longer, shorter } => {
            SemanticPredicate::lifetime_outlives(longer.clone(), shorter.clone())
                .map_err(|error| {
                    push_diagnostic(
                        input,
                        "IDENTITY001",
                        format!("invalid lifetime predicate encoding: {error:?}"),
                        input.definition.key.span,
                        diagnostics,
                    );
                })
                .ok()
        }
        SymbolicPredicate::TypeOutlives { ty, lifetime } => {
            SemanticPredicate::type_outlives(ty.clone(), lifetime.clone())
                .map_err(|error| {
                    push_diagnostic(
                        input,
                        "IDENTITY001",
                        format!("invalid type-outlives predicate encoding: {error:?}"),
                        input.definition.key.span,
                        diagnostics,
                    );
                })
                .ok()
        }
    }
}

fn compiler_trait_for_path<'a>(
    embedded: &'a VerifiedEmbeddedCoreAuthority,
    path: &SemanticDeclarationPath,
) -> Option<&'a CompilerTraitAuthority> {
    if !is_embedded_path(path, embedded) || path.kind != DeclarationKind::Trait {
        return None;
    }
    let definition = embedded.lookup_prelude_definition(&path.name, VirtualNamespace::Type)?;
    embedded
        .typed_c2()
        .compiler_trait_for_c1_definition(definition)
}

fn record_unimplemented_judgments(
    input: &InputRow<'_>,
    shape: &SymbolicDeclarationShapeSkeleton,
    embedded: &VerifiedEmbeddedCoreAuthority,
    blockers: &mut Vec<DeclarationCheckBlocker>,
) {
    let mut judgments = Vec::new();
    match &shape.payload {
        SymbolicDeclarationPayloadSkeleton::Record(_)
        | SymbolicDeclarationPayloadSkeleton::Enum(_) => {
            judgments.push(UnimplementedDeclarationJudgment::SizednessRecursion);
        }
        SymbolicDeclarationPayloadSkeleton::Alias { .. } => {
            judgments.push(UnimplementedDeclarationJudgment::TypeAliasCycle);
        }
        SymbolicDeclarationPayloadSkeleton::Impl { trait_ref, .. } => {
            judgments.push(if trait_ref.is_none() {
                UnimplementedDeclarationJudgment::InherentMethodUniqueness
            } else {
                UnimplementedDeclarationJudgment::ImplCoherenceOverlap
            });
        }
        _ => {}
    }
    if shape_mentions_embedded_map(shape, embedded) {
        judgments.push(UnimplementedDeclarationJudgment::MapKeyComparison);
    }
    for judgment in judgments {
        blockers.push(DeclarationCheckBlocker {
            package: package_scope(input),
            target: input.target,
            path: input.path.clone(),
            span: input.definition.key.span,
            item: input.definition.hir_item,
            debug_spelling: format!("{judgment:?}"),
            reason: DeclarationCheckBlockerReason::MissingDeclarationJudgment(judgment),
        });
    }
}

fn shape_mentions_embedded_map(
    shape: &SymbolicDeclarationShapeSkeleton,
    embedded: &VerifiedEmbeddedCoreAuthority,
) -> bool {
    let map = |ty: &SymbolicType| type_mentions_embedded_map(ty, embedded);
    let map_shape = |shape: &arche_frontend::SymbolicTypeShapeSkeleton| match shape {
        arche_frontend::SymbolicTypeShapeSkeleton::Resolved { value, .. } => map(value),
        arche_frontend::SymbolicTypeShapeSkeleton::Pending(_) => false,
    };
    let map_effects = |effects: &arche_frontend::SymbolicEffectSetsSkeleton| {
        effects
            .requires
            .iter()
            .chain(effects.throws.iter())
            .any(|effect| match effect {
                arche_frontend::SymbolicEffectShapeSkeleton::Resolved { value, .. } => map(value),
                arche_frontend::SymbolicEffectShapeSkeleton::Pending(_) => false,
            })
    };
    let map_callable = |callable: &arche_frontend::SymbolicCallableShapeSkeleton| {
        callable
            .parameters
            .iter()
            .any(|parameter| map_shape(&parameter.ty))
            || map_shape(&callable.result)
            || callable.resume.as_ref().is_some_and(map_shape)
            || callable.yields.as_ref().is_some_and(map_shape)
            || map_effects(&callable.effects)
    };
    let map_methods = |methods: &[arche_frontend::SymbolicMethodShapeSkeleton]| {
        methods
            .iter()
            .any(|method| shape_mentions_embedded_map(&method.shape, embedded))
    };
    let payload = match &shape.payload {
        SymbolicDeclarationPayloadSkeleton::World
        | SymbolicDeclarationPayloadSkeleton::Tag
        | SymbolicDeclarationPayloadSkeleton::Schedule { .. } => false,
        SymbolicDeclarationPayloadSkeleton::Record(record) => {
            record.fields.iter().any(|field| map_shape(&field.ty))
        }
        SymbolicDeclarationPayloadSkeleton::Enum(variants) => variants
            .iter()
            .any(|variant| variant.fields.iter().any(|field| map_shape(&field.ty))),
        SymbolicDeclarationPayloadSkeleton::Callable(callable) => map_callable(callable),
        SymbolicDeclarationPayloadSkeleton::System {
            accesses,
            implied_requires,
            result,
            effects,
        } => {
            accesses.iter().any(|access| match access {
                arche_frontend::SymbolicSystemAccessShapeSkeleton::CapabilityShared(ty)
                | arche_frontend::SymbolicSystemAccessShapeSkeleton::CapabilityMutable(ty)
                | arche_frontend::SymbolicSystemAccessShapeSkeleton::ResourceRead(ty)
                | arche_frontend::SymbolicSystemAccessShapeSkeleton::ResourceWrite(ty) => {
                    map_shape(ty)
                }
                arche_frontend::SymbolicSystemAccessShapeSkeleton::Query(terms) => {
                    terms.iter().any(|term| map_shape(&term.ty))
                }
                arche_frontend::SymbolicSystemAccessShapeSkeleton::Commands => false,
            }) || implied_requires
                .iter()
                .any(|requirement| map_shape(&requirement.referent))
                || map_shape(result)
                || map_effects(effects)
        }
        SymbolicDeclarationPayloadSkeleton::Trait { methods } => map_methods(methods),
        SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref,
            target,
            methods,
            ..
        } => trait_ref.as_ref().is_some_and(map_shape) || map_shape(target) || map_methods(methods),
        SymbolicDeclarationPayloadSkeleton::Alias { target } => map_shape(target),
        SymbolicDeclarationPayloadSkeleton::Const { ty }
        | SymbolicDeclarationPayloadSkeleton::Static { ty, .. } => map_shape(ty),
        SymbolicDeclarationPayloadSkeleton::Query { terms } => {
            terms.iter().any(|term| map_shape(&term.ty))
        }
    };
    payload
        || shape.predicates.iter().any(|predicate| match predicate {
            arche_frontend::SymbolicPredicateShapeSkeleton::Resolved { value, .. } => {
                match &**value {
                    SymbolicPredicate::Trait {
                        self_type,
                        arguments,
                        ..
                    } => {
                        map(self_type)
                            || arguments.iter().any(|argument| match argument {
                                GenericArgumentShape::Type(ty) => map(ty),
                                _ => false,
                            })
                    }
                    SymbolicPredicate::TypeOutlives { ty, .. } => map(ty),
                    SymbolicPredicate::LifetimeOutlives { .. } => false,
                }
            }
            arche_frontend::SymbolicPredicateShapeSkeleton::Pending(_) => false,
        })
}

fn type_mentions_embedded_map(ty: &SymbolicType, embedded: &VerifiedEmbeddedCoreAuthority) -> bool {
    let map = |child: &SymbolicType| type_mentions_embedded_map(child, embedded);
    let map_set = |set: &arche_frontend::SymbolicTypeEffectSet| set.members().iter().any(map);
    match ty {
        SymbolicType::NominalPath {
            declaration,
            arguments,
        } => {
            (declaration.name == "Map" && is_embedded_path(declaration, embedded))
                || arguments.iter().any(|argument| match argument {
                    GenericArgumentShape::Type(child) => map(child),
                    _ => false,
                })
        }
        SymbolicType::Slice(element) => map(element),
        SymbolicType::Array { element, .. } => map(element),
        SymbolicType::Tuple(elements) => elements.iter().any(map),
        SymbolicType::Reference { pointee, .. } | SymbolicType::RawPointer { pointee, .. } => {
            map(pointee)
        }
        SymbolicType::FunctionPointer {
            parameters,
            result,
            requires,
            throws,
            ..
        } => parameters.iter().any(map) || map(result) || map_set(requires) || map_set(throws),
        SymbolicType::Closure {
            captures,
            parameters,
            result,
            requires,
            throws,
            arguments,
            ..
        } => {
            captures.iter().any(|capture| map(&capture.ty))
                || parameters.iter().any(map)
                || map(result)
                || map_set(requires)
                || map_set(throws)
                || arguments.iter().any(|argument| match argument {
                    GenericArgumentShape::Type(child) => map(child),
                    _ => false,
                })
        }
        SymbolicType::Generator {
            captures,
            parameters,
            resume,
            yields,
            result,
            requires,
            throws,
            ..
        } => {
            captures.iter().any(|capture| map(&capture.ty))
                || parameters.iter().any(map)
                || map(resume)
                || map(yields)
                || map(result)
                || map_set(requires)
                || map_set(throws)
        }
        SymbolicType::JoinHandle { result, throws } => map(result) || map_set(throws),
        SymbolicType::GeneratorFactory {
            captures,
            parameters,
            produced_generator,
            ..
        } => {
            captures.iter().any(|capture| map(&capture.ty))
                || parameters.iter().any(map)
                || map(produced_generator)
        }
        _ => false,
    }
}

fn compiler_identity_blocker(
    input: &InputRow<'_>,
    kind: CompilerTraitKind,
) -> DeclarationCheckBlocker {
    DeclarationCheckBlocker {
        package: package_scope(input),
        target: input.target,
        path: input.path.clone(),
        span: input.definition.key.span,
        item: input.definition.hir_item,
        debug_spelling: format!("{kind:?}"),
        reason: DeclarationCheckBlockerReason::MissingFinalEmbeddedTraitIdentity(kind),
    }
}

fn validate_ordinary_trait_impl(
    input: &InputRow<'_>,
    trait_row: &CheckedDeclarationRow,
    arguments: &[GenericArgumentShape],
    target: &SymbolicType,
    impl_methods: &[arche_frontend::SymbolicMethodShapeSkeleton],
    diagnostics: &mut Vec<SemanticDiagnostic>,
) -> bool {
    let SymbolicDeclarationPayloadSkeleton::Trait {
        methods: trait_methods,
    } = &trait_row.declaration_shape.payload
    else {
        push_diagnostic(
            input,
            "IDENTITY001",
            "ordinary trait key resolves to a checked non-trait shape",
            input.definition.key.span,
            diagnostics,
        );
        return false;
    };
    // Trait headers are lowered in the impl declaration frame, while both
    // sides of method conformance are compared inside a method-local frame.
    // Lift every replacement first so substituted impl binders remain at
    // depth 1 instead of colliding with method binders at depth 0.
    let lift = impl_method_lift(&input.definition.symbolic_shape.generic_parameters);
    let method_arguments = match arguments
        .iter()
        .map(|argument| lift_impl_method_argument(&lift, argument))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(arguments) => arguments,
        Err(error) => {
            push_diagnostic(
                input,
                "IDENTITY001",
                format!("could not lift trait impl arguments into the method frame: {error:?}"),
                input.definition.key.span,
                diagnostics,
            );
            return false;
        }
    };
    let method_target = match lift.substitute_type(target, 0) {
        Ok(target) => target,
        Err(error) => {
            push_diagnostic(
                input,
                "IDENTITY001",
                format!("could not lift trait impl target into the method frame: {error:?}"),
                input.definition.key.span,
                diagnostics,
            );
            return false;
        }
    };
    let substitution = match TraitFrameSubstitution::new(
        trait_row.declaration_shape.generic_parameters.clone(),
        method_arguments,
        method_target,
    ) {
        Ok(substitution) => substitution,
        Err(error) => {
            push_diagnostic(
                input,
                "TRAIT001",
                format!("trait impl explicit arguments do not match trait binders: {error:?}"),
                input.definition.key.span,
                diagnostics,
            );
            return false;
        }
    };

    let Some(trait_by_name) = unique_method_map(input, trait_methods, "trait", diagnostics) else {
        return false;
    };
    let Some(impl_by_name) = unique_method_map(input, impl_methods, "impl", diagnostics) else {
        return false;
    };
    if trait_by_name.keys().ne(impl_by_name.keys()) {
        let missing = trait_by_name
            .keys()
            .filter(|name| !impl_by_name.contains_key(*name))
            .cloned()
            .collect::<Vec<_>>();
        let extra = impl_by_name
            .keys()
            .filter(|name| !trait_by_name.contains_key(*name))
            .cloned()
            .collect::<Vec<_>>();
        push_diagnostic(
            input,
            "TRAIT001",
            format!(
                "trait impl method-name set does not exactly match the trait; missing={missing:?}, extra={extra:?}"
            ),
            input.definition.key.span,
            diagnostics,
        );
        return false;
    }

    let mut valid = true;
    for (name, trait_method) in trait_by_name {
        let substituted = match substitute_declaration_shape(&substitution, trait_method, 1) {
            Ok(shape) => shape,
            Err(error) => {
                push_diagnostic(
                    input,
                    "IDENTITY001",
                    format!("could not substitute trait method {name}: {error:?}"),
                    input.definition.key.span,
                    diagnostics,
                );
                valid = false;
                continue;
            }
        };
        let implementation = impl_by_name[&name];
        if let Err(reason) = callable_conforms(&substituted, implementation) {
            push_diagnostic(
                input,
                "TRAIT001",
                format!("trait impl method {name} does not conform: {reason}"),
                input.definition.key.span,
                diagnostics,
            );
            valid = false;
        }
    }
    valid
}

fn lift_impl_method_argument(
    lift: &TraitFrameSubstitution,
    argument: &GenericArgumentShape,
) -> Result<GenericArgumentShape, crate::formation::GenericFormationError> {
    Ok(match argument {
        GenericArgumentShape::Type(ty) => GenericArgumentShape::Type(lift.substitute_type(ty, 0)?),
        GenericArgumentShape::Lifetime(lifetime) => {
            let wrapper = SymbolicPredicate::LifetimeOutlives {
                longer: lifetime.clone(),
                shorter: arche_frontend::SymbolicLifetime::Static,
            };
            let SymbolicPredicate::LifetimeOutlives { longer, .. } =
                lift.substitute_predicate(&wrapper, 0)?
            else {
                unreachable!("lifetime wrapper substitution preserves its predicate variant")
            };
            GenericArgumentShape::Lifetime(longer)
        }
        GenericArgumentShape::IntegerConst(value) => {
            let wrapper = SymbolicType::Array {
                element: Box::new(SymbolicType::Unit),
                length: value.clone(),
            };
            let SymbolicType::Array { length, .. } = lift.substitute_type(&wrapper, 0)? else {
                unreachable!("const wrapper substitution preserves its type variant")
            };
            GenericArgumentShape::IntegerConst(length)
        }
    })
}

fn unique_method_map<'a>(
    input: &InputRow<'_>,
    methods: &'a [arche_frontend::SymbolicMethodShapeSkeleton],
    owner: &str,
    diagnostics: &mut Vec<SemanticDiagnostic>,
) -> Option<BTreeMap<String, &'a SymbolicDeclarationShapeSkeleton>> {
    let mut rows = BTreeMap::new();
    let mut valid = true;
    for method in methods {
        if rows
            .insert(method.name.clone(), method.shape.as_ref())
            .is_some()
        {
            push_diagnostic(
                input,
                "TRAIT001",
                format!("{owner} declares method {} more than once", method.name),
                input.definition.key.span,
                diagnostics,
            );
            valid = false;
        }
    }
    valid.then_some(rows)
}

fn callable_conforms(
    expected: &SymbolicDeclarationShapeSkeleton,
    actual: &SymbolicDeclarationShapeSkeleton,
) -> Result<(), String> {
    if expected.generic_parameters != actual.generic_parameters {
        return Err("method generic kinds or arity differ".to_owned());
    }
    if canonical_predicates(&expected.predicates)? != canonical_predicates(&actual.predicates)? {
        return Err("canonical method predicates differ".to_owned());
    }
    let SymbolicDeclarationPayloadSkeleton::Callable(expected) = &expected.payload else {
        return Err("trait method has a non-callable checked shape".to_owned());
    };
    let SymbolicDeclarationPayloadSkeleton::Callable(actual) = &actual.payload else {
        return Err("impl method has a non-callable checked shape".to_owned());
    };
    if expected.kind != actual.kind {
        return Err("callable kind differs".to_owned());
    }
    if expected.parameters != actual.parameters {
        return Err("receiver mode/type/lifetime or parameter types differ".to_owned());
    }
    if expected.result != actual.result {
        return Err("result type differs".to_owned());
    }
    if expected.unsafe_ != actual.unsafe_ {
        return Err("safety bit differs".to_owned());
    }
    if expected.resume != actual.resume || expected.yields != actual.yields {
        return Err("generator resume/yield shape differs".to_owned());
    }
    if !effect_subset(&actual.effects.requires, &expected.effects.requires)? {
        return Err("impl requires set is not a subset of the trait requires set".to_owned());
    }
    if !effect_subset(&actual.effects.throws, &expected.effects.throws)? {
        return Err("impl throws set is not a subset of the trait throws set".to_owned());
    }
    Ok(())
}

fn canonical_predicates(
    predicates: &[SymbolicPredicateShapeSkeleton],
) -> Result<Vec<Vec<u8>>, String> {
    let mut encoded = Vec::with_capacity(predicates.len());
    for predicate in predicates {
        let SymbolicPredicateShapeSkeleton::Resolved { value, .. } = predicate else {
            return Err("method predicate remains PendingC2".to_owned());
        };
        encoded.push(
            encode_symbolic_predicate(value)
                .map_err(|error| format!("predicate encoding failed: {error:?}"))?,
        );
    }
    encoded.sort();
    encoded.dedup();
    Ok(encoded)
}

fn effect_subset(
    subset: &[SymbolicEffectShapeSkeleton],
    superset: &[SymbolicEffectShapeSkeleton],
) -> Result<bool, String> {
    let encode = |rows: &[SymbolicEffectShapeSkeleton]| -> Result<BTreeSet<Vec<u8>>, String> {
        rows.iter()
            .map(|row| {
                let SymbolicEffectShapeSkeleton::Resolved { value, .. } = row else {
                    return Err("effect member remains PendingC2".to_owned());
                };
                encode_symbolic_type(value)
                    .map_err(|error| format!("effect type encoding failed: {error:?}"))
            })
            .collect()
    };
    Ok(encode(subset)?.is_subset(&encode(superset)?))
}

fn substitute_declaration_shape(
    substitution: &TraitFrameSubstitution,
    shape: &SymbolicDeclarationShapeSkeleton,
    trait_depth: u64,
) -> Result<SymbolicDeclarationShapeSkeleton, crate::formation::GenericFormationError> {
    let predicates = shape
        .predicates
        .iter()
        .map(|predicate| match predicate {
            SymbolicPredicateShapeSkeleton::Resolved { value, readiness } => {
                Ok(SymbolicPredicateShapeSkeleton::Resolved {
                    value: Box::new(substitution.substitute_predicate(value, trait_depth)?),
                    readiness: *readiness,
                })
            }
            SymbolicPredicateShapeSkeleton::Pending(pending) => {
                Ok(SymbolicPredicateShapeSkeleton::Pending(pending.clone()))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let payload = substitute_payload(substitution, &shape.payload, trait_depth)?;
    Ok(SymbolicDeclarationShapeSkeleton {
        generic_parameters: shape.generic_parameters.clone(),
        predicates,
        payload,
    })
}

fn substitute_payload(
    substitution: &TraitFrameSubstitution,
    payload: &SymbolicDeclarationPayloadSkeleton,
    trait_depth: u64,
) -> Result<SymbolicDeclarationPayloadSkeleton, crate::formation::GenericFormationError> {
    use arche_frontend::{SymbolicFieldShapeSkeleton, SymbolicQueryTermShapeSkeleton};
    Ok(match payload {
        SymbolicDeclarationPayloadSkeleton::Callable(callable) => {
            SymbolicDeclarationPayloadSkeleton::Callable(Box::new(substitute_callable(
                substitution,
                callable,
                trait_depth,
            )?))
        }
        // Trait conformance substitutes immediately-owned method callables.
        // Keeping the remaining variants complete makes corruption fail
        // closed instead of accidentally comparing a partial payload.
        SymbolicDeclarationPayloadSkeleton::World => SymbolicDeclarationPayloadSkeleton::World,
        SymbolicDeclarationPayloadSkeleton::Tag => SymbolicDeclarationPayloadSkeleton::Tag,
        SymbolicDeclarationPayloadSkeleton::Record(record) => {
            SymbolicDeclarationPayloadSkeleton::Record(
                arche_frontend::SymbolicRecordShapeSkeleton {
                    form: record.form,
                    fields: record
                        .fields
                        .iter()
                        .map(|field| {
                            Ok(SymbolicFieldShapeSkeleton {
                                name: field.name.clone(),
                                ty: substitute_type_shape(substitution, &field.ty, trait_depth)?,
                            })
                        })
                        .collect::<Result<Vec<_>, crate::formation::GenericFormationError>>()?,
                },
            )
        }
        SymbolicDeclarationPayloadSkeleton::Enum(variants) => {
            SymbolicDeclarationPayloadSkeleton::Enum(
                variants
                    .iter()
                    .map(|variant| {
                        Ok(arche_frontend::SymbolicVariantShapeSkeleton {
                            name: variant.name.clone(),
                            form: variant.form,
                            fields: variant
                                .fields
                                .iter()
                                .map(|field| {
                                    Ok(SymbolicFieldShapeSkeleton {
                                        name: field.name.clone(),
                                        ty: substitute_type_shape(
                                            substitution,
                                            &field.ty,
                                            trait_depth,
                                        )?,
                                    })
                                })
                                .collect::<Result<Vec<_>, crate::formation::GenericFormationError>>(
                                )?,
                        })
                    })
                    .collect::<Result<Vec<_>, crate::formation::GenericFormationError>>()?,
            )
        }
        SymbolicDeclarationPayloadSkeleton::Alias { target } => {
            SymbolicDeclarationPayloadSkeleton::Alias {
                target: substitute_type_shape(substitution, target, trait_depth)?,
            }
        }
        SymbolicDeclarationPayloadSkeleton::Const { ty } => {
            SymbolicDeclarationPayloadSkeleton::Const {
                ty: substitute_type_shape(substitution, ty, trait_depth)?,
            }
        }
        SymbolicDeclarationPayloadSkeleton::Static { mutable, ty } => {
            SymbolicDeclarationPayloadSkeleton::Static {
                mutable: *mutable,
                ty: substitute_type_shape(substitution, ty, trait_depth)?,
            }
        }
        SymbolicDeclarationPayloadSkeleton::Query { terms } => {
            SymbolicDeclarationPayloadSkeleton::Query {
                terms: terms
                    .iter()
                    .map(|term| {
                        Ok(SymbolicQueryTermShapeSkeleton {
                            kind: term.kind,
                            ty: substitute_type_shape(substitution, &term.ty, trait_depth)?,
                        })
                    })
                    .collect::<Result<Vec<_>, crate::formation::GenericFormationError>>()?,
            }
        }
        SymbolicDeclarationPayloadSkeleton::Schedule { effects, readiness } => {
            SymbolicDeclarationPayloadSkeleton::Schedule {
                effects: substitute_effects(substitution, effects, trait_depth)?,
                readiness: *readiness,
            }
        }
        SymbolicDeclarationPayloadSkeleton::Trait { methods } => {
            SymbolicDeclarationPayloadSkeleton::Trait {
                methods: methods.clone(),
            }
        }
        SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref,
            target,
            is_default,
            methods,
        } => SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref: trait_ref
                .as_ref()
                .map(|value| substitute_type_shape(substitution, value, trait_depth))
                .transpose()?,
            target: substitute_type_shape(substitution, target, trait_depth)?,
            is_default: *is_default,
            methods: methods.clone(),
        },
        SymbolicDeclarationPayloadSkeleton::System { .. } => {
            // A trait method can never have a system payload; retaining this
            // clone lets the caller reject it as non-callable.
            payload.clone()
        }
    })
}

fn substitute_callable(
    substitution: &TraitFrameSubstitution,
    callable: &SymbolicCallableShapeSkeleton,
    trait_depth: u64,
) -> Result<SymbolicCallableShapeSkeleton, crate::formation::GenericFormationError> {
    Ok(SymbolicCallableShapeSkeleton {
        kind: callable.kind,
        parameters: callable
            .parameters
            .iter()
            .map(|parameter| {
                Ok(arche_frontend::SymbolicCallableParameterSkeleton {
                    mode: parameter.mode,
                    ty: substitute_type_shape(substitution, &parameter.ty, trait_depth)?,
                })
            })
            .collect::<Result<Vec<_>, crate::formation::GenericFormationError>>()?,
        result: substitute_type_shape(substitution, &callable.result, trait_depth)?,
        unsafe_: callable.unsafe_,
        resume: callable
            .resume
            .as_ref()
            .map(|value| substitute_type_shape(substitution, value, trait_depth))
            .transpose()?,
        yields: callable
            .yields
            .as_ref()
            .map(|value| substitute_type_shape(substitution, value, trait_depth))
            .transpose()?,
        effects: substitute_effects(substitution, &callable.effects, trait_depth)?,
    })
}

fn substitute_type_shape(
    substitution: &TraitFrameSubstitution,
    shape: &SymbolicTypeShapeSkeleton,
    trait_depth: u64,
) -> Result<SymbolicTypeShapeSkeleton, crate::formation::GenericFormationError> {
    Ok(match shape {
        SymbolicTypeShapeSkeleton::Resolved { value, readiness } => {
            SymbolicTypeShapeSkeleton::Resolved {
                value: substitution.substitute_type(value, trait_depth)?,
                readiness: *readiness,
            }
        }
        SymbolicTypeShapeSkeleton::Pending(pending) => {
            SymbolicTypeShapeSkeleton::Pending(pending.clone())
        }
    })
}

fn substitute_effects(
    substitution: &TraitFrameSubstitution,
    effects: &SymbolicEffectSetsSkeleton,
    trait_depth: u64,
) -> Result<SymbolicEffectSetsSkeleton, crate::formation::GenericFormationError> {
    let substitute = |shape: &SymbolicEffectShapeSkeleton| {
        Ok(match shape {
            SymbolicEffectShapeSkeleton::Resolved { value, readiness } => {
                SymbolicEffectShapeSkeleton::Resolved {
                    value: substitution.substitute_type(value, trait_depth)?,
                    readiness: *readiness,
                }
            }
            SymbolicEffectShapeSkeleton::Pending(pending) => {
                SymbolicEffectShapeSkeleton::Pending(pending.clone())
            }
        })
    };
    Ok(SymbolicEffectSetsSkeleton {
        requires: effects
            .requires
            .iter()
            .map(substitute)
            .collect::<Result<Vec<_>, crate::formation::GenericFormationError>>()?,
        throws: effects
            .throws
            .iter()
            .map(substitute)
            .collect::<Result<Vec<_>, crate::formation::GenericFormationError>>()?,
    })
}

struct CompilerTraitImplValidation<'a, 'hir> {
    input: &'a InputRow<'hir>,
    inputs: &'a [InputRow<'hir>],
    authority: &'a CompilerTraitAuthority,
    impl_formals: &'a [GenericParameterKind],
    arguments: &'a [GenericArgumentShape],
    target: &'a SymbolicType,
    methods: &'a [arche_frontend::SymbolicMethodShapeSkeleton],
    embedded: &'a VerifiedEmbeddedCoreAuthority,
}

fn validate_compiler_trait_impl(
    validation: &CompilerTraitImplValidation<'_, '_>,
    diagnostics: &mut Vec<SemanticDiagnostic>,
) -> bool {
    let input = validation.input;
    let inputs = validation.inputs;
    let authority = validation.authority;
    let impl_formals = validation.impl_formals;
    let arguments = validation.arguments;
    let target = validation.target;
    let methods = validation.methods;
    let embedded = validation.embedded;
    if arguments.len() != usize::from(authority.explicit_generic_arity())
        || arguments
            .iter()
            .any(|argument| !matches!(argument, GenericArgumentShape::Type(_)))
    {
        push_diagnostic(
            input,
            "TRAIT001",
            format!(
                "compiler trait {:?} requires exactly {} explicit type arguments",
                authority.kind(),
                authority.explicit_generic_arity()
            ),
            input.definition.key.span,
            diagnostics,
        );
        return false;
    }
    let designated = match authority.designated_self() {
        CompilerTraitSelfRelation::OperatedType | CompilerTraitSelfRelation::CallableType => target,
        CompilerTraitSelfRelation::Target(parameter)
        | CompilerTraitSelfRelation::LeftHandSide(parameter)
        | CompilerTraitSelfRelation::Input(parameter)
        | CompilerTraitSelfRelation::Source(parameter)
        | CompilerTraitSelfRelation::Iterator(parameter) => {
            let Some(GenericArgumentShape::Type(ty)) =
                arguments.get(usize::from(parameter.index()))
            else {
                return false;
            };
            ty
        }
    };
    if designated != target {
        push_diagnostic(
            input,
            "TRAIT001",
            format!(
                "compiler trait {:?} designated-Self relation does not equal the impl target",
                authority.kind()
            ),
            input.definition.key.span,
            diagnostics,
        );
        return false;
    }
    if authority.user_impl_policy() != UserImplPolicy::AllowedAndValidated {
        push_diagnostic(
            input,
            "TRAIT001",
            format!(
                "compiler trait {:?} does not permit an ordinary user impl",
                authority.kind()
            ),
            input.definition.key.span,
            diagnostics,
        );
        return false;
    }

    let expected_names = authority
        .method()
        .into_iter()
        .map(|method| method.source_name().to_owned())
        .collect::<BTreeSet<_>>();
    let actual_names = methods
        .iter()
        .map(|method| method.name.clone())
        .collect::<BTreeSet<_>>();
    if expected_names != actual_names || actual_names.len() != methods.len() {
        push_diagnostic(
            input,
            "TRAIT001",
            format!(
                "compiler trait {:?} requires exact method set {:?}, found {:?}",
                authority.kind(),
                expected_names,
                actual_names
            ),
            input.definition.key.span,
            diagnostics,
        );
        return false;
    }
    let Some(expected) = authority.method() else {
        return true;
    };
    let actual = methods
        .iter()
        .find(|method| method.name == expected.source_name())
        .expect("exact singleton compiler method set");
    let actual_item = inputs
        .iter()
        .find(|candidate| {
            candidate.item.owner == Some(input.definition.hir_item)
                && candidate.item.name.as_deref() == Some(expected.source_name())
                && matches!(candidate.item.source, HirItemSource::ImplMethod(_))
        })
        .expect("retained impl method shape has its owner-branded C1 item");
    let CompilerTraitCallablePattern::Fixed {
        parameters: expected_parameters,
        result,
    } = expected.callable()
    else {
        push_diagnostic(
            input,
            "TRAIT001",
            "callable-signature compiler traits cannot be user implemented",
            input.definition.key.span,
            diagnostics,
        );
        return false;
    };
    let SymbolicDeclarationPayloadSkeleton::Callable(callable) = &actual.shape.payload else {
        push_diagnostic(
            input,
            "TRAIT001",
            "compiler trait impl method is not callable",
            input.definition.key.span,
            diagnostics,
        );
        return false;
    };
    let mut expected_hidden_sources = Vec::new();
    if matches!(
        expected.receiver(),
        CompilerTraitReceiverMode::Shared | CompilerTraitReceiverMode::Mutable
    ) {
        expected_hidden_sources.push(HiddenLifetimeBinderSource::Receiver);
    }
    for pattern in expected_parameters {
        collect_compiler_reference_sources(
            pattern,
            HiddenLifetimeBinderSource::Input,
            &mut expected_hidden_sources,
        );
    }
    let actual_hidden_sources = actual_item
        .item
        .symbolic_shape
        .hidden_lifetime_binders
        .iter()
        .map(|binder| binder.source)
        .collect::<Vec<_>>();
    if !actual_item
        .item
        .symbolic_shape
        .generic_parameters
        .is_empty()
        || !actual.shape.predicates.is_empty()
        || actual_hidden_sources != expected_hidden_sources
        || actual.shape.generic_parameters
            != vec![GenericParameterKind::Lifetime; expected_hidden_sources.len()]
    {
        push_diagnostic(
            input,
            "TRAIT001",
            "compiler trait impl method binder/predicate shape does not match typed Embedded Core",
            input.definition.key.span,
            diagnostics,
        );
        return false;
    }
    if callable.unsafe_ || callable.resume.is_some() || callable.yields.is_some() {
        push_diagnostic(
            input,
            "TRAIT001",
            "compiler trait impl method has incompatible safety or generator shape",
            input.definition.key.span,
            diagnostics,
        );
        return false;
    }

    let lift = impl_method_lift(impl_formals);
    let method_target = lift
        .substitute_type(target, 0)
        .expect("validated impl target uses its retained owner binder frame");
    let method_arguments = arguments
        .iter()
        .map(|argument| match argument {
            GenericArgumentShape::Type(ty) => GenericArgumentShape::Type(
                lift.substitute_type(ty, 0)
                    .expect("validated compiler-trait argument uses its impl binder frame"),
            ),
            GenericArgumentShape::Lifetime(_) | GenericArgumentShape::IntegerConst(_) => {
                unreachable!("compiler trait authority accepts only explicit type arguments")
            }
        })
        .collect::<Vec<_>>();
    let mut parameters = callable.parameters.as_slice();
    if !validate_compiler_receiver(expected.receiver(), &method_target, &mut parameters) {
        push_diagnostic(
            input,
            "TRAIT001",
            format!(
                "compiler trait {:?} method receiver does not match its exact designated-Self mode",
                authority.kind()
            ),
            input.definition.key.span,
            diagnostics,
        );
        return false;
    }
    if parameters.len() != expected_parameters.len()
        || parameters
            .iter()
            .zip(expected_parameters)
            .any(|(actual, expected)| {
                actual.mode != SymbolicCallableParameterMode::Value
                    || resolved_type(&actual.ty).is_none_or(|actual| {
                        !compiler_pattern_matches(
                            expected,
                            actual,
                            &method_target,
                            &method_arguments,
                            embedded,
                        )
                    })
            })
        || resolved_type(&callable.result).is_none_or(|actual| {
            !compiler_pattern_matches(result, actual, &method_target, &method_arguments, embedded)
        })
    {
        push_diagnostic(
            input,
            "TRAIT001",
            format!(
                "compiler trait {:?} method parameter/result signature does not match typed Embedded Core",
                authority.kind()
            ),
            input.definition.key.span,
            diagnostics,
        );
        return false;
    }
    if !callable.effects.requires.is_empty() || !callable.effects.throws.is_empty() {
        push_diagnostic(
            input,
            "TRAIT001",
            "compiler trait method requires/throws sets must be exact empty subsets",
            input.definition.key.span,
            diagnostics,
        );
        return false;
    }
    true
}

fn collect_compiler_reference_sources(
    pattern: &CompilerTraitTypePattern,
    source: HiddenLifetimeBinderSource,
    output: &mut Vec<HiddenLifetimeBinderSource>,
) {
    match pattern {
        CompilerTraitTypePattern::SharedReference(pointee)
        | CompilerTraitTypePattern::MutableReference(pointee) => {
            output.push(source);
            collect_compiler_reference_sources(pointee, source, output);
        }
        CompilerTraitTypePattern::Nominal { arguments, .. } => {
            for argument in arguments {
                collect_compiler_reference_sources(argument, source, output);
            }
        }
        CompilerTraitTypePattern::SelfType
        | CompilerTraitTypePattern::ExplicitGeneric(_)
        | CompilerTraitTypePattern::Primitive(_) => {}
    }
}

fn impl_method_lift(formals: &[GenericParameterKind]) -> TraitFrameSubstitution {
    let arguments = formals
        .iter()
        .enumerate()
        .map(|(index, formal)| {
            let index = u64::try_from(index).expect("checked impl binder count fits u64");
            match formal {
                GenericParameterKind::Type => {
                    GenericArgumentShape::Type(SymbolicType::BoundType { depth: 1, index })
                }
                GenericParameterKind::Lifetime => {
                    GenericArgumentShape::Lifetime(arche_frontend::SymbolicLifetime::Bound {
                        depth: 1,
                        index,
                    })
                }
                GenericParameterKind::IntegerConst(integer_type) => {
                    GenericArgumentShape::IntegerConst(arche_frontend::SymbolicConstExpression {
                        integer_type: *integer_type,
                        node: arche_frontend::SymbolicConstNode::Bound { depth: 1, index },
                    })
                }
            }
        })
        .collect();
    TraitFrameSubstitution::new(formals.to_vec(), arguments, SymbolicType::Unit)
        .expect("identity lift matches every validated impl formal")
}

fn validate_compiler_receiver(
    expected: CompilerTraitReceiverMode,
    self_type: &SymbolicType,
    parameters: &mut &[arche_frontend::SymbolicCallableParameterSkeleton],
) -> bool {
    if expected == CompilerTraitReceiverMode::None {
        return parameters
            .first()
            .is_none_or(|parameter| parameter.mode == SymbolicCallableParameterMode::Value);
    }
    let Some((first, remaining)) = parameters.split_first() else {
        return false;
    };
    let Some(actual) = resolved_type(&first.ty) else {
        return false;
    };
    let matches = match (expected, first.mode, actual) {
        (
            CompilerTraitReceiverMode::Value,
            SymbolicCallableParameterMode::ReceiverValue,
            actual,
        ) => actual == self_type,
        (
            CompilerTraitReceiverMode::Shared,
            SymbolicCallableParameterMode::ReceiverShared,
            SymbolicType::Reference {
                mutability: Mutability::Shared,
                pointee,
                ..
            },
        ) => pointee.as_ref() == self_type,
        (
            CompilerTraitReceiverMode::Mutable,
            SymbolicCallableParameterMode::ReceiverMutable,
            SymbolicType::Reference {
                mutability: Mutability::Mutable,
                pointee,
                ..
            },
        ) => pointee.as_ref() == self_type,
        _ => false,
    };
    if matches {
        *parameters = remaining;
    }
    matches
}

fn compiler_pattern_matches(
    pattern: &CompilerTraitTypePattern,
    actual: &SymbolicType,
    self_type: &SymbolicType,
    explicit: &[GenericArgumentShape],
    embedded: &VerifiedEmbeddedCoreAuthority,
) -> bool {
    match pattern {
        CompilerTraitTypePattern::SelfType => actual == self_type,
        CompilerTraitTypePattern::ExplicitGeneric(parameter) => explicit
            .get(usize::from(parameter.index()))
            .is_some_and(|argument| {
                matches!(argument, GenericArgumentShape::Type(expected) if expected == actual)
            }),
        CompilerTraitTypePattern::SharedReference(pointee) => {
            matches!(
                actual,
                SymbolicType::Reference {
                    mutability: Mutability::Shared,
                    pointee: actual,
                    ..
                } if compiler_pattern_matches(pointee, actual, self_type, explicit, embedded)
            )
        }
        CompilerTraitTypePattern::MutableReference(pointee) => {
            matches!(
                actual,
                SymbolicType::Reference {
                    mutability: Mutability::Mutable,
                    pointee: actual,
                    ..
                } if compiler_pattern_matches(pointee, actual, self_type, explicit, embedded)
            )
        }
        CompilerTraitTypePattern::Primitive(primitive) => matches!(
            (primitive, actual),
            (CompilerPrimitiveTypePattern::Bool, SymbolicType::Bool)
                | (CompilerPrimitiveTypePattern::I32, SymbolicType::I32)
                | (CompilerPrimitiveTypePattern::Unit, SymbolicType::Unit)
        ),
        CompilerTraitTypePattern::Nominal { kind, arguments } => {
            let SymbolicType::NominalPath {
                declaration,
                arguments: actual_arguments,
            } = actual
            else {
                return false;
            };
            let authority = embedded.compiler_nominal(*kind);
            let Some(definition) = embedded.definition(authority.c1_definition()) else {
                return false;
            };
            is_embedded_path(declaration, embedded)
                && declaration.name == definition.name()
                && actual_arguments.len() == arguments.len()
                && actual_arguments.iter().zip(arguments).all(|(actual, pattern)| {
                    matches!(actual, GenericArgumentShape::Type(actual) if compiler_pattern_matches(pattern, actual, self_type, explicit, embedded))
                })
        }
    }
}

fn check_trait_impl_visibility(inputs: &[InputRow<'_>], diagnostics: &mut Vec<SemanticDiagnostic>) {
    for input in inputs {
        let HirItemSource::ImplMethod(method) = &input.item.source else {
            continue;
        };
        let Some(owner) = input.item.owner.and_then(|owner| {
            inputs
                .iter()
                .find(|candidate| candidate.definition.hir_item == owner)
        }) else {
            continue;
        };
        let SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref: Some(_), ..
        } = &owner.definition.symbolic_shape.payload
        else {
            continue;
        };
        if !matches!(
            method.visibility.kind,
            arche_frontend::ast::AstVisibilityKind::Private
        ) {
            push_diagnostic(
                input,
                "TRAIT001",
                "trait-impl methods cannot spell visibility",
                method.visibility.span,
                diagnostics,
            );
        }
    }
}

fn package_scope(input: &InputRow<'_>) -> ScopedPackageBytes {
    ScopedPackageBytes::from_canonical_name(input.package_scope)
        .expect("C1 inventory retains a nonempty canonical package name")
}

fn push_diagnostic(
    input: &InputRow<'_>,
    code: &'static str,
    message: impl Into<String>,
    span: Span,
    output: &mut Vec<SemanticDiagnostic>,
) {
    let message = message.into();
    let diagnostic = Diagnostic::at(code, span, message);
    output.push(
        SemanticDiagnostic::new(
            CompilationPhase::DeclarationTypeTraitCoherence,
            package_scope(input),
            input.target,
            input.path.clone(),
            diagnostic,
            Vec::new(),
        )
        .expect("declaration diagnostics have no unscoped secondary labels"),
    );
}

fn frontend_span(span: arche_frontend::SymbolicSourceSpan) -> Span {
    Span {
        file: arche_frontend::FileId(span.file),
        start: arche_frontend::SourcePosition {
            byte: span.start_byte,
            line: span.start_line,
            column: span.start_column,
        },
        end: arche_frontend::SourcePosition {
            byte: span.end_byte,
            line: span.end_line,
            column: span.end_column,
        },
    }
}

fn span_key(span: Span) -> (u64, u64, u64, u64, u64, u64, u64) {
    (
        span.file.0,
        span.start.byte,
        span.end.byte,
        span.start.line,
        span.start.column,
        span.end.line,
        span.end.column,
    )
}

impl fmt::Display for DeclarationCheckFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "C2 declaration checking did not complete ({} semantic diagnostics, {} authority blockers, {} retained-input errors)",
            self.diagnostics.as_ref().map_or(0, NonEmptySemanticDiagnostics::len),
            self.blockers.len(),
            usize::from(self.internal_error.is_some())
        )
    }
}

impl std::error::Error for DeclarationCheckFailure {}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use arche_frontend::{
        check_workspace_c1, FrontendOutput, SymbolicCallableKind,
        SymbolicCallableParameterSkeleton, SymbolicLifetime, SymbolicShapeReadiness,
        SymbolicSourceSpan,
    };
    use arche_package::{load_workspace, resolve, ManifestRequest, RegistrySnapshot};

    use super::*;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct TemporaryWorkspace(PathBuf);

    impl Drop for TemporaryWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn corpus_frontend(name: &str) -> FrontendOutput {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../tests/m27c2/v1")
            .join(name);
        let workspace = load_workspace(&ManifestRequest::discover_from(&root)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        check_workspace_c1(&workspace, &graph, &[]).unwrap()
    }

    fn corpus_handoff(name: &str) -> C2Handoff {
        C2Handoff::begin(corpus_frontend(name)).unwrap()
    }

    fn inline_frontend(source: &str) -> FrontendOutput {
        let ordinal = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let fixture = TemporaryWorkspace(std::env::temp_dir().join(format!(
            "arche-c2-declaration-check-{}-{ordinal}",
            std::process::id()
        )));
        fs::create_dir_all(fixture.0.join("src")).unwrap();
        fs::write(
            fixture.0.join("Arche.toml"),
            concat!(
                "schema = 1\n\n",
                "[package]\n",
                "name = \"example/declaration-check\"\n",
                "version = \"0.1.0\"\n",
                "edition = \"2026\"\n",
                "arche = \">=0.0.0\"\n",
                "publish = false\n\n",
                "[lib]\n",
                "path = \"src/lib.arc\"\n",
            ),
        )
        .unwrap();
        fs::write(fixture.0.join("src/lib.arc"), source).unwrap();
        let workspace = load_workspace(&ManifestRequest::discover_from(&fixture.0)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        check_workspace_c1(&workspace, &graph, &[]).unwrap()
    }

    fn checked_or_partial(
        handoff: &C2Handoff,
        declarations: &DeclarationTable,
    ) -> CheckedDeclarationFacts {
        match check_declarations_c2(handoff, declarations) {
            Ok(facts) => facts,
            Err(failure) => {
                assert!(failure.internal_error().is_none());
                assert!(
                    failure.diagnostics().is_none(),
                    "{:?}",
                    failure.diagnostics()
                );
                assert!(failure.blockers().iter().all(|blocker| matches!(
                    blocker.reason(),
                    DeclarationCheckBlockerReason::MissingDeclarationJudgment(_)
                )));
                failure.into_partial()
            }
        }
    }

    fn checked_inline(source: &str) -> CheckedDeclarationFacts {
        let handoff = C2Handoff::begin(inline_frontend(source)).unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        checked_or_partial(&handoff, &declarations)
    }

    fn callable(
        receiver: SymbolicCallableParameterMode,
        requires: &[SymbolicType],
        throws: &[SymbolicType],
    ) -> SymbolicDeclarationShapeSkeleton {
        SymbolicDeclarationShapeSkeleton {
            generic_parameters: Vec::new(),
            predicates: Vec::new(),
            payload: SymbolicDeclarationPayloadSkeleton::Callable(Box::new(
                SymbolicCallableShapeSkeleton {
                    kind: SymbolicCallableKind::Function,
                    parameters: vec![SymbolicCallableParameterSkeleton {
                        mode: receiver,
                        ty: SymbolicTypeShapeSkeleton::resolved(SymbolicType::I32),
                    }],
                    result: SymbolicTypeShapeSkeleton::resolved(SymbolicType::Unit),
                    unsafe_: false,
                    resume: None,
                    yields: None,
                    effects: SymbolicEffectSetsSkeleton {
                        requires: requires
                            .iter()
                            .cloned()
                            .map(SymbolicEffectShapeSkeleton::resolved)
                            .collect(),
                        throws: throws
                            .iter()
                            .cloned()
                            .map(SymbolicEffectShapeSkeleton::resolved)
                            .collect(),
                    },
                },
            )),
        }
    }

    fn judgment_blockers(source: &str) -> Vec<UnimplementedDeclarationJudgment> {
        let handoff = C2Handoff::begin(inline_frontend(source)).unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let failure = check_declarations_c2(&handoff, &declarations).unwrap_err();
        assert!(failure.internal_error().is_none());
        assert!(
            failure.diagnostics().is_none(),
            "diagnostics={:?}",
            failure.diagnostics()
        );
        let mut kinds: Vec<_> = failure
            .blockers()
            .iter()
            .filter_map(|blocker| match blocker.reason() {
                DeclarationCheckBlockerReason::MissingDeclarationJudgment(judgment) => {
                    Some(*judgment)
                }
                _ => None,
            })
            .collect();
        kinds.sort();
        kinds.dedup();
        kinds
    }

    #[test]
    fn unimplemented_declaration_judgments_fail_closed() {
        use UnimplementedDeclarationJudgment as J;
        assert_eq!(
            judgment_blockers("pub struct Loopy { pub next: Loopy }\n"),
            vec![J::SizednessRecursion]
        );
        assert_eq!(
            judgment_blockers("pub struct A { pub b: B }\npub struct B { pub a: A }\n"),
            vec![J::SizednessRecursion]
        );
        assert_eq!(
            judgment_blockers("pub enum Tree { Node { left: Tree, right: Tree } }\n"),
            vec![J::SizednessRecursion]
        );
        assert_eq!(
            judgment_blockers("pub struct Holder { pub raw: str }\n"),
            vec![J::SizednessRecursion]
        );
        assert_eq!(
            judgment_blockers("pub type A = B;\npub type B = A;\n"),
            vec![J::TypeAliasCycle]
        );
        assert_eq!(
            judgment_blockers(concat!(
                "pub struct Holder { pub value: i32 }\n",
                "impl Holder { pub fn value(&self) -> i32 { self.value } }\n",
                "impl Holder { pub fn value(&self) -> i32 { self.value } }\n",
            )),
            vec![J::SizednessRecursion, J::InherentMethodUniqueness]
        );
        assert_eq!(
            judgment_blockers(concat!(
                "pub struct Holder { pub value: i32 }\n",
                "pub trait One { fn one(&self) -> i32; }\n",
                "impl One for Holder { fn one(&self) -> i32 { 1i32 } }\n",
                "impl One for Holder { fn one(&self) -> i32 { 2i32 } }\n",
            )),
            vec![J::SizednessRecursion, J::ImplCoherenceOverlap]
        );
        assert_eq!(
            judgment_blockers("pub struct Table { pub scores: Map<f32, i32> }\n"),
            vec![J::SizednessRecursion, J::MapKeyComparison]
        );
        assert_eq!(
            judgment_blockers(
                "pub fn lookup(table: &Map<i32, i32>) -> usize {\n    table.len()\n}\n"
            ),
            vec![J::MapKeyComparison]
        );
    }

    #[test]
    fn real_c2_v1_declarations_block_only_on_recorded_authority_gaps() {
        for corpus in ["language-game", "language-environment"] {
            let handoff = corpus_handoff(corpus);
            let declarations = DeclarationTable::build(&handoff).unwrap();
            let failure = check_declarations_c2(&handoff, &declarations).unwrap_err();
            assert_eq!(failure.internal_error(), None, "{corpus}");
            assert!(
                failure.diagnostics().is_none(),
                "{corpus}: {:?}",
                failure.diagnostics()
            );
            assert!(!failure.blockers().is_empty(), "{corpus}");
            assert!(
                failure.blockers().iter().all(|blocker| matches!(
                    blocker.reason(),
                    DeclarationCheckBlockerReason::MissingDeclarationJudgment(_)
                        | DeclarationCheckBlockerReason::MissingFinalEmbeddedTraitIdentity(_)
                )),
                "{corpus}: {:#?}",
                failure.blockers()
            );
            assert_eq!(failure.partial().len(), declarations.len(), "{corpus}");
        }
    }

    #[test]
    fn nested_contextual_self_closes_with_exact_trait_method_binders() {
        let facts = checked_inline(concat!(
            "pub struct Envelope<T> { value: T }\n",
            "pub trait Nested<T> {\n",
            "    fn project<'a>(&'a self, value: T) -> Envelope<(Self, &'a Self, T)>;\n",
            "}\n",
        ));
        let method = facts
            .declarations()
            .find(|declaration| declaration.name() == "project")
            .unwrap();
        let SymbolicDeclarationPayloadSkeleton::Callable(callable) =
            &method.declaration_shape().payload
        else {
            panic!("project must remain callable")
        };
        let SymbolicTypeShapeSkeleton::Resolved {
            value:
                SymbolicType::NominalPath {
                    declaration,
                    arguments,
                },
            readiness: SymbolicShapeReadiness::ConstIndependent,
        } = &callable.result
        else {
            panic!("project result must be a complete nominal type")
        };
        assert_eq!(declaration.name, "Envelope");
        let [GenericArgumentShape::Type(SymbolicType::Tuple(elements))] = arguments.as_slice()
        else {
            panic!("Envelope must retain its tuple type argument")
        };
        let contextual_self = SymbolicType::BoundType { depth: 1, index: 1 };
        assert_eq!(
            elements,
            &[
                contextual_self.clone(),
                SymbolicType::Reference {
                    mutability: Mutability::Shared,
                    lifetime: SymbolicLifetime::Bound { depth: 0, index: 0 },
                    pointee: Box::new(contextual_self),
                },
                SymbolicType::BoundType { depth: 1, index: 0 },
            ]
        );
    }

    #[test]
    fn ordinary_impl_conformance_lifts_owner_binders_into_method_frames() {
        let facts = checked_inline(concat!(
            "pub struct Wrapper<T> { value: T }\n",
            "pub trait Algebra<T> {\n",
            "    fn combine<'a>(&'a self, left: T, right: T) -> T where T: 'a;\n",
            "    unsafe fn unchecked(&mut self, pointer: *mut T);\n",
            "    fn clone_self(&self) -> Self;\n",
            "}\n",
            "impl<T> Algebra<T> for Wrapper<T> {\n",
            "    fn combine<'a>(&'a self, left: T, right: T) -> T where T: 'a { left }\n",
            "    unsafe fn unchecked(&mut self, pointer: *mut T) { }\n",
            "    fn clone_self(&self) -> Self { Wrapper { value: self.value } }\n",
            "}\n",
        ));
        let implementation = facts
            .declarations()
            .find(|declaration| declaration.kind() == DeclarationKind::Impl)
            .unwrap();
        assert!(implementation.ordinary_impl_candidate().is_some());
        let SymbolicDeclarationPayloadSkeleton::Impl { methods, .. } =
            &implementation.declaration_shape().payload
        else {
            panic!("implementation must retain its method set")
        };
        assert_eq!(
            methods
                .iter()
                .map(|method| method.name.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["clone_self", "combine", "unchecked"]),
        );
    }

    #[test]
    fn impl_header_contextual_self_uses_the_canonical_target() {
        let facts = checked_inline(concat!(
            "pub trait Pair<T> { }\n",
            "pub struct Wrapper;\n",
            "impl Pair<(Self, Self)> for Wrapper { }\n",
        ));
        let implementation = facts
            .declarations()
            .find(|declaration| declaration.kind() == DeclarationKind::Impl)
            .unwrap();
        assert!(implementation.ordinary_impl_candidate().is_some());
        let SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref: Some(trait_ref),
            target: declaration_target,
            ..
        } = &implementation.declaration_shape().payload
        else {
            panic!("implementation declaration must retain its trait header")
        };
        let target = resolved_type(declaration_target).unwrap();
        let SymbolicType::NominalPath {
            declaration,
            arguments,
        } = resolved_type(trait_ref).unwrap()
        else {
            panic!("trait header must be a complete nominal application")
        };
        assert_eq!(declaration.name, "Pair");
        let [GenericArgumentShape::Type(SymbolicType::Tuple(elements))] = arguments.as_slice()
        else {
            panic!("Pair must retain its tuple type argument")
        };
        assert_eq!(elements, &[target.clone(), target.clone()]);
    }

    #[test]
    fn contextual_self_template_failures_preserve_exact_fail_closed_provenance() {
        let pending = SymbolicPendingShape {
            readiness: SymbolicShapeReadiness::PendingC2,
            source_span: SymbolicSourceSpan {
                file: 3,
                start_byte: 8,
                end_byte: 13,
                start_line: 2,
                start_column: 4,
                end_line: 2,
                end_column: 9,
            },
            kind: PendingShapeKind::ContextualSelf,
            debug_spelling: "&Self".to_owned(),
        };
        assert_eq!(
            contextual_self_lookup_failure(&pending, C2TypeTemplateLookupError::Missing),
            (
                pending.source_span,
                "&Self".to_owned(),
                ContextualSelfTemplateFailure::Missing,
            )
        );
        assert_eq!(
            contextual_self_lookup_failure(&pending, C2TypeTemplateLookupError::Ambiguous),
            (
                pending.source_span,
                "&Self".to_owned(),
                ContextualSelfTemplateFailure::Ambiguous,
            )
        );

        let additional_span = SymbolicSourceSpan {
            file: 3,
            start_byte: 9,
            end_byte: 12,
            start_line: 2,
            start_column: 5,
            end_line: 2,
            end_column: 8,
        };
        assert_eq!(
            contextual_self_lookup_failure(
                &pending,
                C2TypeTemplateLookupError::Blocked(C2TypeTemplateBlocker::AdditionalPending {
                    source_span: additional_span,
                    kind: PendingShapeKind::GenericFormation,
                    debug_spelling: "Box<?>".to_owned(),
                }),
            ),
            (
                additional_span,
                "Box<?>".to_owned(),
                ContextualSelfTemplateFailure::AdditionalPending(
                    PendingShapeKind::GenericFormation,
                ),
            )
        );
        assert_eq!(
            contextual_self_lookup_failure(
                &pending,
                C2TypeTemplateLookupError::Blocked(C2TypeTemplateBlocker::FrontendInvariant {
                    source_span: None,
                    code: "TYPE_TEMPLATE".to_owned(),
                    message: "malformed retained template".to_owned(),
                }),
            ),
            (
                pending.source_span,
                "TYPE_TEMPLATE: malformed retained template".to_owned(),
                ContextualSelfTemplateFailure::FrontendInvariant,
            )
        );
    }

    #[test]
    fn mixed_retained_sessions_fail_without_a_source_diagnostic() {
        let game = corpus_handoff("language-game");
        let declarations = DeclarationTable::build(&game).unwrap();
        let environment = corpus_handoff("language-environment");

        let failure = check_declarations_c2(&environment, &declarations).unwrap_err();
        assert!(failure.diagnostics().is_none());
        assert!(failure.blockers().is_empty());
        assert!(failure.partial().is_empty());
        assert_eq!(
            failure.internal_error(),
            Some(
                &DeclarationCheckInternalError::IncompleteRetainedDeclarationJoin {
                    expected_declarations: declarations.len(),
                    joined_declarations: 0,
                }
            )
        );
    }

    #[test]
    fn identical_declaration_keys_do_not_cross_session_candidate_authority() {
        let source = concat!(
            "pub trait Marker { }\n",
            "pub struct Wrapper;\n",
            "impl Marker for Wrapper { }\n",
        );
        let first = C2Handoff::begin(inline_frontend(source)).unwrap();
        let first_declarations = DeclarationTable::build(&first).unwrap();
        let first_facts = checked_or_partial(&first, &first_declarations);
        let second = C2Handoff::begin(inline_frontend(source)).unwrap();
        let second_declarations = DeclarationTable::build(&second).unwrap();

        let first_impl = first_declarations
            .declarations()
            .find(|declaration| declaration.kind() == DeclarationKind::Impl)
            .unwrap();
        let second_impl = second_declarations
            .declarations()
            .find(|declaration| declaration.kind() == DeclarationKind::Impl)
            .unwrap();
        assert_eq!(
            first_impl.session_traversal_bytes(),
            second_impl.session_traversal_bytes()
        );
        assert!(first_facts
            .ordinary_impl_candidate_for(first_impl)
            .is_some());
        assert!(first_facts
            .ordinary_impl_candidate_for(second_impl)
            .is_none());
        assert!(!first_facts
            .session_brand()
            .same_session(&second.session_brand()));
    }

    #[test]
    fn method_effects_are_covariant_but_receivers_are_exact() {
        let expected = callable(
            SymbolicCallableParameterMode::ReceiverShared,
            &[SymbolicType::I32, SymbolicType::Bool],
            &[SymbolicType::U8],
        );
        let narrower = callable(
            SymbolicCallableParameterMode::ReceiverShared,
            &[SymbolicType::I32],
            &[],
        );
        assert_eq!(callable_conforms(&expected, &narrower), Ok(()));

        let broader = callable(
            SymbolicCallableParameterMode::ReceiverShared,
            &[SymbolicType::I32, SymbolicType::Bool, SymbolicType::U8],
            &[],
        );
        assert_eq!(
            callable_conforms(&expected, &broader),
            Err("impl requires set is not a subset of the trait requires set".to_owned())
        );

        let wrong_receiver = callable(
            SymbolicCallableParameterMode::ReceiverMutable,
            &[SymbolicType::I32],
            &[],
        );
        assert_eq!(
            callable_conforms(&expected, &wrong_receiver),
            Err("receiver mode/type/lifetime or parameter types differ".to_owned())
        );
    }

    #[test]
    fn pending_effect_member_cannot_be_treated_as_a_set_atom() {
        let pending = SymbolicEffectShapeSkeleton::pending(
            SymbolicShapeReadiness::PendingC2,
            SymbolicSourceSpan {
                file: 1,
                start_byte: 0,
                end_byte: 4,
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 5,
            },
            PendingShapeKind::ContextualSelf,
            "Self",
        );
        assert_eq!(
            effect_subset(&[pending], &[]),
            Err("effect member remains PendingC2".to_owned())
        );
    }
}
