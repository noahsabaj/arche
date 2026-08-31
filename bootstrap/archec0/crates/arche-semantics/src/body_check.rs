//! Consuming M27-C2 body checking over retained C1 AST and resolved HIR.
//!
//! This adapter is deliberately the only layer that translates source AST
//! bodies into the smaller expression and pattern algebras owned by C2.  It
//! never resolves a name from spelling: paths, locals, generic actuals, and
//! associated candidates are joined back to C1 by their exact retained spans.
//! Constructs whose typed authority belongs to a later gate are retained as an
//! explicit dependency.  A missing C2 substrate is reported separately from a
//! source diagnostic so a compiler gap cannot make valid source look invalid.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use arche_frontend::ast::{
    AstAssignmentOperator, AstBinaryOperator, AstBlock, AstCondition, AstConstExpression,
    AstConstructorPatternPayload, AstDeclarationKind, AstElseBranch, AstExpression,
    AstExpressionKind, AstGeneratorClosure, AstImplMethod, AstLiteral, AstMatchArm,
    AstMethodParameter, AstPattern, AstPatternKind, AstPostfixKind, AstRangeEndpoint,
    AstReceiverKind, AstSlicePatternPart, AstStatementKind, AstStructForm, AstType,
    AstUnaryOperator, AstVariantForm,
};
use arche_frontend::embedded_core::{
    CompilerMethodGenericArgumentPattern, CompilerMethodGenericParameter,
    CompilerMethodGenericParameterAuthority, CompilerMethodGenericParameterKind,
    CompilerMethodLifetimePattern, CompilerMethodSelectorAuthority, CompilerMethodSelectorKind,
    CompilerMethodTypePattern, CompilerNominalKind, CompilerNominalMethodAuthority,
    CompilerNominalMethodEffectPattern, CompilerNominalMethodReceiverMode,
    CompilerPrimitiveTypePattern, VirtualDeclarationKind, VirtualDefinitionId,
    VirtualEnumVariantId, VirtualMethodId, VirtualNamespace, VirtualPreludeTarget,
    VirtualTypeFlavor,
};
use arche_frontend::{
    AssociatedPathCandidate, BuiltinResTarget, DeclarationKind, Diagnostic, FileId,
    GenericArgumentShape, GenericParameterKind, HirBodyId, HirBodySource, HirItemId, HirItemRes,
    HirItemSource, LocalId, Mutability, PathResolution, Res, ResolvedGenericArgument,
    ResolvedSymbolicBody, ResolvedSymbolicItem, ResolvedSymbolicTargetHir, ResolvedSymbolicType,
    SemanticBodyKind, SemanticDeclarationPath, SemanticDefinitionInventorySkeleton, Span,
    SymbolicCallableParameterMode, SymbolicConstExpression, SymbolicConstNode,
    SymbolicDeclarationPayloadSkeleton, SymbolicDeclarationShapeSkeleton,
    SymbolicDefinitionOwnerSkeleton, SymbolicFieldShapeSkeleton, SymbolicLifetime,
    SymbolicPredicate, SymbolicPredicateShapeSkeleton, SymbolicRecordForm, SymbolicType,
    SymbolicTypeEffectSet, SymbolicTypeShapeSkeleton, TargetId, TargetRoot, UnresolvedPathKind,
};
use arche_package::PortablePath;

use crate::declaration_check::CheckedDeclarationFacts;
use crate::declarations::DeclarationTable;
use crate::diagnostic::{
    CompilationPhase, NonEmptySemanticDiagnostics, ScopedPackageBytes, SemanticDiagnostic,
};
use crate::golden::{declaration_kind_atom, integer_type_atom};
use crate::model::{
    C2Handoff, C2Resolution, NeedsCtfeObligation, NeedsCtfeObligations, PendingC4Dependencies,
    PendingC4Dependency, SessionBrand,
};
use crate::pattern::{
    analyze_pattern_match, check_irrefutable_pattern, BindingAnnotation, BindingMode, EnumType,
    EnumVariant, FloatType as PatternFloatType, IntegerType as PatternIntegerType,
    IrrefutablePatternAnalysis, Pattern, PatternArm, PatternBinding, PatternConst,
    PatternErrorKind, PatternErrors, PatternLiteral, PatternMatchAnalysis, PatternScrutinee,
    PatternType, PlaceMutability, RangeEndpoint, RecordField, RecordPatternField, RecordType,
    ReferenceMutability, TypedPattern, TypedPatternKind,
};
use crate::typing::{
    check_typed_expression_in_loops, BinaryTypeOperator, CheckedExpression, CheckedExpressionKind,
    TypeCheckError, TypeCheckErrorKind, TypedExpressionInput, TypingContext, UnaryTypeOperator,
};
use crate::{
    classify_coercion, BinderFrame, BinderStack, LifetimeOutlives, TraitFrameSubstitution,
};

/// Opaque owner-branded handle for one checked C2 body.
#[derive(Clone, Debug)]
pub struct C2BodyHandle {
    owner: Arc<BodyTableOwner>,
    offset: u64,
}

impl C2BodyHandle {
    /// Dense session-local offset. It is not a stable body identity.
    pub const fn offset(&self) -> u64 {
        self.offset
    }
}

/// Immutable checked body facts. Construction remains private to this module.
#[derive(Debug)]
pub struct C2BodyTable {
    session: SessionBrand,
    owner: Arc<BodyTableOwner>,
    rows: Box<[CheckedBodyRow]>,
    by_body: BTreeMap<HirBodyId, u64>,
}

impl C2BodyTable {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Iterates metadata for every attempted retained body. Partial attempts
    /// deliberately expose no typed body facts.
    pub fn attempts(&self) -> impl ExactSizeIterator<Item = C2BodyAttemptView<'_>> + '_ {
        self.rows.iter().map(|row| C2BodyAttemptView { row })
    }

    /// Iterates only source-valid rows whose C2 authority is complete.
    pub fn bodies(&self) -> impl Iterator<Item = C2BodyView<'_>> + '_ {
        self.rows
            .iter()
            .filter(|row| row.is_consumable())
            .map(|row| C2BodyView { row })
    }

    pub fn handle(&self, body: HirBodyId) -> Option<C2BodyHandle> {
        self.by_body.get(&body).map(|&offset| C2BodyHandle {
            owner: Arc::clone(&self.owner),
            offset,
        })
    }

    pub fn body(&self, handle: &C2BodyHandle) -> Option<C2BodyView<'_>> {
        if !Arc::ptr_eq(&self.owner, &handle.owner) {
            return None;
        }
        usize::try_from(handle.offset)
            .ok()
            .and_then(|offset| self.rows.get(offset))
            .filter(|row| row.is_consumable())
            .map(|row| C2BodyView { row })
    }

    pub(crate) const fn session_brand(&self) -> &SessionBrand {
        &self.session
    }

    pub(crate) fn all_authority_complete(&self) -> bool {
        self.rows.iter().all(|row| row.authority_complete)
    }
}

#[derive(Debug)]
struct BodyTableOwner;

/// Metadata-only view of one retained body-check attempt. Typed facts remain
/// quarantined unless `C2BodyTable::bodies` proves both authority completeness
/// and absence of source diagnostics for the row.
#[derive(Clone, Copy, Debug)]
pub struct C2BodyAttemptView<'a> {
    row: &'a CheckedBodyRow,
}

impl<'a> C2BodyAttemptView<'a> {
    pub const fn id(self) -> HirBodyId {
        self.row.id
    }

    pub const fn owner(self) -> HirItemId {
        self.row.owner
    }

    pub const fn kind(self) -> SemanticBodyKind {
        self.row.kind
    }

    pub const fn span(self) -> Span {
        self.row.span
    }

    pub const fn resolution(self) -> &'a C2Resolution {
        &self.row.resolution
    }

    pub const fn authority_complete(self) -> bool {
        self.row.authority_complete
    }

    pub const fn has_source_diagnostics(self) -> bool {
        self.row.has_source_diagnostics
    }
}

/// Borrow-tied read-only view of one checked body.
#[derive(Clone, Copy, Debug)]
pub struct C2BodyView<'a> {
    row: &'a CheckedBodyRow,
}

impl<'a> C2BodyView<'a> {
    pub const fn id(self) -> HirBodyId {
        self.row.id
    }

    pub const fn owner(self) -> HirItemId {
        self.row.owner
    }

    pub const fn kind(self) -> SemanticBodyKind {
        self.row.kind
    }

    pub const fn span(self) -> Span {
        self.row.span
    }

    pub const fn resolution(self) -> &'a C2Resolution {
        &self.row.resolution
    }

    pub const fn pending_c4(self) -> &'a PendingC4Dependencies {
        &self.row.pending_c4
    }

    pub fn expressions(self) -> &'a [CheckedBodyExpression] {
        &self.row.expressions
    }

    pub fn locals(self) -> &'a [CheckedBodyLocal] {
        &self.row.locals
    }

    pub fn patterns(self) -> &'a [CheckedBodyPattern] {
        &self.row.patterns
    }

    pub fn calls(self) -> &'a [CheckedBodyCall] {
        &self.row.calls
    }
}

#[derive(Clone, Debug)]
struct CheckedBodyRow {
    id: HirBodyId,
    owner: HirItemId,
    kind: SemanticBodyKind,
    span: Span,
    authority_complete: bool,
    has_source_diagnostics: bool,
    resolution: C2Resolution,
    pending_c4: PendingC4Dependencies,
    expressions: Box<[CheckedBodyExpression]>,
    locals: Box<[CheckedBodyLocal]>,
    patterns: Box<[CheckedBodyPattern]>,
    calls: Box<[CheckedBodyCall]>,
}

impl CheckedBodyRow {
    const fn is_consumable(&self) -> bool {
        self.authority_complete && !self.has_source_diagnostics
    }
}

/// One expression fact tied to its exact retained source span.
#[derive(Clone, Debug)]
pub struct CheckedBodyExpression {
    span: Span,
    expression: CheckedExpression,
}

impl CheckedBodyExpression {
    pub const fn span(&self) -> Span {
        self.span
    }

    pub const fn expression(&self) -> &CheckedExpression {
        &self.expression
    }
}

/// Final C2 type assigned to one C1 local identity.
#[derive(Clone, Debug)]
pub struct CheckedBodyLocal {
    local: LocalId,
    ty: SymbolicType,
}

impl CheckedBodyLocal {
    pub const fn local(&self) -> LocalId {
        self.local
    }

    pub const fn ty(&self) -> &SymbolicType {
        &self.ty
    }
}

/// Pattern analysis retained for MIR construction.
#[derive(Clone, Debug)]
pub enum CheckedBodyPatternAnalysis {
    Irrefutable(IrrefutablePatternAnalysis),
    Refutable(PatternMatchAnalysis),
}

/// One exact-span pattern fact.
#[derive(Clone, Debug)]
pub struct CheckedBodyPattern {
    span: Span,
    analysis: CheckedBodyPatternAnalysis,
}

impl CheckedBodyPattern {
    pub const fn span(&self) -> Span {
        self.span
    }

    pub const fn analysis(&self) -> &CheckedBodyPatternAnalysis {
        &self.analysis
    }
}

/// Selected C2 call category. Stable ordinary IDs are intentionally absent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckedBodyCallee {
    DirectItem(HirItemId),
    AssociatedItem(HirItemId),
    FunctionPointer,
    /// A call through a first-class closure value.
    ClosureValue,
    /// A generator-factory call constructing the produced generator state.
    GeneratorFactoryValue,
    /// The reserved resume postfix on a pinned generator state.
    GeneratorResume,
    EmbeddedMethod(VirtualMethodId),
    EmbeddedDefinition(VirtualDefinitionId),
    TraitMethod {
        trait_path: Box<SemanticDeclarationPath>,
        method: Box<str>,
    },
    QueryIteration,
    CommandSpawn,
}

/// One call fact after exact argument checking.
#[derive(Clone, Debug)]
pub struct CheckedBodyCall {
    span: Span,
    callee: CheckedBodyCallee,
    result: SymbolicType,
}

impl CheckedBodyCall {
    pub const fn span(&self) -> Span {
        self.span
    }

    pub const fn callee(&self) -> &CheckedBodyCallee {
        &self.callee
    }

    pub const fn result(&self) -> &SymbolicType {
        &self.result
    }
}

/// A compiler implementation gap discovered while consuming real C1 bodies.
/// These rows are never rendered as source diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BodyCheckIncompleteness {
    body: HirBodyId,
    span: Span,
    kind: BodyCheckIncompletenessKind,
    detail: Box<str>,
}

impl BodyCheckIncompleteness {
    pub const fn body(&self) -> HirBodyId {
        self.body
    }

    pub const fn span(&self) -> Span {
        self.span
    }

    pub const fn kind(&self) -> BodyCheckIncompletenessKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Closed classes of missing substrate; adding a class is a reviewed C2 API
/// change rather than an ad-hoc successful fallback.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum BodyCheckIncompletenessKind {
    PendingC2Type,
    MissingRetainedJoin,
    MissingEmbeddedTraitIdentity,
    MissingTypedEmbeddedCallable,
    MissingPatternAlgebra,
    MissingBodyLocalLifetime,
    MissingGenericInference,
    MissingMethodSelection,
    MissingEffectAuthority,
    MissingClosureType,
    MissingGeneratorType,
    UnsupportedC2AdapterSurface,
}

/// Failed body pass with both partial checked facts and independent failure
/// channels. Source diagnostics are canonical; compiler gaps are canonical
/// internal rows and never masquerade as user errors.
#[derive(Debug)]
pub struct C2BodyCheckFailure {
    partial: C2BodyTable,
    diagnostics: Option<NonEmptySemanticDiagnostics>,
    incompleteness: Box<[BodyCheckIncompleteness]>,
}

impl C2BodyCheckFailure {
    pub const fn partial(&self) -> &C2BodyTable {
        &self.partial
    }

    pub const fn diagnostics(&self) -> Option<&NonEmptySemanticDiagnostics> {
        self.diagnostics.as_ref()
    }

    pub fn incompleteness(&self) -> &[BodyCheckIncompleteness] {
        &self.incompleteness
    }
}

/// Check every retained body exactly once. The declaration table is required
/// so this pass cannot accidentally operate on a different C2 session.
pub(crate) fn check_workspace_bodies_c2(
    handoff: &C2Handoff,
    declarations: &DeclarationTable,
    checked_declarations: &CheckedDeclarationFacts,
) -> Result<C2BodyTable, C2BodyCheckFailure> {
    let catalog = match BodyCatalog::build(handoff, declarations, checked_declarations) {
        Ok(catalog) => catalog,
        Err(gap) => {
            return Err(C2BodyCheckFailure {
                partial: empty_body_table(handoff.session_brand()),
                diagnostics: None,
                incompleteness: vec![gap].into_boxed_slice(),
            });
        }
    };

    let mut rows = Vec::new();
    let mut diagnostics = Vec::new();
    let mut gaps = Vec::new();
    let mut cross_body_locals = BTreeMap::new();
    for scope in &catalog.body_order {
        let mut outcome = match (scope.declaration_shape, scope.owner_shape) {
            (Some(declaration_shape), Some(owner_shape)) => {
                let mut checker = BodyChecker::new(
                    &catalog,
                    scope,
                    declaration_shape,
                    owner_shape,
                    &mut cross_body_locals,
                );
                checker.check()
            }
            (None, None) => missing_declaration_outcome(scope),
            (Some(_), None) | (None, Some(_)) => {
                unreachable!("checked declaration shapes are retained atomically")
            }
        };
        outcome.row.authority_complete = outcome.gaps.is_empty();
        outcome.row.has_source_diagnostics = !outcome.diagnostics.is_empty();
        rows.push(outcome.row);
        if outcome.gaps.is_empty() {
            diagnostics.append(&mut outcome.diagnostics);
        }
        gaps.append(&mut outcome.gaps);
    }

    let table = body_table(handoff.session_brand(), rows);
    gaps.sort_by(compare_incompleteness);
    gaps.dedup();
    let diagnostics = NonEmptySemanticDiagnostics::from_unsorted(diagnostics).ok();
    if diagnostics.is_some() || !gaps.is_empty() {
        return Err(C2BodyCheckFailure {
            partial: table,
            diagnostics,
            incompleteness: gaps.into_boxed_slice(),
        });
    }
    Ok(table)
}

fn empty_body_table(session: SessionBrand) -> C2BodyTable {
    body_table(session, Vec::new())
}

fn body_table(session: SessionBrand, mut rows: Vec<CheckedBodyRow>) -> C2BodyTable {
    rows.sort_by_key(|row| row.id);
    let owner = Arc::new(BodyTableOwner);
    let by_body = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.is_consumable())
        .map(|(offset, row)| {
            (
                row.id,
                u64::try_from(offset).expect("checked body count fits u64"),
            )
        })
        .collect();
    C2BodyTable {
        session,
        owner,
        rows: rows.into_boxed_slice(),
        by_body,
    }
}

fn compare_incompleteness(
    left: &BodyCheckIncompleteness,
    right: &BodyCheckIncompleteness,
) -> std::cmp::Ordering {
    left.body
        .cmp(&right.body)
        .then_with(|| span_key(left.span).cmp(&span_key(right.span)))
        .then_with(|| left.kind.cmp(&right.kind))
        .then_with(|| left.detail.as_bytes().cmp(right.detail.as_bytes()))
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SpanKey {
    file: u64,
    start: u64,
    end: u64,
    start_line: u64,
    start_column: u64,
    end_line: u64,
    end_column: u64,
}

fn span_key(span: Span) -> SpanKey {
    SpanKey {
        file: span.file.0,
        start: span.start.byte,
        end: span.end.byte,
        start_line: span.start.line,
        start_column: span.start.column,
        end_line: span.end.line,
        end_column: span.end.column,
    }
}

#[derive(Clone, Copy)]
struct BodyScope<'a> {
    package_scope: &'a str,
    target_id: TargetId,
    target: &'a ResolvedSymbolicTargetHir,
    item: &'a ResolvedSymbolicItem,
    declaration_shape: Option<&'a SymbolicDeclarationShapeSkeleton>,
    owner_shape: Option<&'a SymbolicDefinitionOwnerSkeleton>,
    body: &'a ResolvedSymbolicBody,
    path: &'a PortablePath,
}

struct DefinitionEntry<'a> {
    registry_origin: &'a str,
    package_scope: &'a str,
    item: &'a ResolvedSymbolicItem,
    definition: &'a SemanticDefinitionInventorySkeleton,
    declaration_shape: Option<&'a SymbolicDeclarationShapeSkeleton>,
}

impl DefinitionEntry<'_> {
    fn semantic_path(&self) -> SemanticDeclarationPath {
        SemanticDeclarationPath {
            registry_origin: self.registry_origin.to_owned(),
            package_name: self.package_scope.to_owned(),
            target: self.definition.key.module.target.clone(),
            modules: self.definition.key.module.path.clone(),
            kind: self.definition.key.kind,
            name: self.definition.key.name.clone(),
        }
    }
}

struct BodyCatalog<'a> {
    handoff: &'a C2Handoff,
    definitions: BTreeMap<HirItemId, DefinitionEntry<'a>>,
    paths: BTreeMap<SemanticDeclarationPath, HirItemId>,
    body_order: Vec<BodyScope<'a>>,
}

impl<'a> BodyCatalog<'a> {
    fn build(
        handoff: &'a C2Handoff,
        declarations: &'a DeclarationTable,
        checked_declarations: &'a CheckedDeclarationFacts,
    ) -> Result<Self, BodyCheckIncompleteness> {
        if !handoff
            .session_brand()
            .same_session(checked_declarations.session_brand())
        {
            return Err(catalog_gap(
                "checked declaration facts belong to another C2 session",
            ));
        }
        // Resolve every declaration handle through the supplied table. This is
        // the owner-brand/session proof; metadata is consumed from inventory.
        let mut definitions = BTreeMap::new();
        let mut paths = BTreeMap::new();
        let mut body_order = Vec::new();

        for package in &handoff.frontend().hir().packages {
            let inventory_package = handoff
                .frontend()
                .inventory()
                .packages
                .iter()
                .find(|candidate| candidate.package == package.package)
                .ok_or_else(|| catalog_gap("HIR package has no inventory package"))?;
            for target in &package.targets {
                let inventory_target = inventory_package
                    .targets
                    .iter()
                    .find(|candidate| candidate.target_id == target.id)
                    .ok_or_else(|| catalog_gap("HIR target has no inventory target"))?;
                if inventory_target.target != target.target {
                    return Err(catalog_gap("HIR/inventory target root mismatch"));
                }
                for item in &target.items {
                    let handle = declarations
                        .handle_for_session_item(item.id)
                        .ok_or_else(|| catalog_gap("HIR item is absent from declaration table"))?;
                    let view = declarations
                        .declaration(&handle)
                        .ok_or_else(|| catalog_gap("declaration-table owner brand mismatch"))?;
                    if handoff
                        .indexes()
                        .item(view.session_item())
                        .map(|row| row.id())
                        != Some(item.id)
                    {
                        return Err(catalog_gap("session item does not round-trip"));
                    }
                    let checked_view = checked_declarations
                        .handle_for_session_item(item.id)
                        .and_then(|handle| checked_declarations.declaration(&handle));
                    if let Some(checked_view) = checked_view {
                        if !handoff
                            .session_brand()
                            .owns_item(checked_view.session_item())
                            || handoff
                                .indexes()
                                .item(checked_view.session_item())
                                .map(|row| row.id())
                                != Some(item.id)
                        {
                            return Err(catalog_gap(
                                "checked declaration item belongs to another C2 session",
                            ));
                        }
                        if checked_view.session_traversal_bytes() != view.session_traversal_bytes()
                            || checked_view.package() != view.package()
                            || checked_view.target() != view.target()
                            || checked_view.kind() != view.kind()
                            || checked_view.name() != view.name()
                        {
                            return Err(catalog_gap(
                                "checked declaration metadata differs from retained declaration",
                            ));
                        }
                    }
                    let definition = inventory_package
                        .definitions
                        .iter()
                        .find(|candidate| candidate.hir_item == item.id)
                        .ok_or_else(|| catalog_gap("HIR item has no inventory definition"))?;
                    let canonical = SemanticDeclarationPath {
                        registry_origin: inventory_package.provenance.registry_origin.clone(),
                        package_name: inventory_package.provenance.scoped_name.clone(),
                        target: definition.key.module.target.clone(),
                        modules: definition.key.module.path.clone(),
                        kind: definition.key.kind,
                        name: definition.key.name.clone(),
                    };
                    if matches!(
                        definition.key.kind,
                        DeclarationKind::Tag
                            | DeclarationKind::Struct
                            | DeclarationKind::Enum
                            | DeclarationKind::Component
                            | DeclarationKind::Resource
                    ) && paths.insert(canonical, item.id).is_some()
                    {
                        return Err(catalog_gap("duplicate canonical nominal declaration path"));
                    }
                    definitions.insert(
                        item.id,
                        DefinitionEntry {
                            registry_origin: &inventory_package.provenance.registry_origin,
                            package_scope: &inventory_package.provenance.scoped_name,
                            item,
                            definition,
                            declaration_shape: checked_view
                                .map(|checked| checked.declaration_shape()),
                        },
                    );
                }
                for body in &target.bodies {
                    let item = target
                        .items
                        .iter()
                        .find(|item| item.id == body.owner)
                        .ok_or_else(|| catalog_gap("body owner is absent from target items"))?;
                    let checked_view = checked_declarations
                        .handle_for_session_item(body.owner)
                        .and_then(|handle| checked_declarations.declaration(&handle));
                    let file = handoff
                        .frontend()
                        .sources()
                        .file(body.span.file)
                        .ok_or_else(|| catalog_gap("body span has no retained source"))?;
                    body_order.push(BodyScope {
                        package_scope: &inventory_package.provenance.scoped_name,
                        target_id: target.id,
                        target,
                        item,
                        declaration_shape: checked_view.map(|checked| checked.declaration_shape()),
                        owner_shape: checked_view.map(|checked| checked.owner_shape()),
                        body,
                        path: file.portable_path(),
                    });
                }
            }
        }
        body_order.sort_by_key(|scope| scope.body.id);
        Ok(Self {
            handoff,
            definitions,
            paths,
            body_order,
        })
    }

    fn definition(&self, item: HirItemId) -> Option<&DefinitionEntry<'a>> {
        self.definitions.get(&item)
    }

    fn item_for_path(&self, path: &SemanticDeclarationPath) -> Option<&DefinitionEntry<'a>> {
        self.paths
            .get(path)
            .and_then(|item| self.definitions.get(item))
    }
}

fn catalog_gap(detail: &'static str) -> BodyCheckIncompleteness {
    BodyCheckIncompleteness {
        body: HirBodyId(u64::MAX),
        span: Span {
            file: FileId(u64::MAX),
            start: arche_frontend::SourcePosition::START,
            end: arche_frontend::SourcePosition::START,
        },
        kind: BodyCheckIncompletenessKind::MissingRetainedJoin,
        detail: detail.into(),
    }
}

fn checked_entry_shape<'a>(
    entry: &DefinitionEntry<'a>,
    body: HirBodyId,
    span: Span,
    gaps: &mut Vec<BodyCheckIncompleteness>,
) -> Option<&'a SymbolicDeclarationShapeSkeleton> {
    match entry.declaration_shape {
        Some(shape) => Some(shape),
        None => {
            gaps.push(BodyCheckIncompleteness {
                body,
                span,
                kind: BodyCheckIncompletenessKind::MissingRetainedJoin,
                detail: format!(
                    "referenced item {:?} has no structurally closed checked declaration row",
                    entry.definition.hir_item
                )
                .into_boxed_str(),
            });
            None
        }
    }
}

struct BodyCheckOutcome {
    row: CheckedBodyRow,
    diagnostics: Vec<SemanticDiagnostic>,
    gaps: Vec<BodyCheckIncompleteness>,
}

fn missing_declaration_outcome(scope: &BodyScope<'_>) -> BodyCheckOutcome {
    BodyCheckOutcome {
        row: CheckedBodyRow {
            id: scope.body.id,
            owner: scope.body.owner,
            kind: scope.body.kind,
            span: scope.body.span,
            authority_complete: false,
            has_source_diagnostics: false,
            resolution: C2Resolution::Complete,
            pending_c4: PendingC4Dependencies::from_unsorted(Vec::new()),
            expressions: Box::new([]),
            locals: Box::new([]),
            patterns: Box::new([]),
            calls: Box::new([]),
        },
        diagnostics: Vec::new(),
        gaps: vec![BodyCheckIncompleteness {
            body: scope.body.id,
            span: scope.body.span,
            kind: BodyCheckIncompletenessKind::MissingRetainedJoin,
            detail: format!(
                "body owner {:?} has no structurally closed checked declaration row",
                scope.body.owner
            )
            .into_boxed_str(),
        }],
    }
}

#[derive(Clone, Debug)]
enum LocalValue {
    Typed(SymbolicType),
    Query { item: SymbolicType },
    Commands,
}

#[derive(Clone, Debug)]
enum ValueCategory {
    Ordinary,
    DirectFunction(HirItemId),
    AssociatedFunction(HirItemId),
    EmbeddedFunction {
        method: VirtualMethodId,
        is_unsafe: bool,
        has_effects: bool,
    },
    PendingDirectFunction(HirItemId),
    PendingAssociatedFunction(HirItemId),
    Constructor(ConstructorSelection),
    Query {
        item: SymbolicType,
    },
    Commands,
}

#[derive(Clone, Debug)]
struct CompilerNominalMethodSpec {
    method: VirtualMethodId,
    generics: Vec<CompilerMethodGenericParameterAuthority>,
    receiver: CompilerNominalMethodReceiverMode,
    receiver_type: Option<CompilerMethodTypePattern>,
    parameters: Vec<CompilerMethodTypePattern>,
    selectors: Vec<CompilerMethodSelectorAuthority>,
    result: CompilerMethodTypePattern,
    requires: Vec<CompilerNominalMethodEffectPattern>,
    throws: Vec<CompilerNominalMethodEffectPattern>,
    is_unsafe: bool,
}

impl CompilerNominalMethodSpec {
    fn from_authority(authority: &CompilerNominalMethodAuthority) -> Self {
        Self {
            method: authority.c1_method(),
            generics: authority.generics().to_vec(),
            receiver: authority.receiver(),
            receiver_type: authority.receiver_type().cloned(),
            parameters: authority.parameters().to_vec(),
            selectors: authority.selectors().to_vec(),
            result: authority.result().clone(),
            requires: authority.requires().to_vec(),
            throws: authority.throws().to_vec(),
            is_unsafe: authority.is_unsafe(),
        }
    }

    fn has_effects(&self) -> bool {
        !self.requires.is_empty() || !self.throws.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
struct CompilerMethodSubstitution {
    types: BTreeMap<CompilerMethodGenericParameter, SymbolicType>,
    lifetimes: BTreeMap<CompilerMethodGenericParameter, SymbolicLifetime>,
    capability_packs: BTreeMap<CompilerMethodGenericParameter, Vec<SymbolicType>>,
}

#[derive(Clone, Debug)]
struct LoweredValue {
    input: TypedExpressionInput,
    category: ValueCategory,
}

impl LoweredValue {
    fn ordinary(input: TypedExpressionInput) -> Self {
        Self {
            input,
            category: ValueCategory::Ordinary,
        }
    }
}

#[derive(Clone, Debug)]
enum ConstructorSelection {
    Item {
        item: HirItemId,
        variant: Option<u64>,
    },
    PendingInference {
        item: HirItemId,
        variant: Option<u64>,
    },
    EmbeddedRecord(VirtualDefinitionId),
    EmbeddedVariant(VirtualEnumVariantId),
}

/// The source form of an enclosing loop, deciding which `break` operands it
/// accepts: `loop` joins break values, while `while` and `for` accept only a
/// bare `break` and have type `()`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceLoopKind {
    Loop,
    While,
    For,
}

impl SourceLoopKind {
    const fn accepts_break_value(self) -> bool {
        matches!(self, Self::Loop)
    }
}

/// One enclosing source loop during lowering. `swallowed` records that an
/// isolated subtree materialization consumed a `break` targeting this loop:
/// that break never reaches the loop's authoritative typing frame, so a
/// `loop` with the flag set cannot be typed soundly and stays an honest gap.
/// `while` and `for` are unit-typed regardless of exit paths, so the flag is
/// harmless for them.
#[derive(Clone, Copy, Debug)]
struct SourceLoopFrame {
    kind: SourceLoopKind,
    swallowed: bool,
}

struct BodyChecker<'catalog, 'hir, 'locals> {
    catalog: &'catalog BodyCatalog<'hir>,
    scope: &'catalog BodyScope<'hir>,
    declaration_shape: &'catalog SymbolicDeclarationShapeSkeleton,
    owner_shape: &'catalog SymbolicDefinitionOwnerSkeleton,
    typing: TypingContext,
    cross_body_locals: &'locals mut BTreeMap<LocalId, LocalValue>,
    expressions: Vec<CheckedBodyExpression>,
    patterns: Vec<CheckedBodyPattern>,
    calls: Vec<CheckedBodyCall>,
    pattern_symbolic: BTreeMap<PatternType, SymbolicType>,
    ambiguous_pattern_symbolic: BTreeSet<PatternType>,
    ctfe: Vec<NeedsCtfeObligation>,
    pending_c4: Vec<PendingC4Dependency>,
    diagnostics: Vec<SemanticDiagnostic>,
    gaps: Vec<BodyCheckIncompleteness>,
    source_loops: Vec<SourceLoopFrame>,
    unsafe_depth: usize,
    generator_resume_type: Option<SymbolicType>,
    generator_yield_type: Option<SymbolicType>,
    or_binding_aliases: Vec<BTreeMap<String, Span>>,
}

impl<'catalog, 'hir, 'locals> BodyChecker<'catalog, 'hir, 'locals> {
    fn new(
        catalog: &'catalog BodyCatalog<'hir>,
        scope: &'catalog BodyScope<'hir>,
        declaration_shape: &'catalog SymbolicDeclarationShapeSkeleton,
        owner_shape: &'catalog SymbolicDefinitionOwnerSkeleton,
        cross_body_locals: &'locals mut BTreeMap<LocalId, LocalValue>,
    ) -> Self {
        let typing = typing_context(declaration_shape, owner_shape).unwrap_or_default();
        let unsafe_depth = usize::from(matches!(
            &declaration_shape.payload,
            SymbolicDeclarationPayloadSkeleton::Callable(callable) if callable.unsafe_
        ));
        Self {
            catalog,
            scope,
            declaration_shape,
            owner_shape,
            typing,
            cross_body_locals,
            expressions: Vec::new(),
            patterns: Vec::new(),
            calls: Vec::new(),
            pattern_symbolic: BTreeMap::new(),
            ambiguous_pattern_symbolic: BTreeSet::new(),
            ctfe: Vec::new(),
            pending_c4: Vec::new(),
            diagnostics: Vec::new(),
            gaps: Vec::new(),
            source_loops: Vec::new(),
            unsafe_depth,
            generator_resume_type: None,
            generator_yield_type: None,
            or_binding_aliases: Vec::new(),
        }
    }

    fn check(&mut self) -> BodyCheckOutcome {
        if let Err(detail) = typing_context(self.declaration_shape, self.owner_shape) {
            self.gap(
                self.scope.body.span,
                BodyCheckIncompletenessKind::PendingC2Type,
                detail,
            );
        }
        self.seed_parameters();
        match &self.scope.body.source {
            HirBodySource::Block(block) => {
                let expected = callable_result(self.declaration_shape)
                    .and_then(|shape| self.require_type_shape(shape, self.scope.body.span));
                self.check_block(block, expected.as_ref());
            }
            HirBodySource::Expression(expression) => {
                let expected = declared_value_type(self.declaration_shape)
                    .and_then(|shape| self.require_type_shape(shape, expression.span));
                self.check_expression(expression, expected.as_ref());
            }
            HirBodySource::WorldInitializer(initializer) => {
                for entry in &initializer.entries {
                    match &entry.kind {
                        arche_frontend::ast::AstWorldInitKind::Resource { ty, value } => {
                            if let Some(expected) = self.type_at_span(ty.span) {
                                self.check_expression(value, Some(&expected));
                            } else {
                                self.walk_expression(value);
                            }
                        }
                        arche_frontend::ast::AstWorldInitKind::Spawn { values } => {
                            for value in values {
                                self.check_expression(value, None);
                            }
                        }
                    }
                }
            }
            HirBodySource::Schedule(schedule) => {
                for run in &schedule.runs {
                    self.check_schedule_run(run);
                }
            }
            HirBodySource::ConstExpression(expression) => {
                self.check_const_body(expression);
            }
            HirBodySource::Closure(closure) => {
                self.check_closure_body(closure);
            }
            HirBodySource::GeneratorClosure(generator) => {
                self.check_generator_closure_body(generator);
            }
        }

        let resolution = if self.ctfe.is_empty() {
            C2Resolution::Complete
        } else {
            C2Resolution::NeedsCtfe(
                NeedsCtfeObligations::from_unsorted(std::mem::take(&mut self.ctfe))
                    .expect("body CTFE set is nonempty"),
            )
        };
        let pending_c4 = PendingC4Dependencies::from_unsorted(std::mem::take(&mut self.pending_c4));
        let mut locals = self
            .cross_body_locals
            .iter()
            .filter_map(|(local, value)| match value {
                LocalValue::Typed(ty) if local.owner == self.scope.item.id => {
                    Some(CheckedBodyLocal {
                        local: *local,
                        ty: ty.clone(),
                    })
                }
                LocalValue::Typed(_) | LocalValue::Query { .. } | LocalValue::Commands => None,
            })
            .collect::<Vec<_>>();
        locals.sort_by_key(|row| row.local);
        self.expressions.sort_by_key(|row| span_key(row.span));
        self.patterns.sort_by_key(|row| span_key(row.span));
        self.calls.sort_by_key(|row| span_key(row.span));
        BodyCheckOutcome {
            row: CheckedBodyRow {
                id: self.scope.body.id,
                owner: self.scope.body.owner,
                kind: self.scope.body.kind,
                span: self.scope.body.span,
                authority_complete: false,
                has_source_diagnostics: false,
                resolution,
                pending_c4,
                expressions: std::mem::take(&mut self.expressions).into_boxed_slice(),
                locals: locals.into_boxed_slice(),
                patterns: std::mem::take(&mut self.patterns).into_boxed_slice(),
                calls: std::mem::take(&mut self.calls).into_boxed_slice(),
            },
            diagnostics: std::mem::take(&mut self.diagnostics),
            gaps: std::mem::take(&mut self.gaps),
        }
    }

    fn check_schedule_run(&mut self, run: &arche_frontend::ast::AstScheduleRun) {
        let path_uses = self
            .scope
            .item
            .path_uses
            .iter()
            .filter(|candidate| candidate.path.span == run.target.span)
            .collect::<Vec<_>>();
        if path_uses.len() != 1 {
            self.gap(
                run.span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                format!(
                    "schedule target joins to {} retained HIR path uses",
                    path_uses.len()
                ),
            );
            return;
        }
        let resolution = match self.path_resolution(run.target.span) {
            Some(resolution) => resolution.clone(),
            None => return,
        };
        let [Res::Item(HirItemRes::Definition(item))] = resolution.resolutions.as_slice() else {
            self.source_error(
                run.span,
                "TYPE002",
                "schedule run target is not one system declaration",
            );
            return;
        };
        let Some(entry) = self.catalog.definition(*item) else {
            self.gap(
                run.span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "schedule run target has no declaration catalog row",
            );
            return;
        };
        if checked_entry_shape(entry, self.scope.body.id, run.span, &mut self.gaps).is_none() {
            return;
        }
        if entry.definition.key.kind != DeclarationKind::System {
            self.source_error(
                run.span,
                "TYPE002",
                format!(
                    "schedule run target `{}` is not a system",
                    entry.definition.key.name
                ),
            );
            return;
        }
        let mut actuals = match self.path_actuals(path_uses[0], run.span) {
            Some(actuals) => actuals,
            None => return,
        };
        if let Some(arguments) = &run.arguments {
            if !actuals.is_empty() {
                self.source_error(
                    run.span,
                    "TYPE001",
                    "schedule run supplies generic arguments in both path and system syntax",
                );
                return;
            }
            for argument in &arguments.arguments {
                let actual = match argument {
                    arche_frontend::ast::AstSystemGenericArgument::Type(ty) => {
                        let Some(ty) = self.type_at_span(ty.span) else {
                            return;
                        };
                        GenericArgumentShape::Type(ty)
                    }
                    arche_frontend::ast::AstSystemGenericArgument::IntegerConst(value) => {
                        let Some(value) = self.const_at_span(value.span) else {
                            return;
                        };
                        GenericArgumentShape::IntegerConst(value)
                    }
                };
                actuals.push(actual);
            }
        }
        let source_formals = match &entry.item.source {
            HirItemSource::Declaration(declaration) => match &declaration.kind {
                AstDeclarationKind::System(system) => system
                    .generics
                    .as_ref()
                    .map_or(&[][..], |generics| generics.parameters.as_slice()),
                _ => {
                    self.gap(
                        run.span,
                        BodyCheckIncompletenessKind::MissingRetainedJoin,
                        "system declaration catalog row has non-system source syntax",
                    );
                    return;
                }
            },
            _ => {
                self.gap(
                    run.span,
                    BodyCheckIncompletenessKind::MissingRetainedJoin,
                    "system declaration catalog row has no declaration source syntax",
                );
                return;
            }
        };
        if actuals.len() != source_formals.len() {
            self.source_error(
                run.span,
                "TYPE001",
                format!(
                    "system `{}` expects {} generic arguments, found {}",
                    entry.definition.key.name,
                    source_formals.len(),
                    actuals.len()
                ),
            );
            return;
        }
        for (formal, actual) in source_formals.iter().zip(&actuals) {
            let matches = matches!(
                (&formal.kind, actual),
                (
                    arche_frontend::ast::AstGenericParameterKind::Type { .. },
                    GenericArgumentShape::Type(_)
                ) | (
                    arche_frontend::ast::AstGenericParameterKind::Lifetime { .. },
                    GenericArgumentShape::Lifetime(_)
                ) | (
                    arche_frontend::ast::AstGenericParameterKind::IntegerConst { .. },
                    GenericArgumentShape::IntegerConst(_)
                )
            );
            if !matches {
                self.source_error(
                    run.span,
                    "TYPE001",
                    format!(
                        "system `{}` generic argument kind does not match its source parameter",
                        entry.definition.key.name
                    ),
                );
                return;
            }
        }
        self.calls.push(CheckedBodyCall {
            span: run.span,
            callee: CheckedBodyCallee::DirectItem(*item),
            result: SymbolicType::Unit,
        });
    }

    fn check_const_body(&mut self, expression: &AstConstExpression) {
        let _ = self.const_at_span(expression.span);
    }

    /// Types a closure expression exactly when its shape is fully decided at
    /// C2: a non-generic owner, no captured enclosing locals, every parameter
    /// and the result annotated, and explicitly empty effect boundaries.
    /// Anything else returns `None` and keeps the caller's honest gap for the
    /// C4 capture/Fn-category authority.
    fn lower_noncapturing_closure(
        &mut self,
        closure: &arche_frontend::ast::AstClosure,
        span: Span,
    ) -> Option<LoweredValue> {
        let (owner, expression_ordinal, arguments) =
            self.closure_expression_identity(span, SemanticBodyKind::Closure)?;
        if !self.closure_is_noncapturing(span) {
            return None;
        }
        let (parameters, result, throws) = self.annotated_closure_signature(
            &closure.parameters,
            &closure.effects,
            closure.result.as_ref(),
        )?;
        let ty = SymbolicType::Closure {
            owner: Box::new(owner),
            expression_ordinal,
            captures: Vec::new(),
            parameters,
            result: Box::new(result),
            requires: SymbolicTypeEffectSet::default(),
            throws,
            arguments,
        };
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(ty)))
    }

    /// Types a generator-closure expression as its anonymous factory value
    /// under the same full-decision gates as `lower_noncapturing_closure`.
    /// A zero-capture factory implements `Fn` exactly, with empty factory
    /// effect sets per the generator-construction contract.
    fn lower_noncapturing_generator_closure(
        &mut self,
        generator: &arche_frontend::ast::AstGeneratorClosure,
        span: Span,
    ) -> Option<LoweredValue> {
        let (owner, expression_ordinal, arguments) =
            self.closure_expression_identity(span, SemanticBodyKind::Generator)?;
        if !self.closure_is_noncapturing(span) {
            return None;
        }
        let (parameters, result, throws) = self.annotated_closure_signature(
            &generator.parameters,
            &generator.effects,
            generator.result.as_ref(),
        )?;
        let resume = self.type_at_span(generator.resume.span)?;
        let yields = self.type_at_span(generator.yields.span)?;
        let target = arche_frontend::GeneratorTarget::Anonymous {
            owner,
            expression_ordinal,
            arguments,
        };
        let produced = SymbolicType::Generator {
            target: Box::new(target.clone()),
            captures: Vec::new(),
            parameters: parameters.clone(),
            factory_unsafe: false,
            resume: Box::new(resume),
            yields: Box::new(yields),
            result: Box::new(result),
            requires: SymbolicTypeEffectSet::default(),
            throws,
        };
        let ty = SymbolicType::GeneratorFactory {
            target: Box::new(target),
            captures: Vec::new(),
            call_trait: arche_frontend::CallTrait::Fn,
            parameters,
            factory_unsafe: false,
            produced_generator: Box::new(produced),
        };
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(ty)))
    }

    /// Resolves the owner declaration path and the C1 expression ordinal of
    /// the closure or generator body row whose span sits inside this
    /// expression. Non-generic owners only; anything ambiguous fails closed.
    fn closure_expression_identity(
        &self,
        span: Span,
        kind: SemanticBodyKind,
    ) -> Option<(SemanticDeclarationPath, u64, Vec<GenericArgumentShape>)> {
        let entry = self.catalog.definitions.get(&self.scope.item.id)?;
        let shape = entry.declaration_shape?;
        // Inside its own owner, a closure's owner instantiation is exactly
        // the identity arguments of the owner's generic parameters.
        let arguments = shape
            .generic_parameters
            .iter()
            .enumerate()
            .map(|(index, kind)| {
                let index = u64::try_from(index).ok()?;
                Some(match kind {
                    GenericParameterKind::Type => {
                        GenericArgumentShape::Type(SymbolicType::BoundType { depth: 0, index })
                    }
                    GenericParameterKind::Lifetime => {
                        GenericArgumentShape::Lifetime(SymbolicLifetime::Bound { depth: 0, index })
                    }
                    GenericParameterKind::IntegerConst(integer_type) => {
                        GenericArgumentShape::IntegerConst(SymbolicConstExpression {
                            integer_type: *integer_type,
                            node: SymbolicConstNode::Bound { depth: 0, index },
                        })
                    }
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let mut rows = self.scope.target.bodies.iter().filter(|body| {
            body.owner == self.scope.item.id && body.kind == kind && span_contains(span, body.span)
        });
        let row = rows.next()?;
        if rows.next().is_some() {
            return None;
        }
        Some((entry.semantic_path(), row.ordinal, arguments))
    }

    /// True when no path inside the expression span resolves to a local
    /// declared outside it — i.e. the closure captures nothing.
    fn closure_is_noncapturing(&self, span: Span) -> bool {
        !self.scope.target.path_resolutions.iter().any(|resolution| {
            if !span_contains(span, resolution.span) {
                return false;
            }
            let [Res::Local(local)] = resolution.resolutions.as_slice() else {
                return false;
            };
            self.scope
                .item
                .locals
                .iter()
                .find(|binding| binding.id == *local)
                .is_some_and(|binding| !span_contains(span, binding.span))
        })
    }

    /// Fully annotated closure signature with explicitly empty effect
    /// boundaries; `None` whenever inference would be required.
    fn annotated_closure_signature(
        &mut self,
        parameters: &[arche_frontend::ast::AstClosureParameter],
        effects: &arche_frontend::ast::AstEffectSets,
        result: Option<&arche_frontend::ast::AstType>,
    ) -> Option<(Vec<SymbolicType>, SymbolicType, SymbolicTypeEffectSet)> {
        let explicitly_empty_requires = effects
            .requires
            .as_ref()
            .is_some_and(|set| set.members.is_empty());
        if !explicitly_empty_requires {
            return None;
        }
        // Spelled throws members lower by annotation; C1's convention wraps a
        // nonempty source list as PendingC4 until canonicalization, and an
        // explicitly empty set is already canonical.
        let throws_members = effects
            .throws
            .as_ref()?
            .members
            .iter()
            .map(|member| self.type_at_span(member.span))
            .collect::<Option<Vec<_>>>()?;
        let throws = SymbolicTypeEffectSet::pending_c4(throws_members);
        let mut parameter_types = Vec::new();
        for parameter in parameters {
            let annotation = parameter.ty.as_ref()?;
            parameter_types.push(self.type_at_span(annotation.span)?);
        }
        let result = self.type_at_span(result?.span)?;
        Some((parameter_types, result, throws))
    }

    fn check_closure_body(&mut self, closure: &arche_frontend::ast::AstClosure) {
        for parameter in &closure.parameters {
            let Some(annotation) = &parameter.ty else {
                self.gap(
                    parameter.span,
                    BodyCheckIncompletenessKind::MissingClosureType,
                    "unannotated closure parameter requires contextual closure inference",
                );
                continue;
            };
            if let Some(ty) = self.type_at_span(annotation.span) {
                self.check_and_bind_irrefutable(&parameter.pattern, &ty, PlaceMutability::Mutable);
            }
        }
        let expected = closure
            .result
            .as_ref()
            .and_then(|result| self.type_at_span(result.span));
        self.check_expression(&closure.body, expected.as_ref());
    }

    fn check_generator_closure_body(&mut self, generator: &AstGeneratorClosure) {
        for parameter in &generator.parameters {
            let Some(annotation) = &parameter.ty else {
                self.gap(
                    parameter.span,
                    BodyCheckIncompletenessKind::MissingGeneratorType,
                    "unannotated generator-closure parameter requires contextual inference",
                );
                continue;
            };
            if let Some(ty) = self.type_at_span(annotation.span) {
                self.check_and_bind_irrefutable(&parameter.pattern, &ty, PlaceMutability::Mutable);
            }
        }
        // Resume and yield contracts are declaration-local C2 types even
        // though suspension/capture state remains C4 authority.
        self.generator_resume_type = self.type_at_span(generator.resume.span);
        self.generator_yield_type = self.type_at_span(generator.yields.span);
        let expected = generator
            .result
            .as_ref()
            .and_then(|result| self.type_at_span(result.span));
        self.check_expression(&generator.body, expected.as_ref());
    }

    fn seed_parameters(&mut self) {
        match &self.scope.item.source {
            HirItemSource::Declaration(declaration) => match &declaration.kind {
                AstDeclarationKind::Function(function) => {
                    let Some(parameters) = callable_parameters(self.declaration_shape) else {
                        self.gap(
                            declaration.span,
                            BodyCheckIncompletenessKind::MissingRetainedJoin,
                            "function has no callable inventory payload",
                        );
                        return;
                    };
                    let parameter_types = parameters
                        .iter()
                        .map(|parameter| &parameter.ty)
                        .collect::<Vec<_>>();
                    self.seed_ast_parameters(
                        function
                            .signature
                            .parameters
                            .iter()
                            .map(|parameter| (&parameter.pattern, parameter.span)),
                        &parameter_types,
                    );
                }
                AstDeclarationKind::Generator(generator) => {
                    self.generator_resume_type = self.type_at_span(generator.resume.span);
                    self.generator_yield_type = self.type_at_span(generator.yields.span);
                    let Some(parameters) = callable_parameters(self.declaration_shape) else {
                        self.gap(
                            declaration.span,
                            BodyCheckIncompletenessKind::MissingRetainedJoin,
                            "generator has no callable inventory payload",
                        );
                        return;
                    };
                    let parameter_types = parameters
                        .iter()
                        .map(|parameter| &parameter.ty)
                        .collect::<Vec<_>>();
                    self.seed_ast_parameters(
                        generator
                            .parameters
                            .iter()
                            .map(|parameter| (&parameter.pattern, parameter.span)),
                        &parameter_types,
                    );
                }
                AstDeclarationKind::System(system) => self.seed_system_parameters(system),
                AstDeclarationKind::World { .. }
                | AstDeclarationKind::Component(_)
                | AstDeclarationKind::Resource(_)
                | AstDeclarationKind::Tag
                | AstDeclarationKind::Struct(_)
                | AstDeclarationKind::Enum(_)
                | AstDeclarationKind::TypeAlias(_)
                | AstDeclarationKind::Const(_)
                | AstDeclarationKind::Static(_)
                | AstDeclarationKind::Schedule(_)
                | AstDeclarationKind::Trait(_) => {}
            },
            HirItemSource::ImplMethod(method) => self.seed_method_parameters(method),
            HirItemSource::Impl(_)
            | HirItemSource::TraitMethod(_)
            | HirItemSource::QueryParameter { .. } => {}
        }
    }

    fn seed_ast_parameters<'p>(
        &mut self,
        parameters: impl Iterator<Item = (&'p AstPattern, Span)>,
        types: &[&SymbolicTypeShapeSkeleton],
    ) {
        let parameters = parameters.collect::<Vec<_>>();
        if parameters.len() != types.len() {
            self.gap(
                self.scope.body.span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "AST parameter count differs from callable payload",
            );
            return;
        }
        for ((pattern, span), shape) in parameters.into_iter().zip(types) {
            if let Some(ty) = self.require_type_shape(shape, span) {
                self.check_and_bind_irrefutable(pattern, &ty, PlaceMutability::Mutable);
            }
        }
    }

    fn seed_method_parameters(&mut self, method: &AstImplMethod) {
        let Some(parameters) = callable_parameters(self.declaration_shape) else {
            self.gap(
                method.span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "impl method has no callable inventory payload",
            );
            return;
        };
        if method.signature.parameters.len() != parameters.len() {
            self.gap(
                method.signature.span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "method AST parameter count differs from callable payload",
            );
            return;
        }
        for (ast, parameter) in method.signature.parameters.iter().zip(parameters) {
            let Some(ty) = self.require_type_shape(&parameter.ty, method.signature.span) else {
                continue;
            };
            match ast {
                AstMethodParameter::Receiver(receiver) => {
                    self.bind_named_local("self", receiver.span, LocalValue::Typed(ty));
                }
                AstMethodParameter::Parameter(parameter) => self.check_and_bind_irrefutable(
                    &parameter.pattern,
                    &ty,
                    PlaceMutability::Mutable,
                ),
            }
        }
    }

    fn seed_system_parameters(&mut self, system: &arche_frontend::ast::AstSystem) {
        let SymbolicDeclarationPayloadSkeleton::System { accesses, .. } =
            &self.declaration_shape.payload
        else {
            self.gap(
                self.scope.item.span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "system item has no system inventory payload",
            );
            return;
        };
        if accesses.len() != system.parameters.len() {
            self.gap(
                self.scope.item.span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "system access count differs from AST parameters",
            );
            return;
        }
        for (parameter, access) in system.parameters.iter().zip(accesses) {
            use arche_frontend::SymbolicSystemAccessShapeSkeleton as Access;
            let value = match access {
                Access::ResourceRead(shape)
                | Access::ResourceWrite(shape)
                | Access::CapabilityShared(shape)
                | Access::CapabilityMutable(shape) => self
                    .require_type_shape(shape, parameter.span)
                    .map(LocalValue::Typed),
                Access::Commands => Some(LocalValue::Commands),
                Access::Query(terms) => {
                    let mut item_types = Vec::new();
                    for term in terms {
                        if term.kind == arche_frontend::SymbolicQueryTermKind::Exclude {
                            continue;
                        }
                        let Some(ty) = self.require_type_shape(&term.ty, parameter.span) else {
                            continue;
                        };
                        item_types.push(ty);
                    }
                    let item = match item_types.as_slice() {
                        [] => SymbolicType::Unit,
                        [one] => one.clone(),
                        _ => SymbolicType::Tuple(item_types),
                    };
                    Some(LocalValue::Query { item })
                }
            };
            if let Some(value) = value {
                self.bind_named_local(parameter.name.as_str(), parameter.span, value);
            }
        }
    }

    fn bind_named_local(&mut self, name: &str, span: Span, value: LocalValue) {
        let candidates = self
            .scope
            .item
            .locals
            .iter()
            .filter(|local| local.name == name && local.span == span)
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                format!(
                    "local `{name}` at exact span joins to {} C1 rows",
                    candidates.len()
                ),
            );
            return;
        }
        self.cross_body_locals.insert(candidates[0].id, value);
    }

    fn require_type_shape(
        &mut self,
        shape: &SymbolicTypeShapeSkeleton,
        span: Span,
    ) -> Option<SymbolicType> {
        match shape {
            SymbolicTypeShapeSkeleton::Resolved { value, .. } => Some(value.clone()),
            SymbolicTypeShapeSkeleton::Pending(pending) => {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::PendingC2Type,
                    format!(
                        "unresolved {:?} type leaf `{}` reached body checking",
                        pending.kind, pending.debug_spelling
                    ),
                );
                None
            }
        }
    }

    fn type_at_span(&mut self, span: Span) -> Option<SymbolicType> {
        let mut collector = TypeSpanCollector::default();
        collector.collect_item(self.scope.item);
        let declaration_count = collector.declaration.len();
        if declaration_count != self.scope.item.symbolic_shape.types.len()
            || collector.body.len() != self.scope.item.body_symbolic_shape.types.len()
        {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                format!(
                    "AST type traversal ({declaration_count}/{}) differs from C1 symbolic traversal ({}/{})",
                    collector.body.len(),
                    self.scope.item.symbolic_shape.types.len(),
                    self.scope.item.body_symbolic_shape.types.len()
                ),
            );
            return None;
        }
        for (candidate_span, resolved) in collector
            .declaration
            .into_iter()
            .zip(&self.scope.item.symbolic_shape.types)
            .chain(
                collector
                    .body
                    .into_iter()
                    .zip(&self.scope.item.body_symbolic_shape.types),
            )
        {
            if candidate_span != span {
                continue;
            }
            return match resolved {
                ResolvedSymbolicType::Resolved(ty) => Some((**ty).clone()),
                ResolvedSymbolicType::Pending {
                    reason, canonical, ..
                } => {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::PendingC2Type,
                        format!("{reason:?}: `{canonical}`"),
                    );
                    None
                }
            };
        }
        self.gap(
            span,
            BodyCheckIncompletenessKind::MissingRetainedJoin,
            "AST type span has no C1 symbolic type row",
        );
        None
    }

    fn gap(&mut self, span: Span, kind: BodyCheckIncompletenessKind, detail: impl Into<Box<str>>) {
        self.gaps.push(BodyCheckIncompleteness {
            body: self.scope.body.id,
            span,
            kind,
            detail: detail.into(),
        });
    }

    fn source_error(&mut self, span: Span, code: &'static str, message: impl Into<String>) {
        let Some(package) = ScopedPackageBytes::from_canonical_name(self.scope.package_scope)
        else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "empty canonical package scope",
            );
            return;
        };
        let diagnostic = SemanticDiagnostic::new(
            CompilationPhase::BodyCallOperatorPattern,
            package,
            self.scope.target_id,
            self.scope.path.clone(),
            Diagnostic::at(code, span, message),
            Vec::new(),
        )
        .expect("body diagnostic has no secondary paths");
        self.diagnostics.push(diagnostic);
    }

    fn type_error(&mut self, span: Span, error: TypeCheckError) {
        self.source_error(span, error.code().as_str(), error.message().to_string());
    }

    fn pattern_errors(&mut self, fallback_span: Span, arms: &[AstMatchArm], errors: PatternErrors) {
        for error in errors.as_slice() {
            let span = error
                .arm_index()
                .and_then(|index| arms.get(index))
                .map_or(fallback_span, |arm| arm.pattern.span);
            self.source_error(span, error.code().as_str(), error.message());
        }
    }
}

fn typing_context(
    declaration: &SymbolicDeclarationShapeSkeleton,
    owner: &SymbolicDefinitionOwnerSkeleton,
) -> Result<TypingContext, &'static str> {
    let mut frames = Vec::new();
    match owner {
        SymbolicDefinitionOwnerSkeleton::TopLevel => {
            frames.push(BinderFrame::declaration(
                declaration.generic_parameters.clone(),
            ));
        }
        SymbolicDefinitionOwnerSkeleton::Trait { shape, .. } => {
            frames.push(BinderFrame::declaration(
                declaration.generic_parameters.clone(),
            ));
            frames.push(BinderFrame::trait_declaration(
                shape.generic_parameters.clone(),
            ));
        }
        SymbolicDefinitionOwnerSkeleton::InherentImpl {
            generic_parameters, ..
        }
        | SymbolicDefinitionOwnerSkeleton::TraitImpl {
            generic_parameters, ..
        } => {
            frames.push(BinderFrame::declaration(
                declaration.generic_parameters.clone(),
            ));
            frames.push(BinderFrame::declaration(generic_parameters.clone()));
        }
        SymbolicDefinitionOwnerSkeleton::SystemQuery { shape, .. } => {
            frames.push(BinderFrame::declaration(
                declaration.generic_parameters.clone(),
            ));
            frames.push(BinderFrame::declaration(shape.generic_parameters.clone()));
        }
    }
    let outlives = collect_outlives(declaration, owner)?;
    let return_type = callable_result(declaration)
        .and_then(resolved_type_shape)
        .or_else(|| match &declaration.payload {
            SymbolicDeclarationPayloadSkeleton::System { result, .. } => {
                resolved_type_shape(result)
            }
            _ => None,
        });
    Ok(TypingContext::new(
        BinderStack::new(frames),
        LifetimeOutlives::new(outlives),
        return_type,
    ))
}

fn collect_outlives(
    declaration: &SymbolicDeclarationShapeSkeleton,
    owner: &SymbolicDefinitionOwnerSkeleton,
) -> Result<Vec<(SymbolicLifetime, SymbolicLifetime)>, &'static str> {
    let mut edges = Vec::new();
    collect_predicate_outlives(&declaration.predicates, &mut edges)?;
    match owner {
        SymbolicDefinitionOwnerSkeleton::Trait { shape, .. }
        | SymbolicDefinitionOwnerSkeleton::SystemQuery { shape, .. } => {
            collect_predicate_outlives(&shape.predicates, &mut edges)?;
        }
        SymbolicDefinitionOwnerSkeleton::InherentImpl { predicates, .. }
        | SymbolicDefinitionOwnerSkeleton::TraitImpl { predicates, .. } => {
            collect_predicate_outlives(predicates, &mut edges)?;
        }
        SymbolicDefinitionOwnerSkeleton::TopLevel => {}
    }
    Ok(edges)
}

fn collect_predicate_outlives(
    predicates: &[SymbolicPredicateShapeSkeleton],
    output: &mut Vec<(SymbolicLifetime, SymbolicLifetime)>,
) -> Result<(), &'static str> {
    for predicate in predicates {
        match predicate {
            SymbolicPredicateShapeSkeleton::Resolved { value, .. } => {
                if let SymbolicPredicate::LifetimeOutlives { longer, shorter } = value.as_ref() {
                    output.push((longer.clone(), shorter.clone()));
                }
            }
            SymbolicPredicateShapeSkeleton::Pending(_) => {
                return Err("pending predicate reached body typing context");
            }
        }
    }
    Ok(())
}

fn callable_parameters(
    declaration: &SymbolicDeclarationShapeSkeleton,
) -> Option<&[arche_frontend::SymbolicCallableParameterSkeleton]> {
    match &declaration.payload {
        SymbolicDeclarationPayloadSkeleton::Callable(callable) => Some(&callable.parameters),
        _ => None,
    }
}

fn callable_result(
    declaration: &SymbolicDeclarationShapeSkeleton,
) -> Option<&SymbolicTypeShapeSkeleton> {
    match &declaration.payload {
        SymbolicDeclarationPayloadSkeleton::Callable(callable) => Some(&callable.result),
        SymbolicDeclarationPayloadSkeleton::System { result, .. } => Some(result),
        _ => None,
    }
}

fn declared_value_type(
    declaration: &SymbolicDeclarationShapeSkeleton,
) -> Option<&SymbolicTypeShapeSkeleton> {
    match &declaration.payload {
        SymbolicDeclarationPayloadSkeleton::Const { ty }
        | SymbolicDeclarationPayloadSkeleton::Static { ty, .. } => Some(ty),
        _ => None,
    }
}

fn resolved_type_shape(shape: &SymbolicTypeShapeSkeleton) -> Option<SymbolicType> {
    match shape {
        SymbolicTypeShapeSkeleton::Resolved { value, .. } => Some(value.clone()),
        SymbolicTypeShapeSkeleton::Pending(_) => None,
    }
}

#[derive(Clone, Copy)]
enum TypeDomain {
    Declaration,
    Body,
}

#[derive(Default)]
struct TypeSpanCollector {
    declaration: Vec<Span>,
    body: Vec<Span>,
    domain: Option<TypeDomain>,
}

impl TypeSpanCollector {
    fn collect_item(&mut self, item: &ResolvedSymbolicItem) {
        self.domain = Some(TypeDomain::Declaration);
        match &item.source {
            HirItemSource::Declaration(declaration) => self.declaration(declaration),
            HirItemSource::Impl(implementation) => {
                self.generics(implementation.generics.as_ref());
                if let Some(path) = &implementation.trait_path {
                    self.path(path);
                }
                self.ty(&implementation.target);
                self.where_clause(implementation.where_clause.as_ref());
            }
            HirItemSource::TraitMethod(method) => self.method_signature(&method.signature),
            HirItemSource::ImplMethod(method) => {
                self.method_signature(&method.signature);
                self.domain = Some(TypeDomain::Body);
                self.block(&method.body);
            }
            HirItemSource::QueryParameter { terms, .. } => {
                for term in terms {
                    self.ty(&term.ty);
                }
            }
        }
    }

    fn push(&mut self, span: Span) {
        match self.domain.expect("type collector domain is initialized") {
            TypeDomain::Declaration => self.declaration.push(span),
            TypeDomain::Body => self.body.push(span),
        }
    }

    fn declaration(&mut self, declaration: &arche_frontend::ast::AstDeclaration) {
        match &declaration.kind {
            AstDeclarationKind::World { initializer } => {
                self.domain = Some(TypeDomain::Body);
                for entry in &initializer.entries {
                    match &entry.kind {
                        arche_frontend::ast::AstWorldInitKind::Resource { ty, value } => {
                            self.ty(ty);
                            self.expression(value);
                        }
                        arche_frontend::ast::AstWorldInitKind::Spawn { values } => {
                            for value in values {
                                self.expression(value);
                            }
                        }
                    }
                }
            }
            AstDeclarationKind::Component(record) | AstDeclarationKind::Resource(record) => {
                self.generics(record.generics.as_ref());
                self.where_clause(record.where_clause.as_ref());
                for field in &record.fields {
                    self.ty(&field.ty);
                }
            }
            AstDeclarationKind::Struct(structure) => {
                self.generics(structure.generics.as_ref());
                self.where_clause(structure.where_clause.as_ref());
                match &structure.form {
                    AstStructForm::Unit => {}
                    AstStructForm::Tuple(fields) => {
                        for field in fields {
                            self.ty(&field.ty);
                        }
                    }
                    AstStructForm::Record(fields) => {
                        for field in fields {
                            self.ty(&field.ty);
                        }
                    }
                }
            }
            AstDeclarationKind::Enum(enumeration) => {
                self.generics(enumeration.generics.as_ref());
                self.where_clause(enumeration.where_clause.as_ref());
                for variant in &enumeration.variants {
                    match &variant.form {
                        AstVariantForm::Unit => {}
                        AstVariantForm::Tuple(fields) => {
                            for field in fields {
                                self.ty(field);
                            }
                        }
                        AstVariantForm::Record(fields) => {
                            for field in fields {
                                self.ty(&field.ty);
                            }
                        }
                    }
                }
            }
            AstDeclarationKind::TypeAlias(alias) => {
                self.generics(alias.generics.as_ref());
                self.ty(&alias.target);
                self.where_clause(alias.where_clause.as_ref());
            }
            AstDeclarationKind::Const(item) => {
                self.ty(&item.ty);
                self.domain = Some(TypeDomain::Body);
                self.expression(&item.value);
            }
            AstDeclarationKind::Static(item) => {
                self.ty(&item.ty);
                self.domain = Some(TypeDomain::Body);
                self.expression(&item.value);
            }
            AstDeclarationKind::Function(function) => {
                self.function_signature(&function.signature);
                self.domain = Some(TypeDomain::Body);
                self.block(&function.body);
            }
            AstDeclarationKind::Generator(generator) => {
                self.generics(generator.generics.as_ref());
                for parameter in &generator.parameters {
                    self.ty(&parameter.ty);
                }
                self.ty(&generator.resume);
                self.ty(&generator.yields);
                self.effects(&generator.effects);
                if let Some(result) = &generator.result {
                    self.ty(result);
                }
                self.where_clause(generator.where_clause.as_ref());
                self.domain = Some(TypeDomain::Body);
                self.block(&generator.body);
            }
            AstDeclarationKind::System(system) => {
                self.generics(system.generics.as_ref());
                for parameter in &system.parameters {
                    match &parameter.kind {
                        arche_frontend::ast::AstSystemParameterKind::ResourceRead(ty)
                        | arche_frontend::ast::AstSystemParameterKind::ResourceWrite(ty)
                        | arche_frontend::ast::AstSystemParameterKind::Capability(ty) => {
                            self.ty(ty);
                        }
                        arche_frontend::ast::AstSystemParameterKind::Query(_)
                        | arche_frontend::ast::AstSystemParameterKind::Commands => {}
                    }
                }
                self.effects(&system.effects);
                self.where_clause(system.where_clause.as_ref());
                self.domain = Some(TypeDomain::Body);
                self.block(&system.body);
            }
            AstDeclarationKind::Schedule(schedule) => {
                self.domain = Some(TypeDomain::Body);
                for run in &schedule.runs {
                    self.path(&run.target);
                    if let Some(arguments) = &run.arguments {
                        for argument in &arguments.arguments {
                            if let arche_frontend::ast::AstSystemGenericArgument::Type(ty) =
                                argument
                            {
                                self.ty(ty);
                            }
                        }
                    }
                }
            }
            AstDeclarationKind::Trait(trait_) => {
                self.generics(trait_.generics.as_ref());
                self.where_clause(trait_.where_clause.as_ref());
            }
            AstDeclarationKind::Tag => {}
        }
    }

    fn function_signature(&mut self, signature: &arche_frontend::ast::AstFunctionSignature) {
        self.generics(signature.generics.as_ref());
        for parameter in &signature.parameters {
            self.ty(&parameter.ty);
        }
        self.effects(&signature.effects);
        if let Some(result) = &signature.result {
            self.ty(result);
        }
        self.where_clause(signature.where_clause.as_ref());
    }

    fn method_signature(&mut self, signature: &arche_frontend::ast::AstMethodSignature) {
        self.generics(signature.generics.as_ref());
        for parameter in &signature.parameters {
            match parameter {
                AstMethodParameter::Receiver(receiver) => match receiver.kind {
                    AstReceiverKind::Value { .. } => self.push(receiver.span),
                    AstReceiverKind::Reference { .. } => {
                        self.push(receiver.span);
                        self.push(receiver.span);
                    }
                },
                AstMethodParameter::Parameter(parameter) => self.ty(&parameter.ty),
            }
        }
        self.effects(&signature.effects);
        if let Some(result) = &signature.result {
            self.ty(result);
        }
        self.where_clause(signature.where_clause.as_ref());
    }

    fn generics(&mut self, generics: Option<&arche_frontend::ast::AstGenericParameters>) {
        let Some(generics) = generics else {
            return;
        };
        for parameter in &generics.parameters {
            if let arche_frontend::ast::AstGenericParameterKind::Type { bounds, .. } =
                &parameter.kind
            {
                for bound in bounds {
                    if let arche_frontend::ast::AstTypeBoundKind::Trait(path) = &bound.kind {
                        self.path(path);
                    }
                }
            }
        }
    }

    fn where_clause(&mut self, clause: Option<&arche_frontend::ast::AstWhereClause>) {
        let Some(clause) = clause else {
            return;
        };
        for predicate in &clause.predicates {
            if let arche_frontend::ast::AstWherePredicateKind::Type { ty, bounds } = &predicate.kind
            {
                self.ty(ty);
                for bound in bounds {
                    if let arche_frontend::ast::AstTypeBoundKind::Trait(path) = &bound.kind {
                        self.path(path);
                    }
                }
            }
        }
    }

    fn effects(&mut self, effects: &arche_frontend::ast::AstEffectSets) {
        if let Some(requires) = &effects.requires {
            for path in &requires.members {
                self.path(path);
            }
        }
        if let Some(throws) = &effects.throws {
            for ty in &throws.members {
                self.ty(ty);
            }
        }
    }

    fn ty(&mut self, ty: &AstType) {
        self.push(ty.span);
        match &ty.kind {
            arche_frontend::ast::AstTypeKind::Path(path) => self.path(path),
            arche_frontend::ast::AstTypeKind::Tuple(types) => {
                for ty in types {
                    self.ty(ty);
                }
            }
            arche_frontend::ast::AstTypeKind::Array { element, .. }
            | arche_frontend::ast::AstTypeKind::Slice(element) => self.ty(element),
            arche_frontend::ast::AstTypeKind::Reference { pointee, .. }
            | arche_frontend::ast::AstTypeKind::RawPointer { pointee, .. } => self.ty(pointee),
            arche_frontend::ast::AstTypeKind::FunctionPointer {
                parameters,
                effects,
                result,
                ..
            } => {
                for parameter in parameters {
                    self.ty(parameter);
                }
                self.effects(effects);
                if let Some(result) = result {
                    self.ty(result);
                }
            }
            arche_frontend::ast::AstTypeKind::Scalar(_)
            | arche_frontend::ast::AstTypeKind::Never
            | arche_frontend::ast::AstTypeKind::Unit
            | arche_frontend::ast::AstTypeKind::Str
            | arche_frontend::ast::AstTypeKind::SelfType => {}
        }
    }

    fn path(&mut self, path: &arche_frontend::ast::AstPath) {
        if let Some(arguments) = &path.generic_arguments {
            self.generic_arguments(arguments);
        }
        for segment in &path.segments {
            if let Some(arguments) = &segment.generic_arguments {
                self.generic_arguments(arguments);
            }
        }
    }

    fn generic_arguments(&mut self, arguments: &arche_frontend::ast::AstGenericArguments) {
        for argument in &arguments.arguments {
            if let arche_frontend::ast::AstGenericArgumentKind::Type(ty) = &argument.kind {
                self.ty(ty);
            }
        }
    }

    fn block(&mut self, block: &AstBlock) {
        for statement in &block.statements {
            match &statement.kind {
                AstStatementKind::Let {
                    ty,
                    value,
                    else_block,
                    ..
                } => {
                    if let Some(ty) = ty {
                        self.ty(ty);
                    }
                    self.expression(value);
                    if let Some(block) = else_block {
                        self.block(block);
                    }
                }
                AstStatementKind::For { iterator, body, .. } => {
                    self.expression(iterator);
                    self.block(body);
                }
                AstStatementKind::Assignment { place, value, .. } => {
                    self.expression(place);
                    self.expression(value);
                }
                AstStatementKind::Expression { expression, .. } => self.expression(expression),
            }
        }
        if let Some(tail) = &block.tail {
            self.expression(tail);
        }
    }

    fn expression(&mut self, expression: &AstExpression) {
        match &expression.kind {
            AstExpressionKind::Path(path) => self.path(path),
            AstExpressionKind::Group(child)
            | AstExpressionKind::Unary { operand: child, .. }
            | AstExpressionKind::Yield(child) => self.expression(child),
            AstExpressionKind::Tuple(values) | AstExpressionKind::Array(values) => {
                for value in values {
                    self.expression(value);
                }
            }
            AstExpressionKind::ArrayRepeat { value, .. } => self.expression(value),
            AstExpressionKind::Record {
                constructor,
                fields,
            } => {
                self.path(constructor);
                for field in fields {
                    self.expression(&field.value);
                }
            }
            AstExpressionKind::Block(block)
            | AstExpressionKind::Loop(block)
            | AstExpressionKind::Unsafe(block) => self.block(block),
            AstExpressionKind::If(if_) => {
                self.condition(&if_.condition);
                self.block(&if_.then_block);
                if let Some(branch) = &if_.else_branch {
                    match branch {
                        AstElseBranch::Block(block) => self.block(block),
                        AstElseBranch::If(expression) => self.expression(expression),
                    }
                }
            }
            AstExpressionKind::While(while_) => {
                self.condition(&while_.condition);
                self.block(&while_.body);
            }
            AstExpressionKind::Match { operand, arms }
            | AstExpressionKind::Catch { operand, arms } => {
                self.expression(operand);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.expression(guard);
                    }
                    self.expression(&arm.value);
                }
            }
            AstExpressionKind::Closure(closure) => {
                for parameter in &closure.parameters {
                    if let Some(ty) = &parameter.ty {
                        self.ty(ty);
                    }
                }
                self.effects(&closure.effects);
                if let Some(result) = &closure.result {
                    self.ty(result);
                }
                self.expression(&closure.body);
            }
            AstExpressionKind::GeneratorClosure(generator) => {
                for parameter in &generator.parameters {
                    if let Some(ty) = &parameter.ty {
                        self.ty(ty);
                    }
                }
                self.ty(&generator.resume);
                self.ty(&generator.yields);
                self.effects(&generator.effects);
                if let Some(result) = &generator.result {
                    self.ty(result);
                }
                self.expression(&generator.body);
            }
            AstExpressionKind::Return(value)
            | AstExpressionKind::Break(value)
            | AstExpressionKind::Throw(value) => {
                if let Some(value) = value {
                    self.expression(value);
                }
            }
            AstExpressionKind::Binary { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            }
            AstExpressionKind::Cast { value, ty } => {
                self.expression(value);
                self.ty(ty);
            }
            AstExpressionKind::Postfix { base, parts } => {
                self.expression(base);
                for part in parts {
                    match &part.kind {
                        AstPostfixKind::Call(arguments)
                        | AstPostfixKind::CommandSpawn(arguments) => {
                            for argument in arguments {
                                self.expression(argument);
                            }
                        }
                        AstPostfixKind::Index(index) | AstPostfixKind::Resume(index) => {
                            self.expression(index);
                        }
                        AstPostfixKind::Method {
                            generic_arguments,
                            arguments,
                            ..
                        } => {
                            if let Some(arguments_) = generic_arguments {
                                self.generic_arguments(arguments_);
                            }
                            for argument in arguments {
                                self.expression(argument);
                            }
                        }
                        AstPostfixKind::TurbofishCall {
                            generic_arguments,
                            arguments,
                        } => {
                            self.generic_arguments(generic_arguments);
                            for argument in arguments {
                                self.expression(argument);
                            }
                        }
                        AstPostfixKind::Field(_) | AstPostfixKind::TupleField(_) => {}
                    }
                }
            }
            AstExpressionKind::Literal(_)
            | AstExpressionKind::SelfValue
            | AstExpressionKind::Unit
            | AstExpressionKind::Continue => {}
        }
    }

    fn condition(&mut self, condition: &AstCondition) {
        match condition {
            AstCondition::Expression(expression) => self.expression(expression),
            AstCondition::Let { value, .. } => self.expression(value),
        }
    }
}

impl BodyChecker<'_, '_, '_> {
    fn walk_expression(&mut self, expression: &AstExpression) {
        let _ = self.lower_expression(expression, None);
    }

    fn check_expression(
        &mut self,
        expression: &AstExpression,
        expected: Option<&SymbolicType>,
    ) -> Option<CheckedExpression> {
        let lowered = self.lower_expression(expression, expected)?;
        self.materialize(&lowered, expected, expression.span)
    }

    fn materialize(
        &mut self,
        lowered: &LoweredValue,
        expected: Option<&SymbolicType>,
        span: Span,
    ) -> Option<CheckedExpression> {
        if !matches!(lowered.category, ValueCategory::Ordinary) {
            self.gap(
                span,
                BodyCheckIncompletenessKind::UnsupportedC2AdapterSurface,
                "special body value reached an ordinary expression context",
            );
            return None;
        }
        match check_typed_expression_in_loops(
            &lowered.input,
            expected,
            &self.typing,
            self.source_loops.len(),
        ) {
            Ok((checked, seeded_break_counts)) => {
                for (index, count) in seeded_break_counts.iter().enumerate() {
                    if *count > 0 {
                        self.source_loops[index].swallowed = true;
                    }
                }
                self.expressions.push(CheckedBodyExpression {
                    span,
                    expression: checked.clone(),
                });
                Some(checked)
            }
            Err(error) => {
                if let TypeCheckErrorKind::Mismatch { expected, actual } = error.kind() {
                    if types_match_with_erased_body_lifetime(expected, actual) {
                        // Body-local region inference is deliberately separate
                        // from checked types: the erased-local marker stands in
                        // for this borrow's region, and C3's NLL rederives the
                        // declaration-bound relation from Core region facts
                        // rather than from a session type.
                        return self.materialize(lowered, None, span);
                    }
                }
                if let TypeCheckErrorKind::UnsatisfiedPrimitiveOperator {
                    operator,
                    left,
                    right,
                    ..
                } = error.kind()
                {
                    if !is_scalar_primitive(left)
                        || right
                            .as_ref()
                            .is_some_and(|right| !is_scalar_primitive(right))
                    {
                        self.gap(
                            span,
                            BodyCheckIncompletenessKind::MissingEmbeddedTraitIdentity,
                            format!(
                                "operator {operator:?} on {left:?}/{right:?} requires compiler-trait selection before stable embedded trait identities exist"
                            ),
                        );
                        return None;
                    }
                }
                self.type_error(span, error);
                None
            }
        }
    }

    fn lower_expression(
        &mut self,
        expression: &AstExpression,
        expected: Option<&SymbolicType>,
    ) -> Option<LoweredValue> {
        match &expression.kind {
            AstExpressionKind::Literal(literal) => Some(LoweredValue::ordinary(match literal {
                AstLiteral::Integer(literal) => TypedExpressionInput::Integer(literal.clone()),
                AstLiteral::Float(literal) => TypedExpressionInput::Float(literal.clone()),
                AstLiteral::Character(value) => TypedExpressionInput::Character(*value),
                AstLiteral::String(value) => TypedExpressionInput::String(value.as_ref().into()),
                AstLiteral::Boolean(value) => TypedExpressionInput::Boolean(*value),
            })),
            AstExpressionKind::Path(path) => self.lower_path(path, expression.span, expected),
            AstExpressionKind::SelfValue => self.lower_self(expression.span),
            AstExpressionKind::Unit => Some(LoweredValue::ordinary(TypedExpressionInput::Unit)),
            AstExpressionKind::Group(child) => self.lower_expression(child, expected),
            AstExpressionKind::Tuple(elements) => {
                let expected_elements = match expected {
                    Some(SymbolicType::Tuple(elements)) => Some(elements.as_slice()),
                    _ => None,
                };
                let mut lowered = Vec::with_capacity(elements.len());
                for (index, element) in elements.iter().enumerate() {
                    let expected = expected_elements.and_then(|types| types.get(index));
                    if let Some(value) = self.lower_expression(element, expected) {
                        lowered.push(value.input);
                    }
                }
                (lowered.len() == elements.len())
                    .then(|| LoweredValue::ordinary(TypedExpressionInput::Tuple(lowered)))
            }
            AstExpressionKind::Array(elements) => {
                let expected_element = match expected {
                    Some(SymbolicType::Array { element, .. }) => Some(element.as_ref()),
                    _ => None,
                };
                let mut lowered = Vec::with_capacity(elements.len());
                for element in elements {
                    if let Some(value) = self.lower_expression(element, expected_element) {
                        lowered.push(value.input);
                    }
                }
                (lowered.len() == elements.len())
                    .then(|| LoweredValue::ordinary(TypedExpressionInput::Array(lowered)))
            }
            AstExpressionKind::ArrayRepeat { value, count } => {
                let expected_element = match expected {
                    Some(SymbolicType::Array { element, .. }) => Some(element.as_ref()),
                    _ => None,
                };
                let value = self.lower_expression(value, expected_element)?;
                let length = self.const_at_span(count.span)?;
                Some(LoweredValue::ordinary(TypedExpressionInput::ArrayRepeat {
                    value: Box::new(value.input),
                    length,
                }))
            }
            AstExpressionKind::Record {
                constructor,
                fields,
            } => self.lower_record(expression.span, constructor, fields, expected),
            AstExpressionKind::Block(block) => self.lower_block(block, expected),
            AstExpressionKind::If(if_) => self.lower_if(expression.span, if_, expected),
            AstExpressionKind::While(while_) => self.lower_while(expression.span, while_, expected),
            AstExpressionKind::Loop(block) => {
                let (body, swallowed) = self.lower_loop_body(block, SourceLoopKind::Loop);
                let body = body?;
                if swallowed {
                    self.gap(
                        expression.span,
                        BodyCheckIncompletenessKind::UnsupportedC2AdapterSurface,
                        "the expression typing algebra does not retain the enclosing loop join for a break inside an isolated subtree",
                    );
                    return None;
                }
                Some(LoweredValue::ordinary(TypedExpressionInput::Loop {
                    body: Box::new(body.input),
                }))
            }
            AstExpressionKind::Match { operand, arms } => {
                self.lower_match(expression.span, operand, arms, expected, false)
            }
            AstExpressionKind::Catch { operand, arms } => {
                self.lower_match(expression.span, operand, arms, expected, true)
            }
            AstExpressionKind::Unsafe(block) => {
                self.unsafe_depth += 1;
                let lowered = self.lower_block(block, expected);
                self.unsafe_depth -= 1;
                lowered
            }
            AstExpressionKind::Closure(closure) => {
                if let Some(value) = self.lower_noncapturing_closure(closure, expression.span) {
                    Some(value)
                } else {
                    self.gap(
                        expression.span,
                        BodyCheckIncompletenessKind::MissingClosureType,
                        "closure expression type requires the C4 capture/Fn-category authority",
                    );
                    None
                }
            }
            AstExpressionKind::GeneratorClosure(generator) => {
                if let Some(value) =
                    self.lower_noncapturing_generator_closure(generator, expression.span)
                {
                    Some(value)
                } else {
                    self.gap(
                        expression.span,
                        BodyCheckIncompletenessKind::MissingGeneratorType,
                        "generator-closure factory type requires the C4 capture authority",
                    );
                    None
                }
            }
            AstExpressionKind::Return(value) => {
                let expected = self.typing.return_type().cloned();
                let value = match value.as_deref() {
                    Some(value) => Some(Box::new(
                        self.lower_expression(value, expected.as_ref())?.input,
                    )),
                    None => None,
                };
                Some(LoweredValue::ordinary(TypedExpressionInput::Return(value)))
            }
            AstExpressionKind::Break(value) => {
                let Some(&SourceLoopFrame {
                    kind: enclosing, ..
                }) = self.source_loops.last()
                else {
                    self.source_error(expression.span, "TYPE002", "break used outside a loop");
                    if let Some(value) = value {
                        self.walk_expression(value);
                    }
                    return None;
                };
                if value.is_some() && !enclosing.accepts_break_value() {
                    self.source_error(
                        expression.span,
                        "TYPE002",
                        "`while` and `for` loops accept only a bare `break`",
                    );
                    if let Some(value) = value {
                        self.walk_expression(value);
                    }
                    return None;
                }
                let value = match value.as_deref() {
                    Some(value) => Some(Box::new(self.lower_expression(value, expected)?.input)),
                    None => None,
                };
                Some(LoweredValue::ordinary(TypedExpressionInput::Break(value)))
            }
            AstExpressionKind::Continue => {
                if self.source_loops.is_empty() {
                    self.source_error(expression.span, "TYPE002", "continue used outside a loop");
                    return None;
                }
                Some(LoweredValue::ordinary(TypedExpressionInput::Continue))
            }
            AstExpressionKind::Throw(value) => {
                if let Some(value) = value {
                    self.check_expression(value, None)?;
                }
                self.pending_c4(
                    expression.span,
                    b"throw-effect-membership".to_vec(),
                    "throw payload membership is finalized by C4 effects",
                );
                Some(LoweredValue::ordinary(TypedExpressionInput::Known(
                    SymbolicType::Never,
                )))
            }
            AstExpressionKind::Yield(value) => {
                let expected_yield = self.generator_yield_type.clone();
                self.check_expression(value, expected_yield.as_ref())?;
                self.pending_c4(
                    expression.span,
                    b"generator-yield-contract".to_vec(),
                    "yield/resume state contract is finalized by C4",
                );
                let Some(resume) = self.generator_resume_type.clone() else {
                    self.gap(
                        expression.span,
                        BodyCheckIncompletenessKind::MissingGeneratorType,
                        "yield expression has no retained generator resume contract",
                    );
                    return None;
                };
                Some(LoweredValue::ordinary(TypedExpressionInput::Known(resume)))
            }
            AstExpressionKind::Unary { operator, operand } => {
                self.lower_unary(expression.span, *operator, operand, expected)
            }
            AstExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let left = self.lower_expression(left, None)?;
                let right = self.lower_expression(right, None)?;
                Some(LoweredValue::ordinary(TypedExpressionInput::Binary {
                    operator: binary_operator(*operator),
                    left: Box::new(left.input),
                    right: Box::new(right.input),
                }))
            }
            AstExpressionKind::Cast { value, ty } => self.lower_cast(expression.span, value, ty),
            AstExpressionKind::Postfix { base, parts } => {
                self.lower_postfix(expression.span, base, parts, expected)
            }
        }
    }

    fn lower_record(
        &mut self,
        span: Span,
        constructor: &arche_frontend::ast::AstPath,
        fields: &[arche_frontend::ast::AstRecordExpressionField],
        expected: Option<&SymbolicType>,
    ) -> Option<LoweredValue> {
        let selected = self.lower_path(constructor, constructor.span, expected)?;
        if let ValueCategory::Constructor(ConstructorSelection::PendingInference {
            item,
            variant,
        }) = selected.category
        {
            return self.infer_record_literal(item, variant, fields, span);
        }
        let constructed = match &selected.input {
            TypedExpressionInput::Known(ty) => ty.clone(),
            _ => {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingRetainedJoin,
                    "constructor selection did not retain its constructed nominal type",
                );
                return None;
            }
        };
        let actuals = match &constructed {
            SymbolicType::NominalPath { arguments, .. } => arguments.clone(),
            _ => Vec::new(),
        };
        let (form, declared_fields) = match selected.category {
            ValueCategory::Constructor(ConstructorSelection::Item { item, variant }) => {
                let Some(entry) = self.catalog.definition(item) else {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingRetainedJoin,
                        "record constructor has no declaration catalog row",
                    );
                    return None;
                };
                let declaration_shape =
                    checked_entry_shape(entry, self.scope.body.id, span, &mut self.gaps)?;
                match (&declaration_shape.payload, variant) {
                    (SymbolicDeclarationPayloadSkeleton::Record(record), None) => {
                        (record.form, record.fields.clone())
                    }
                    (SymbolicDeclarationPayloadSkeleton::Enum(variants), Some(ordinal)) => {
                        let Some(variant) = usize::try_from(ordinal)
                            .ok()
                            .and_then(|ordinal| variants.get(ordinal))
                        else {
                            self.gap(
                                span,
                                BodyCheckIncompletenessKind::MissingRetainedJoin,
                                "enum constructor ordinal is absent from its declaration payload",
                            );
                            return None;
                        };
                        (variant.form, variant.fields.clone())
                    }
                    (SymbolicDeclarationPayloadSkeleton::Tag, None) => {
                        (SymbolicRecordForm::Unit, Vec::new())
                    }
                    _ => {
                        self.source_error(
                            span,
                            "TYPE002",
                            "record literal path does not select a record-form constructor",
                        );
                        return None;
                    }
                }
            }
            ValueCategory::Constructor(ConstructorSelection::EmbeddedRecord(definition)) => {
                match self.embedded_record_form(definition) {
                    Some(pair) => pair,
                    None => {
                        for field in fields {
                            self.check_expression(&field.value, None);
                        }
                        self.gap(
                            span,
                            BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                            format!(
                                "embedded record constructor {definition:?} has no typed C2 field descriptor"
                            ),
                        );
                        return None;
                    }
                }
            }
            ValueCategory::Constructor(ConstructorSelection::EmbeddedVariant(variant)) => {
                match self.embedded_variant_form(variant) {
                    Some((_, form, variant_fields)) => (form, variant_fields),
                    None => {
                        for field in fields {
                            self.check_expression(&field.value, None);
                        }
                        self.gap(
                            span,
                            BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                            format!(
                                "embedded variant {variant:?} has no typed C2 field descriptor"
                            ),
                        );
                        return None;
                    }
                }
            }
            _ => {
                self.source_error(span, "TYPE002", "record literal path is not a constructor");
                return None;
            }
        };
        if form != SymbolicRecordForm::Record {
            self.source_error(
                span,
                "TYPE002",
                "named fields require a record-form constructor",
            );
            return None;
        }

        let mut by_name = BTreeMap::new();
        for field in declared_fields {
            let Some(name) = field.name.clone() else {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingRetainedJoin,
                    "record-form declaration payload contains an unnamed field",
                );
                return None;
            };
            if by_name.insert(name, field.ty).is_some() {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingRetainedJoin,
                    "record declaration payload contains duplicate field names",
                );
                return None;
            }
        }
        let mut seen = BTreeSet::new();
        for field in fields {
            let name = field.name.as_str();
            if !seen.insert(name.to_owned()) {
                self.source_error(
                    field.span,
                    "TYPE002",
                    format!("duplicate record field `{name}`"),
                );
                self.check_expression(&field.value, None);
                continue;
            }
            let Some(shape) = by_name.get(name) else {
                self.source_error(
                    field.span,
                    "TYPE002",
                    format!("unknown record field `{name}`"),
                );
                self.check_expression(&field.value, None);
                continue;
            };
            if let Some(field_ty) = self.require_type_shape(shape, field.span) {
                let field_ty = substitute_type(&field_ty, &actuals);
                self.check_expression(&field.value, Some(&field_ty));
            }
        }
        for name in by_name.keys() {
            if !seen.contains(name) {
                self.source_error(span, "TYPE002", format!("missing record field `{name}`"));
            }
        }
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(
            constructed,
        )))
    }

    fn lower_postfix(
        &mut self,
        _span: Span,
        base: &AstExpression,
        parts: &[arche_frontend::ast::AstPostfix],
        expected: Option<&SymbolicType>,
    ) -> Option<LoweredValue> {
        let (mut value, parts) = match self.embedded_prelude_call_head(base, parts) {
            Some((value, rest)) => (value?, rest),
            None => match self.verified_associated_call_head(base, parts, expected) {
                Some((value, rest)) => (value?, rest),
                None => (self.lower_expression(base, expected)?, parts),
            },
        };
        for part in parts {
            value = match &part.kind {
                AstPostfixKind::Call(arguments) => {
                    self.lower_call_part(value, arguments, part.span)?
                }
                AstPostfixKind::TurbofishCall {
                    generic_arguments,
                    arguments,
                } => {
                    value = self.instantiate_postfix_function(
                        value,
                        generic_arguments.span,
                        part.span,
                    )?;
                    self.lower_call_part(value, arguments, part.span)?
                }
                AstPostfixKind::Field(name) => {
                    self.lower_field_part(value, name.as_str(), part.span)?
                }
                AstPostfixKind::TupleField(index) => {
                    let index = integer_literal_usize(index)
                        .map_err(|message| {
                            self.source_error(part.span, "TYPE002", message);
                        })
                        .ok()?;
                    self.lower_tuple_field_part(value, index, part.span)?
                }
                AstPostfixKind::Index(index) => self.lower_index_part(value, index, part.span)?,
                AstPostfixKind::Method {
                    name,
                    generic_arguments,
                    arguments,
                } => {
                    let receiver = self.materialize(&value, None, part.span);
                    let owner = receiver.as_ref().and_then(|receiver| {
                        let SymbolicType::NominalPath { declaration, .. } =
                            peel_references(receiver.ty())
                        else {
                            return None;
                        };
                        self.embedded_nominal_kind(declaration)
                    });
                    let method = owner.and_then(|owner| {
                        self.catalog
                            .handoff
                            .frontend()
                            .inventory()
                            .embedded_core
                            .compiler_nominal_method(owner, name.as_str())
                            .map(CompilerNominalMethodSpec::from_authority)
                    });
                    if let Some(method) = method {
                        let receiver = receiver?;
                        if owner == Some(CompilerNominalKind::App)
                            && matches!(name, arche_frontend::ast::AstMethodName::Run)
                        {
                            self.lower_app_run_method(
                                &method,
                                receiver.ty(),
                                generic_arguments.as_ref(),
                                arguments,
                                part.span,
                            )?
                        } else {
                            self.lower_verified_nominal_method(
                                &method,
                                receiver.ty(),
                                generic_arguments.as_ref(),
                                arguments,
                                part.span,
                            )?
                        }
                    } else if let Some(handled) = receiver.as_ref().and_then(|receiver| {
                        self.lower_bound_trait_method(
                            receiver,
                            name.as_str(),
                            generic_arguments.as_ref(),
                            arguments,
                            part.span,
                        )
                    }) {
                        handled?
                    } else if let Some(handled) = receiver.as_ref().and_then(|receiver| {
                        self.lower_nominal_user_method(
                            receiver,
                            name.as_str(),
                            generic_arguments.as_ref(),
                            arguments,
                            part.span,
                        )
                    }) {
                        handled?
                    } else {
                        for argument in arguments {
                            self.check_expression(argument, None);
                        }
                        self.gap(
                            part.span,
                            BodyCheckIncompletenessKind::MissingMethodSelection,
                            format!(
                                "postfix method `{}` on {:?} has no retained C1 candidate row for C2 viability filtering",
                                name.as_str(),
                                receiver.as_ref().map(CheckedExpression::ty)
                            ),
                        );
                        return None;
                    }
                }
                AstPostfixKind::CommandSpawn(arguments) => {
                    if !matches!(value.category, ValueCategory::Commands) {
                        self.source_error(
                            part.span,
                            "TYPE002",
                            "spawn postfix requires a commands system parameter",
                        );
                        return None;
                    }
                    for argument in arguments {
                        if let Some(checked) = self.check_expression(argument, None) {
                            let nominal = peel_references(checked.ty());
                            let is_component = match nominal {
                                SymbolicType::NominalPath { declaration, .. } => {
                                    let Some(entry) = self.catalog.item_for_path(declaration)
                                    else {
                                        self.source_error(
                                            argument.span,
                                            "TYPE002",
                                            "commands spawn argument is not a component value",
                                        );
                                        continue;
                                    };
                                    if checked_entry_shape(
                                        entry,
                                        self.scope.body.id,
                                        argument.span,
                                        &mut self.gaps,
                                    )
                                    .is_none()
                                    {
                                        continue;
                                    }
                                    matches!(
                                        entry.definition.key.kind,
                                        DeclarationKind::Component | DeclarationKind::Tag
                                    )
                                }
                                _ => false,
                            };
                            if !is_component {
                                self.source_error(
                                    argument.span,
                                    "TYPE002",
                                    "commands spawn argument is not a component value",
                                );
                            }
                        }
                    }
                    self.calls.push(CheckedBodyCall {
                        span: part.span,
                        callee: CheckedBodyCallee::CommandSpawn,
                        result: SymbolicType::Unit,
                    });
                    LoweredValue::ordinary(TypedExpressionInput::Unit)
                }
                AstPostfixKind::Resume(resume) => {
                    // The reserved resume postfix types exactly on Pin<&mut G>
                    // for a known generator G: the argument checks against
                    // G's resume type and the call yields
                    // GeneratorState<G::Yield, G::Return>.
                    let receiver_ty = match &value.input {
                        TypedExpressionInput::Known(ty) => Some(ty.clone()),
                        _ => self
                            .materialize(&value, None, part.span)
                            .map(|checked| checked.ty().clone()),
                    };
                    let pinned = match &receiver_ty {
                        Some(SymbolicType::NominalPath {
                            declaration,
                            arguments,
                        }) if self.embedded_nominal_kind(declaration)
                            == Some(CompilerNominalKind::Pin) =>
                        {
                            match arguments.as_slice() {
                                [GenericArgumentShape::Type(SymbolicType::Reference {
                                    mutability: Mutability::Mutable,
                                    pointee,
                                    ..
                                })] => match &**pointee {
                                    SymbolicType::Generator {
                                        resume: resume_ty,
                                        yields,
                                        result,
                                        throws,
                                        ..
                                    } => Some((
                                        resume_ty.as_ref().clone(),
                                        yields.as_ref().clone(),
                                        result.as_ref().clone(),
                                        throws.clone(),
                                    )),
                                    _ => None,
                                },
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    let state_path = pinned.as_ref().and_then(|_| {
                        let definition = {
                            let core = &self.catalog.handoff.frontend().inventory().embedded_core;
                            core.typed_c2()
                                .nominal(CompilerNominalKind::GeneratorState)
                                .c1_definition()
                        };
                        self.embedded_declaration_path(definition)
                    });
                    match (pinned, state_path) {
                        (Some((resume_ty, yields, completion, throws)), Some(declaration)) => {
                            self.check_expression(resume, Some(&resume_ty));
                            if !throws.members().is_empty() {
                                self.pending_c4(
                                    part.span,
                                    b"generator-resume-throws".to_vec(),
                                    "resume exception propagation is finalized by C4 effects",
                                );
                            }
                            let state = SymbolicType::NominalPath {
                                declaration,
                                arguments: vec![
                                    GenericArgumentShape::Type(yields),
                                    GenericArgumentShape::Type(completion),
                                ],
                            };
                            self.calls.push(CheckedBodyCall {
                                span: part.span,
                                callee: CheckedBodyCallee::GeneratorResume,
                                result: state.clone(),
                            });
                            LoweredValue::ordinary(TypedExpressionInput::Known(state))
                        }
                        _ => {
                            self.check_expression(resume, None);
                            self.gap(
                                part.span,
                                BodyCheckIncompletenessKind::MissingGeneratorType,
                                "resume postfix requires finalized generator suspension-state typing",
                            );
                            return None;
                        }
                    }
                }
            };
        }
        Some(value)
    }

    fn instantiate_postfix_function(
        &mut self,
        mut value: LoweredValue,
        generic_span: Span,
        call_span: Span,
    ) -> Option<LoweredValue> {
        let (item, associated) = match value.category {
            ValueCategory::PendingDirectFunction(item) => (item, false),
            ValueCategory::PendingAssociatedFunction(item) => (item, true),
            ValueCategory::DirectFunction(_)
            | ValueCategory::AssociatedFunction(_)
            | ValueCategory::EmbeddedFunction { .. }
            | ValueCategory::Ordinary
            | ValueCategory::Constructor(_)
            | ValueCategory::Query { .. }
            | ValueCategory::Commands => {
                self.source_error(
                    call_span,
                    "TYPE002",
                    "postfix generic arguments apply only to an uninstantiated generic callable",
                );
                return None;
            }
        };
        let actuals = self.postfix_actuals(generic_span)?;
        let Some(entry) = self.catalog.definition(item) else {
            self.gap(
                call_span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "postfix generic callable has no declaration catalog row",
            );
            return None;
        };
        let declaration_shape =
            checked_entry_shape(entry, self.scope.body.id, call_span, &mut self.gaps)?;
        let signature = self.callable_signature(declaration_shape, &actuals, call_span)?;
        value.input = TypedExpressionInput::Known(signature.function_pointer());
        value.category = if associated {
            ValueCategory::AssociatedFunction(item)
        } else {
            ValueCategory::DirectFunction(item)
        };
        Some(value)
    }

    fn lower_verified_associated_method(
        &mut self,
        method: &CompilerNominalMethodSpec,
        path_use: &arche_frontend::HirPathUse,
        expected: Option<&SymbolicType>,
        span: Span,
    ) -> Option<LoweredValue> {
        if method.receiver != CompilerNominalMethodReceiverMode::None
            || method.receiver_type.is_some()
        {
            self.source_error(
                span,
                "TYPE002",
                "instance compiler nominal method requires a receiver",
            );
            return None;
        }
        if !method.selectors.is_empty() {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                "associated compiler nominal selectors are not representable as a function value",
            );
            return None;
        }

        let mut substitution = CompilerMethodSubstitution::default();
        let actuals = self.path_actuals(path_use, span)?;
        if !actuals.is_empty()
            && !self.bind_compiler_method_actuals(method, &actuals, &mut substitution, span)
        {
            return None;
        }
        if let Some(expected) = expected {
            match self.bind_compiler_method_pattern(
                &method.result,
                expected,
                &mut substitution,
                false,
            ) {
                Some(true) => {}
                Some(false) => {
                    self.source_error(
                        span,
                        "TYPE002",
                        "expected type does not match verified associated method result",
                    );
                    return None;
                }
                None => return None,
            }
        }
        if !self.validate_compiler_method_generics(method, &substitution, &BTreeSet::new(), span) {
            return None;
        }
        if !self.validate_compiler_method_effects(method, &substitution, span) {
            return None;
        }
        let parameters = method
            .parameters
            .iter()
            .map(|pattern| self.compiler_method_pattern_type(pattern, &substitution, span))
            .collect::<Option<Vec<_>>>()?;
        let result = self.compiler_method_pattern_type(&method.result, &substitution, span)?;
        let function = SymbolicType::FunctionPointer {
            unsafe_: method.is_unsafe,
            parameters,
            result: Box::new(result),
            requires: SymbolicTypeEffectSet::default(),
            throws: SymbolicTypeEffectSet::default(),
        };
        Some(LoweredValue {
            input: TypedExpressionInput::Known(function),
            category: ValueCategory::EmbeddedFunction {
                method: method.method,
                is_unsafe: method.is_unsafe,
                has_effects: method.has_effects(),
            },
        })
    }

    fn lower_verified_nominal_method(
        &mut self,
        method: &CompilerNominalMethodSpec,
        receiver_ty: &SymbolicType,
        generic_arguments: Option<&arche_frontend::ast::AstGenericArguments>,
        arguments: &[AstExpression],
        span: Span,
    ) -> Option<LoweredValue> {
        let Some(receiver_pattern) = method.receiver_type.as_ref() else {
            for argument in arguments {
                self.check_expression(argument, None);
            }
            self.source_error(
                span,
                "TYPE002",
                "associated compiler nominal method cannot be called with receiver syntax",
            );
            return None;
        };
        if !method.selectors.is_empty() {
            for argument in arguments {
                self.check_expression(argument, None);
            }
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                "compiler nominal method selectors require dedicated source lowering",
            );
            return None;
        }
        if method.is_unsafe && self.unsafe_depth == 0 {
            for argument in arguments {
                self.check_expression(argument, None);
            }
            self.source_error(
                span,
                "TYPE002",
                "unsafe compiler nominal method call requires an unsafe context",
            );
            return None;
        }
        if !receiver_mode_matches_pattern(method.receiver, receiver_pattern) {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                "verified compiler nominal receiver mode disagrees with its typed pattern",
            );
            return None;
        }

        let mut substitution = CompilerMethodSubstitution::default();
        if let Some(generic_arguments) = generic_arguments {
            let actuals = self.postfix_actuals(generic_arguments.span)?;
            if !self.bind_compiler_method_actuals(method, &actuals, &mut substitution, span) {
                return None;
            }
        }
        match self.bind_compiler_method_pattern(
            receiver_pattern,
            receiver_ty,
            &mut substitution,
            true,
        ) {
            Some(true) => {}
            Some(false) => {
                for argument in arguments {
                    self.check_expression(argument, None);
                }
                self.source_error(
                    span,
                    "TYPE002",
                    "receiver does not match verified compiler nominal method type",
                );
                return None;
            }
            None => return None,
        }
        let receiver_bound = substitution
            .types
            .keys()
            .chain(substitution.capability_packs.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        if arguments.len() != method.parameters.len() {
            for argument in arguments {
                self.check_expression(argument, None);
            }
            self.source_error(
                span,
                "TYPE002",
                format!(
                    "compiler nominal method expects {} runtime arguments, found {}",
                    method.parameters.len(),
                    arguments.len()
                ),
            );
            return None;
        }

        let mut complete = true;
        for (pattern, argument) in method.parameters.iter().zip(arguments) {
            let expected = self.compiler_method_pattern_type_if_bound(pattern, &substitution);
            let checked = match self.check_expression(argument, expected.as_ref()) {
                Some(checked) => checked,
                None => {
                    complete = false;
                    continue;
                }
            };
            match self.bind_compiler_method_pattern(pattern, checked.ty(), &mut substitution, false)
            {
                Some(true) => {}
                Some(false) => {
                    self.source_error(
                        argument.span,
                        "TYPE002",
                        "argument does not match verified compiler nominal method parameter",
                    );
                    complete = false;
                }
                None => complete = false,
            }
        }
        if !complete
            || !self.validate_compiler_method_generics(method, &substitution, &receiver_bound, span)
            || !self.validate_compiler_method_effects(method, &substitution, span)
        {
            return None;
        }
        let result = self.compiler_method_pattern_type(&method.result, &substitution, span)?;
        if method.has_effects() {
            self.pending_c4(
                span,
                compiler_method_dependency_bytes(method.method, b"method-effects"),
                "compiler nominal method effect membership is finalized by C4",
            );
        }
        self.calls.push(CheckedBodyCall {
            span,
            callee: CheckedBodyCallee::EmbeddedMethod(method.method),
            result: result.clone(),
        });
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(result)))
    }

    fn lower_app_run_method(
        &mut self,
        method: &CompilerNominalMethodSpec,
        receiver_ty: &SymbolicType,
        generic_arguments: Option<&arche_frontend::ast::AstGenericArguments>,
        arguments: &[AstExpression],
        span: Span,
    ) -> Option<LoweredValue> {
        let Some(receiver_pattern) = method.receiver_type.as_ref() else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                "verified App.run authority has no receiver type",
            );
            return None;
        };

        let mut complete = true;
        let mut substitution = CompilerMethodSubstitution::default();
        if let Some(generic_arguments) = generic_arguments {
            let actuals = self.postfix_actuals(generic_arguments.span)?;
            complete &= self.bind_compiler_method_actuals(
                method,
                &actuals,
                &mut substitution,
                generic_arguments.span,
            );
        }
        if method.receiver != CompilerNominalMethodReceiverMode::Mutable
            || !receiver_mode_matches_pattern(method.receiver, receiver_pattern)
        {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                "verified App.run receiver mode disagrees with its typed pattern",
            );
            return None;
        }
        match self.bind_compiler_method_pattern(
            receiver_pattern,
            receiver_ty,
            &mut substitution,
            true,
        ) {
            Some(true) => {}
            Some(false) => {
                self.source_error(span, "TYPE002", "App.run requires a mutable App receiver");
                complete = false;
            }
            None => complete = false,
        }
        if arguments.len() != method.selectors.len() + method.parameters.len() {
            self.source_error(
                span,
                "TYPE002",
                format!(
                    "App.run expects {} selector/runtime arguments, found {}",
                    method.selectors.len() + method.parameters.len(),
                    arguments.len()
                ),
            );
            complete = false;
        }

        let mut selected_items = Vec::new();
        for (index, selector) in method.selectors.iter().enumerate() {
            let Some(argument) = arguments.get(index) else {
                continue;
            };
            if selector.coordinate().index() != u8::try_from(index).unwrap_or(u8::MAX) {
                self.gap(
                    argument.span,
                    BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                    "verified App.run selector coordinates are not dense and ordered",
                );
                complete = false;
                continue;
            }
            if selector.kind() != CompilerMethodSelectorKind::DefinitionId {
                self.gap(
                    argument.span,
                    BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                    "App.run exposes an unsupported selector kind",
                );
                complete = false;
                continue;
            }
            let Some(item) = self.definition_selector_item(argument) else {
                complete = false;
                continue;
            };
            let Some(entry) = self.catalog.definition(item) else {
                self.gap(
                    argument.span,
                    BodyCheckIncompletenessKind::MissingRetainedJoin,
                    "App.run schedule selector has no declaration catalog row",
                );
                complete = false;
                continue;
            };
            if checked_entry_shape(entry, self.scope.body.id, argument.span, &mut self.gaps)
                .is_none()
            {
                complete = false;
                continue;
            }
            if entry.definition.key.kind != DeclarationKind::Schedule {
                self.source_error(
                    argument.span,
                    "TYPE002",
                    "App.run DefinitionId selector must name a schedule declaration",
                );
                complete = false;
                continue;
            }
            selected_items.push(item);
        }

        for (index, pattern) in method.parameters.iter().enumerate() {
            let Some(argument) = arguments.get(method.selectors.len() + index) else {
                continue;
            };
            let Some(checked) = self.check_expression(argument, None) else {
                complete = false;
                continue;
            };
            match self.bind_compiler_method_pattern(pattern, checked.ty(), &mut substitution, true)
            {
                Some(true) => {}
                Some(false) => {
                    self.source_error(
                        argument.span,
                        "TYPE002",
                        "App.run runtime argument does not match its verified intrinsic type pattern",
                    );
                    complete = false;
                }
                None => complete = false,
            }
        }

        if !complete
            || !self.validate_compiler_method_generics(
                method,
                &substitution,
                &BTreeSet::new(),
                span,
            )
            || !self.validate_compiler_method_effects(method, &substitution, span)
        {
            return None;
        }
        let result = self.compiler_method_pattern_type(&method.result, &substitution, span)?;
        if method.has_effects() {
            let mut bytes = b"ARCHE-C2-APP-RUN-EFFECTS\0".to_vec();
            bytes.extend_from_slice(&method.method.ordinal().to_le_bytes());
            for item in selected_items {
                bytes.extend_from_slice(&item.0.to_le_bytes());
            }
            self.pending_c4(
                span,
                bytes,
                "App.run schedule requires/throws membership is finalized by C4",
            );
        }
        self.calls.push(CheckedBodyCall {
            span,
            callee: CheckedBodyCallee::EmbeddedMethod(method.method),
            result: result.clone(),
        });
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(result)))
    }

    fn definition_selector_item(&mut self, expression: &AstExpression) -> Option<HirItemId> {
        let AstExpressionKind::Path(path) = &expression.kind else {
            self.source_error(
                expression.span,
                "TYPE002",
                "DefinitionId selector must be a declaration path",
            );
            self.walk_expression(expression);
            return None;
        };
        let resolution = self.path_resolution(path.span)?.clone();
        let [Res::Item(HirItemRes::Definition(item))] = resolution.resolutions.as_slice() else {
            self.source_error(
                expression.span,
                "TYPE002",
                "DefinitionId selector path does not name one declaration",
            );
            return None;
        };
        Some(*item)
    }

    fn bind_compiler_method_actuals(
        &mut self,
        method: &CompilerNominalMethodSpec,
        actuals: &[GenericArgumentShape],
        substitution: &mut CompilerMethodSubstitution,
        span: Span,
    ) -> bool {
        if actuals.len() != method.generics.len() {
            self.source_error(
                span,
                "TYPE001",
                format!(
                    "compiler nominal method expects {} generic arguments, found {}",
                    method.generics.len(),
                    actuals.len()
                ),
            );
            return false;
        }
        let mut complete = true;
        for (formal, actual) in method.generics.iter().zip(actuals) {
            let matches = match (formal.kind(), actual) {
                (CompilerMethodGenericParameterKind::Type, GenericArgumentShape::Type(ty)) => {
                    bind_compiler_type_generic(substitution, formal.coordinate(), ty)
                }
                (
                    CompilerMethodGenericParameterKind::Lifetime,
                    GenericArgumentShape::Lifetime(lifetime),
                ) => bind_compiler_lifetime_generic(substitution, formal.coordinate(), lifetime),
                _ => false,
            };
            if !matches {
                self.source_error(
                    span,
                    "TYPE001",
                    "compiler nominal generic argument kind or repeated value does not match verified authority",
                );
                complete = false;
            }
        }
        complete
    }

    fn bind_compiler_method_pattern(
        &mut self,
        pattern: &CompilerMethodTypePattern,
        actual: &SymbolicType,
        substitution: &mut CompilerMethodSubstitution,
        allow_implicit_borrow: bool,
    ) -> Option<bool> {
        let matches = match pattern {
            CompilerMethodTypePattern::Generic(generic) => {
                bind_compiler_type_generic(substitution, *generic, actual)
            }
            CompilerMethodTypePattern::Definition {
                definition,
                arguments,
            } => {
                if let Some(primitive) = self.compiler_primitive_type(*definition) {
                    arguments.is_empty() && primitive == *actual
                } else {
                    let SymbolicType::NominalPath {
                        declaration,
                        arguments: actual_arguments,
                    } = actual
                    else {
                        return Some(false);
                    };
                    if self.embedded_core_definition_for_path(declaration) != Some(*definition) {
                        return Some(false);
                    }
                    if self.is_exact_caps_pack_pattern(*definition, arguments, actual_arguments) {
                        let [CompilerMethodGenericArgumentPattern::Type(
                            CompilerMethodTypePattern::Generic(generic),
                        )] = arguments.as_ref()
                        else {
                            unreachable!("exact Caps pack shape was checked")
                        };
                        let pack = actual_arguments
                            .iter()
                            .filter_map(|argument| match argument {
                                GenericArgumentShape::Type(ty) => Some(ty.clone()),
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        match substitution.capability_packs.get(generic) {
                            Some(existing) => existing == &pack,
                            None => {
                                substitution.capability_packs.insert(*generic, pack);
                                true
                            }
                        }
                    } else if arguments.len() != actual_arguments.len() {
                        false
                    } else {
                        let mut complete = true;
                        for (pattern, actual) in arguments.iter().zip(actual_arguments) {
                            let matched = match (pattern, actual) {
                                (
                                    CompilerMethodGenericArgumentPattern::Type(pattern),
                                    GenericArgumentShape::Type(actual),
                                ) => self.bind_compiler_method_pattern(
                                    pattern,
                                    actual,
                                    substitution,
                                    false,
                                )?,
                                (
                                    CompilerMethodGenericArgumentPattern::Lifetime(generic),
                                    GenericArgumentShape::Lifetime(actual),
                                ) => bind_compiler_lifetime_generic(substitution, *generic, actual),
                                _ => false,
                            };
                            complete &= matched;
                        }
                        complete
                    }
                }
            }
            CompilerMethodTypePattern::SharedReference { lifetime, referent } => match actual {
                SymbolicType::Reference {
                    mutability,
                    lifetime: actual_lifetime,
                    pointee,
                } => {
                    (*mutability == Mutability::Shared || *mutability == Mutability::Mutable)
                        && bind_compiler_method_lifetime(substitution, lifetime, actual_lifetime)
                        && self.bind_compiler_method_pattern(
                            referent,
                            pointee,
                            substitution,
                            false,
                        )?
                }
                _ if allow_implicit_borrow => {
                    self.bind_compiler_method_pattern(referent, actual, substitution, false)?
                }
                _ => false,
            },
            CompilerMethodTypePattern::MutableReference { lifetime, referent } => match actual {
                SymbolicType::Reference {
                    mutability: Mutability::Mutable,
                    lifetime: actual_lifetime,
                    pointee,
                } => {
                    bind_compiler_method_lifetime(substitution, lifetime, actual_lifetime)
                        && self.bind_compiler_method_pattern(
                            referent,
                            pointee,
                            substitution,
                            false,
                        )?
                }
                _ if allow_implicit_borrow => {
                    self.bind_compiler_method_pattern(referent, actual, substitution, false)?
                }
                _ => false,
            },
            CompilerMethodTypePattern::Slice(element) => match actual {
                SymbolicType::Slice(actual) => {
                    self.bind_compiler_method_pattern(element, actual, substitution, false)?
                }
                _ => false,
            },
            CompilerMethodTypePattern::Tuple(fields) => match actual {
                SymbolicType::Tuple(actual) if fields.len() == actual.len() => {
                    let mut complete = true;
                    for (pattern, actual) in fields.iter().zip(actual) {
                        complete &= self.bind_compiler_method_pattern(
                            pattern,
                            actual,
                            substitution,
                            false,
                        )?;
                    }
                    complete
                }
                _ => false,
            },
        };
        Some(matches)
    }

    fn compiler_method_pattern_type_if_bound(
        &self,
        pattern: &CompilerMethodTypePattern,
        substitution: &CompilerMethodSubstitution,
    ) -> Option<SymbolicType> {
        match pattern {
            CompilerMethodTypePattern::Generic(generic) => substitution.types.get(generic).cloned(),
            CompilerMethodTypePattern::Definition {
                definition,
                arguments,
            } => {
                if let Some(primitive) = self.compiler_primitive_type(*definition) {
                    return arguments.is_empty().then_some(primitive);
                }
                let declaration = self.embedded_declaration_path(*definition)?;
                let mut lowered = Vec::with_capacity(arguments.len());
                for argument in arguments.iter() {
                    lowered.push(match argument {
                        CompilerMethodGenericArgumentPattern::Type(pattern) => {
                            GenericArgumentShape::Type(
                                self.compiler_method_pattern_type_if_bound(pattern, substitution)?,
                            )
                        }
                        CompilerMethodGenericArgumentPattern::Lifetime(generic) => {
                            GenericArgumentShape::Lifetime(
                                substitution.lifetimes.get(generic)?.clone(),
                            )
                        }
                    });
                }
                Some(SymbolicType::NominalPath {
                    declaration,
                    arguments: lowered,
                })
            }
            CompilerMethodTypePattern::SharedReference { lifetime, referent } => {
                Some(SymbolicType::Reference {
                    mutability: Mutability::Shared,
                    lifetime: compiler_method_lifetime_type(substitution, lifetime)?,
                    pointee: Box::new(
                        self.compiler_method_pattern_type_if_bound(referent, substitution)?,
                    ),
                })
            }
            CompilerMethodTypePattern::MutableReference { lifetime, referent } => {
                Some(SymbolicType::Reference {
                    mutability: Mutability::Mutable,
                    lifetime: compiler_method_lifetime_type(substitution, lifetime)?,
                    pointee: Box::new(
                        self.compiler_method_pattern_type_if_bound(referent, substitution)?,
                    ),
                })
            }
            CompilerMethodTypePattern::Slice(element) => Some(SymbolicType::Slice(Box::new(
                self.compiler_method_pattern_type_if_bound(element, substitution)?,
            ))),
            CompilerMethodTypePattern::Tuple(fields) => Some(SymbolicType::Tuple(
                fields
                    .iter()
                    .map(|field| self.compiler_method_pattern_type_if_bound(field, substitution))
                    .collect::<Option<Vec<_>>>()?,
            )),
        }
    }

    fn compiler_method_pattern_type(
        &mut self,
        pattern: &CompilerMethodTypePattern,
        substitution: &CompilerMethodSubstitution,
        span: Span,
    ) -> Option<SymbolicType> {
        match self.compiler_method_pattern_type_if_bound(pattern, substitution) {
            Some(ty) => Some(ty),
            None => {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingGenericInference,
                    "verified compiler nominal method pattern retains an uninferred generic coordinate",
                );
                None
            }
        }
    }

    fn validate_compiler_method_generics(
        &mut self,
        method: &CompilerNominalMethodSpec,
        substitution: &CompilerMethodSubstitution,
        receiver_bound: &BTreeSet<CompilerMethodGenericParameter>,
        span: Span,
    ) -> bool {
        for generic in &method.generics {
            let bound = match generic.kind() {
                CompilerMethodGenericParameterKind::Type => {
                    substitution.types.contains_key(&generic.coordinate())
                        || substitution
                            .capability_packs
                            .contains_key(&generic.coordinate())
                }
                CompilerMethodGenericParameterKind::Lifetime => {
                    substitution.lifetimes.contains_key(&generic.coordinate())
                }
            };
            if !bound {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingGenericInference,
                    format!(
                        "verified compiler nominal generic `{}` was not inferred",
                        generic.source_name()
                    ),
                );
                return false;
            }
            if !generic.bounds().is_empty()
                && !receiver_bound.contains(&generic.coordinate())
                && !substitution
                    .capability_packs
                    .contains_key(&generic.coordinate())
            {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingEmbeddedTraitIdentity,
                    format!(
                        "compiler nominal generic `{}` requires {:?}, whose stable compiler trait identities are absent",
                        generic.source_name(),
                        generic.bounds()
                    ),
                );
                return false;
            }
        }
        true
    }

    fn validate_compiler_method_effects(
        &mut self,
        method: &CompilerNominalMethodSpec,
        substitution: &CompilerMethodSubstitution,
        span: Span,
    ) -> bool {
        for effect in method.requires.iter().chain(&method.throws) {
            match effect {
                CompilerNominalMethodEffectPattern::Drop(pattern) => {
                    if self
                        .compiler_method_pattern_type_if_bound(pattern, substitution)
                        .is_none()
                    {
                        self.gap(
                            span,
                            BodyCheckIncompletenessKind::MissingGenericInference,
                            "compiler nominal Drop effect references an uninferred type pattern",
                        );
                        return false;
                    }
                }
                CompilerNominalMethodEffectPattern::Selector(coordinate) => {
                    let Some(selector) = method.selectors.get(usize::from(coordinate.index()))
                    else {
                        self.gap(
                            span,
                            BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                            "compiler nominal effect references an absent selector coordinate",
                        );
                        return false;
                    };
                    if selector.coordinate() != *coordinate {
                        self.gap(
                            span,
                            BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                            "compiler nominal selector effect disagrees with typed selector authority",
                        );
                        return false;
                    }
                }
            }
        }
        true
    }

    fn compiler_primitive_type(&self, definition: VirtualDefinitionId) -> Option<SymbolicType> {
        self.catalog
            .handoff
            .frontend()
            .inventory()
            .embedded_core
            .compiler_primitive_for_c1_definition(definition)
            .map(compiler_primitive_symbolic_type)
    }

    fn embedded_declaration_path(
        &self,
        definition: VirtualDefinitionId,
    ) -> Option<SemanticDeclarationPath> {
        let core = &self.catalog.handoff.frontend().inventory().embedded_core;
        let row = core.definition(definition)?;
        let kind = match row.declaration_kind() {
            VirtualDeclarationKind::Struct => DeclarationKind::Struct,
            VirtualDeclarationKind::Enum => DeclarationKind::Enum,
            VirtualDeclarationKind::Trait => DeclarationKind::Trait,
            VirtualDeclarationKind::Function => DeclarationKind::Function,
            VirtualDeclarationKind::Primitive => return None,
        };
        let projection = core.projection();
        Some(SemanticDeclarationPath {
            registry_origin: projection.registry_origin().to_owned(),
            package_name: projection.scoped_name().to_owned(),
            target: TargetRoot::Library,
            modules: Vec::new(),
            kind,
            name: row.name().to_owned(),
        })
    }

    fn embedded_nominal_shape(
        &self,
        definition: VirtualDefinitionId,
    ) -> Option<&SymbolicDeclarationShapeSkeleton> {
        self.catalog
            .handoff
            .frontend()
            .inventory()
            .embedded_core
            .typed_c2()
            .nominal_declaration_shape(definition)
    }

    fn embedded_record_form(
        &self,
        definition: VirtualDefinitionId,
    ) -> Option<(SymbolicRecordForm, Vec<SymbolicFieldShapeSkeleton>)> {
        match &self.embedded_nominal_shape(definition)?.payload {
            SymbolicDeclarationPayloadSkeleton::Record(record) => {
                Some((record.form, record.fields.clone()))
            }
            _ => None,
        }
    }

    fn embedded_variant_form(
        &self,
        variant: VirtualEnumVariantId,
    ) -> Option<(
        VirtualDefinitionId,
        SymbolicRecordForm,
        Vec<SymbolicFieldShapeSkeleton>,
    )> {
        let core = &self.catalog.handoff.frontend().inventory().embedded_core;
        let row = core.enum_variant(variant)?;
        let owner = row.owner();
        let ordinal = usize::try_from(row.ordinal()).ok()?;
        match &core.typed_c2().nominal_declaration_shape(owner)?.payload {
            SymbolicDeclarationPayloadSkeleton::Enum(variants) => {
                let variant = variants.get(ordinal)?;
                Some((owner, variant.form, variant.fields.clone()))
            }
            _ => None,
        }
    }

    /// Adopts the contextual type for an embedded construction only when it
    /// names the same embedded nominal with the exact generic arity; any other
    /// expected type never instantiates a foreign constructor.
    fn adopt_expected_embedded_nominal(
        &self,
        definition: VirtualDefinitionId,
        expected: Option<&SymbolicType>,
    ) -> Option<SymbolicType> {
        let SymbolicType::NominalPath {
            declaration,
            arguments,
        } = expected?
        else {
            return None;
        };
        let owner_path = self.embedded_declaration_path(definition)?;
        let formals = self
            .embedded_nominal_shape(definition)?
            .generic_parameters
            .len();
        if declaration == &owner_path && arguments.len() == formals {
            Some(expected?.clone())
        } else {
            None
        }
    }

    fn zero_generic_embedded_nominal_type(
        &self,
        definition: VirtualDefinitionId,
    ) -> Option<SymbolicType> {
        let shape = self.embedded_nominal_shape(definition)?;
        if !shape.generic_parameters.is_empty() {
            return None;
        }
        let declaration = self.embedded_declaration_path(definition)?;
        Some(SymbolicType::NominalPath {
            declaration,
            arguments: Vec::new(),
        })
    }

    fn embedded_core_definition_for_path(
        &self,
        declaration: &SemanticDeclarationPath,
    ) -> Option<VirtualDefinitionId> {
        let core = &self.catalog.handoff.frontend().inventory().embedded_core;
        let projection = core.projection();
        if declaration.registry_origin != projection.registry_origin()
            || declaration.package_name != projection.scoped_name()
            || declaration.target != TargetRoot::Library
            || !declaration.modules.is_empty()
        {
            return None;
        }
        projection
            .definitions()
            .iter()
            .find(|row| row.namespace() == VirtualNamespace::Type && row.name() == declaration.name)
            .map(|row| row.id())
    }

    fn is_exact_caps_pack_pattern(
        &self,
        definition: VirtualDefinitionId,
        pattern_arguments: &[CompilerMethodGenericArgumentPattern],
        actual_arguments: &[GenericArgumentShape],
    ) -> bool {
        let core = &self.catalog.handoff.frontend().inventory().embedded_core;
        let caps = core
            .typed_c2()
            .nominals()
            .iter()
            .find(|authority| authority.kind() == CompilerNominalKind::Caps)
            .map(|authority| authority.c1_definition());
        if caps != Some(definition)
            || !matches!(
                pattern_arguments,
                [CompilerMethodGenericArgumentPattern::Type(
                    CompilerMethodTypePattern::Generic(_)
                )]
            )
        {
            return false;
        }
        actual_arguments.iter().all(|argument| {
            let GenericArgumentShape::Type(SymbolicType::NominalPath { declaration, .. }) =
                argument
            else {
                return false;
            };
            let Some(definition) = self.embedded_core_definition_for_path(declaration) else {
                return false;
            };
            core.projection()
                .types()
                .iter()
                .find(|row| row.definition() == definition)
                .is_some_and(|row| row.flavor() == VirtualTypeFlavor::Capability)
        })
    }

    fn postfix_actuals(&mut self, span: Span) -> Option<Vec<GenericArgumentShape>> {
        let rows = self
            .scope
            .item
            .postfix_generic_argument_uses
            .iter()
            .filter(|candidate| candidate.span == span)
            .collect::<Vec<_>>();
        if rows.len() != 1 {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                format!(
                    "postfix generic span joins to {} retained generic-argument rows",
                    rows.len()
                ),
            );
            return None;
        }
        let arguments = rows[0].arguments.clone();
        self.resolved_generic_actuals(&arguments, span)
    }

    /// Detects `include_str(...)`, `include_bytes(...)`, and `panic(...)` as a
    /// direct postfix call on a prelude-function path and types the call
    /// without ever materializing the function as a value. Every other use of
    /// those names falls through to the honest prelude gap.
    fn embedded_prelude_call_head<'p>(
        &mut self,
        base: &AstExpression,
        parts: &'p [arche_frontend::ast::AstPostfix],
    ) -> Option<(Option<LoweredValue>, &'p [arche_frontend::ast::AstPostfix])> {
        use arche_frontend::embedded_core::VirtualFunctionLowering;
        use arche_frontend::include_inputs::IncludeInputKind;
        let AstExpressionKind::Path(path) = &base.kind else {
            return None;
        };
        if path.generic_arguments.is_some() {
            return None;
        }
        let first = parts.first()?;
        let AstPostfixKind::Call(arguments) = &first.kind else {
            return None;
        };
        let resolution = self
            .scope
            .target
            .path_resolutions
            .iter()
            .find(|resolution| resolution.span == path.span)?;
        if resolution.unresolved.is_some() {
            return None;
        }
        let [Res::Builtin(arche_frontend::BuiltinRes {
            target: BuiltinResTarget::Prelude(VirtualPreludeTarget::Definition(definition)),
        })] = resolution.resolutions.as_slice()
        else {
            return None;
        };
        let definition = *definition;
        let lowering = {
            let core = &self.catalog.handoff.frontend().inventory().embedded_core;
            core.projection()
                .functions()
                .iter()
                .find(|row| row.definition() == definition)
                .map(|row| row.lowering())
        }?;
        let value = match lowering {
            VirtualFunctionLowering::Intrinsic { id: 70, .. } => self.lower_embedded_include_call(
                definition,
                IncludeInputKind::Bytes,
                arguments,
                first.span,
            ),
            VirtualFunctionLowering::Intrinsic { id: 71, .. } => self.lower_embedded_include_call(
                definition,
                IncludeInputKind::Str,
                arguments,
                first.span,
            ),
            VirtualFunctionLowering::CompilerOwnedBody => {
                self.lower_embedded_panic_call(definition, arguments, first.span)
            }
            VirtualFunctionLowering::Intrinsic { .. } => return None,
        };
        Some((value, &parts[1..]))
    }

    /// Lowers every argument once, bottom-up, returning spans and checked
    /// types for inference and acceptance checking.
    fn lower_arguments_bottom_up(
        &mut self,
        arguments: &[AstExpression],
    ) -> Option<Vec<(Span, SymbolicType)>> {
        let mut checked = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let value = self.lower_expression(argument, None)?;
            let expression = self.materialize(&value, None, argument.span)?;
            checked.push((argument.span, expression.ty().clone()));
        }
        Some(checked)
    }

    /// Emits exact mismatches between substituted parameter types and the
    /// already-checked argument types without re-lowering any argument.
    fn check_prepared_arguments(
        &mut self,
        parameters: &[SymbolicType],
        checked: &[(Span, SymbolicType)],
        span: Span,
    ) {
        if parameters.len() != checked.len() {
            self.source_error(
                span,
                "TYPE002",
                format!(
                    "call expects {} arguments, found {}",
                    parameters.len(),
                    checked.len()
                ),
            );
        }
        let outlives = LifetimeOutlives::new([]);
        for (parameter, (argument_span, actual)) in parameters.iter().zip(checked) {
            let accepted = parameter == actual
                || types_match_with_erased_body_lifetime(parameter, actual)
                || classify_coercion(actual, parameter, &outlives).is_some();
            if !accepted {
                self.source_error(
                    *argument_span,
                    "TYPE002",
                    format!(
                        "expected {}, found {}",
                        crate::golden::spell_symbolic_type(parameter),
                        crate::golden::spell_symbolic_type(actual)
                    ),
                );
            }
        }
    }

    /// Infers a declaration's generic actuals from declared parameter types
    /// and checked argument types. Type slots bind by structural first-order
    /// unification, lifetime slots erase to the body-local marker, and an
    /// unbound type or const slot fails closed with `None`.
    fn infer_generic_actuals(
        formals: &[GenericParameterKind],
        declared: &[SymbolicType],
        checked: &[(Span, SymbolicType)],
    ) -> Option<Vec<GenericArgumentShape>> {
        let mut slots: Vec<Option<SymbolicType>> = vec![None; formals.len()];
        for (declared, (_, actual)) in declared.iter().zip(checked) {
            bind_inference_slots(declared, actual, &mut slots);
        }
        formals
            .iter()
            .enumerate()
            .map(|(index, formal)| match formal {
                GenericParameterKind::Type => slots[index].take().map(GenericArgumentShape::Type),
                GenericParameterKind::Lifetime => Some(GenericArgumentShape::Lifetime(
                    SymbolicLifetime::ErasedLocal,
                )),
                GenericParameterKind::IntegerConst(_) => None,
            })
            .collect()
    }

    /// Argument-driven inference for a generic tuple/call-form constructor.
    fn infer_tuple_constructor_call(
        &mut self,
        item: HirItemId,
        variant: Option<u64>,
        arguments: &[AstExpression],
        span: Span,
    ) -> Option<LoweredValue> {
        let (formals, form, fields) = self.constructor_shape(item, variant, span)?;
        if form == SymbolicRecordForm::Record {
            self.source_error(span, "TYPE002", "record constructor requires named fields");
            return None;
        }
        let mut declared = Vec::new();
        for field in &fields {
            declared.push(self.require_type_shape(&field.ty, span)?);
        }
        let checked = self.lower_arguments_bottom_up(arguments)?;
        let Some(actuals) = Self::infer_generic_actuals(&formals, &declared, &checked) else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingGenericInference,
                "constructor arguments do not determine every generic actual",
            );
            return None;
        };
        let substituted: Vec<SymbolicType> = declared
            .iter()
            .map(|ty| substitute_type(ty, &actuals))
            .collect();
        self.check_prepared_arguments(&substituted, &checked, span);
        let entry = self.catalog.definitions.get(&item)?;
        let constructed = nominal_type(entry, actuals);
        self.calls.push(CheckedBodyCall {
            span,
            callee: CheckedBodyCallee::DirectItem(item),
            result: constructed.clone(),
        });
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(
            constructed,
        )))
    }

    /// Argument-driven inference for a generic record literal.
    fn infer_record_literal(
        &mut self,
        item: HirItemId,
        variant: Option<u64>,
        fields: &[arche_frontend::ast::AstRecordExpressionField],
        span: Span,
    ) -> Option<LoweredValue> {
        let (formals, form, declared_fields) = self.constructor_shape(item, variant, span)?;
        if form != SymbolicRecordForm::Record {
            self.source_error(
                span,
                "TYPE002",
                "record literal path does not select a record-form constructor",
            );
            return None;
        }
        let mut declared_by_name = BTreeMap::new();
        for field in &declared_fields {
            let Some(name) = field.name.as_deref() else {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingRetainedJoin,
                    "record declaration payload contains an unnamed field",
                );
                return None;
            };
            declared_by_name.insert(name.to_owned(), field.ty.clone());
        }
        let mut declared = Vec::new();
        let mut checked = Vec::new();
        let mut seen = BTreeSet::new();
        for field in fields {
            let name = field.name.as_str();
            if !seen.insert(name.to_owned()) {
                self.source_error(
                    field.span,
                    "TYPE002",
                    format!("duplicate record field `{name}`"),
                );
                continue;
            }
            let Some(shape) = declared_by_name.get(name) else {
                self.source_error(
                    field.span,
                    "TYPE002",
                    format!("unknown record field `{name}`"),
                );
                self.check_expression(&field.value, None);
                continue;
            };
            let declared_ty = self.require_type_shape(shape, field.span)?;
            let value = self.lower_expression(&field.value, None)?;
            let expression = self.materialize(&value, None, field.span)?;
            declared.push(declared_ty);
            checked.push((field.span, expression.ty().clone()));
        }
        for name in declared_by_name.keys() {
            if !seen.contains(name) {
                self.source_error(span, "TYPE002", format!("missing record field `{name}`"));
            }
        }
        let Some(actuals) = Self::infer_generic_actuals(&formals, &declared, &checked) else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingGenericInference,
                "record fields do not determine every generic actual",
            );
            return None;
        };
        let substituted: Vec<SymbolicType> = declared
            .iter()
            .map(|ty| substitute_type(ty, &actuals))
            .collect();
        self.check_prepared_arguments(&substituted, &checked, span);
        let entry = self.catalog.definitions.get(&item)?;
        let constructed = nominal_type(entry, actuals);
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(
            constructed,
        )))
    }

    /// Returns a constructor's generic formals plus the selected variant or
    /// record form and fields.
    fn constructor_shape(
        &mut self,
        item: HirItemId,
        variant: Option<u64>,
        span: Span,
    ) -> Option<(
        Vec<GenericParameterKind>,
        SymbolicRecordForm,
        Vec<SymbolicFieldShapeSkeleton>,
    )> {
        let Some(entry) = self.catalog.definitions.get(&item) else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "constructor item has no declaration catalog row",
            );
            return None;
        };
        let declaration_shape =
            checked_entry_shape(entry, self.scope.body.id, span, &mut self.gaps)?;
        let formals = declaration_shape.generic_parameters.clone();
        match (&declaration_shape.payload, variant) {
            (SymbolicDeclarationPayloadSkeleton::Record(record), None) => {
                Some((formals, record.form, record.fields.clone()))
            }
            (SymbolicDeclarationPayloadSkeleton::Enum(variants), Some(ordinal)) => {
                let variant = usize::try_from(ordinal)
                    .ok()
                    .and_then(|ordinal| variants.get(ordinal))?;
                Some((formals, variant.form, variant.fields.clone()))
            }
            (SymbolicDeclarationPayloadSkeleton::Tag, None) => {
                Some((formals, SymbolicRecordForm::Unit, Vec::new()))
            }
            _ => {
                self.source_error(span, "TYPE002", "value is not a constructor");
                None
            }
        }
    }

    /// Argument-driven inference for a generic named-function call.
    fn infer_function_call(
        &mut self,
        item: HirItemId,
        associated: bool,
        arguments: &[AstExpression],
        span: Span,
    ) -> Option<LoweredValue> {
        let Some(entry) = self.catalog.definitions.get(&item) else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "call target has no declaration catalog row",
            );
            return None;
        };
        let Some(declaration_shape) = entry.declaration_shape else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "call target has no structurally closed checked declaration row",
            );
            return None;
        };
        let SymbolicDeclarationPayloadSkeleton::Callable(callable) = &declaration_shape.payload
        else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "call target has no callable payload",
            );
            return None;
        };
        let formals = declaration_shape.generic_parameters.clone();
        let mut declared = Vec::new();
        for parameter in &callable.parameters {
            declared.push(self.require_type_shape(&parameter.ty, span)?);
        }
        let checked = self.lower_arguments_bottom_up(arguments)?;
        let Some(actuals) = Self::infer_generic_actuals(&formals, &declared, &checked) else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingGenericInference,
                format!(
                    "generic callable {item:?} has no explicit actuals and its arguments do not determine every generic actual"
                ),
            );
            return None;
        };
        let signature = self.callable_signature(declaration_shape, &actuals, span)?;
        self.check_prepared_arguments(&signature.parameters, &checked, span);
        if !signature.requires.members().is_empty() || !signature.throws.members().is_empty() {
            self.pending_c4(
                span,
                b"call-effect-membership".to_vec(),
                "call effect membership is finalized by C4",
            );
        }
        let result = signature.result.clone();
        self.calls.push(CheckedBodyCall {
            span,
            callee: if associated {
                CheckedBodyCallee::AssociatedItem(item)
            } else {
                CheckedBodyCallee::DirectItem(item)
            },
            result: result.clone(),
        });
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(result)))
    }

    /// Attempts inherent and trait-impl method selection for a receiver whose
    /// peeled type is a user nominal path, over the current target's impl
    /// declarations.
    ///
    /// Returns `None` when not applicable or when a potentially viable
    /// candidate is still pending authority, keeping the caller's fail-closed
    /// gap. Impls with declared predicates need entailment authority and are
    /// treated as pending for now.
    fn lower_nominal_user_method(
        &mut self,
        receiver: &CheckedExpression,
        name: &str,
        generic_arguments: Option<&arche_frontend::ast::AstGenericArguments>,
        arguments: &[AstExpression],
        span: Span,
    ) -> Option<Option<LoweredValue>> {
        let SymbolicType::NominalPath {
            declaration: receiver_declaration,
            arguments: receiver_arguments,
        } = peel_references(receiver.ty())
        else {
            return None;
        };
        if self
            .embedded_core_definition_for_path(receiver_declaration)
            .is_some()
        {
            return None;
        }
        let explicit_actuals = match generic_arguments {
            Some(generic_arguments) => Some(self.postfix_actuals(generic_arguments.span)?),
            None => None,
        };
        let mut pending_candidates = false;
        let mut selected: Vec<(
            HirItemId,
            SymbolicDeclarationShapeSkeleton,
            Vec<GenericArgumentShape>,
        )> = Vec::new();
        for item in &self.scope.target.items {
            if item.kind != DeclarationKind::Impl {
                continue;
            }
            let Some(entry) = self.catalog.definitions.get(&item.id) else {
                pending_candidates = true;
                continue;
            };
            let Some(shape) = entry.declaration_shape else {
                pending_candidates = true;
                continue;
            };
            let SymbolicDeclarationPayloadSkeleton::Impl {
                target, methods, ..
            } = &shape.payload
            else {
                continue;
            };
            let target_ty = match target {
                SymbolicTypeShapeSkeleton::Resolved { value, .. } => value,
                SymbolicTypeShapeSkeleton::Pending(_) => {
                    pending_candidates = true;
                    continue;
                }
            };
            let SymbolicType::NominalPath {
                declaration: target_declaration,
                arguments: target_arguments,
            } = target_ty
            else {
                continue;
            };
            if target_declaration != receiver_declaration
                || target_arguments.len() != receiver_arguments.len()
            {
                continue;
            }
            let Some(method) = methods.iter().find(|method| method.name == name) else {
                continue;
            };
            if !shape.predicates.is_empty() {
                // Predicate entailment for impl selection needs the solver;
                // fail closed rather than guessing viability.
                pending_candidates = true;
                continue;
            }
            // First-order head match: an impl-frame bound type binds the
            // receiver's argument; a concrete argument must match exactly.
            let mut impl_actuals: Vec<Option<GenericArgumentShape>> =
                vec![None; shape.generic_parameters.len()];
            let mut viable = true;
            for (target_argument, receiver_argument) in
                target_arguments.iter().zip(receiver_arguments)
            {
                match target_argument {
                    GenericArgumentShape::Type(SymbolicType::BoundType { depth: 0, index }) => {
                        let Ok(slot) = usize::try_from(*index) else {
                            viable = false;
                            break;
                        };
                        match impl_actuals.get_mut(slot) {
                            Some(entry @ None) => *entry = Some(receiver_argument.clone()),
                            Some(Some(previous)) if previous == receiver_argument => {}
                            _ => {
                                viable = false;
                                break;
                            }
                        }
                    }
                    other => {
                        if other != receiver_argument {
                            viable = false;
                            break;
                        }
                    }
                }
            }
            if !viable {
                continue;
            }
            let impl_arguments = match impl_actuals.into_iter().collect::<Option<Vec<_>>>() {
                Some(arguments) => arguments,
                None => {
                    // An impl generic not determined by the head cannot be
                    // inferred here; keep the fail-closed gap.
                    pending_candidates = true;
                    continue;
                }
            };
            selected.push((item.id, (*method.shape).clone(), impl_arguments));
        }
        if pending_candidates {
            // A pending candidate could change zero/unique/ambiguous
            // viability; the contract selects nothing until it resolves.
            return None;
        }
        match selected.len() {
            0 => None,
            1 => {
                let (impl_item, method_shape, impl_arguments) =
                    selected.pop().expect("one selection row");
                Some(self.type_nominal_user_method_call(
                    receiver,
                    impl_item,
                    method_shape,
                    impl_arguments,
                    explicit_actuals,
                    name,
                    arguments,
                    span,
                ))
            }
            _ => {
                for argument in arguments {
                    self.check_expression(argument, None);
                }
                self.source_error(
                    span,
                    "TRAIT002",
                    format!("method `{name}` has multiple viable impl candidates"),
                );
                Some(None)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn type_nominal_user_method_call(
        &mut self,
        receiver: &CheckedExpression,
        impl_item: HirItemId,
        method_shape: SymbolicDeclarationShapeSkeleton,
        impl_arguments: Vec<GenericArgumentShape>,
        explicit_actuals: Option<Vec<GenericArgumentShape>>,
        name: &str,
        arguments: &[AstExpression],
        span: Span,
    ) -> Option<LoweredValue> {
        let entry = self.catalog.definitions.get(&impl_item)?;
        let impl_formals = entry
            .declaration_shape
            .map(|shape| shape.generic_parameters.clone())
            .unwrap_or_default();
        let impl_frame = match TraitFrameSubstitution::new(
            impl_formals,
            impl_arguments,
            peel_references(receiver.ty()).clone(),
        ) {
            Ok(frame) => frame,
            Err(error) => {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingMethodSelection,
                    format!("impl head is not a usable frame: {error:?}"),
                );
                return None;
            }
        };
        let SymbolicDeclarationPayloadSkeleton::Callable(callable) = &method_shape.payload else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingMethodSelection,
                "impl method entry is not a callable shape",
            );
            return None;
        };
        let explicit_actuals = explicit_actuals.unwrap_or_default();
        let resolve = |shape: &SymbolicTypeShapeSkeleton| -> Option<SymbolicType> {
            let ty = match shape {
                SymbolicTypeShapeSkeleton::Resolved { value, .. } => value.clone(),
                SymbolicTypeShapeSkeleton::Pending(_) => return None,
            };
            let ty = impl_frame.substitute_type(&ty, 1).ok()?;
            instantiate_method_frame(&ty, &explicit_actuals)
        };
        let Some((receiver_parameter, value_parameters)) = callable.parameters.split_first() else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingMethodSelection,
                "receiverless impl method reached postfix selection",
            );
            return None;
        };
        let Some(expected_receiver) = resolve(&receiver_parameter.ty) else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingMethodSelection,
                "impl method receiver type is pending",
            );
            return None;
        };
        let receiver_ok = match receiver_parameter.mode {
            SymbolicCallableParameterMode::ReceiverShared
            | SymbolicCallableParameterMode::ReceiverMutable => {
                let SymbolicType::Reference { pointee, .. } = &expected_receiver else {
                    for argument in arguments {
                        self.check_expression(argument, None);
                    }
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingMethodSelection,
                        "borrowed-receiver impl method resolved to a non-reference receiver type",
                    );
                    return None;
                };
                let actual = receiver.ty();
                let borrowable = actual == &**pointee;
                let reborrowable = match actual {
                    SymbolicType::Reference {
                        mutability,
                        pointee: actual_pointee,
                        ..
                    } => {
                        actual_pointee == pointee
                            && (receiver_parameter.mode
                                == SymbolicCallableParameterMode::ReceiverShared
                                || *mutability == Mutability::Mutable)
                    }
                    _ => false,
                };
                borrowable || reborrowable
            }
            SymbolicCallableParameterMode::ReceiverValue => receiver.ty() == &expected_receiver,
            SymbolicCallableParameterMode::Value => {
                for argument in arguments {
                    self.check_expression(argument, None);
                }
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingMethodSelection,
                    "receiverless method reached postfix receiver selection",
                );
                return None;
            }
        };
        if !receiver_ok {
            for argument in arguments {
                self.check_expression(argument, None);
            }
            self.source_error(
                span,
                "TYPE002",
                format!("method `{name}` receiver mode does not accept this receiver type"),
            );
            return None;
        }
        let mut parameter_types = Vec::new();
        for parameter in value_parameters {
            let Some(ty) = resolve(&parameter.ty) else {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingMethodSelection,
                    "impl method parameter type is pending",
                );
                return None;
            };
            parameter_types.push(ty);
        }
        self.check_call_arguments(&parameter_types, arguments, span);
        let Some(result) = resolve(&callable.result) else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingMethodSelection,
                "impl method result type is pending",
            );
            return None;
        };
        let method_item = self
            .scope
            .target
            .items
            .iter()
            .find(|candidate| {
                candidate.owner == Some(impl_item) && candidate.name.as_deref() == Some(name)
            })
            .map(|candidate| candidate.id);
        self.calls.push(CheckedBodyCall {
            span,
            callee: match method_item {
                Some(item) => CheckedBodyCallee::AssociatedItem(item),
                None => CheckedBodyCallee::DirectItem(impl_item),
            },
            result: result.clone(),
        });
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(result)))
    }

    /// Attempts bound-witness trait-method selection for a receiver whose
    /// peeled type is a bound generic parameter.
    ///
    /// Returns `None` when this path is not applicable (non-bound receiver or
    /// no resolvable environment authority), letting the caller fall through
    /// to its honest gap. Returns `Some(result)` when the environment decides:
    /// a unique viable predicate types the call, ambiguity or a violated
    /// receiver mode is a source error, and a potentially relevant pending
    /// predicate keeps the gap fail-closed.
    fn lower_bound_trait_method(
        &mut self,
        receiver: &CheckedExpression,
        name: &str,
        generic_arguments: Option<&arche_frontend::ast::AstGenericArguments>,
        arguments: &[AstExpression],
        span: Span,
    ) -> Option<Option<LoweredValue>> {
        let subject = peel_references(receiver.ty());
        if !matches!(subject, SymbolicType::BoundType { .. }) {
            return None;
        }
        if generic_arguments.is_some() {
            // Explicit method generics on a bound-witness call are not yet
            // represented; keep the existing gap.
            return None;
        }
        let mut pending_predicates = false;
        let mut matches: Vec<(
            SymbolicPredicate,
            SymbolicDeclarationShapeSkeleton,
            Vec<GenericParameterKind>,
            SemanticDeclarationPath,
        )> = Vec::new();
        // Owner-frame predicates are spelled at the owner's depth 0; seen from
        // inside an owned method body they sit one binder frame out.
        let mut predicate_sets: Vec<(&[arche_frontend::SymbolicPredicateShapeSkeleton], u64)> =
            vec![(&self.declaration_shape.predicates, 0)];
        match self.owner_shape {
            SymbolicDefinitionOwnerSkeleton::Trait { shape, .. }
            | SymbolicDefinitionOwnerSkeleton::SystemQuery { shape, .. } => {
                predicate_sets.push((&shape.predicates, 1));
            }
            SymbolicDefinitionOwnerSkeleton::InherentImpl { predicates, .. }
            | SymbolicDefinitionOwnerSkeleton::TraitImpl { predicates, .. } => {
                predicate_sets.push((predicates, 1));
            }
            SymbolicDefinitionOwnerSkeleton::TopLevel => {}
        }
        for (predicates, shift) in predicate_sets {
            for predicate in predicates {
                let value = match predicate {
                    arche_frontend::SymbolicPredicateShapeSkeleton::Resolved { value, .. } => value,
                    arche_frontend::SymbolicPredicateShapeSkeleton::Pending(_) => {
                        pending_predicates = true;
                        continue;
                    }
                };
                let value = if shift == 0 {
                    (**value).clone()
                } else {
                    shift_predicate_binders(value, shift)
                };
                let SymbolicPredicate::Trait {
                    trait_path,
                    self_type,
                    arguments: trait_arguments,
                } = &value
                else {
                    continue;
                };
                if self_type != subject {
                    continue;
                }
                let Some((method_shape, trait_formals)) = self.trait_method_shape(trait_path, name)
                else {
                    continue;
                };
                matches.push((
                    value.clone(),
                    method_shape,
                    trait_formals,
                    trait_path.clone(),
                ));
                let _ = trait_arguments;
            }
        }
        if pending_predicates {
            // A pending predicate could still supply this method and change
            // zero/unique/ambiguous viability; select nothing until it
            // resolves.
            return None;
        }
        match matches.len() {
            0 => None,
            1 => {
                let (predicate, method_shape, trait_formals, trait_path) =
                    matches.pop().expect("one selection row");
                Some(self.type_bound_trait_method_call(
                    receiver,
                    predicate,
                    method_shape,
                    trait_formals,
                    trait_path,
                    name,
                    arguments,
                    span,
                ))
            }
            _ => {
                for argument in arguments {
                    self.check_expression(argument, None);
                }
                self.source_error(
                    span,
                    "TRAIT002",
                    format!("method `{name}` is supplied by multiple environment predicates"),
                );
                Some(None)
            }
        }
    }

    /// Selects the exact ordinary `IntoIterator<Source,Iter>` and
    /// `Iterator<Iter,Item>` impls for an ordinary `for` source, per the
    /// contract's lang-item desugar. Outer `None` means no authoritative
    /// selection (the caller records its gap); `Some(None)` means a source
    /// diagnostic was minted; `Some(Some((iter, item)))` records both
    /// selected trait calls and returns the loop element type.
    fn lower_ordinary_for_items(
        &mut self,
        source: &SymbolicType,
        span: Span,
    ) -> Option<Option<(SymbolicType, SymbolicType)>> {
        use arche_frontend::embedded_core::CompilerTraitKind;
        let (into_definition, iterator_definition) = {
            let core = &self.catalog.handoff.frontend().inventory().embedded_core;
            let typed = core.typed_c2();
            (
                typed
                    .compiler_trait(CompilerTraitKind::IntoIterator)
                    .c1_definition(),
                typed
                    .compiler_trait(CompilerTraitKind::Iterator)
                    .c1_definition(),
            )
        };
        let into_path = self.embedded_declaration_path(into_definition)?;
        let iterator_path = self.embedded_declaration_path(iterator_definition)?;
        let into_arguments = match self.ordinary_for_trait_selection(&into_path, source, span)? {
            Some(arguments) => arguments,
            None => return Some(None),
        };
        let [_, GenericArgumentShape::Type(iter)] = into_arguments.as_slice() else {
            return None;
        };
        let iter = iter.clone();
        let iterator_arguments =
            match self.ordinary_for_trait_selection(&iterator_path, &iter, span)? {
                Some(arguments) => arguments,
                None => return Some(None),
            };
        let [_, GenericArgumentShape::Type(item)] = iterator_arguments.as_slice() else {
            return None;
        };
        let item = item.clone();
        let into_result = self.selected_trait_call_result(
            &into_path,
            "into_iter",
            into_arguments.clone(),
            source.clone(),
        )?;
        let next_result = self.selected_trait_call_result(
            &iterator_path,
            "next",
            iterator_arguments.clone(),
            iter.clone(),
        )?;
        self.calls.push(CheckedBodyCall {
            span,
            callee: CheckedBodyCallee::TraitMethod {
                trait_path: Box::new(into_path),
                method: "into_iter".into(),
            },
            result: into_result,
        });
        self.calls.push(CheckedBodyCall {
            span,
            callee: CheckedBodyCallee::TraitMethod {
                trait_path: Box::new(iterator_path),
                method: "next".into(),
            },
            result: next_result,
        });
        Some(Some((iter, item)))
    }

    /// Substitutes a selected trait method's declared result type through the
    /// trait frame `(explicit arguments, Self)`.
    fn selected_trait_call_result(
        &mut self,
        trait_path: &SemanticDeclarationPath,
        name: &str,
        arguments: Vec<GenericArgumentShape>,
        self_type: SymbolicType,
    ) -> Option<SymbolicType> {
        let (method_shape, trait_formals) = self.trait_method_shape(trait_path, name)?;
        let substitution = TraitFrameSubstitution::new(trait_formals, arguments, self_type).ok()?;
        let SymbolicDeclarationPayloadSkeleton::Callable(callable) = &method_shape.payload else {
            return None;
        };
        let ty = match &callable.result {
            SymbolicTypeShapeSkeleton::Resolved { value, .. } => value.clone(),
            SymbolicTypeShapeSkeleton::Pending(_) => return None,
        };
        let ty = substitution.substitute_type(&ty, 1).ok()?;
        Some(erase_method_frame_lifetimes(ty))
    }

    /// Scans the target's ordinary impls for `trait_path` with `Self` exactly
    /// `self_ty`. Outer `None`: no authoritative candidate (pending shapes,
    /// generic or predicate-bearing impls, and relation-violating heads all
    /// stay fail-closed). `Some(None)`: multiple coherent candidates, minted
    /// as TRAIT002. `Some(Some(arguments))`: the unique impl head's trait
    /// arguments.
    fn ordinary_for_trait_selection(
        &mut self,
        trait_path: &SemanticDeclarationPath,
        self_ty: &SymbolicType,
        span: Span,
    ) -> Option<Option<Vec<GenericArgumentShape>>> {
        let mut selected: Vec<Vec<GenericArgumentShape>> = Vec::new();
        for item in &self.scope.target.items {
            if item.kind != DeclarationKind::Impl {
                continue;
            }
            let Some(entry) = self.catalog.definitions.get(&item.id) else {
                continue;
            };
            let Some(shape) = entry.declaration_shape else {
                continue;
            };
            let SymbolicDeclarationPayloadSkeleton::Impl {
                trait_ref: Some(trait_ref),
                target,
                ..
            } = &shape.payload
            else {
                continue;
            };
            let SymbolicTypeShapeSkeleton::Resolved {
                value: trait_ty, ..
            } = trait_ref
            else {
                continue;
            };
            let SymbolicType::NominalPath {
                declaration,
                arguments,
            } = trait_ty
            else {
                continue;
            };
            if declaration != trait_path {
                continue;
            }
            let SymbolicTypeShapeSkeleton::Resolved {
                value: target_ty, ..
            } = target
            else {
                continue;
            };
            if target_ty != self_ty {
                continue;
            }
            if !shape.generic_parameters.is_empty() || !shape.predicates.is_empty() {
                // Generic or predicate-bearing impls need head binding and
                // entailment authority; keep the caller's fail-closed gap.
                return None;
            }
            if arguments.first() != Some(&GenericArgumentShape::Type(self_ty.clone())) {
                // The compiler-trait relation pins Self to the first explicit
                // argument; a violating impl is declaration-judgment
                // territory, never a body-side selection.
                return None;
            }
            selected.push(arguments.clone());
        }
        match selected.len() {
            0 => None,
            1 => Some(Some(selected.pop().expect("one selection row"))),
            _ => {
                self.source_error(
                    span,
                    "TRAIT002",
                    "ordinary `for` selects among multiple coherent iterator impls",
                );
                Some(None)
            }
        }
    }

    /// Returns the named required-method shape and the trait's generic formal
    /// kinds for an ordinary or compiler-known trait path.
    fn trait_method_shape(
        &self,
        trait_path: &SemanticDeclarationPath,
        name: &str,
    ) -> Option<(SymbolicDeclarationShapeSkeleton, Vec<GenericParameterKind>)> {
        if let Some(item) = self.catalog.paths.get(trait_path) {
            let entry = self.catalog.definitions.get(item)?;
            let shape = entry.declaration_shape?;
            let SymbolicDeclarationPayloadSkeleton::Trait { methods } = &shape.payload else {
                return None;
            };
            let method = methods.iter().find(|method| method.name == name)?;
            return Some(((*method.shape).clone(), shape.generic_parameters.clone()));
        }
        let core = &self.catalog.handoff.frontend().inventory().embedded_core;
        let definition = self.embedded_core_definition_for_path(trait_path)?;
        let row = core
            .typed_c2()
            .compiler_trait_for_c1_definition(definition)?;
        if row.method().map(|method| method.source_name()) != Some(name) {
            return None;
        }
        let SymbolicDeclarationPayloadSkeleton::Trait { methods } =
            &row.declaration_shape().payload
        else {
            return None;
        };
        let method = methods.iter().find(|method| method.name == name)?;
        Some((
            (*method.shape).clone(),
            row.declaration_shape().generic_parameters.clone(),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn type_bound_trait_method_call(
        &mut self,
        receiver: &CheckedExpression,
        predicate: SymbolicPredicate,
        method_shape: SymbolicDeclarationShapeSkeleton,
        trait_formals: Vec<GenericParameterKind>,
        trait_path: SemanticDeclarationPath,
        name: &str,
        arguments: &[AstExpression],
        span: Span,
    ) -> Option<LoweredValue> {
        let SymbolicPredicate::Trait {
            self_type,
            arguments: trait_arguments,
            ..
        } = predicate
        else {
            return None;
        };
        let substitution =
            match TraitFrameSubstitution::new(trait_formals, trait_arguments, self_type) {
                Ok(substitution) => substitution,
                Err(error) => {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingMethodSelection,
                        format!("environment predicate is not a usable trait frame: {error:?}"),
                    );
                    return None;
                }
            };
        let SymbolicDeclarationPayloadSkeleton::Callable(callable) = &method_shape.payload else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingMethodSelection,
                "trait method entry is not a callable shape",
            );
            return None;
        };
        let resolve = |shape: &SymbolicTypeShapeSkeleton| -> Option<SymbolicType> {
            let ty = match shape {
                SymbolicTypeShapeSkeleton::Resolved { value, .. } => value.clone(),
                SymbolicTypeShapeSkeleton::Pending(_) => return None,
            };
            let ty = substitution.substitute_type(&ty, 1).ok()?;
            Some(erase_method_frame_lifetimes(ty))
        };
        let Some((receiver_parameter, value_parameters)) = callable.parameters.split_first() else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingMethodSelection,
                "receiverless trait method reached postfix selection",
            );
            return None;
        };
        let Some(expected_receiver) = resolve(&receiver_parameter.ty) else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingMethodSelection,
                "trait method receiver type is pending",
            );
            return None;
        };
        let receiver_ok = match receiver_parameter.mode {
            SymbolicCallableParameterMode::ReceiverShared => {
                let SymbolicType::Reference { pointee, .. } = &expected_receiver else {
                    for argument in arguments {
                        self.check_expression(argument, None);
                    }
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingMethodSelection,
                        "shared-receiver trait method resolved to a non-reference receiver type",
                    );
                    return None;
                };
                let actual = receiver.ty();
                actual == &**pointee
                    || matches!(
                        actual,
                        SymbolicType::Reference { pointee: actual_pointee, .. }
                            if actual_pointee == pointee
                    )
            }
            SymbolicCallableParameterMode::ReceiverValue => receiver.ty() == &expected_receiver,
            SymbolicCallableParameterMode::ReceiverMutable
            | SymbolicCallableParameterMode::Value => {
                for argument in arguments {
                    self.check_expression(argument, None);
                }
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingMethodSelection,
                    "mutable-receiver and value-mode bound trait methods need place-mutability authority",
                );
                return None;
            }
        };
        if !receiver_ok {
            for argument in arguments {
                self.check_expression(argument, None);
            }
            self.source_error(
                span,
                "TYPE002",
                format!("method `{name}` receiver mode does not accept this receiver type"),
            );
            return None;
        }
        let mut parameter_types = Vec::new();
        for parameter in value_parameters {
            let Some(ty) = resolve(&parameter.ty) else {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingMethodSelection,
                    "trait method parameter type is pending",
                );
                return None;
            };
            parameter_types.push(ty);
        }
        self.check_call_arguments(&parameter_types, arguments, span);
        let Some(result) = resolve(&callable.result) else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingMethodSelection,
                "trait method result type is pending",
            );
            return None;
        };
        self.calls.push(CheckedBodyCall {
            span,
            callee: CheckedBodyCallee::TraitMethod {
                trait_path: Box::new(trait_path),
                method: name.into(),
            },
            result: result.clone(),
        });
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(result)))
    }

    /// Types a direct call of an associated compiler nominal method whose
    /// generics are only decidable from the call arguments (`Pin::new_unchecked`
    /// on a concrete referent, say). Engages only when the value path would
    /// gap: no turbofish, generics present, and the expected type alone does
    /// not bind every generic. Everything else falls through to the ordinary
    /// function-value lowering unchanged.
    fn verified_associated_call_head<'p>(
        &mut self,
        base: &AstExpression,
        parts: &'p [arche_frontend::ast::AstPostfix],
        expected: Option<&SymbolicType>,
    ) -> Option<(Option<LoweredValue>, &'p [arche_frontend::ast::AstPostfix])> {
        let AstExpressionKind::Path(path) = &base.kind else {
            return None;
        };
        if path.generic_arguments.is_some() {
            return None;
        }
        let [first, rest @ ..] = parts else {
            return None;
        };
        let AstPostfixKind::Call(arguments) = &first.kind else {
            return None;
        };
        {
            let path_uses = self
                .scope
                .item
                .path_uses
                .iter()
                .filter(|candidate| candidate.path.span == path.span)
                .collect::<Vec<_>>();
            let [path_use] = path_uses.as_slice() else {
                return None;
            };
            if path_use.lexical_local.is_some() || !path_use.generic_arguments.is_empty() {
                return None;
            }
        }
        let resolution = self
            .scope
            .target
            .path_resolutions
            .iter()
            .find(|resolution| resolution.span == path.span)?;
        if resolution.unresolved.is_some() {
            return None;
        }
        let [Res::Builtin(arche_frontend::BuiltinRes {
            target: BuiltinResTarget::Method(method),
        })] = resolution.resolutions.as_slice()
        else {
            return None;
        };
        let method = *method;
        let spec = {
            let core = &self.catalog.handoff.frontend().inventory().embedded_core;
            let authority = core.compiler_nominal_method_for_c1_method(method)?;
            CompilerNominalMethodSpec::from_authority(authority)
        };
        if spec.receiver != CompilerNominalMethodReceiverMode::None
            || spec.receiver_type.is_some()
            || !spec.selectors.is_empty()
            || spec.generics.is_empty()
        {
            return None;
        }
        let mut substitution = CompilerMethodSubstitution::default();
        if rest.is_empty() {
            if let Some(expected) = expected {
                match self.bind_compiler_method_pattern(
                    &spec.result,
                    expected,
                    &mut substitution,
                    false,
                ) {
                    Some(true) => {}
                    // A mismatch or an unrepresentable pattern is the value
                    // path's diagnostic to mint; stay out of its way.
                    Some(false) | None => return None,
                }
            }
        }
        let fully_bound = spec.generics.iter().all(|generic| {
            let coordinate = generic.coordinate();
            match generic.kind() {
                CompilerMethodGenericParameterKind::Type => {
                    substitution.types.contains_key(&coordinate)
                        || substitution.capability_packs.contains_key(&coordinate)
                }
                CompilerMethodGenericParameterKind::Lifetime => {
                    substitution.lifetimes.contains_key(&coordinate)
                }
            }
        });
        if fully_bound {
            return None;
        }
        // Committed: bind the remaining generics from the checked arguments.
        let checked = match self.lower_arguments_bottom_up(arguments) {
            Some(checked) => checked,
            None => return Some((None, rest)),
        };
        if spec.parameters.len() != checked.len() {
            self.source_error(
                first.span,
                "TYPE002",
                format!(
                    "call expects {} arguments, found {}",
                    spec.parameters.len(),
                    checked.len()
                ),
            );
            return Some((None, rest));
        }
        for (pattern, (argument_span, actual)) in spec.parameters.iter().zip(&checked) {
            match self.bind_compiler_method_pattern(pattern, actual, &mut substitution, false) {
                Some(true) => {}
                Some(false) => {
                    self.source_error(
                        *argument_span,
                        "TYPE002",
                        "argument does not match the verified compiler method parameter",
                    );
                    return Some((None, rest));
                }
                None => return Some((None, rest)),
            }
        }
        if !self.validate_compiler_method_generics(
            &spec,
            &substitution,
            &BTreeSet::new(),
            first.span,
        ) {
            return Some((None, rest));
        }
        if !self.validate_compiler_method_effects(&spec, &substitution, first.span) {
            return Some((None, rest));
        }
        let parameters = match spec
            .parameters
            .iter()
            .map(|pattern| self.compiler_method_pattern_type(pattern, &substitution, first.span))
            .collect::<Option<Vec<_>>>()
        {
            Some(parameters) => parameters,
            None => return Some((None, rest)),
        };
        let result =
            match self.compiler_method_pattern_type(&spec.result, &substitution, first.span) {
                Some(result) => result,
                None => return Some((None, rest)),
            };
        if spec.is_unsafe && self.unsafe_depth == 0 {
            self.source_error(
                first.span,
                "TYPE002",
                "unsafe compiler nominal method call requires an unsafe context",
            );
            return Some((None, rest));
        }
        if !spec.requires.is_empty() || !spec.throws.is_empty() {
            self.pending_c4(
                first.span,
                compiler_method_dependency_bytes(spec.method, b"associated-effects"),
                "compiler nominal method effect membership is finalized by C4",
            );
        }
        self.check_prepared_arguments(&parameters, &checked, first.span);
        self.calls.push(CheckedBodyCall {
            span: first.span,
            callee: CheckedBodyCallee::EmbeddedMethod(spec.method),
            result: result.clone(),
        });
        Some((
            Some(LoweredValue::ordinary(TypedExpressionInput::Known(result))),
            rest,
        ))
    }

    fn lower_embedded_include_call(
        &mut self,
        definition: VirtualDefinitionId,
        kind: arche_frontend::include_inputs::IncludeInputKind,
        arguments: &[AstExpression],
        span: Span,
    ) -> Option<LoweredValue> {
        use arche_frontend::include_inputs::IncludeInputKind;
        let static_str = SymbolicType::Reference {
            mutability: Mutability::Shared,
            lifetime: SymbolicLifetime::Static,
            pointee: Box::new(SymbolicType::Str),
        };
        let [argument] = arguments else {
            for argument in arguments {
                self.check_expression(argument, None);
            }
            self.source_error(
                span,
                "TYPE002",
                format!(
                    "`{}` requires exactly one string-literal portable path",
                    kind.source_name()
                ),
            );
            return None;
        };
        let AstExpressionKind::Literal(AstLiteral::String(path)) = &argument.kind else {
            self.check_expression(argument, None);
            self.source_error(
                span,
                "TYPE002",
                format!(
                    "`{}` requires exactly one string-literal portable path",
                    kind.source_name()
                ),
            );
            return None;
        };
        let path = path.clone();
        self.check_expression(argument, Some(&static_str));
        let result = match kind {
            IncludeInputKind::Str => static_str,
            IncludeInputKind::Bytes => {
                let Some(length) = self.include_input_byte_length(path.as_ref()) else {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                        "include input has no retained source-tree commitment",
                    );
                    return None;
                };
                SymbolicType::Reference {
                    mutability: Mutability::Shared,
                    lifetime: SymbolicLifetime::Static,
                    pointee: Box::new(SymbolicType::Array {
                        element: Box::new(SymbolicType::U8),
                        length: SymbolicConstExpression {
                            integer_type: arche_frontend::IntegerType::Usize,
                            node: SymbolicConstNode::IntegerLiteral(length.to_le_bytes().to_vec()),
                        },
                    }),
                }
            }
        };
        self.calls.push(CheckedBodyCall {
            span,
            callee: CheckedBodyCallee::EmbeddedDefinition(definition),
            result: result.clone(),
        });
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(result)))
    }

    fn include_input_byte_length(&self, path: &str) -> Option<u64> {
        let portable = arche_package::PortablePath::new(path).ok()?;
        let frontend = self.catalog.handoff.frontend();
        let package_node = frontend
            .hir()
            .packages
            .iter()
            .find(|package| {
                package
                    .targets
                    .iter()
                    .any(|target| std::ptr::eq(target, self.scope.target))
            })
            .map(|package| package.package_node)?;
        frontend
            .sources()
            .source_entries(package_node)
            .into_iter()
            .find(|entry| entry.path == portable)
            .map(|entry| entry.byte_length)
    }

    fn lower_embedded_panic_call(
        &mut self,
        definition: VirtualDefinitionId,
        arguments: &[AstExpression],
        span: Span,
    ) -> Option<LoweredValue> {
        let [argument] = arguments else {
            for argument in arguments {
                self.check_expression(argument, None);
            }
            self.source_error(
                span,
                "TYPE002",
                "`panic` requires exactly one payload argument",
            );
            return None;
        };
        self.check_expression(argument, None);
        let mut dependency = b"embedded-panic-unwind-payload".to_vec();
        dependency.extend_from_slice(&u64::from(definition.ordinal()).to_le_bytes());
        self.pending_c4(
            span,
            dependency,
            "panic UnwindPayload judgment is finalized by C4",
        );
        self.calls.push(CheckedBodyCall {
            span,
            callee: CheckedBodyCallee::EmbeddedDefinition(definition),
            result: SymbolicType::Never,
        });
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(
            SymbolicType::Never,
        )))
    }

    fn lower_call_part(
        &mut self,
        value: LoweredValue,
        arguments: &[AstExpression],
        span: Span,
    ) -> Option<LoweredValue> {
        if let ValueCategory::Constructor(selection) = &value.category {
            return self.lower_tuple_constructor_call(&value, selection.clone(), arguments, span);
        }
        let callee = match value.category {
            ValueCategory::DirectFunction(item) => CheckedBodyCallee::DirectItem(item),
            ValueCategory::AssociatedFunction(item) => CheckedBodyCallee::AssociatedItem(item),
            ValueCategory::EmbeddedFunction {
                method,
                is_unsafe,
                has_effects,
            } => {
                if is_unsafe && self.unsafe_depth == 0 {
                    for argument in arguments {
                        self.check_expression(argument, None);
                    }
                    self.source_error(
                        span,
                        "TYPE002",
                        "unsafe compiler nominal method call requires an unsafe context",
                    );
                    return None;
                }
                if has_effects {
                    self.pending_c4(
                        span,
                        compiler_method_dependency_bytes(method, b"associated-effects"),
                        "compiler nominal method effect membership is finalized by C4",
                    );
                }
                CheckedBodyCallee::EmbeddedMethod(method)
            }
            ValueCategory::PendingDirectFunction(item)
            | ValueCategory::PendingAssociatedFunction(item) => {
                let associated =
                    matches!(value.category, ValueCategory::PendingAssociatedFunction(_));
                return self.infer_function_call(item, associated, arguments, span);
            }
            ValueCategory::Ordinary => CheckedBodyCallee::FunctionPointer,
            ValueCategory::Query { .. }
            | ValueCategory::Commands
            | ValueCategory::Constructor(_) => {
                self.source_error(span, "TYPE002", "value is not callable");
                return None;
            }
        };
        let function_ty = match &value.input {
            TypedExpressionInput::Known(ty) => ty.clone(),
            _ => self.materialize(&value, None, span)?.ty().clone(),
        };
        let (callee, parameters, result, has_effects) = match function_ty {
            SymbolicType::FunctionPointer {
                parameters,
                result,
                requires,
                throws,
                ..
            } => {
                let has_effects = !requires.members().is_empty() || !throws.members().is_empty();
                (callee, parameters, *result, has_effects)
            }
            SymbolicType::Closure {
                parameters,
                result,
                requires,
                throws,
                ..
            } => {
                let has_effects = !requires.members().is_empty() || !throws.members().is_empty();
                (
                    CheckedBodyCallee::ClosureValue,
                    parameters,
                    *result,
                    has_effects,
                )
            }
            SymbolicType::GeneratorFactory {
                parameters,
                factory_unsafe,
                produced_generator,
                ..
            } => {
                if factory_unsafe && self.unsafe_depth == 0 {
                    for argument in arguments {
                        self.check_expression(argument, None);
                    }
                    self.source_error(
                        span,
                        "TYPE002",
                        "unsafe generator factory call requires an unsafe context",
                    );
                    return None;
                }
                // Factory calls have exact empty requires/throws by contract.
                (
                    CheckedBodyCallee::GeneratorFactoryValue,
                    parameters,
                    *produced_generator,
                    false,
                )
            }
            _ => {
                self.source_error(span, "TYPE002", "value is not a function pointer");
                return None;
            }
        };
        self.check_call_arguments(&parameters, arguments, span);
        if has_effects {
            self.pending_c4(
                span,
                b"call-effect-membership".to_vec(),
                "call effect membership is finalized by C4",
            );
        }
        self.calls.push(CheckedBodyCall {
            span,
            callee,
            result: result.clone(),
        });
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(result)))
    }

    fn lower_tuple_constructor_call(
        &mut self,
        value: &LoweredValue,
        selection: ConstructorSelection,
        arguments: &[AstExpression],
        span: Span,
    ) -> Option<LoweredValue> {
        if let ConstructorSelection::PendingInference { item, variant } = selection {
            return self.infer_tuple_constructor_call(item, variant, arguments, span);
        }
        let constructed = match &value.input {
            TypedExpressionInput::Known(ty) => ty.clone(),
            _ => {
                for argument in arguments {
                    self.check_expression(argument, None);
                }
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingRetainedJoin,
                    "constructor call value did not retain its constructed nominal type",
                );
                return None;
            }
        };
        let actuals = match &constructed {
            SymbolicType::NominalPath { arguments, .. } => arguments.clone(),
            _ => Vec::new(),
        };
        let (callee, form, fields) = match selection {
            ConstructorSelection::PendingInference { .. } => {
                unreachable!("PendingInference is dispatched before value extraction")
            }
            ConstructorSelection::Item { item, variant } => {
                let Some(entry) = self.catalog.definition(item) else {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingRetainedJoin,
                        "constructor item has no declaration catalog row",
                    );
                    return None;
                };
                let declaration_shape =
                    checked_entry_shape(entry, self.scope.body.id, span, &mut self.gaps)?;
                let (form, fields) = match (&declaration_shape.payload, variant) {
                    (SymbolicDeclarationPayloadSkeleton::Record(record), None) => {
                        (record.form, record.fields.clone())
                    }
                    (SymbolicDeclarationPayloadSkeleton::Enum(variants), Some(ordinal)) => {
                        let variant = usize::try_from(ordinal)
                            .ok()
                            .and_then(|ordinal| variants.get(ordinal))?;
                        (variant.form, variant.fields.clone())
                    }
                    (SymbolicDeclarationPayloadSkeleton::Tag, None) => {
                        (SymbolicRecordForm::Unit, Vec::new())
                    }
                    _ => {
                        self.source_error(span, "TYPE002", "value is not a constructor");
                        return None;
                    }
                };
                (CheckedBodyCallee::DirectItem(item), form, fields)
            }
            ConstructorSelection::EmbeddedRecord(definition) => {
                let Some((form, fields)) = self.embedded_record_form(definition) else {
                    for argument in arguments {
                        self.check_expression(argument, None);
                    }
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                        format!("embedded constructor {definition:?} lacks typed field signatures"),
                    );
                    return None;
                };
                (
                    CheckedBodyCallee::EmbeddedDefinition(definition),
                    form,
                    fields,
                )
            }
            ConstructorSelection::EmbeddedVariant(variant) => {
                let Some((owner, form, fields)) = self.embedded_variant_form(variant) else {
                    for argument in arguments {
                        self.check_expression(argument, None);
                    }
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                        format!("embedded variant {variant:?} lacks typed field signatures"),
                    );
                    return None;
                };
                (CheckedBodyCallee::EmbeddedDefinition(owner), form, fields)
            }
        };
        if form == SymbolicRecordForm::Record {
            self.source_error(span, "TYPE002", "record constructor requires named fields");
            return None;
        }
        let mut parameter_types = Vec::new();
        for field in fields {
            let ty = self.require_type_shape(&field.ty, span)?;
            parameter_types.push(substitute_type(&ty, &actuals));
        }
        self.check_call_arguments(&parameter_types, arguments, span);
        self.calls.push(CheckedBodyCall {
            span,
            callee,
            result: constructed.clone(),
        });
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(
            constructed,
        )))
    }

    fn check_call_arguments(
        &mut self,
        parameters: &[SymbolicType],
        arguments: &[AstExpression],
        span: Span,
    ) {
        if parameters.len() != arguments.len() {
            self.source_error(
                span,
                "TYPE002",
                format!(
                    "call expects {} arguments, found {}",
                    parameters.len(),
                    arguments.len()
                ),
            );
        }
        for (index, argument) in arguments.iter().enumerate() {
            self.check_expression(argument, parameters.get(index));
        }
    }

    fn lower_field_part(
        &mut self,
        value: LoweredValue,
        name: &str,
        span: Span,
    ) -> Option<LoweredValue> {
        let checked = self.materialize(&value, None, span)?;
        let SymbolicType::NominalPath {
            declaration,
            arguments,
        } = peel_references(checked.ty())
        else {
            self.source_error(
                span,
                "TYPE002",
                format!("field `{name}` requires a record value"),
            );
            return None;
        };
        let Some(entry) = self.catalog.item_for_path(declaration) else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                format!(
                    "nominal `{}` has no typed user field descriptor",
                    declaration.name
                ),
            );
            return None;
        };
        let declaration_shape =
            checked_entry_shape(entry, self.scope.body.id, span, &mut self.gaps)?;
        let SymbolicDeclarationPayloadSkeleton::Record(record) = &declaration_shape.payload else {
            self.source_error(span, "TYPE002", format!("type has no field `{name}`"));
            return None;
        };
        let Some(field) = record
            .fields
            .iter()
            .find(|field| field.name.as_deref() == Some(name))
        else {
            self.source_error(span, "TYPE002", format!("unknown field `{name}`"));
            return None;
        };
        let field_shape = field.ty.clone();
        let actuals = arguments.clone();
        let ty = self.require_type_shape(&field_shape, span)?;
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(
            substitute_type(&ty, &actuals),
        )))
    }

    fn lower_tuple_field_part(
        &mut self,
        value: LoweredValue,
        index: usize,
        span: Span,
    ) -> Option<LoweredValue> {
        let checked = self.materialize(&value, None, span)?;
        let ty = match peel_references(checked.ty()) {
            SymbolicType::Tuple(fields) => fields.get(index).cloned(),
            SymbolicType::NominalPath {
                declaration,
                arguments,
            } => {
                let Some(entry) = self.catalog.item_for_path(declaration) else {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                        format!(
                            "nominal `{}` has no typed tuple-field descriptor",
                            declaration.name
                        ),
                    );
                    return None;
                };
                let declaration_shape =
                    checked_entry_shape(entry, self.scope.body.id, span, &mut self.gaps)?;
                let SymbolicDeclarationPayloadSkeleton::Record(record) = &declaration_shape.payload
                else {
                    self.source_error(span, "TYPE002", "tuple field requires a tuple value");
                    return None;
                };
                if record.form != SymbolicRecordForm::Tuple {
                    self.source_error(span, "TYPE002", "tuple field requires a tuple value");
                    return None;
                }
                record.fields.get(index).and_then(|field| {
                    self.require_type_shape(&field.ty, span)
                        .map(|ty| substitute_type(&ty, arguments))
                })
            }
            _ => {
                self.source_error(span, "TYPE002", "tuple field requires a tuple value");
                return None;
            }
        };
        let Some(ty) = ty else {
            self.source_error(
                span,
                "TYPE002",
                format!("tuple index {index} is out of bounds"),
            );
            return None;
        };
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(ty)))
    }

    fn lower_index_part(
        &mut self,
        value: LoweredValue,
        index: &AstExpression,
        span: Span,
    ) -> Option<LoweredValue> {
        let checked = self.materialize(&value, None, span)?;
        self.check_expression(index, Some(&SymbolicType::Usize));
        let element = match peel_references(checked.ty()) {
            SymbolicType::Array { element, .. } | SymbolicType::Slice(element) => {
                element.as_ref().clone()
            }
            actual => {
                self.source_error(span, "TYPE002", format!("cannot index type {actual:?}"));
                return None;
            }
        };
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(element)))
    }

    fn lower_self(&mut self, span: Span) -> Option<LoweredValue> {
        let uses = self
            .scope
            .item
            .self_uses
            .iter()
            .filter(|candidate| candidate.span == span)
            .collect::<Vec<_>>();
        if uses.len() != 1 {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "lowercase self span does not join to one C1 self-use row",
            );
            return None;
        }
        self.lower_local(uses[0].receiver, span)
    }

    fn lower_local(&mut self, local: LocalId, span: Span) -> Option<LoweredValue> {
        match self.cross_body_locals.get(&local).cloned() {
            Some(LocalValue::Typed(ty)) => {
                Some(LoweredValue::ordinary(TypedExpressionInput::Known(ty)))
            }
            Some(LocalValue::Query { item }) => Some(LoweredValue {
                input: TypedExpressionInput::Unit,
                category: ValueCategory::Query { item },
            }),
            Some(LocalValue::Commands) => Some(LoweredValue {
                input: TypedExpressionInput::Unit,
                category: ValueCategory::Commands,
            }),
            None => {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingRetainedJoin,
                    format!("local {local:?} has no established C2 type"),
                );
                None
            }
        }
    }

    fn lower_unary(
        &mut self,
        span: Span,
        operator: AstUnaryOperator,
        operand: &AstExpression,
        expected: Option<&SymbolicType>,
    ) -> Option<LoweredValue> {
        match operator {
            AstUnaryOperator::Negate | AstUnaryOperator::LogicalNot | AstUnaryOperator::BitNot => {
                let operand = self.lower_expression(operand, expected)?;
                Some(LoweredValue::ordinary(TypedExpressionInput::Unary {
                    operator: match operator {
                        AstUnaryOperator::Negate => UnaryTypeOperator::Negate,
                        AstUnaryOperator::LogicalNot => UnaryTypeOperator::LogicalNot,
                        AstUnaryOperator::BitNot => UnaryTypeOperator::BitNot,
                        AstUnaryOperator::Dereference
                        | AstUnaryOperator::BorrowShared
                        | AstUnaryOperator::BorrowMutable => unreachable!(),
                    },
                    operand: Box::new(operand.input),
                }))
            }
            AstUnaryOperator::BorrowShared | AstUnaryOperator::BorrowMutable => {
                let operand = self.lower_expression(operand, None)?;
                Some(LoweredValue::ordinary(TypedExpressionInput::Borrow {
                    mutability: if operator == AstUnaryOperator::BorrowMutable {
                        Mutability::Mutable
                    } else {
                        Mutability::Shared
                    },
                    value: Box::new(operand.input),
                }))
            }
            AstUnaryOperator::Dereference => {
                let operand_source_span = operand.span;
                let operand = self.lower_expression(operand, None)?;
                let checked = self.materialize(&operand, None, operand_source_span)?;
                let pointee = match checked.ty() {
                    SymbolicType::Reference { pointee, .. }
                    | SymbolicType::RawPointer { pointee, .. } => pointee.as_ref().clone(),
                    actual => {
                        self.source_error(
                            span,
                            "TYPE002",
                            format!("cannot dereference non-pointer type {actual:?}"),
                        );
                        return None;
                    }
                };
                Some(LoweredValue::ordinary(TypedExpressionInput::Known(pointee)))
            }
        }
    }

    fn lower_cast(
        &mut self,
        span: Span,
        value: &AstExpression,
        ty: &AstType,
    ) -> Option<LoweredValue> {
        let target = self.type_at_span(ty.span)?;
        let value_source_span = value.span;
        let value = self.lower_expression(value, None)?;
        let checked = self.materialize(&value, None, value_source_span)?;
        let allowed = match (checked.ty(), &target) {
            (
                SymbolicType::RawPointer { .. },
                SymbolicType::RawPointer { .. } | SymbolicType::Usize | SymbolicType::Isize,
            )
            | (SymbolicType::Usize | SymbolicType::Isize, SymbolicType::RawPointer { .. }) => true,
            (
                SymbolicType::Reference {
                    mutability: source_mutability,
                    pointee: source_pointee,
                    ..
                },
                SymbolicType::RawPointer {
                    mutability: target_mutability,
                    pointee: target_pointee,
                },
            ) => {
                source_pointee == target_pointee
                    && (*source_mutability == Mutability::Mutable
                        || *target_mutability == Mutability::Shared)
            }
            _ => false,
        };
        if !allowed {
            self.source_error(
                span,
                "TYPE002",
                format!(
                    "`as` supports only raw-pointer/address reconstruction, not {} to {}",
                    crate::golden::spell_symbolic_type(checked.ty()),
                    crate::golden::spell_symbolic_type(&target)
                ),
            );
            return None;
        }
        Some(LoweredValue::ordinary(TypedExpressionInput::Known(target)))
    }

    fn lower_path(
        &mut self,
        path: &arche_frontend::ast::AstPath,
        span: Span,
        expected: Option<&SymbolicType>,
    ) -> Option<LoweredValue> {
        let path_uses = self
            .scope
            .item
            .path_uses
            .iter()
            .filter(|candidate| candidate.path.span == path.span)
            .collect::<Vec<_>>();
        if path_uses.len() != 1 {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                format!(
                    "value path span joins to {} retained HIR path uses",
                    path_uses.len()
                ),
            );
            return None;
        }
        let path_use = path_uses[0];
        if let Some(local) = path_use.lexical_local {
            return self.lower_local(local, span);
        }
        let resolution = self.path_resolution(path.span)?.clone();
        if let Some(unresolved) = &resolution.unresolved {
            if *unresolved == UnresolvedPathKind::AssociatedItemPendingC2 {
                return self.lower_associated_path(&resolution, path_use, span, expected);
            }
            self.gap(
                span,
                BodyCheckIncompletenessKind::PendingC2Type,
                format!("C1 path remained unresolved for C2: {unresolved:?}"),
            );
            return None;
        }
        if resolution.resolutions.len() != 1 {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                format!(
                    "value expression path has {} namespace resolutions",
                    resolution.resolutions.len()
                ),
            );
            return None;
        }
        self.lower_resolution(&resolution.resolutions[0], path_use, span, expected)
    }

    fn path_resolution(&mut self, span: Span) -> Option<&PathResolution> {
        let rows = self
            .scope
            .target
            .path_resolutions
            .iter()
            .filter(|candidate| candidate.span == span)
            .collect::<Vec<_>>();
        if rows.len() != 1 {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                format!("path span joins to {} C1 resolution rows", rows.len()),
            );
            return None;
        }
        Some(rows[0])
    }

    fn lower_resolution(
        &mut self,
        resolution: &Res,
        path_use: &arche_frontend::HirPathUse,
        span: Span,
        expected: Option<&SymbolicType>,
    ) -> Option<LoweredValue> {
        match resolution {
            Res::Local(local) => self.lower_local(*local, span),
            Res::Item(item) => self.lower_item_resolution(*item, path_use, span, expected),
            Res::Builtin(builtin) => {
                self.lower_builtin_resolution(builtin.target, path_use, span, expected)
            }
            Res::Generic(_) => {
                self.source_error(span, "TYPE002", "generic parameter is not a value");
                None
            }
            Res::Module(_) => {
                self.source_error(span, "TYPE002", "module path is not a value");
                None
            }
        }
    }

    fn lower_item_resolution(
        &mut self,
        resolution: HirItemRes,
        path_use: &arche_frontend::HirPathUse,
        span: Span,
        expected: Option<&SymbolicType>,
    ) -> Option<LoweredValue> {
        let owner = resolution.owner();
        let Some(entry) = self.catalog.definition(owner) else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                format!("resolved item {owner:?} has no C2 declaration entry"),
            );
            return None;
        };
        let declaration_shape =
            checked_entry_shape(entry, self.scope.body.id, span, &mut self.gaps)?;
        match resolution {
            HirItemRes::Definition(item) => match entry.definition.key.kind {
                DeclarationKind::Function | DeclarationKind::Generator => {
                    let actuals = self.path_actuals(path_use, span)?;
                    if actuals.is_empty() && !declaration_shape.generic_parameters.is_empty() {
                        return Some(LoweredValue {
                            input: TypedExpressionInput::Unit,
                            category: ValueCategory::PendingDirectFunction(item),
                        });
                    }
                    let signature = self.callable_signature(declaration_shape, &actuals, span)?;
                    Some(LoweredValue {
                        input: TypedExpressionInput::Known(signature.function_pointer()),
                        category: ValueCategory::DirectFunction(item),
                    })
                }
                DeclarationKind::Const | DeclarationKind::Static => {
                    let shape = declared_value_type(declaration_shape)?;
                    let ty = self.require_type_shape(shape, span)?;
                    Some(LoweredValue::ordinary(TypedExpressionInput::Known(ty)))
                }
                DeclarationKind::Tag
                | DeclarationKind::Struct
                | DeclarationKind::Component
                | DeclarationKind::Resource => {
                    let Some(actuals) =
                        self.path_actuals_expected_or_inference(path_use, entry, expected, span)?
                    else {
                        return Some(LoweredValue {
                            input: TypedExpressionInput::Unit,
                            category: ValueCategory::Constructor(
                                ConstructorSelection::PendingInference {
                                    item,
                                    variant: None,
                                },
                            ),
                        });
                    };
                    let ty = nominal_type(entry, actuals);
                    let input = TypedExpressionInput::Known(ty);
                    if constructor_is_unit(declaration_shape, None) {
                        Some(LoweredValue::ordinary(input))
                    } else {
                        Some(LoweredValue {
                            input,
                            category: ValueCategory::Constructor(ConstructorSelection::Item {
                                item,
                                variant: None,
                            }),
                        })
                    }
                }
                _ => {
                    self.source_error(
                        span,
                        "TYPE002",
                        format!(
                            "{} declaration `{}` is not a value",
                            declaration_kind_atom(entry.definition.key.kind),
                            entry.definition.key.name
                        ),
                    );
                    None
                }
            },
            HirItemRes::NominalConstructor { owner } => {
                let Some(actuals) =
                    self.path_actuals_expected_or_inference(path_use, entry, expected, span)?
                else {
                    return Some(LoweredValue {
                        input: TypedExpressionInput::Unit,
                        category: ValueCategory::Constructor(
                            ConstructorSelection::PendingInference {
                                item: owner,
                                variant: None,
                            },
                        ),
                    });
                };
                let ty = nominal_type(entry, actuals);
                let input = TypedExpressionInput::Known(ty);
                if constructor_is_unit(declaration_shape, None) {
                    Some(LoweredValue::ordinary(input))
                } else {
                    Some(LoweredValue {
                        input,
                        category: ValueCategory::Constructor(ConstructorSelection::Item {
                            item: owner,
                            variant: None,
                        }),
                    })
                }
            }
            HirItemRes::EnumVariant { owner, ordinal } => {
                let Some(actuals) =
                    self.path_actuals_expected_or_inference(path_use, entry, expected, span)?
                else {
                    if constructor_is_unit(declaration_shape, Some(ordinal)) {
                        self.gap(
                            span,
                            BodyCheckIncompletenessKind::MissingGenericInference,
                            "unit variant of a generic enum has no argument to infer from",
                        );
                        return None;
                    }
                    return Some(LoweredValue {
                        input: TypedExpressionInput::Unit,
                        category: ValueCategory::Constructor(
                            ConstructorSelection::PendingInference {
                                item: owner,
                                variant: Some(ordinal),
                            },
                        ),
                    });
                };
                let ty = nominal_type(entry, actuals);
                let input = TypedExpressionInput::Known(ty);
                if constructor_is_unit(declaration_shape, Some(ordinal)) {
                    Some(LoweredValue::ordinary(input))
                } else {
                    Some(LoweredValue {
                        input,
                        category: ValueCategory::Constructor(ConstructorSelection::Item {
                            item: owner,
                            variant: Some(ordinal),
                        }),
                    })
                }
            }
        }
    }

    fn lower_builtin_resolution(
        &mut self,
        target: BuiltinResTarget,
        path_use: &arche_frontend::HirPathUse,
        span: Span,
        expected: Option<&SymbolicType>,
    ) -> Option<LoweredValue> {
        match target {
            BuiltinResTarget::Method(method) => {
                let core = &self.catalog.handoff.frontend().inventory().embedded_core;
                if let Some(authority) = core.compiler_nominal_method_for_c1_method(method) {
                    let spec = CompilerNominalMethodSpec::from_authority(authority);
                    return self.lower_verified_associated_method(&spec, path_use, expected, span);
                }
                if core
                    .typed_c2()
                    .compiler_trait_method_for_c1_method(method)
                    .is_some()
                {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingEmbeddedTraitIdentity,
                        format!(
                            "compiler-trait method {method:?} cannot be selected before its stable embedded trait DefinitionId exists"
                        ),
                    );
                    return None;
                }
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                    format!(
                        "embedded method {method:?} has only a string C1 signature, not a typed C2 callable"
                    ),
                );
                None
            }
            BuiltinResTarget::EnumVariant(variant) => {
                let owner = {
                    let core = &self.catalog.handoff.frontend().inventory().embedded_core;
                    core.enum_variant(variant).map(|row| row.owner())
                };
                let known = owner.and_then(|owner| {
                    self.adopt_expected_embedded_nominal(owner, expected)
                        .or_else(|| self.zero_generic_embedded_nominal_type(owner))
                });
                let Some(known) = known else {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingGenericInference,
                        format!(
                            "embedded enum variant {variant:?} requires contextual nominal arguments"
                        ),
                    );
                    return None;
                };
                Some(LoweredValue {
                    input: TypedExpressionInput::Known(known),
                    category: ValueCategory::Constructor(ConstructorSelection::EmbeddedVariant(
                        variant,
                    )),
                })
            }
            BuiltinResTarget::RecordConstructor(definition) => {
                let known = self
                    .adopt_expected_embedded_nominal(definition, expected)
                    .or_else(|| self.zero_generic_embedded_nominal_type(definition));
                let Some(known) = known else {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingGenericInference,
                        format!(
                            "embedded record constructor {definition:?} requires contextual nominal arguments"
                        ),
                    );
                    return None;
                };
                Some(LoweredValue {
                    input: TypedExpressionInput::Known(known),
                    category: ValueCategory::Constructor(ConstructorSelection::EmbeddedRecord(
                        definition,
                    )),
                })
            }
            BuiltinResTarget::Prelude(prelude) => match prelude {
                VirtualPreludeTarget::Definition(definition) => {
                    let known = self
                        .adopt_expected_embedded_nominal(definition, expected)
                        .or_else(|| self.zero_generic_embedded_nominal_type(definition));
                    if let Some(known) = known {
                        Some(LoweredValue {
                            input: TypedExpressionInput::Known(known),
                            category: ValueCategory::Constructor(
                                ConstructorSelection::EmbeddedRecord(definition),
                            ),
                        })
                    } else {
                        self.gap(
                            span,
                            BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                            format!(
                                "embedded prelude definition {definition:?} lacks a typed value descriptor at this site"
                            ),
                        );
                        None
                    }
                }
                VirtualPreludeTarget::SemanticType(_) => {
                    self.source_error(span, "TYPE002", "semantic type name is not a value");
                    None
                }
            },
        }
    }

    fn lower_associated_path(
        &mut self,
        resolution: &PathResolution,
        path_use: &arche_frontend::HirPathUse,
        span: Span,
        expected: Option<&SymbolicType>,
    ) -> Option<LoweredValue> {
        let associated = resolution.associated.as_ref()?;
        if associated.candidates.len() != 1 {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingMethodSelection,
                format!(
                    "associated path `{}` has {} C1 candidates requiring C2 viability filtering",
                    associated.member,
                    associated.candidates.len()
                ),
            );
            return None;
        }
        match associated.candidates[0] {
            AssociatedPathCandidate::Item(item) => self
                .lower_item_resolution(item, path_use, span, expected)
                .map(|mut value| {
                    value.category = match value.category {
                        ValueCategory::DirectFunction(item) => {
                            ValueCategory::AssociatedFunction(item)
                        }
                        ValueCategory::PendingDirectFunction(item) => {
                            ValueCategory::PendingAssociatedFunction(item)
                        }
                        category => category,
                    };
                    value
                }),
            AssociatedPathCandidate::Builtin(builtin) => {
                self.lower_builtin_resolution(builtin.target, path_use, span, expected)
            }
        }
    }

    fn path_actuals(
        &mut self,
        path_use: &arche_frontend::HirPathUse,
        span: Span,
    ) -> Option<Vec<GenericArgumentShape>> {
        self.resolved_generic_actuals(&path_use.generic_arguments, span)
    }

    fn resolved_generic_actuals(
        &mut self,
        arguments: &[arche_frontend::HirGenericArgumentUse],
        span: Span,
    ) -> Option<Vec<GenericArgumentShape>> {
        let mut output = Vec::new();
        for argument in arguments {
            let value = match &argument.value {
                ResolvedGenericArgument::Type(ResolvedSymbolicType::Resolved(ty)) => {
                    GenericArgumentShape::Type((**ty).clone())
                }
                ResolvedGenericArgument::Lifetime(
                    arche_frontend::ResolvedSymbolicLifetime::Resolved(lifetime),
                ) => GenericArgumentShape::Lifetime(lifetime.clone()),
                ResolvedGenericArgument::IntegerConst(
                    arche_frontend::ResolvedSymbolicConst::Resolved(value),
                ) => GenericArgumentShape::IntegerConst(value.clone()),
                ResolvedGenericArgument::Type(ResolvedSymbolicType::Pending {
                    reason,
                    canonical,
                    ..
                })
                | ResolvedGenericArgument::IntegerConst(
                    arche_frontend::ResolvedSymbolicConst::Pending {
                        reason, canonical, ..
                    },
                )
                | ResolvedGenericArgument::Lifetime(
                    arche_frontend::ResolvedSymbolicLifetime::Pending {
                        reason, canonical, ..
                    },
                ) => {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::PendingC2Type,
                        format!("pending generic argument {reason:?}: `{canonical}`"),
                    );
                    return None;
                }
            };
            output.push(value);
        }
        Some(output)
    }

    /// Like `path_actuals_or_expected`, but a zero-actual generic use with no
    /// matching contextual type reports `Some(None)` (argument inference may
    /// still decide) instead of recording a gap.
    fn path_actuals_expected_or_inference(
        &mut self,
        path_use: &arche_frontend::HirPathUse,
        entry: &DefinitionEntry<'_>,
        expected: Option<&SymbolicType>,
        span: Span,
    ) -> Option<Option<Vec<GenericArgumentShape>>> {
        let actuals = self.path_actuals(path_use, span)?;
        let declaration_shape =
            checked_entry_shape(entry, self.scope.body.id, span, &mut self.gaps)?;
        let formal_count = declaration_shape.generic_parameters.len();
        if actuals.len() == formal_count {
            return Some(Some(actuals));
        }
        if actuals.is_empty() {
            if let Some(SymbolicType::NominalPath {
                declaration,
                arguments,
            }) = expected
            {
                if declaration == &entry.semantic_path() && arguments.len() == formal_count {
                    return Some(Some(arguments.clone()));
                }
            }
            return Some(None);
        }
        self.gap(
            span,
            BodyCheckIncompletenessKind::MissingGenericInference,
            format!(
                "constructor/call has {} explicit actuals for {formal_count} formals and no exact contextual instantiation",
                actuals.len()
            ),
        );
        None
    }
}

fn binary_operator(operator: AstBinaryOperator) -> BinaryTypeOperator {
    match operator {
        AstBinaryOperator::LogicalOr => BinaryTypeOperator::LogicalOr,
        AstBinaryOperator::LogicalAnd => BinaryTypeOperator::LogicalAnd,
        AstBinaryOperator::BitOr => BinaryTypeOperator::BitOr,
        AstBinaryOperator::BitXor => BinaryTypeOperator::BitXor,
        AstBinaryOperator::BitAnd => BinaryTypeOperator::BitAnd,
        AstBinaryOperator::Equal => BinaryTypeOperator::Equal,
        AstBinaryOperator::NotEqual => BinaryTypeOperator::NotEqual,
        AstBinaryOperator::Less => BinaryTypeOperator::Less,
        AstBinaryOperator::LessEqual => BinaryTypeOperator::LessEqual,
        AstBinaryOperator::Greater => BinaryTypeOperator::Greater,
        AstBinaryOperator::GreaterEqual => BinaryTypeOperator::GreaterEqual,
        AstBinaryOperator::ShiftLeft => BinaryTypeOperator::ShiftLeft,
        AstBinaryOperator::ShiftRight => BinaryTypeOperator::ShiftRight,
        AstBinaryOperator::Add => BinaryTypeOperator::Add,
        AstBinaryOperator::Subtract => BinaryTypeOperator::Subtract,
        AstBinaryOperator::Multiply => BinaryTypeOperator::Multiply,
        AstBinaryOperator::Divide => BinaryTypeOperator::Divide,
        AstBinaryOperator::Remainder => BinaryTypeOperator::Remainder,
    }
}

#[derive(Clone)]
struct CallableSignature {
    unsafe_: bool,
    parameters: Vec<SymbolicType>,
    result: SymbolicType,
    requires: SymbolicTypeEffectSet,
    throws: SymbolicTypeEffectSet,
}

impl CallableSignature {
    fn function_pointer(&self) -> SymbolicType {
        SymbolicType::FunctionPointer {
            unsafe_: self.unsafe_,
            parameters: self.parameters.clone(),
            result: Box::new(self.result.clone()),
            requires: self.requires.clone(),
            throws: self.throws.clone(),
        }
    }
}

impl BodyChecker<'_, '_, '_> {
    fn callable_signature(
        &mut self,
        declaration: &SymbolicDeclarationShapeSkeleton,
        actuals: &[GenericArgumentShape],
        span: Span,
    ) -> Option<CallableSignature> {
        let SymbolicDeclarationPayloadSkeleton::Callable(callable) = &declaration.payload else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "call target has no callable payload",
            );
            return None;
        };
        if declaration.generic_parameters.len() != actuals.len() {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingGenericInference,
                format!(
                    "call target expects {} generic actuals, found {}",
                    declaration.generic_parameters.len(),
                    actuals.len()
                ),
            );
            return None;
        }
        let mut parameters = Vec::new();
        for parameter in &callable.parameters {
            let ty = self.require_type_shape(&parameter.ty, span)?;
            parameters.push(substitute_type(&ty, actuals));
        }
        let result = substitute_type(&self.require_type_shape(&callable.result, span)?, actuals);
        let requires = self.effect_set(&callable.effects.requires, actuals, span)?;
        let throws = self.effect_set(&callable.effects.throws, actuals, span)?;
        Some(CallableSignature {
            unsafe_: callable.unsafe_,
            parameters,
            result,
            requires,
            throws,
        })
    }

    fn effect_set(
        &mut self,
        members: &[arche_frontend::SymbolicEffectShapeSkeleton],
        actuals: &[GenericArgumentShape],
        span: Span,
    ) -> Option<SymbolicTypeEffectSet> {
        let mut output = Vec::new();
        for member in members {
            match member {
                arche_frontend::SymbolicEffectShapeSkeleton::Resolved { value, .. } => {
                    output.push(substitute_type(value, actuals));
                }
                arche_frontend::SymbolicEffectShapeSkeleton::Pending(pending) => {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::PendingC2Type,
                        format!("pending effect member `{}`", pending.debug_spelling),
                    );
                    return None;
                }
            }
        }
        Some(SymbolicTypeEffectSet::pending_c4(output))
    }

    fn check_block(&mut self, block: &AstBlock, expected: Option<&SymbolicType>) {
        if let Some(lowered) = self.lower_block(block, expected) {
            let _ = self.materialize(&lowered, expected, block.span);
        }
    }

    fn lower_block(
        &mut self,
        block: &AstBlock,
        expected: Option<&SymbolicType>,
    ) -> Option<LoweredValue> {
        let mut statements = Vec::new();
        let mut complete = true;
        for statement in &block.statements {
            match &statement.kind {
                AstStatementKind::Let {
                    pattern,
                    ty,
                    value,
                    else_block,
                } => {
                    let annotation = ty.as_deref().and_then(|ty| self.type_at_span(ty.span));
                    if ty.is_some() && annotation.is_none() {
                        complete = false;
                    }
                    let lowered = self.lower_expression(value, annotation.as_ref());
                    let checked = lowered.as_ref().and_then(|lowered| {
                        self.materialize(lowered, annotation.as_ref(), value.span)
                    });
                    if lowered.is_none() || checked.is_none() {
                        complete = false;
                    }
                    if let Some(checked) = &checked {
                        if else_block.is_some() {
                            self.check_and_bind_refutable(
                                pattern,
                                checked.ty(),
                                PlaceMutability::Mutable,
                            );
                        } else {
                            self.check_and_bind_irrefutable(
                                pattern,
                                checked.ty(),
                                PlaceMutability::Mutable,
                            );
                        }
                    }
                    if let Some(else_block) = else_block {
                        // The `else` block must diverge: its checked value
                        // types as never, or a statement or nested block
                        // position inside it does.
                        let value = self.lower_block(else_block, None);
                        let checked = value
                            .as_ref()
                            .and_then(|value| self.materialize(value, None, else_block.span));
                        match &checked {
                            Some(checked) => {
                                if !checked_expression_diverges(checked) {
                                    self.source_error(
                                        else_block.span,
                                        "TYPE002",
                                        "`let ... else` block must diverge",
                                    );
                                    complete = false;
                                }
                            }
                            None => {
                                complete = false;
                            }
                        }
                    }
                    if let Some(lowered) = lowered {
                        // The body-level pass re-infers this value; an
                        // annotation must ride along or an
                        // annotation-determined inference variable (an empty
                        // array's element, say) would be re-allocated with no
                        // context and survive unresolved.
                        statements.push(match &annotation {
                            Some(target) => TypedExpressionInput::Coerce {
                                value: Box::new(lowered.input),
                                target: target.clone(),
                            },
                            None => lowered.input,
                        });
                    }
                }
                AstStatementKind::For {
                    pattern,
                    iterator,
                    body,
                    ..
                } => {
                    let iterator_value = self.lower_expression(iterator, None);
                    if iterator_value.is_none() {
                        complete = false;
                    }
                    let item_type = match iterator_value.as_ref().map(|value| &value.category) {
                        Some(ValueCategory::Query { item }) => {
                            self.calls.push(CheckedBodyCall {
                                span: iterator.span,
                                callee: CheckedBodyCallee::QueryIteration,
                                result: item.clone(),
                            });
                            Some(Some(item.clone()))
                        }
                        Some(_) => {
                            // A compound source (a literal, block, or if) is
                            // materialized for its type; every failure path
                            // inside records its own gap or diagnostic.
                            let source =
                                iterator_value
                                    .as_ref()
                                    .and_then(|value| match &value.input {
                                        TypedExpressionInput::Known(ty) => Some(ty.clone()),
                                        _ => self
                                            .materialize(value, None, iterator.span)
                                            .map(|checked| checked.ty().clone()),
                                    });
                            match source {
                                Some(source) => {
                                    match self.lower_ordinary_for_items(&source, iterator.span) {
                                        Some(Some((_, item))) => Some(Some(item)),
                                        // A definitive ambiguity rejection was
                                        // minted for this loop.
                                        Some(None) => {
                                            complete = false;
                                            Some(None)
                                        }
                                        None => {
                                            self.gap(
                                                iterator.span,
                                                BodyCheckIncompletenessKind::MissingMethodSelection,
                                                "ordinary `for` has no authoritative IntoIterator/Iterator selection among the target's ordinary impls",
                                            );
                                            complete = false;
                                            None
                                        }
                                    }
                                }
                                None => {
                                    complete = false;
                                    None
                                }
                            }
                        }
                        None => {
                            complete = false;
                            None
                        }
                    };
                    if let Some(Some(item_type)) = &item_type {
                        self.check_and_bind_irrefutable(
                            pattern,
                            item_type,
                            PlaceMutability::Mutable,
                        );
                    }
                    if matches!(item_type, Some(None)) {
                        // The ambiguity rejection stands for this loop; do not
                        // manufacture missing-local gaps from a body whose
                        // binding cannot have a typed selection.
                    } else {
                        let (lowered_body, _swallowed) =
                            self.lower_loop_body(body, SourceLoopKind::For);
                        if lowered_body.is_none() {
                            complete = false;
                        }
                        if let (Some(Some(_)), Some(body)) = (item_type, lowered_body) {
                            // `for` has while-like break/continue typing and
                            // unit result.
                            statements.push(TypedExpressionInput::While {
                                condition: Box::new(TypedExpressionInput::Boolean(true)),
                                body: Box::new(body.input),
                            });
                        }
                    }
                }
                AstStatementKind::Assignment {
                    place,
                    operator,
                    value,
                } => {
                    let place_value = self.lower_expression(place, None);
                    let place_type = place_value.as_ref().and_then(|place_value| {
                        self.materialize(place_value, None, place.span)
                            .map(|checked| checked.ty().clone())
                    });
                    let value = place_type
                        .as_ref()
                        .and_then(|place_type| self.lower_expression(value, Some(place_type)));
                    if place_type.is_none() || value.is_none() {
                        complete = false;
                    }
                    if let (Some(place_type), Some(value)) = (place_type, value) {
                        let assignment = match operator {
                            AstAssignmentOperator::Assign => TypedExpressionInput::Assignment {
                                place_type,
                                value: Box::new(value.input),
                            },
                            AstAssignmentOperator::AddAssign => {
                                TypedExpressionInput::AddAssignment {
                                    place_type,
                                    value: Box::new(value.input),
                                }
                            }
                        };
                        let lowered = LoweredValue::ordinary(assignment);
                        if self.materialize(&lowered, None, statement.span).is_some() {
                            statements.push(lowered.input);
                        } else {
                            // Validate compound operators at their retained statement span.
                            // Otherwise a recursive block check would attach the operator
                            // failure to the entire enclosing block and obscure its source.
                            complete = false;
                        }
                    }
                }
                AstStatementKind::Expression { expression, .. } => {
                    if let Some(value) = self.lower_expression(expression, None) {
                        statements.push(value.input);
                    } else {
                        complete = false;
                    }
                }
            }
        }
        let tail = match block.tail.as_deref() {
            Some(tail) => match self.lower_expression(tail, expected) {
                Some(tail) => Some(Box::new(tail.input)),
                None => {
                    complete = false;
                    None
                }
            },
            None => None,
        };
        complete.then(|| LoweredValue::ordinary(TypedExpressionInput::Block { statements, tail }))
    }

    fn lower_loop_body(
        &mut self,
        block: &AstBlock,
        kind: SourceLoopKind,
    ) -> (Option<LoweredValue>, bool) {
        self.source_loops.push(SourceLoopFrame {
            kind,
            swallowed: false,
        });
        let lowered = self.lower_block(block, None);
        let frame = self
            .source_loops
            .pop()
            .expect("source loop frame was pushed");
        (lowered, frame.swallowed)
    }

    fn lower_if(
        &mut self,
        span: Span,
        if_: &arche_frontend::ast::AstIfExpression,
        expected: Option<&SymbolicType>,
    ) -> Option<LoweredValue> {
        let condition = match &if_.condition {
            AstCondition::Expression(condition) => self
                .lower_expression(condition, None)
                .map(|value| value.input),
            AstCondition::Let { pattern, value } => {
                let value_lowered = self.lower_expression(value, None);
                let checked = value_lowered
                    .as_ref()
                    .and_then(|lowered| self.materialize(lowered, None, value.span));
                if let Some(checked) = checked {
                    self.check_and_bind_refutable(pattern, checked.ty(), PlaceMutability::Mutable);
                    Some(TypedExpressionInput::Boolean(true))
                } else {
                    None
                }
            }
        };
        let then_branch = self.lower_block(&if_.then_block, expected);
        let else_branch = if_.else_branch.as_ref().map(|branch| match branch {
            AstElseBranch::Block(block) => self.lower_block(block, expected),
            AstElseBranch::If(expression) => self.lower_expression(expression, expected),
        });
        let complete = condition.is_some()
            && then_branch.is_some()
            && else_branch.as_ref().is_none_or(Option::is_some);
        if !complete {
            return None;
        }
        let _ = span;
        Some(LoweredValue::ordinary(TypedExpressionInput::If {
            condition: Box::new(condition.expect("complete condition")),
            then_branch: Box::new(then_branch.expect("complete branch").input),
            else_branch: else_branch.flatten().map(|branch| Box::new(branch.input)),
        }))
    }

    fn lower_while(
        &mut self,
        _span: Span,
        while_: &arche_frontend::ast::AstWhileExpression,
        _expected: Option<&SymbolicType>,
    ) -> Option<LoweredValue> {
        let condition = match &while_.condition {
            AstCondition::Expression(condition) => self
                .lower_expression(condition, None)
                .map(|value| value.input),
            AstCondition::Let { pattern, value } => {
                let value_lowered = self.lower_expression(value, None);
                let checked = value_lowered
                    .as_ref()
                    .and_then(|lowered| self.materialize(lowered, None, value.span));
                if let Some(checked) = checked {
                    self.check_and_bind_refutable(pattern, checked.ty(), PlaceMutability::Mutable);
                    Some(TypedExpressionInput::Boolean(true))
                } else {
                    None
                }
            }
        };
        let (body, _swallowed) = self.lower_loop_body(&while_.body, SourceLoopKind::While);
        let (Some(condition), Some(body)) = (condition, body) else {
            return None;
        };
        Some(LoweredValue::ordinary(TypedExpressionInput::While {
            condition: Box::new(condition),
            body: Box::new(body.input),
        }))
    }

    fn lower_match(
        &mut self,
        span: Span,
        operand: &AstExpression,
        arms: &[AstMatchArm],
        expected: Option<&SymbolicType>,
        catch: bool,
    ) -> Option<LoweredValue> {
        let operand_value = self.lower_expression(operand, None);
        let operand_checked = operand_value
            .as_ref()
            .and_then(|value| self.materialize(value, None, operand.span));
        let Some(operand_checked) = operand_checked else {
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    self.walk_expression(guard);
                }
                self.walk_expression(&arm.value);
            }
            return None;
        };
        let scrutinee_ty = if catch {
            match self
                .catch_operand_throws(operand)
                .filter(|members| !members.iter().any(symbolic_type_mentions_bound))
                .as_deref()
            {
                Some([thrown]) => {
                    // The declared singleton throws set types the catch arms;
                    // canonical escaping-set accounting stays C4 authority.
                    self.pending_c4(
                        span,
                        b"catch-canonical-throws-set".to_vec(),
                        "catch escaping-set accounting is finalized by C4",
                    );
                    Some(thrown.clone())
                }
                _ => {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingEffectAuthority,
                        "catch scrutinee needs one resolved declared throws type; wider sets await the C4 canonical throws-set authority",
                    );
                    None
                }
            }
        } else {
            Some(operand_checked.ty().clone())
        };
        let patterns_valid = match &scrutinee_ty {
            Some(ty) => self.check_match_patterns(ty, arms, operand.span),
            None => false,
        };

        let mut joined = expected.cloned();
        let mut complete = patterns_valid;
        if catch && scrutinee_ty.is_some() {
            // The non-throwing path yields the operand's result, so the catch
            // expression's type joins it with every arm value.
            let operand_ty = operand_checked.ty();
            match &joined {
                // A never-typed operand is the join identity: it constrains
                // nothing and must not poison the arm join.
                None if operand_ty != &SymbolicType::Never => {
                    joined = Some(operand_ty.clone());
                }
                None => {}
                Some(expected_ty) => {
                    let outlives = LifetimeOutlives::new([]);
                    let accepted = expected_ty == operand_ty
                        || types_match_with_erased_body_lifetime(expected_ty, operand_ty)
                        || classify_coercion(operand_ty, expected_ty, &outlives).is_some();
                    if !accepted {
                        self.source_error(
                            operand.span,
                            "TYPE002",
                            format!(
                                "expected {}, found {}",
                                crate::golden::spell_symbolic_type(expected_ty),
                                crate::golden::spell_symbolic_type(operand_ty)
                            ),
                        );
                        complete = false;
                    }
                }
            }
        }
        let mut all_arms_diverge = !arms.is_empty();
        for arm in arms {
            if !patterns_valid {
                // A definitive pattern diagnostic already rejects this body.
                // Do not manufacture missing-local gaps from an arm whose
                // bindings cannot have a successful typed-pattern analysis.
                continue;
            }
            let Some(scrutinee) = &scrutinee_ty else {
                continue;
            };
            self.bind_refutable_arm(&arm.pattern, scrutinee);
            if let Some(guard) = &arm.guard {
                let bool_type = SymbolicType::Bool;
                if self.check_expression(guard, Some(&bool_type)).is_none() {
                    complete = false;
                }
            }
            let value = self.lower_expression(&arm.value, joined.as_ref());
            // A diverging arm contributes no value: it is typed on its own
            // and never joined, so a unit-typed block ending in return or
            // throw cannot fabricate a mismatch against the joined type.
            let arm_diverges = value
                .as_ref()
                .is_some_and(|value| input_diverges(&value.input));
            let expected_arm = if arm_diverges { None } else { joined.as_ref() };
            let checked = value
                .as_ref()
                .and_then(|value| self.materialize(value, expected_arm, arm.value.span));
            if let Some(checked) = checked {
                // An arm whose checked type is never (a bare loop, say) is
                // also diverging: it must not seed the join, or later
                // value-carrying arms would fabricate mismatches against it.
                let arm_never = arm_diverges || checked.ty() == &SymbolicType::Never;
                all_arms_diverge &= arm_never;
                if !arm_never && joined.is_none() {
                    joined = Some(checked.ty().clone());
                }
            } else {
                complete = false;
            }
        }
        if !complete {
            return None;
        }
        if all_arms_diverge {
            // Every arm diverges. A plain match can then never complete
            // normally and joins to the never type — but a typed catch still
            // completes through its non-throwing path, whose seeded operand
            // result remains the expression's type.
            let result = if catch && scrutinee_ty.is_some() {
                joined.unwrap_or(SymbolicType::Never)
            } else {
                SymbolicType::Never
            };
            return Some(LoweredValue::ordinary(TypedExpressionInput::Known(result)));
        }
        joined.map(|ty| LoweredValue::ordinary(TypedExpressionInput::Known(ty)))
    }

    /// Reads the declared throws set of a catch operand: a direct call whose
    /// callee is a resolved item with a resolved declared throws list, or a
    /// local of function-pointer type. Anything else is `None` and keeps the
    /// caller's fail-closed gap.
    fn catch_operand_throws(&self, operand: &AstExpression) -> Option<Vec<SymbolicType>> {
        let AstExpressionKind::Postfix { base, parts } = &operand.kind else {
            return None;
        };
        let [part] = parts.as_slice() else {
            return None;
        };
        let AstPostfixKind::Call(_) = &part.kind else {
            return None;
        };
        let AstExpressionKind::Path(path) = &base.kind else {
            return None;
        };
        let resolution = self
            .scope
            .target
            .path_resolutions
            .iter()
            .find(|resolution| resolution.span == path.span)?;
        if resolution.unresolved.is_some() {
            return None;
        }
        match resolution.resolutions.as_slice() {
            [Res::Item(HirItemRes::Definition(item))] => {
                let entry = self.catalog.definitions.get(item)?;
                let shape = entry.declaration_shape?;
                let SymbolicDeclarationPayloadSkeleton::Callable(callable) = &shape.payload else {
                    return None;
                };
                callable
                    .effects
                    .throws
                    .iter()
                    .map(|effect| match effect {
                        arche_frontend::SymbolicEffectShapeSkeleton::Resolved { value, .. } => {
                            Some(value.clone())
                        }
                        arche_frontend::SymbolicEffectShapeSkeleton::Pending(_) => None,
                    })
                    .collect()
            }
            [Res::Local(local)] => {
                let LocalValue::Typed(SymbolicType::FunctionPointer { throws, .. }) =
                    self.cross_body_locals.get(local)?
                else {
                    return None;
                };
                Some(throws.members().to_vec())
            }
            _ => None,
        }
    }

    fn pending_c4(&mut self, span: Span, mut bytes: Vec<u8>, detail: &'static str) {
        bytes.extend_from_slice(&self.scope.body.id.0.to_le_bytes());
        bytes.extend_from_slice(&span.start.byte.to_le_bytes());
        match PendingC4Dependency::from_canonical_bytes(bytes) {
            Ok(dependency) => self.pending_c4.push(dependency),
            Err(_) => self.gap(
                span,
                BodyCheckIncompletenessKind::MissingEffectAuthority,
                detail,
            ),
        }
    }

    fn const_at_span(&mut self, span: Span) -> Option<SymbolicConstExpression> {
        let rows = self
            .scope
            .target
            .bodies
            .iter()
            .filter(|body| {
                body.owner == self.scope.item.id
                    && matches!(
                        body.kind,
                        SemanticBodyKind::ArrayLength
                            | SemanticBodyKind::RepeatCount
                            | SemanticBodyKind::IntegerGenericArgument
                    )
            })
            .collect::<Vec<_>>();
        let values = self
            .scope
            .item
            .symbolic_shape
            .consts
            .iter()
            .chain(&self.scope.item.body_symbolic_shape.consts)
            .collect::<Vec<_>>();
        if rows.len() != values.len() {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "const-expression body rows differ from symbolic const rows",
            );
            return None;
        }
        let Some((_, value)) = rows
            .into_iter()
            .zip(values)
            .find(|(body, _)| body.span == span)
        else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "const expression span has no symbolic const row",
            );
            return None;
        };
        match value {
            arche_frontend::ResolvedSymbolicConst::Resolved(value) => {
                self.collect_const_ctfe(value);
                Some(value.clone())
            }
            arche_frontend::ResolvedSymbolicConst::Pending {
                reason, canonical, ..
            } => {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::PendingC2Type,
                    format!("pending const {reason:?}: `{canonical}`"),
                );
                None
            }
        }
    }

    fn collect_const_ctfe(&mut self, value: &SymbolicConstExpression) {
        let mut paths = BTreeSet::new();
        collect_const_paths(value, &mut paths);
        for path in paths {
            let mut bytes = b"ARCHE-C2-BODY-CONST\0".to_vec();
            encode_declaration_path_canonical(&mut bytes, &path);
            if let Ok(obligation) = NeedsCtfeObligation::from_canonical_bytes(bytes) {
                self.ctfe.push(obligation);
            }
        }
    }
}

fn nominal_type(entry: &DefinitionEntry<'_>, arguments: Vec<GenericArgumentShape>) -> SymbolicType {
    SymbolicType::NominalPath {
        declaration: entry.semantic_path(),
        arguments,
    }
}

fn constructor_is_unit(
    declaration: &SymbolicDeclarationShapeSkeleton,
    variant: Option<u64>,
) -> bool {
    match (&declaration.payload, variant) {
        (SymbolicDeclarationPayloadSkeleton::Tag, None) => true,
        (SymbolicDeclarationPayloadSkeleton::Record(record), None) => {
            record.form == SymbolicRecordForm::Unit
        }
        (SymbolicDeclarationPayloadSkeleton::Enum(variants), Some(ordinal)) => {
            usize::try_from(ordinal)
                .ok()
                .and_then(|ordinal| variants.get(ordinal))
                .is_some_and(|variant| variant.form == SymbolicRecordForm::Unit)
        }
        _ => false,
    }
}

fn substitute_type(ty: &SymbolicType, actuals: &[GenericArgumentShape]) -> SymbolicType {
    match ty {
        SymbolicType::BoundType { depth: 0, index } => usize::try_from(*index)
            .ok()
            .and_then(|index| actuals.get(index))
            .and_then(|argument| match argument {
                GenericArgumentShape::Type(ty) => Some(ty.clone()),
                GenericArgumentShape::Lifetime(_) | GenericArgumentShape::IntegerConst(_) => None,
            })
            .unwrap_or_else(|| ty.clone()),
        SymbolicType::Slice(element) => {
            SymbolicType::Slice(Box::new(substitute_type(element, actuals)))
        }
        SymbolicType::Array { element, length } => SymbolicType::Array {
            element: Box::new(substitute_type(element, actuals)),
            length: substitute_const(length, actuals),
        },
        SymbolicType::Tuple(elements) => SymbolicType::Tuple(
            elements
                .iter()
                .map(|element| substitute_type(element, actuals))
                .collect(),
        ),
        SymbolicType::Reference {
            mutability,
            lifetime,
            pointee,
        } => SymbolicType::Reference {
            mutability: *mutability,
            lifetime: substitute_lifetime(lifetime, actuals),
            pointee: Box::new(substitute_type(pointee, actuals)),
        },
        SymbolicType::RawPointer {
            mutability,
            pointee,
        } => SymbolicType::RawPointer {
            mutability: *mutability,
            pointee: Box::new(substitute_type(pointee, actuals)),
        },
        SymbolicType::NominalPath {
            declaration,
            arguments,
        } => SymbolicType::NominalPath {
            declaration: declaration.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute_argument(argument, actuals))
                .collect(),
        },
        SymbolicType::FunctionPointer {
            unsafe_,
            parameters,
            result,
            requires,
            throws,
        } => SymbolicType::FunctionPointer {
            unsafe_: *unsafe_,
            parameters: parameters
                .iter()
                .map(|parameter| substitute_type(parameter, actuals))
                .collect(),
            result: Box::new(substitute_type(result, actuals)),
            requires: SymbolicTypeEffectSet::pending_c4(
                requires
                    .members()
                    .iter()
                    .map(|member| substitute_type(member, actuals))
                    .collect(),
            ),
            throws: SymbolicTypeEffectSet::pending_c4(
                throws
                    .members()
                    .iter()
                    .map(|member| substitute_type(member, actuals))
                    .collect(),
            ),
        },
        _ => ty.clone(),
    }
}

fn substitute_argument(
    argument: &GenericArgumentShape,
    actuals: &[GenericArgumentShape],
) -> GenericArgumentShape {
    match argument {
        GenericArgumentShape::Type(ty) => GenericArgumentShape::Type(substitute_type(ty, actuals)),
        GenericArgumentShape::Lifetime(lifetime) => {
            GenericArgumentShape::Lifetime(substitute_lifetime(lifetime, actuals))
        }
        GenericArgumentShape::IntegerConst(value) => {
            GenericArgumentShape::IntegerConst(substitute_const(value, actuals))
        }
    }
}

fn substitute_lifetime(
    lifetime: &SymbolicLifetime,
    actuals: &[GenericArgumentShape],
) -> SymbolicLifetime {
    match lifetime {
        SymbolicLifetime::Bound { depth: 0, index } => usize::try_from(*index)
            .ok()
            .and_then(|index| actuals.get(index))
            .and_then(|argument| match argument {
                GenericArgumentShape::Lifetime(lifetime) => Some(lifetime.clone()),
                GenericArgumentShape::Type(_) | GenericArgumentShape::IntegerConst(_) => None,
            })
            .unwrap_or_else(|| lifetime.clone()),
        _ => lifetime.clone(),
    }
}

fn substitute_const(
    value: &SymbolicConstExpression,
    actuals: &[GenericArgumentShape],
) -> SymbolicConstExpression {
    if let SymbolicConstNode::Bound { depth: 0, index } = &value.node {
        if let Some(GenericArgumentShape::IntegerConst(actual)) = usize::try_from(*index)
            .ok()
            .and_then(|index| actuals.get(index))
        {
            return actual.clone();
        }
    }
    value.clone()
}

impl BodyChecker<'_, '_, '_> {
    fn check_and_bind_irrefutable(
        &mut self,
        pattern: &AstPattern,
        ty: &SymbolicType,
        mutability: PlaceMutability,
    ) {
        if self.bind_simple_irrefutable_pattern(pattern, ty) {
            return;
        }
        self.reset_pattern_symbolic();
        let Some(pattern_ty) = self.pattern_type(ty, pattern.span) else {
            return;
        };
        let Some(lowered) = self.lower_pattern(pattern, &pattern_ty) else {
            return;
        };
        let scrutinee = PatternScrutinee::new(pattern_ty, mutability);
        match check_irrefutable_pattern(&scrutinee, &lowered) {
            Ok(analysis) => {
                self.collect_irrefutable_ctfe(&analysis);
                self.bind_pattern_locals(pattern, irrefutable_typed_pattern(&analysis));
                self.patterns.push(CheckedBodyPattern {
                    span: pattern.span,
                    analysis: CheckedBodyPatternAnalysis::Irrefutable(analysis),
                });
            }
            Err(errors) => self.pattern_errors_simple(pattern.span, errors),
        }
    }

    fn bind_simple_irrefutable_pattern(&mut self, pattern: &AstPattern, ty: &SymbolicType) -> bool {
        match &pattern.kind {
            AstPatternKind::Wildcard => true,
            AstPatternKind::Binding {
                name,
                by_reference,
                reference_mutable,
                ..
            } => {
                let ty = if *by_reference {
                    SymbolicType::Reference {
                        mutability: if *reference_mutable {
                            Mutability::Mutable
                        } else {
                            Mutability::Shared
                        },
                        lifetime: SymbolicLifetime::ErasedLocal,
                        pointee: Box::new(ty.clone()),
                    }
                } else {
                    ty.clone()
                };
                let value = self.local_value_for_bound_type(ty);
                self.bind_named_local(name.as_str(), pattern.span, value);
                true
            }
            AstPatternKind::BarePathOrBinding(path) if self.bare_pattern_is_binding(path) => {
                let Some(name) = path.segments.last() else {
                    return false;
                };
                let value = self.local_value_for_bound_type(ty.clone());
                self.bind_named_local(name.name.as_str(), pattern.span, value);
                true
            }
            AstPatternKind::Unit
            | AstPatternKind::Literal(_)
            | AstPatternKind::BarePathOrBinding(_)
            | AstPatternKind::Reference { .. }
            | AstPatternKind::Tuple(_)
            | AstPatternKind::Slice(_)
            | AstPatternKind::Constructor { .. }
            | AstPatternKind::Range { .. }
            | AstPatternKind::At { .. }
            | AstPatternKind::Or(_) => false,
        }
    }

    fn local_value_for_bound_type(&self, ty: SymbolicType) -> LocalValue {
        let is_commands = match peel_references(&ty) {
            SymbolicType::NominalPath { declaration, .. } => {
                self.embedded_nominal_kind(declaration) == Some(CompilerNominalKind::Commands)
            }
            _ => false,
        };
        if is_commands {
            LocalValue::Commands
        } else {
            LocalValue::Typed(ty)
        }
    }

    fn bare_pattern_is_binding(&self, path: &arche_frontend::ast::AstPath) -> bool {
        let Some(name) = path.segments.last().map(|segment| segment.name.as_str()) else {
            return false;
        };
        self.scope
            .item
            .locals
            .iter()
            .filter(|local| local.name == name && local.span == path.span)
            .count()
            == 1
    }

    fn retained_pattern_binding(&self, pattern: &AstPattern) -> Option<PatternBinding> {
        match &pattern.kind {
            AstPatternKind::BarePathOrBinding(path) => {
                let name = path.segments.last()?.name.as_str();
                (self.bare_pattern_is_binding(path) || self.or_alias_is_binding(name))
                    .then(|| PatternBinding::inferred(name))
            }
            AstPatternKind::Binding {
                name,
                mutable,
                by_reference,
                reference_mutable,
            } => Some(PatternBinding::new(
                name.as_str(),
                if *by_reference {
                    if *reference_mutable {
                        BindingAnnotation::RefMut
                    } else {
                        BindingAnnotation::Ref
                    }
                } else {
                    BindingAnnotation::Inferred
                },
                *mutable,
            )),
            _ => None,
        }
    }

    fn or_alias_is_binding(&self, name: &str) -> bool {
        let Some(span) = self
            .or_binding_aliases
            .last()
            .and_then(|bindings| bindings.get(name))
        else {
            return false;
        };
        self.scope
            .item
            .locals
            .iter()
            .filter(|local| local.name == name && local.span == *span)
            .count()
            == 1
    }

    fn check_and_bind_refutable(
        &mut self,
        pattern: &AstPattern,
        ty: &SymbolicType,
        mutability: PlaceMutability,
    ) {
        self.reset_pattern_symbolic();
        let Some(pattern_ty) = self.pattern_type(ty, pattern.span) else {
            return;
        };
        let Some(lowered) = self.lower_pattern(pattern, &pattern_ty) else {
            return;
        };
        let arms = [
            PatternArm::new(lowered.clone(), false),
            PatternArm::new(Pattern::Wildcard, false),
        ];
        let scrutinee = PatternScrutinee::new(pattern_ty, mutability);
        match analyze_pattern_match(&scrutinee, &arms) {
            Ok(analysis) => {
                self.collect_match_ctfe(&analysis);
                if let Some(typed) = match_first_typed_pattern(&analysis) {
                    self.bind_pattern_locals(pattern, typed);
                }
                self.patterns.push(CheckedBodyPattern {
                    span: pattern.span,
                    analysis: CheckedBodyPatternAnalysis::Refutable(analysis),
                });
            }
            Err(errors) if helper_wildcard_unreachable(&errors) => {
                // An irrefutable pattern in a refutable position covers every
                // value; analyze it as irrefutable rather than minting an
                // unreachable-arm rejection for the helper wildcard row.
                match check_irrefutable_pattern(&scrutinee, &lowered) {
                    Ok(analysis) => {
                        self.collect_irrefutable_ctfe(&analysis);
                        self.bind_pattern_locals(pattern, irrefutable_typed_pattern(&analysis));
                        self.patterns.push(CheckedBodyPattern {
                            span: pattern.span,
                            analysis: CheckedBodyPatternAnalysis::Irrefutable(analysis),
                        });
                    }
                    Err(errors) => self.pattern_errors_simple(pattern.span, errors),
                }
            }
            Err(errors) => self.pattern_errors_simple(pattern.span, errors),
        }
    }

    fn check_match_patterns(
        &mut self,
        ty: &SymbolicType,
        arms: &[AstMatchArm],
        span: Span,
    ) -> bool {
        self.reset_pattern_symbolic();
        let Some(pattern_ty) = self.pattern_type(ty, span) else {
            return false;
        };
        let mut lowered = Vec::new();
        for arm in arms {
            let Some(pattern) = self.lower_pattern(&arm.pattern, &pattern_ty) else {
                continue;
            };
            lowered.push(PatternArm::new(pattern, arm.guard.is_some()));
        }
        if lowered.len() != arms.len() {
            return false;
        }
        let scrutinee = PatternScrutinee::new(pattern_ty, PlaceMutability::Mutable);
        match analyze_pattern_match(&scrutinee, &lowered) {
            Ok(analysis) => {
                self.collect_match_ctfe(&analysis);
                self.patterns.push(CheckedBodyPattern {
                    span,
                    analysis: CheckedBodyPatternAnalysis::Refutable(analysis),
                });
                true
            }
            Err(errors) => {
                self.pattern_errors(span, arms, errors);
                false
            }
        }
    }

    fn bind_refutable_arm(&mut self, pattern: &AstPattern, ty: &SymbolicType) {
        self.reset_pattern_symbolic();
        let Some(pattern_ty) = self.pattern_type(ty, pattern.span) else {
            return;
        };
        let Some(lowered) = self.lower_pattern(pattern, &pattern_ty) else {
            return;
        };
        let arms = [
            PatternArm::new(lowered.clone(), false),
            PatternArm::new(Pattern::Wildcard, false),
        ];
        let scrutinee = PatternScrutinee::new(pattern_ty, PlaceMutability::Mutable);
        match analyze_pattern_match(&scrutinee, &arms) {
            Ok(analysis) => {
                if let Some(typed) = match_first_typed_pattern(&analysis) {
                    self.bind_pattern_locals(pattern, typed);
                }
            }
            Err(errors) if helper_wildcard_unreachable(&errors) => {
                // The arm is irrefutable, so the helper wildcard row is
                // unreachable; recover the bindings through the irrefutable
                // analysis instead of silently dropping them.
                if let Ok(analysis) = check_irrefutable_pattern(&scrutinee, &lowered) {
                    self.bind_pattern_locals(pattern, irrefutable_typed_pattern(&analysis));
                }
            }
            Err(_) => {}
        }
    }

    fn pattern_errors_simple(&mut self, span: Span, errors: PatternErrors) {
        for error in errors.as_slice() {
            self.source_error(span, error.code().as_str(), error.message());
        }
    }

    fn pattern_type(&mut self, ty: &SymbolicType, span: Span) -> Option<PatternType> {
        let output = match ty {
            SymbolicType::Unit => PatternType::Unit,
            SymbolicType::F32 => PatternType::Float(PatternFloatType::F32),
            SymbolicType::F64 => PatternType::Float(PatternFloatType::F64),
            SymbolicType::BoundType { depth, index } => {
                PatternType::Opaque(format!("bound-type:{depth}#{index}").into())
            }
            SymbolicType::Bool => PatternType::Bool,
            SymbolicType::Char => PatternType::Char,
            SymbolicType::I8 => PatternType::Integer(PatternIntegerType::Signed(8)),
            SymbolicType::I16 => PatternType::Integer(PatternIntegerType::Signed(16)),
            SymbolicType::I32 => PatternType::Integer(PatternIntegerType::Signed(32)),
            SymbolicType::I64 | SymbolicType::Isize => {
                PatternType::Integer(PatternIntegerType::Signed(64))
            }
            SymbolicType::U8 => PatternType::Integer(PatternIntegerType::Unsigned(8)),
            SymbolicType::U16 => PatternType::Integer(PatternIntegerType::Unsigned(16)),
            SymbolicType::U32 => PatternType::Integer(PatternIntegerType::Unsigned(32)),
            SymbolicType::U64 | SymbolicType::Usize => {
                PatternType::Integer(PatternIntegerType::Unsigned(64))
            }
            SymbolicType::Tuple(fields) => PatternType::tuple(
                fields
                    .iter()
                    .map(|field| self.pattern_type(field, span))
                    .collect::<Option<Vec<_>>>()?,
            ),
            SymbolicType::Array { element, length } => {
                let element = self.pattern_type(element, span)?;
                if let Some(length) = const_literal_usize(length) {
                    PatternType::array(element, length)
                } else {
                    PatternType::symbolic_array(
                        element,
                        PatternConst::new(
                            symbolic_const_dependency_string(length),
                            PatternType::Integer(PatternIntegerType::Unsigned(64)),
                        ),
                    )
                }
            }
            SymbolicType::Slice(element) => PatternType::slice(self.pattern_type(element, span)?),
            SymbolicType::Str => PatternType::Str,
            SymbolicType::Reference {
                mutability,
                pointee,
                ..
            } => PatternType::reference(
                match mutability {
                    Mutability::Shared => ReferenceMutability::Shared,
                    Mutability::Mutable => ReferenceMutability::Mutable,
                },
                self.pattern_type(pointee, span)?,
            ),
            SymbolicType::NominalPath {
                declaration,
                arguments,
            } => {
                let Some(entry) = self.catalog.item_for_path(declaration) else {
                    if self.embedded_nominal_kind(declaration) == Some(CompilerNominalKind::String)
                    {
                        let output = PatternType::String;
                        self.register_pattern_symbolic(&output, ty);
                        return Some(output);
                    }
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                        format!(
                            "embedded nominal pattern type `{}` has no typed constructor/field descriptor",
                            declaration.name
                        ),
                    );
                    return None;
                };
                let declaration_shape =
                    checked_entry_shape(entry, self.scope.body.id, span, &mut self.gaps)?;
                let payload = declaration_shape.payload.clone();
                match payload {
                    SymbolicDeclarationPayloadSkeleton::Record(record) => {
                        let mut fields = Vec::new();
                        for (index, field) in record.fields.iter().enumerate() {
                            let field_ty = self.require_type_shape(&field.ty, span)?;
                            fields.push(RecordField::new(
                                field.name.clone().unwrap_or_else(|| index.to_string()),
                                self.pattern_type(&substitute_type(&field_ty, arguments), span)?,
                            ));
                        }
                        PatternType::Record(RecordType::new(declaration.name.clone(), fields))
                    }
                    SymbolicDeclarationPayloadSkeleton::Tag => {
                        PatternType::Record(RecordType::new(declaration.name.clone(), Vec::new()))
                    }
                    SymbolicDeclarationPayloadSkeleton::Enum(variants) => {
                        let mut pattern_variants = Vec::new();
                        for variant in variants {
                            let mut positional = Vec::new();
                            let mut record = Vec::new();
                            for (index, field) in variant.fields.iter().enumerate() {
                                let field_ty = self.require_type_shape(&field.ty, span)?;
                                let field_ty = self
                                    .pattern_type(&substitute_type(&field_ty, arguments), span)?;
                                positional.push(field_ty.clone());
                                record.push(RecordField::new(
                                    field.name.clone().unwrap_or_else(|| index.to_string()),
                                    field_ty,
                                ));
                            }
                            pattern_variants.push(if variant.form == SymbolicRecordForm::Record {
                                EnumVariant::record(variant.name, record)
                            } else {
                                EnumVariant::new(variant.name, positional)
                            });
                        }
                        PatternType::Enum(EnumType::new(declaration.name.clone(), pattern_variants))
                    }
                    _ => {
                        self.gap(
                            span,
                            BodyCheckIncompletenessKind::MissingPatternAlgebra,
                            format!(
                                "nominal `{}` is not a record, tag, or enum pattern domain",
                                declaration.name
                            ),
                        );
                        return None;
                    }
                }
            }
            other => {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingPatternAlgebra,
                    format!("pattern engine has no constructor space for {other:?}"),
                );
                return None;
            }
        };
        self.register_pattern_symbolic(&output, ty);
        Some(output)
    }

    fn register_pattern_symbolic(&mut self, pattern: &PatternType, symbolic: &SymbolicType) {
        if self.ambiguous_pattern_symbolic.contains(pattern) {
            return;
        }
        match self.pattern_symbolic.get(pattern) {
            Some(existing) if existing != symbolic => {
                self.pattern_symbolic.remove(pattern);
                self.ambiguous_pattern_symbolic.insert(pattern.clone());
            }
            Some(_) => {}
            None => {
                self.pattern_symbolic
                    .insert(pattern.clone(), symbolic.clone());
            }
        }
    }

    fn reset_pattern_symbolic(&mut self) {
        self.pattern_symbolic.clear();
        self.ambiguous_pattern_symbolic.clear();
    }

    fn embedded_nominal_kind(
        &self,
        declaration: &SemanticDeclarationPath,
    ) -> Option<CompilerNominalKind> {
        let core = &self.catalog.handoff.frontend().inventory().embedded_core;
        let projection = core.projection();
        if declaration.registry_origin != projection.registry_origin()
            || declaration.package_name != projection.scoped_name()
            || declaration.target != TargetRoot::Library
            || !declaration.modules.is_empty()
        {
            return None;
        }
        core.typed_c2().nominals().iter().find_map(|authority| {
            core.definition(authority.c1_definition())
                .filter(|row| row.name() == declaration.name)
                .map(|_| authority.kind())
        })
    }

    fn lower_pattern(&mut self, pattern: &AstPattern, ty: &PatternType) -> Option<Pattern> {
        match &pattern.kind {
            AstPatternKind::Wildcard => Some(Pattern::Wildcard),
            AstPatternKind::Unit => Some(Pattern::Unit),
            AstPatternKind::Literal(literal) => Some(Pattern::Literal(self.pattern_literal(
                literal,
                ty,
                pattern.span,
            )?)),
            AstPatternKind::BarePathOrBinding(path) => {
                if let Some(resolution) = self
                    .scope
                    .target
                    .path_resolutions
                    .iter()
                    .find(|resolution| resolution.span == path.span)
                {
                    if resolution.resolutions.len() == 1 {
                        match &resolution.resolutions[0] {
                            Res::Item(HirItemRes::EnumVariant { .. })
                            | Res::Builtin(arche_frontend::BuiltinRes {
                                target: BuiltinResTarget::EnumVariant(_),
                            }) => {
                                let name = path
                                    .segments
                                    .last()
                                    .map(|segment| segment.name.as_str())
                                    .unwrap_or_default();
                                return Some(Pattern::constructor(name, Vec::new()));
                            }
                            Res::Item(HirItemRes::NominalConstructor { .. }) => {
                                let name = path
                                    .segments
                                    .last()
                                    .map(|segment| segment.name.as_str())
                                    .unwrap_or_default();
                                return Some(Pattern::record(name, Vec::new()));
                            }
                            Res::Item(HirItemRes::Definition(item)) => {
                                let Some(entry) = self.catalog.definition(*item) else {
                                    self.gap(
                                        pattern.span,
                                        BodyCheckIncompletenessKind::MissingRetainedJoin,
                                        "bare pattern item has no declaration catalog row",
                                    );
                                    return None;
                                };
                                if entry.definition.key.kind == DeclarationKind::Const {
                                    let dependency =
                                        self.pattern_const_dependency(*item, ty, pattern.span)?;
                                    return Some(Pattern::Const(dependency));
                                }
                                let declaration_shape = checked_entry_shape(
                                    entry,
                                    self.scope.body.id,
                                    pattern.span,
                                    &mut self.gaps,
                                )?;
                                if constructor_is_unit(declaration_shape, None) {
                                    let name = path
                                        .segments
                                        .last()
                                        .map(|segment| segment.name.as_str())
                                        .unwrap_or_default();
                                    return Some(Pattern::record(name, Vec::new()));
                                }
                                self.source_error(
                                    pattern.span,
                                    "PATTERN001",
                                    "bare pattern path is neither a const nor a unit constructor",
                                );
                                return None;
                            }
                            _ => {}
                        }
                    } else if resolution.resolutions.len() > 1 {
                        self.source_error(
                            pattern.span,
                            "PATTERN001",
                            "bare pattern identifier has ambiguous value-namespace lookup",
                        );
                        return None;
                    }
                }
                let name = path
                    .segments
                    .last()
                    .map(|segment| segment.name.as_str())
                    .unwrap_or_default();
                Some(Pattern::Binding(PatternBinding::inferred(name)))
            }
            AstPatternKind::Binding {
                name,
                mutable,
                by_reference,
                reference_mutable,
            } => Some(Pattern::Binding(PatternBinding::new(
                name.as_str(),
                if *by_reference {
                    if *reference_mutable {
                        BindingAnnotation::RefMut
                    } else {
                        BindingAnnotation::Ref
                    }
                } else {
                    BindingAnnotation::Inferred
                },
                *mutable,
            ))),
            AstPatternKind::Reference { mutable, pattern } => {
                let child_ty = match ty {
                    PatternType::Reference { referent, .. } => referent.as_ref(),
                    _ => ty,
                };
                Some(Pattern::Reference {
                    mutability: if *mutable {
                        ReferenceMutability::Mutable
                    } else {
                        ReferenceMutability::Shared
                    },
                    pattern: Box::new(self.lower_pattern(pattern, child_ty)?),
                })
            }
            AstPatternKind::Tuple(fields) => {
                let expected = match ty {
                    PatternType::Tuple(expected) => Some(expected.as_ref()),
                    _ => None,
                };
                Some(Pattern::tuple(
                    fields
                        .iter()
                        .enumerate()
                        .map(|(index, field)| {
                            self.lower_pattern(
                                field,
                                expected
                                    .and_then(|expected| expected.get(index))
                                    .unwrap_or(ty),
                            )
                        })
                        .collect::<Option<Vec<_>>>()?,
                ))
            }
            AstPatternKind::Slice(parts) => {
                let element_ty = match ty {
                    PatternType::Array { element, .. }
                    | PatternType::SymbolicArray { element, .. } => element.as_ref(),
                    PatternType::Slice(element) => element.as_ref(),
                    _ => ty,
                };
                let rest = parts
                    .iter()
                    .position(|part| matches!(part, AstSlicePatternPart::Rest(_)));
                let split = rest.unwrap_or(parts.len());
                let prefix = parts[..split]
                    .iter()
                    .filter_map(|part| match part {
                        AstSlicePatternPart::Pattern(pattern) => {
                            self.lower_pattern(pattern, element_ty)
                        }
                        AstSlicePatternPart::Rest(_) => None,
                    })
                    .collect();
                let suffix = rest
                    .map(|rest| {
                        parts[rest + 1..]
                            .iter()
                            .filter_map(|part| match part {
                                AstSlicePatternPart::Pattern(pattern) => {
                                    self.lower_pattern(pattern, element_ty)
                                }
                                AstSlicePatternPart::Rest(_) => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Some(Pattern::slice(prefix, rest.is_some(), suffix))
            }
            AstPatternKind::Constructor { path, payload } => {
                let resolution = self.path_resolution(path.span)?.clone();
                if matches!(payload, AstConstructorPatternPayload::Unit) {
                    if let [Res::Item(HirItemRes::Definition(item))] =
                        resolution.resolutions.as_slice()
                    {
                        if self.catalog.definition(*item).is_some_and(|entry| {
                            entry.definition.key.kind == DeclarationKind::Const
                        }) {
                            return Some(Pattern::Const(self.pattern_const_dependency(
                                *item,
                                ty,
                                pattern.span,
                            )?));
                        }
                    }
                }
                if resolution.resolutions.len() != 1
                    || !matches!(
                        resolution.resolutions[0],
                        Res::Item(HirItemRes::NominalConstructor { .. })
                            | Res::Item(HirItemRes::EnumVariant { .. })
                            | Res::Builtin(arche_frontend::BuiltinRes {
                                target: BuiltinResTarget::RecordConstructor(_)
                                    | BuiltinResTarget::EnumVariant(_),
                            })
                    )
                {
                    self.source_error(
                        pattern.span,
                        "PATTERN001",
                        "constructor pattern path is not one retained constructor",
                    );
                    return None;
                }
                let name = path
                    .segments
                    .last()
                    .map(|segment| segment.name.as_str())
                    .unwrap_or_default();
                let (field_types, record_names, is_record_type) =
                    pattern_constructor_fields(ty, name);
                match payload {
                    AstConstructorPatternPayload::Unit => {
                        if is_record_type {
                            Some(Pattern::record(name, Vec::new()))
                        } else {
                            Some(Pattern::constructor(name, Vec::new()))
                        }
                    }
                    AstConstructorPatternPayload::Tuple(fields) => {
                        let lowered = fields
                            .iter()
                            .enumerate()
                            .map(|(index, field)| {
                                self.lower_pattern(
                                    field,
                                    field_types.get(index).copied().unwrap_or(ty),
                                )
                            })
                            .collect::<Option<Vec<_>>>()?;
                        if is_record_type {
                            Some(Pattern::record(
                                name,
                                lowered
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, pattern)| {
                                        RecordPatternField::new(index.to_string(), pattern)
                                    })
                                    .collect(),
                            ))
                        } else {
                            Some(Pattern::constructor(name, lowered))
                        }
                    }
                    AstConstructorPatternPayload::Record(fields) => Some(Pattern::record(
                        name,
                        fields
                            .iter()
                            .map(|field| {
                                let field_name = field.name.as_str();
                                let field_ty = record_names
                                    .iter()
                                    .position(|name| *name == field_name)
                                    .and_then(|index| field_types.get(index).copied())
                                    .unwrap_or(ty);
                                Some(RecordPatternField::new(
                                    field_name,
                                    self.lower_pattern(&field.pattern, field_ty)?,
                                ))
                            })
                            .collect::<Option<Vec<_>>>()?,
                    )),
                }
            }
            AstPatternKind::Range {
                inclusive,
                start,
                end,
            } => Some(Pattern::Range {
                start: self.range_endpoint(start, ty, pattern.span)?,
                end: self.range_endpoint(end, ty, pattern.span)?,
                inclusive: *inclusive,
            }),
            AstPatternKind::At {
                binding,
                pattern: child,
            } => {
                let Some(binding) = self.retained_pattern_binding(binding) else {
                    self.source_error(
                        binding.span,
                        "PATTERN001",
                        "left side of @ is not a binding",
                    );
                    return None;
                };
                Some(Pattern::At {
                    binding,
                    pattern: Box::new(self.lower_pattern(child, ty)?),
                })
            }
            AstPatternKind::Or(alternatives) => {
                let mut aliases = BTreeMap::new();
                if let Some(first) = alternatives.first() {
                    collect_binding_spans(first, &mut aliases);
                }
                self.or_binding_aliases.push(aliases);
                let mut lowered = Vec::with_capacity(alternatives.len());
                let mut complete = true;
                for alternative in alternatives {
                    match self.lower_pattern(alternative, ty) {
                        Some(pattern) => lowered.push(pattern),
                        None => complete = false,
                    }
                }
                self.or_binding_aliases.pop();
                complete.then(|| Pattern::or(lowered))
            }
        }
    }

    fn pattern_literal(
        &mut self,
        literal: &arche_frontend::ast::AstPatternLiteral,
        ty: &PatternType,
        span: Span,
    ) -> Option<PatternLiteral> {
        match literal {
            arche_frontend::ast::AstPatternLiteral::Integer { negative, literal } => {
                integer_pattern_literal(literal, *negative, ty)
                    .map_err(|message| {
                        self.source_error(span, "PATTERN001", message);
                    })
                    .ok()
            }
            arche_frontend::ast::AstPatternLiteral::Character(value) => {
                Some(PatternLiteral::Char(*value))
            }
            arche_frontend::ast::AstPatternLiteral::Boolean(value) => {
                Some(PatternLiteral::Bool(*value))
            }
            arche_frontend::ast::AstPatternLiteral::String(value) => {
                Some(PatternLiteral::String(value.as_ref().into()))
            }
        }
    }

    fn range_endpoint(
        &mut self,
        endpoint: &AstRangeEndpoint,
        ty: &PatternType,
        span: Span,
    ) -> Option<RangeEndpoint> {
        match endpoint {
            AstRangeEndpoint::Integer {
                negative, literal, ..
            } => Some(RangeEndpoint::Literal(
                integer_pattern_literal(literal, *negative, ty)
                    .map_err(|message| self.source_error(span, "PATTERN001", message))
                    .ok()?,
            )),
            AstRangeEndpoint::Character { value, .. } => {
                Some(RangeEndpoint::Literal(PatternLiteral::Char(*value)))
            }
            AstRangeEndpoint::Const(path) => {
                let resolution = self.path_resolution(path.span)?.clone();
                let [Res::Item(HirItemRes::Definition(item))] = resolution.resolutions.as_slice()
                else {
                    self.source_error(
                        span,
                        "PATTERN001",
                        "const range endpoint is not one const item",
                    );
                    return None;
                };
                Some(RangeEndpoint::Const(
                    self.pattern_const_dependency(*item, ty, span)?,
                ))
            }
        }
    }

    fn pattern_const_dependency(
        &mut self,
        item: HirItemId,
        ty: &PatternType,
        span: Span,
    ) -> Option<PatternConst> {
        let Some(entry) = self.catalog.definition(item) else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingRetainedJoin,
                "const pattern item has no declaration entry",
            );
            return None;
        };
        checked_entry_shape(entry, self.scope.body.id, span, &mut self.gaps)?;
        if entry.definition.key.kind != DeclarationKind::Const {
            self.source_error(
                span,
                "PATTERN001",
                "pattern path is not a const declaration",
            );
            return None;
        }
        Some(PatternConst::new(
            declaration_dependency_string(&entry.semantic_path()),
            ty.clone(),
        ))
    }

    fn bind_pattern_locals(&mut self, source: &AstPattern, typed: &TypedPattern) {
        let mut source_spans = BTreeMap::new();
        collect_binding_spans(source, &mut source_spans);
        let mut bindings = Vec::new();
        collect_typed_bindings(typed, &mut bindings);
        for binding in bindings {
            let Some(span) = source_spans.get(binding.name()).copied() else {
                self.gap(
                    source.span,
                    BodyCheckIncompletenessKind::MissingRetainedJoin,
                    format!("typed binding `{}` has no AST binding span", binding.name()),
                );
                continue;
            };
            let matched = if self
                .ambiguous_pattern_symbolic
                .contains(binding.matched_type())
            {
                None
            } else {
                self.pattern_symbolic
                    .get(binding.matched_type())
                    .cloned()
                    .or_else(|| pattern_type_to_symbolic(binding.matched_type()))
            };
            let Some(matched) = matched else {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingPatternAlgebra,
                    format!(
                        "typed binding `{}` has no unique retained symbolic type for its pattern projection",
                        binding.name()
                    ),
                );
                continue;
            };
            let ty = match binding.mode() {
                BindingMode::Move => matched,
                BindingMode::Ref | BindingMode::RefMut => SymbolicType::Reference {
                    mutability: if binding.mode() == BindingMode::RefMut {
                        Mutability::Mutable
                    } else {
                        Mutability::Shared
                    },
                    lifetime: SymbolicLifetime::ErasedLocal,
                    pointee: Box::new(matched),
                },
            };
            self.bind_named_local(binding.name(), span, LocalValue::Typed(ty));
        }
    }

    fn collect_irrefutable_ctfe(&mut self, analysis: &IrrefutablePatternAnalysis) {
        if let IrrefutablePatternAnalysis::NeedsCtfe { dependencies, .. } = analysis {
            for dependency in dependencies.iter() {
                self.push_pattern_ctfe(dependency);
            }
        }
    }

    fn collect_match_ctfe(&mut self, analysis: &PatternMatchAnalysis) {
        if let PatternMatchAnalysis::NeedsCtfe(pending) = analysis {
            for dependency in pending.dependencies() {
                self.push_pattern_ctfe(dependency);
            }
        }
    }

    fn push_pattern_ctfe(&mut self, dependency: &PatternConst) {
        if let Ok(obligation) =
            NeedsCtfeObligation::from_canonical_bytes(dependency.dependency().as_bytes().to_vec())
        {
            self.ctfe.push(obligation);
        }
    }
}

fn irrefutable_typed_pattern(analysis: &IrrefutablePatternAnalysis) -> &TypedPattern {
    match analysis {
        IrrefutablePatternAnalysis::Complete(pattern)
        | IrrefutablePatternAnalysis::NeedsCtfe { pattern, .. } => pattern,
    }
}

fn pattern_constructor_fields<'a>(
    ty: &'a PatternType,
    constructor: &str,
) -> (Vec<&'a PatternType>, Vec<&'a str>, bool) {
    match ty {
        PatternType::Record(record) if record.name() == constructor => (
            record.fields().iter().map(RecordField::ty).collect(),
            record.fields().iter().map(RecordField::name).collect(),
            true,
        ),
        PatternType::Enum(enumeration) => enumeration
            .variants()
            .iter()
            .find(|variant| variant.name() == constructor)
            .map(|variant| {
                (
                    variant.fields().iter().collect(),
                    variant
                        .record_field_names()
                        .map(|names| names.iter().map(|name| name.as_ref()).collect())
                        .unwrap_or_default(),
                    variant.is_record(),
                )
            })
            .unwrap_or_default(),
        PatternType::Unit
        | PatternType::Bool
        | PatternType::Integer(_)
        | PatternType::Char
        | PatternType::String
        | PatternType::Str
        | PatternType::Tuple(_)
        | PatternType::Array { .. }
        | PatternType::SymbolicArray { .. }
        | PatternType::Slice(_)
        | PatternType::Record(_)
        | PatternType::Reference { .. }
        | PatternType::Float(_)
        | PatternType::Opaque(_)
        | PatternType::Unsupported(_) => (Vec::new(), Vec::new(), false),
    }
}

fn match_first_typed_pattern(analysis: &PatternMatchAnalysis) -> Option<&TypedPattern> {
    match analysis {
        PatternMatchAnalysis::Complete(complete) => {
            complete.arms().first().map(|arm| arm.pattern())
        }
        PatternMatchAnalysis::NeedsCtfe(pending) => pending.arms().first().map(|arm| arm.pattern()),
    }
}

fn collect_typed_bindings<'a>(
    pattern: &'a TypedPattern,
    output: &mut Vec<&'a crate::TypedBinding>,
) {
    match pattern.kind() {
        TypedPatternKind::Binding(binding) => output.push(binding),
        TypedPatternKind::Dereference { pattern, .. } => collect_typed_bindings(pattern, output),
        TypedPatternKind::Tuple(fields) | TypedPatternKind::Or(fields) => {
            for field in fields.iter() {
                collect_typed_bindings(field, output);
            }
        }
        TypedPatternKind::Slice { elements, .. }
        | TypedPatternKind::Constructor {
            fields: elements, ..
        }
        | TypedPatternKind::Record {
            fields: elements, ..
        }
        | TypedPatternKind::RecordConstructor {
            fields: elements, ..
        } => {
            for field in elements.iter() {
                collect_typed_bindings(field, output);
            }
        }
        TypedPatternKind::DynamicSlice { prefix, suffix, .. }
        | TypedPatternKind::SymbolicSlice { prefix, suffix, .. } => {
            for field in prefix.iter().chain(suffix.iter()) {
                collect_typed_bindings(field, output);
            }
        }
        TypedPatternKind::At { binding, pattern } => {
            output.push(binding);
            collect_typed_bindings(pattern, output);
        }
        TypedPatternKind::Wildcard
        | TypedPatternKind::Unit
        | TypedPatternKind::Literal(_)
        | TypedPatternKind::NeedsCtfe(_)
        | TypedPatternKind::Range { .. } => {}
    }
}

fn collect_binding_spans(pattern: &AstPattern, output: &mut BTreeMap<String, Span>) {
    match &pattern.kind {
        AstPatternKind::BarePathOrBinding(path) => {
            if let Some(segment) = path.segments.last() {
                output
                    .entry(segment.name.as_str().to_owned())
                    .or_insert(pattern.span);
            }
        }
        AstPatternKind::Binding { name, .. } => {
            output
                .entry(name.as_str().to_owned())
                .or_insert(pattern.span);
        }
        AstPatternKind::Reference { pattern, .. } => collect_binding_spans(pattern, output),
        AstPatternKind::Tuple(fields) | AstPatternKind::Or(fields) => {
            for field in fields {
                collect_binding_spans(field, output);
            }
        }
        AstPatternKind::Slice(parts) => {
            for part in parts {
                if let AstSlicePatternPart::Pattern(pattern) = part {
                    collect_binding_spans(pattern, output);
                }
            }
        }
        AstPatternKind::Constructor { payload, .. } => match payload {
            AstConstructorPatternPayload::Unit => {}
            AstConstructorPatternPayload::Tuple(fields) => {
                for field in fields {
                    collect_binding_spans(field, output);
                }
            }
            AstConstructorPatternPayload::Record(fields) => {
                for field in fields {
                    collect_binding_spans(&field.pattern, output);
                }
            }
        },
        AstPatternKind::At { binding, pattern } => {
            collect_binding_spans(binding, output);
            collect_binding_spans(pattern, output);
        }
        AstPatternKind::Wildcard
        | AstPatternKind::Unit
        | AstPatternKind::Literal(_)
        | AstPatternKind::Range { .. } => {}
    }
}

/// True when the type mentions a bound generic coordinate anywhere — such a
/// declared throws member needs the call's substitution before a catch can
/// type against it.
fn symbolic_type_mentions_bound(ty: &SymbolicType) -> bool {
    match ty {
        SymbolicType::BoundType { .. } => true,
        SymbolicType::Reference { pointee, .. } | SymbolicType::RawPointer { pointee, .. } => {
            symbolic_type_mentions_bound(pointee)
        }
        SymbolicType::Slice(element) | SymbolicType::Array { element, .. } => {
            symbolic_type_mentions_bound(element)
        }
        SymbolicType::Tuple(fields) => fields.iter().any(symbolic_type_mentions_bound),
        SymbolicType::NominalPath { arguments, .. } => arguments.iter().any(|argument| {
            matches!(argument, GenericArgumentShape::Type(ty) if symbolic_type_mentions_bound(ty))
        }),
        SymbolicType::FunctionPointer {
            parameters,
            result,
            requires,
            throws,
            ..
        } => parameters
            .iter()
            .chain(std::iter::once(result.as_ref()))
            .chain(requires.members())
            .chain(throws.members())
            .any(symbolic_type_mentions_bound),
        _ => false,
    }
}

/// True when a helper two-arm analysis failed only because the appended
/// wildcard row is unreachable — i.e. the real arm is irrefutable.
fn helper_wildcard_unreachable(errors: &PatternErrors) -> bool {
    !errors.as_slice().is_empty()
        && errors.as_slice().iter().all(|error| {
            error.kind() == &PatternErrorKind::UnreachableArm && error.arm_index() == Some(1)
        })
}

fn pattern_type_to_symbolic(ty: &PatternType) -> Option<SymbolicType> {
    match ty {
        PatternType::Unit => Some(SymbolicType::Unit),
        PatternType::Bool => Some(SymbolicType::Bool),
        PatternType::Char => Some(SymbolicType::Char),
        PatternType::Integer(integer) => Some(match (integer.is_signed(), integer.bits()) {
            (true, 8) => SymbolicType::I8,
            (true, 16) => SymbolicType::I16,
            (true, 32) => SymbolicType::I32,
            (true, 64) => SymbolicType::I64,
            (false, 8) => SymbolicType::U8,
            (false, 16) => SymbolicType::U16,
            (false, 32) => SymbolicType::U32,
            (false, 64) => SymbolicType::U64,
            _ => return None,
        }),
        PatternType::Tuple(fields) => Some(SymbolicType::Tuple(
            fields
                .iter()
                .map(pattern_type_to_symbolic)
                .collect::<Option<Vec<_>>>()?,
        )),
        PatternType::Array { element, length } => Some(SymbolicType::Array {
            element: Box::new(pattern_type_to_symbolic(element)?),
            length: SymbolicConstExpression {
                integer_type: arche_frontend::IntegerType::Usize,
                node: SymbolicConstNode::IntegerLiteral((*length as u64).to_le_bytes().to_vec()),
            },
        }),
        PatternType::Slice(element) => Some(SymbolicType::Slice(Box::new(
            pattern_type_to_symbolic(element)?,
        ))),
        PatternType::Str => Some(SymbolicType::Str),
        PatternType::Reference {
            mutability,
            referent,
        } => Some(SymbolicType::Reference {
            mutability: match mutability {
                ReferenceMutability::Shared => Mutability::Shared,
                ReferenceMutability::Mutable => Mutability::Mutable,
            },
            lifetime: SymbolicLifetime::ErasedLocal,
            pointee: Box::new(pattern_type_to_symbolic(referent)?),
        }),
        PatternType::String
        | PatternType::SymbolicArray { .. }
        | PatternType::Record(_)
        | PatternType::Enum(_)
        | PatternType::Opaque(_)
        | PatternType::Unsupported(_) => None,
        PatternType::Float(PatternFloatType::F32) => Some(SymbolicType::F32),
        PatternType::Float(PatternFloatType::F64) => Some(SymbolicType::F64),
    }
}

/// First-order structural unification binding depth-0 declaration type slots
/// from a checked argument type. Mismatched constructors bind nothing; the
/// caller's completeness check decides.
fn bind_inference_slots(
    declared: &SymbolicType,
    actual: &SymbolicType,
    slots: &mut [Option<SymbolicType>],
) {
    match (declared, actual) {
        (SymbolicType::BoundType { depth: 0, index }, actual) => {
            if let Ok(slot) = usize::try_from(*index) {
                if let Some(entry) = slots.get_mut(slot) {
                    if entry.is_none() {
                        *entry = Some(actual.clone());
                    }
                }
            }
        }
        (
            SymbolicType::Reference { pointee: left, .. },
            SymbolicType::Reference { pointee: right, .. },
        )
        | (
            SymbolicType::RawPointer { pointee: left, .. },
            SymbolicType::RawPointer { pointee: right, .. },
        )
        | (SymbolicType::Slice(left), SymbolicType::Slice(right))
        | (SymbolicType::Array { element: left, .. }, SymbolicType::Array { element: right, .. }) => {
            bind_inference_slots(left, right, slots)
        }
        (SymbolicType::Tuple(left), SymbolicType::Tuple(right)) if left.len() == right.len() => {
            for (left, right) in left.iter().zip(right) {
                bind_inference_slots(left, right, slots);
            }
        }
        (
            SymbolicType::NominalPath {
                declaration: left_declaration,
                arguments: left_arguments,
            },
            SymbolicType::NominalPath {
                declaration: right_declaration,
                arguments: right_arguments,
            },
        ) if left_declaration == right_declaration
            && left_arguments.len() == right_arguments.len() =>
        {
            for (left, right) in left_arguments.iter().zip(right_arguments) {
                if let (GenericArgumentShape::Type(left), GenericArgumentShape::Type(right)) =
                    (left, right)
                {
                    bind_inference_slots(left, right, slots);
                }
            }
        }
        _ => {}
    }
}

/// Shifts every bound binder coordinate in a predicate outward by `by`
/// frames, re-expressing an owner-frame predicate in an owned method body's
/// coordinates.
fn shift_predicate_binders(predicate: &SymbolicPredicate, by: u64) -> SymbolicPredicate {
    match predicate {
        SymbolicPredicate::Trait {
            trait_path,
            self_type,
            arguments,
        } => SymbolicPredicate::Trait {
            trait_path: trait_path.clone(),
            self_type: shift_type_binders(self_type, by),
            arguments: arguments
                .iter()
                .map(|argument| shift_argument_binders(argument, by))
                .collect(),
        },
        SymbolicPredicate::LifetimeOutlives { longer, shorter } => {
            SymbolicPredicate::LifetimeOutlives {
                longer: shift_lifetime_binders(longer, by),
                shorter: shift_lifetime_binders(shorter, by),
            }
        }
        SymbolicPredicate::TypeOutlives { ty, lifetime } => SymbolicPredicate::TypeOutlives {
            ty: shift_type_binders(ty, by),
            lifetime: shift_lifetime_binders(lifetime, by),
        },
    }
}

fn shift_lifetime_binders(lifetime: &SymbolicLifetime, by: u64) -> SymbolicLifetime {
    match lifetime {
        SymbolicLifetime::Bound { depth, index } => SymbolicLifetime::Bound {
            depth: depth.saturating_add(by),
            index: *index,
        },
        other => other.clone(),
    }
}

fn shift_argument_binders(argument: &GenericArgumentShape, by: u64) -> GenericArgumentShape {
    match argument {
        GenericArgumentShape::Type(ty) => GenericArgumentShape::Type(shift_type_binders(ty, by)),
        GenericArgumentShape::Lifetime(lifetime) => {
            GenericArgumentShape::Lifetime(shift_lifetime_binders(lifetime, by))
        }
        other => other.clone(),
    }
}

fn shift_type_binders(ty: &SymbolicType, by: u64) -> SymbolicType {
    match ty {
        SymbolicType::BoundType { depth, index } => SymbolicType::BoundType {
            depth: depth.saturating_add(by),
            index: *index,
        },
        SymbolicType::Reference {
            mutability,
            lifetime,
            pointee,
        } => SymbolicType::Reference {
            mutability: *mutability,
            lifetime: shift_lifetime_binders(lifetime, by),
            pointee: Box::new(shift_type_binders(pointee, by)),
        },
        SymbolicType::RawPointer {
            mutability,
            pointee,
        } => SymbolicType::RawPointer {
            mutability: *mutability,
            pointee: Box::new(shift_type_binders(pointee, by)),
        },
        SymbolicType::Slice(element) => {
            SymbolicType::Slice(Box::new(shift_type_binders(element, by)))
        }
        SymbolicType::Array { element, length } => SymbolicType::Array {
            element: Box::new(shift_type_binders(element, by)),
            length: length.clone(),
        },
        SymbolicType::Tuple(elements) => SymbolicType::Tuple(
            elements
                .iter()
                .map(|element| shift_type_binders(element, by))
                .collect(),
        ),
        SymbolicType::NominalPath {
            declaration,
            arguments,
        } => SymbolicType::NominalPath {
            declaration: declaration.clone(),
            arguments: arguments
                .iter()
                .map(|argument| shift_argument_binders(argument, by))
                .collect(),
        },
        other => other.clone(),
    }
}

/// Instantiates a method's own binder frame: depth-0 type/const slots take
/// the explicit turbofish actuals, depth-0 lifetimes erase to the body-local
/// marker, and an uninstantiable slot fails closed with `None`.
fn instantiate_method_frame(
    ty: &SymbolicType,
    explicit_actuals: &[GenericArgumentShape],
) -> Option<SymbolicType> {
    Some(match ty {
        SymbolicType::BoundType { depth: 0, index } => {
            let slot = usize::try_from(*index).ok()?;
            match explicit_actuals.get(slot)? {
                GenericArgumentShape::Type(actual) => actual.clone(),
                _ => return None,
            }
        }
        SymbolicType::Reference {
            mutability,
            lifetime,
            pointee,
        } => SymbolicType::Reference {
            mutability: *mutability,
            lifetime: match lifetime {
                SymbolicLifetime::Bound { depth: 0, .. } => SymbolicLifetime::ErasedLocal,
                other => other.clone(),
            },
            pointee: Box::new(instantiate_method_frame(pointee, explicit_actuals)?),
        },
        SymbolicType::RawPointer {
            mutability,
            pointee,
        } => SymbolicType::RawPointer {
            mutability: *mutability,
            pointee: Box::new(instantiate_method_frame(pointee, explicit_actuals)?),
        },
        SymbolicType::Slice(element) => SymbolicType::Slice(Box::new(instantiate_method_frame(
            element,
            explicit_actuals,
        )?)),
        SymbolicType::Array { element, length } => SymbolicType::Array {
            element: Box::new(instantiate_method_frame(element, explicit_actuals)?),
            length: length.clone(),
        },
        SymbolicType::Tuple(elements) => SymbolicType::Tuple(
            elements
                .iter()
                .map(|element| instantiate_method_frame(element, explicit_actuals))
                .collect::<Option<Vec<_>>>()?,
        ),
        SymbolicType::NominalPath {
            declaration,
            arguments,
        } => SymbolicType::NominalPath {
            declaration: declaration.clone(),
            arguments: arguments
                .iter()
                .map(|argument| match argument {
                    GenericArgumentShape::Type(ty) => Some(GenericArgumentShape::Type(
                        instantiate_method_frame(ty, explicit_actuals)?,
                    )),
                    GenericArgumentShape::Lifetime(SymbolicLifetime::Bound {
                        depth: 0, ..
                    }) => Some(GenericArgumentShape::Lifetime(
                        SymbolicLifetime::ErasedLocal,
                    )),
                    other => Some(other.clone()),
                })
                .collect::<Option<Vec<_>>>()?,
        },
        other => other.clone(),
    })
}

/// Replaces method-frame bound lifetimes (depth 0 inside the method shape)
/// with the body-local erased marker: an inferred call-site region never
/// enters a checked C2 type.
fn erase_method_frame_lifetimes(ty: SymbolicType) -> SymbolicType {
    fn erase_lifetime(lifetime: SymbolicLifetime) -> SymbolicLifetime {
        match lifetime {
            SymbolicLifetime::Bound { depth: 0, .. } => SymbolicLifetime::ErasedLocal,
            other => other,
        }
    }
    match ty {
        SymbolicType::Reference {
            mutability,
            lifetime,
            pointee,
        } => SymbolicType::Reference {
            mutability,
            lifetime: erase_lifetime(lifetime),
            pointee: Box::new(erase_method_frame_lifetimes(*pointee)),
        },
        SymbolicType::Slice(element) => {
            SymbolicType::Slice(Box::new(erase_method_frame_lifetimes(*element)))
        }
        SymbolicType::Array { element, length } => SymbolicType::Array {
            element: Box::new(erase_method_frame_lifetimes(*element)),
            length,
        },
        SymbolicType::Tuple(elements) => SymbolicType::Tuple(
            elements
                .into_iter()
                .map(erase_method_frame_lifetimes)
                .collect(),
        ),
        SymbolicType::RawPointer {
            mutability,
            pointee,
        } => SymbolicType::RawPointer {
            mutability,
            pointee: Box::new(erase_method_frame_lifetimes(*pointee)),
        },
        SymbolicType::NominalPath {
            declaration,
            arguments,
        } => SymbolicType::NominalPath {
            declaration,
            arguments: arguments
                .into_iter()
                .map(|argument| match argument {
                    GenericArgumentShape::Type(ty) => {
                        GenericArgumentShape::Type(erase_method_frame_lifetimes(ty))
                    }
                    GenericArgumentShape::Lifetime(lifetime) => {
                        GenericArgumentShape::Lifetime(erase_lifetime(lifetime))
                    }
                    other => other,
                })
                .collect(),
        },
        other => other,
    }
}

/// Divergence over a pre-typing expression input: return, throw, break,
/// continue, a known never value, a block with a diverging statement or
/// tail, or an if whose branches both diverge. Loop shapes are left to the
/// typing algebra, whose checked never type the match join also honors.
fn input_diverges(input: &TypedExpressionInput) -> bool {
    match input {
        TypedExpressionInput::Known(SymbolicType::Never)
        | TypedExpressionInput::Return(_)
        | TypedExpressionInput::Break(_)
        | TypedExpressionInput::Continue => true,
        TypedExpressionInput::Block { statements, tail } => {
            statements.iter().any(input_diverges) || tail.as_deref().is_some_and(input_diverges)
        }
        TypedExpressionInput::If {
            then_branch,
            else_branch,
            ..
        } => else_branch
            .as_deref()
            .is_some_and(|else_branch| input_diverges(then_branch) && input_diverges(else_branch)),
        _ => false,
    }
}

/// True when evaluating this checked expression can never complete normally:
/// it types as the never type, or a statement or block position inside it
/// does. Only block structure needs recursion: if requires both branches,
/// loops report never through their checked type, and lower_match joins an
/// all-diverging match to the never type before it reaches this judgment.
fn checked_expression_diverges(expression: &CheckedExpression) -> bool {
    if expression.ty() == &SymbolicType::Never {
        return true;
    }
    match expression.kind() {
        CheckedExpressionKind::Block { statements, tail } => {
            statements.iter().any(checked_expression_diverges)
                || tail.as_deref().is_some_and(checked_expression_diverges)
        }
        CheckedExpressionKind::If {
            then_branch,
            else_branch,
            ..
        } => else_branch.as_deref().is_some_and(|else_branch| {
            checked_expression_diverges(then_branch) && checked_expression_diverges(else_branch)
        }),
        _ => false,
    }
}

/// True when `inner` lies entirely within `outer` in the same file.
fn span_contains(outer: Span, inner: Span) -> bool {
    outer.file == inner.file
        && inner.start.byte >= outer.start.byte
        && inner.end.byte <= outer.end.byte
}

fn peel_references(mut ty: &SymbolicType) -> &SymbolicType {
    while let SymbolicType::Reference { pointee, .. } = ty {
        ty = pointee;
    }
    ty
}

fn types_match_with_erased_body_lifetime(left: &SymbolicType, right: &SymbolicType) -> bool {
    match (left, right) {
        (
            SymbolicType::Reference {
                mutability: left_mutability,
                lifetime: left_lifetime,
                pointee: left_pointee,
            },
            SymbolicType::Reference {
                mutability: right_mutability,
                lifetime: right_lifetime,
                pointee: right_pointee,
            },
        ) => {
            left_mutability == right_mutability
                && (left_lifetime == right_lifetime
                    || matches!(left_lifetime, SymbolicLifetime::ErasedLocal)
                    || matches!(right_lifetime, SymbolicLifetime::ErasedLocal))
                && types_match_with_erased_body_lifetime(left_pointee, right_pointee)
        }
        (SymbolicType::Tuple(left), SymbolicType::Tuple(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| types_match_with_erased_body_lifetime(left, right))
        }
        (
            SymbolicType::Array {
                element: left_element,
                length: left_length,
            },
            SymbolicType::Array {
                element: right_element,
                length: right_length,
            },
        ) => {
            left_length == right_length
                && types_match_with_erased_body_lifetime(left_element, right_element)
        }
        (SymbolicType::Slice(left), SymbolicType::Slice(right)) => {
            types_match_with_erased_body_lifetime(left, right) && left == right
        }
        (
            SymbolicType::RawPointer {
                mutability: left_mutability,
                pointee: left,
            },
            SymbolicType::RawPointer {
                mutability: right_mutability,
                pointee: right,
            },
        ) => {
            left_mutability == right_mutability
                && types_match_with_erased_body_lifetime(left, right)
                && left == right
        }
        _ => left == right,
    }
}

fn bind_compiler_type_generic(
    substitution: &mut CompilerMethodSubstitution,
    generic: CompilerMethodGenericParameter,
    ty: &SymbolicType,
) -> bool {
    match substitution.types.get(&generic) {
        Some(existing) => types_match_with_erased_body_lifetime(existing, ty),
        None => {
            substitution.types.insert(generic, ty.clone());
            true
        }
    }
}

fn bind_compiler_lifetime_generic(
    substitution: &mut CompilerMethodSubstitution,
    generic: CompilerMethodGenericParameter,
    lifetime: &SymbolicLifetime,
) -> bool {
    match substitution.lifetimes.get(&generic) {
        Some(existing) => {
            existing == lifetime
                || matches!(existing, SymbolicLifetime::ErasedLocal)
                || matches!(lifetime, SymbolicLifetime::ErasedLocal)
        }
        None => {
            substitution.lifetimes.insert(generic, lifetime.clone());
            true
        }
    }
}

fn bind_compiler_method_lifetime(
    substitution: &mut CompilerMethodSubstitution,
    pattern: &CompilerMethodLifetimePattern,
    lifetime: &SymbolicLifetime,
) -> bool {
    match pattern {
        CompilerMethodLifetimePattern::Elided => true,
        CompilerMethodLifetimePattern::Generic(generic) => {
            bind_compiler_lifetime_generic(substitution, *generic, lifetime)
        }
    }
}

fn compiler_method_lifetime_type(
    substitution: &CompilerMethodSubstitution,
    pattern: &CompilerMethodLifetimePattern,
) -> Option<SymbolicLifetime> {
    match pattern {
        CompilerMethodLifetimePattern::Elided => Some(SymbolicLifetime::ErasedLocal),
        CompilerMethodLifetimePattern::Generic(generic) => {
            substitution.lifetimes.get(generic).cloned()
        }
    }
}

fn receiver_mode_matches_pattern(
    mode: CompilerNominalMethodReceiverMode,
    pattern: &CompilerMethodTypePattern,
) -> bool {
    matches!(
        (mode, pattern),
        (
            CompilerNominalMethodReceiverMode::Value,
            CompilerMethodTypePattern::Definition { .. }
        ) | (
            CompilerNominalMethodReceiverMode::Shared,
            CompilerMethodTypePattern::SharedReference { .. }
        ) | (
            CompilerNominalMethodReceiverMode::Mutable,
            CompilerMethodTypePattern::MutableReference { .. }
        )
    )
}

fn compiler_primitive_symbolic_type(primitive: CompilerPrimitiveTypePattern) -> SymbolicType {
    match primitive {
        CompilerPrimitiveTypePattern::Never => SymbolicType::Never,
        CompilerPrimitiveTypePattern::Unit => SymbolicType::Unit,
        CompilerPrimitiveTypePattern::Bool => SymbolicType::Bool,
        CompilerPrimitiveTypePattern::Char => SymbolicType::Char,
        CompilerPrimitiveTypePattern::Entity => SymbolicType::Entity,
        CompilerPrimitiveTypePattern::F32 => SymbolicType::F32,
        CompilerPrimitiveTypePattern::F64 => SymbolicType::F64,
        CompilerPrimitiveTypePattern::I8 => SymbolicType::I8,
        CompilerPrimitiveTypePattern::I16 => SymbolicType::I16,
        CompilerPrimitiveTypePattern::I32 => SymbolicType::I32,
        CompilerPrimitiveTypePattern::I64 => SymbolicType::I64,
        CompilerPrimitiveTypePattern::Isize => SymbolicType::Isize,
        CompilerPrimitiveTypePattern::Str => SymbolicType::Str,
        CompilerPrimitiveTypePattern::U8 => SymbolicType::U8,
        CompilerPrimitiveTypePattern::U16 => SymbolicType::U16,
        CompilerPrimitiveTypePattern::U32 => SymbolicType::U32,
        CompilerPrimitiveTypePattern::U64 => SymbolicType::U64,
        CompilerPrimitiveTypePattern::Usize => SymbolicType::Usize,
    }
}

fn compiler_method_dependency_bytes(method: VirtualMethodId, domain: &[u8]) -> Vec<u8> {
    let mut bytes = b"ARCHE-C2-COMPILER-NOMINAL-METHOD\0".to_vec();
    bytes.extend_from_slice(&method.ordinal().to_le_bytes());
    bytes.extend_from_slice(
        &u64::try_from(domain.len())
            .expect("compiler method dependency domain fits u64")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(domain);
    bytes
}

fn is_scalar_primitive(ty: &SymbolicType) -> bool {
    matches!(
        ty,
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
            | SymbolicType::RawPointer { .. }
    )
}

fn integer_literal_usize(literal: &arche_frontend::lexer::IntegerLiteral) -> Result<usize, String> {
    let typed =
        crate::check_integer_literal(literal, Some(arche_frontend::IntegerType::Usize), false)
            .map_err(|error| format!("invalid tuple index: {error:?}"))?;
    little_endian_usize(typed.little_endian_bits())
        .ok_or_else(|| "tuple index does not fit host usize".to_owned())
}

fn const_literal_usize(value: &SymbolicConstExpression) -> Option<usize> {
    let SymbolicConstNode::IntegerLiteral(bytes) = &value.node else {
        return None;
    };
    little_endian_usize(bytes)
}

fn little_endian_usize(bytes: &[u8]) -> Option<usize> {
    if bytes.len() > std::mem::size_of::<usize>()
        && bytes[std::mem::size_of::<usize>()..]
            .iter()
            .any(|byte| *byte != 0)
    {
        return None;
    }
    let mut output = [0_u8; std::mem::size_of::<usize>()];
    let copied = bytes.len().min(output.len());
    output[..copied].copy_from_slice(&bytes[..copied]);
    Some(usize::from_le_bytes(output))
}

fn integer_pattern_literal(
    literal: &arche_frontend::lexer::IntegerLiteral,
    negative: bool,
    ty: &PatternType,
) -> Result<PatternLiteral, String> {
    let PatternType::Integer(integer) = ty else {
        return Err("integer pattern requires an integer scrutinee".to_owned());
    };
    let contextual = match (integer.is_signed(), integer.bits()) {
        (true, 8) => arche_frontend::IntegerType::I8,
        (true, 16) => arche_frontend::IntegerType::I16,
        (true, 32) => arche_frontend::IntegerType::I32,
        (true, 64) => arche_frontend::IntegerType::I64,
        (false, 8) => arche_frontend::IntegerType::U8,
        (false, 16) => arche_frontend::IntegerType::U16,
        (false, 32) => arche_frontend::IntegerType::U32,
        (false, 64) => arche_frontend::IntegerType::U64,
        (_, bits) => return Err(format!("unsupported pattern integer width {bits}")),
    };
    let typed = crate::check_integer_literal(literal, Some(contextual), negative)
        .map_err(|error| format!("invalid integer pattern literal: {error:?}"))?;
    if integer.is_signed() {
        let sign = typed
            .little_endian_bits()
            .last()
            .is_some_and(|byte| byte & 0x80 != 0);
        let mut bytes = [if sign { 0xff } else { 0 }; 16];
        bytes[..typed.little_endian_bits().len()].copy_from_slice(typed.little_endian_bits());
        Ok(PatternLiteral::Signed(i128::from_le_bytes(bytes)))
    } else {
        let mut bytes = [0_u8; 16];
        bytes[..typed.little_endian_bits().len()].copy_from_slice(typed.little_endian_bits());
        Ok(PatternLiteral::Unsigned(u128::from_le_bytes(bytes)))
    }
}

fn collect_const_paths(
    value: &SymbolicConstExpression,
    output: &mut BTreeSet<SemanticDeclarationPath>,
) {
    match &value.node {
        SymbolicConstNode::ConstDefinitionPath(path) => {
            output.insert(path.clone());
        }
        SymbolicConstNode::WrappingNeg(value) | SymbolicConstNode::BitNot(value) => {
            collect_const_paths(value, output);
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
            collect_const_paths(left, output);
            collect_const_paths(right, output);
        }
        SymbolicConstNode::IntegerLiteral(_) | SymbolicConstNode::Bound { .. } => {}
    }
}

fn declaration_dependency_string(path: &SemanticDeclarationPath) -> String {
    let mut bytes = Vec::new();
    encode_declaration_path_canonical(&mut bytes, path);
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02X}").expect("writing to String cannot fail");
    }
    output
}

fn symbolic_const_dependency_string(value: &SymbolicConstExpression) -> String {
    let mut bytes = b"ARCHE-C2-PATTERN-SYMBOLIC-CONST\0".to_vec();
    encode_length_prefixed_string(&mut bytes, integer_type_atom(value.integer_type));
    encode_symbolic_const_node(&mut bytes, &value.node);
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02X}").expect("writing to String cannot fail");
    }
    output
}

fn encode_symbolic_const_node(output: &mut Vec<u8>, node: &SymbolicConstNode) {
    match node {
        SymbolicConstNode::IntegerLiteral(bytes) => {
            output.push(1);
            output.extend_from_slice(
                &u64::try_from(bytes.len())
                    .expect("const byte length fits u64")
                    .to_le_bytes(),
            );
            output.extend_from_slice(bytes);
        }
        SymbolicConstNode::Bound { depth, index } => {
            output.push(2);
            output.extend_from_slice(&depth.to_le_bytes());
            output.extend_from_slice(&index.to_le_bytes());
        }
        SymbolicConstNode::ConstDefinitionPath(path) => {
            output.push(3);
            encode_declaration_path_canonical(output, path);
        }
        SymbolicConstNode::WrappingNeg(value) => {
            output.push(4);
            encode_symbolic_const_node(output, &value.node);
        }
        SymbolicConstNode::BitNot(value) => {
            output.push(5);
            encode_symbolic_const_node(output, &value.node);
        }
        SymbolicConstNode::WrappingMul(left, right) => {
            output.push(6);
            encode_symbolic_const_pair(output, left, right);
        }
        SymbolicConstNode::IntegerDivide(left, right) => {
            output.push(7);
            encode_symbolic_const_pair(output, left, right);
        }
        SymbolicConstNode::IntegerRemainder(left, right) => {
            output.push(8);
            encode_symbolic_const_pair(output, left, right);
        }
        SymbolicConstNode::WrappingAdd(left, right) => {
            output.push(9);
            encode_symbolic_const_pair(output, left, right);
        }
        SymbolicConstNode::WrappingSub(left, right) => {
            output.push(10);
            encode_symbolic_const_pair(output, left, right);
        }
        SymbolicConstNode::MaskedShiftLeft(left, right) => {
            output.push(11);
            encode_symbolic_const_pair(output, left, right);
        }
        SymbolicConstNode::MaskedShiftRight(left, right) => {
            output.push(12);
            encode_symbolic_const_pair(output, left, right);
        }
        SymbolicConstNode::BitAnd(left, right) => {
            output.push(13);
            encode_symbolic_const_pair(output, left, right);
        }
        SymbolicConstNode::BitXor(left, right) => {
            output.push(14);
            encode_symbolic_const_pair(output, left, right);
        }
        SymbolicConstNode::BitOr(left, right) => {
            output.push(15);
            encode_symbolic_const_pair(output, left, right);
        }
    }
}

fn encode_symbolic_const_pair(
    output: &mut Vec<u8>,
    left: &SymbolicConstExpression,
    right: &SymbolicConstExpression,
) {
    encode_length_prefixed_string(output, integer_type_atom(left.integer_type));
    encode_symbolic_const_node(output, &left.node);
    encode_length_prefixed_string(output, integer_type_atom(right.integer_type));
    encode_symbolic_const_node(output, &right.node);
}

fn target_root_spelling(target: &arche_frontend::TargetRoot) -> String {
    match target {
        arche_frontend::TargetRoot::Library => "library".to_owned(),
        arche_frontend::TargetRoot::Binary(name) => format!("binary:{name}"),
        arche_frontend::TargetRoot::Environment(name) => format!("environment:{name}"),
    }
}

fn encode_declaration_path_canonical(output: &mut Vec<u8>, path: &SemanticDeclarationPath) {
    output.extend_from_slice(b"ARCHE-SEMANTIC-PATH\0");
    encode_length_prefixed_string(output, &path.registry_origin);
    encode_length_prefixed_string(output, &path.package_name);
    encode_length_prefixed_string(output, &target_root_spelling(&path.target));
    output.extend_from_slice(
        &u64::try_from(path.modules.len())
            .expect("module count fits u64")
            .to_le_bytes(),
    );
    for module in &path.modules {
        encode_length_prefixed_string(output, module);
    }
    encode_length_prefixed_string(output, declaration_kind_atom(path.kind));
    encode_length_prefixed_string(output, &path.name);
}

fn encode_length_prefixed_string(output: &mut Vec<u8>, value: &str) {
    output.extend_from_slice(
        &u64::try_from(value.len())
            .expect("string length fits u64")
            .to_le_bytes(),
    );
    output.extend_from_slice(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use arche_frontend::{check_workspace_c1, FrontendOutput};
    use arche_package::{load_workspace, resolve, ManifestRequest, RegistrySnapshot};

    use crate::declaration_check::check_declarations_c2;

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

    fn inline_frontend(source: &str) -> FrontendOutput {
        let ordinal = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let fixture = TemporaryWorkspace(std::env::temp_dir().join(format!(
            "arche-c2-body-check-{}-{ordinal}",
            std::process::id()
        )));
        fs::create_dir_all(fixture.0.join("src")).unwrap();
        fs::write(
            fixture.0.join("Arche.toml"),
            concat!(
                "schema = 1\n\n",
                "[package]\n",
                "name = \"example/body-check\"\n",
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

    fn corpus_body_complete(name: &str) -> C2BodyTable {
        let handoff = C2Handoff::begin(corpus_frontend(name)).unwrap();
        let expected_bodies = handoff
            .frontend()
            .hir()
            .packages
            .iter()
            .flat_map(|package| &package.targets)
            .map(|target| target.bodies.len())
            .sum::<usize>();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_declarations = check_declarations_c2(&handoff, &declarations);
        let checked_declarations = match &checked_declarations {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let table = check_workspace_bodies_c2(&handoff, &declarations, checked_declarations)
            .unwrap_or_else(|failure| {
                panic!(
                    "corpus={name} bodies must all close, gaps={:?}",
                    failure.incompleteness()
                )
            });
        assert_eq!(table.len(), expected_bodies, "corpus={name}");
        assert!(table.all_authority_complete(), "corpus={name}");
        table
    }

    #[test]
    fn real_v1_bodies_retain_every_body_and_never_turn_adapter_gaps_into_diagnostics() {
        // Both corpora's bodies close completely: every body is retained and
        // authority-complete, with no diagnostics minted.
        for corpus in ["language-game", "language-environment"] {
            let table = corpus_body_complete(corpus);
            assert_eq!(table.bodies().count(), table.len(), "corpus={corpus}");
        }
    }

    #[test]
    fn verified_nominal_methods_record_exact_vec_map_and_app_calls() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn empty() -> Vec<i32> { Vec::new() }\n",
            "pub fn mutate(\n",
            "    values: &mut Vec<i32>,\n",
            "    entries: &mut Map<i32, i32>,\n",
            ") -> Option<i32> {\n",
            "    values.push(1i32);\n",
            "    entries.insert(1i32, 2i32);\n",
            "    entries.remove(&1i32)\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("verified Vec/Map calls must close: {failure:#?}"));
        let core = &handoff.frontend().inventory().embedded_core;
        let expected = [
            (
                core.compiler_nominal_method(CompilerNominalKind::Vec, "new")
                    .unwrap()
                    .c1_method(),
                "Vec",
            ),
            (
                core.compiler_nominal_method(CompilerNominalKind::Vec, "push")
                    .unwrap()
                    .c1_method(),
                "()",
            ),
            (
                core.compiler_nominal_method(CompilerNominalKind::Map, "insert")
                    .unwrap()
                    .c1_method(),
                "Option",
            ),
            (
                core.compiler_nominal_method(CompilerNominalKind::Map, "remove")
                    .unwrap()
                    .c1_method(),
                "Option",
            ),
        ];
        let calls = bodies
            .bodies()
            .flat_map(C2BodyView::calls)
            .collect::<Vec<_>>();
        for (method, result_name) in expected {
            let matching = calls
                .iter()
                .filter(|call| call.callee() == &CheckedBodyCallee::EmbeddedMethod(method))
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), 1, "method={method:?}, calls={calls:#?}");
            match (result_name, matching[0].result()) {
                ("()", SymbolicType::Unit) => {}
                (
                    expected,
                    SymbolicType::NominalPath {
                        declaration,
                        arguments,
                    },
                ) => {
                    assert_eq!(declaration.name, expected);
                    assert_eq!(arguments, &[GenericArgumentShape::Type(SymbolicType::I32)]);
                }
                (_, result) => panic!("method={method:?} has unexpected result {result:?}"),
            }
        }
        assert!(
            bodies
                .bodies()
                .map(|body| body.pending_c4().len())
                .sum::<usize>()
                >= 2
        );

        let game_handoff = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let app_run_method = game_handoff
            .frontend()
            .inventory()
            .embedded_core
            .compiler_nominal_method(CompilerNominalKind::App, "run")
            .unwrap()
            .c1_method();
        let game_declarations = DeclarationTable::build(&game_handoff).unwrap();
        let game_checked_result = check_declarations_c2(&game_handoff, &game_declarations);
        let game_checked = match &game_checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let game_result =
            check_workspace_bodies_c2(&game_handoff, &game_declarations, game_checked);
        let game_bodies = match &game_result {
            Ok(bodies) => bodies,
            Err(failure) => failure.partial(),
        };
        let app_run = game_bodies
            .bodies()
            .flat_map(C2BodyView::calls)
            .find(|call| call.callee() == &CheckedBodyCallee::EmbeddedMethod(app_run_method))
            .expect("the verified App.run call must remain consumable in the v1 corpus");
        assert_eq!(app_run.result(), &SymbolicType::Unit);
    }

    #[test]
    fn verified_nominal_method_parameter_near_miss_is_a_source_error_not_a_gap() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn invalid(values: &mut Vec<i32>) -> () {\n",
            "    values.push(true);\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked = check_declarations_c2(&handoff, &declarations).unwrap();
        let failure = check_workspace_bodies_c2(&handoff, &declarations, &checked).unwrap_err();
        assert!(failure.incompleteness().is_empty());
        let diagnostics = failure.diagnostics().unwrap().as_slice();
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        let diagnostic = diagnostics[0].diagnostic();
        assert_eq!(diagnostic.code, "TYPE002");
        assert_eq!(diagnostic.message, "expected i32, found bool");
        assert_eq!(
            diagnostic
                .primary
                .span
                .map(|span| (span.start.line, span.start.column)),
            Some((2, 17))
        );
    }

    #[test]
    fn checked_declarations_from_another_session_fail_before_body_traversal() {
        let game = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let game_declarations = DeclarationTable::build(&game).unwrap();
        let environment = C2Handoff::begin(corpus_frontend("language-environment")).unwrap();
        let environment_declarations = DeclarationTable::build(&environment).unwrap();
        let environment_checked =
            match check_declarations_c2(&environment, &environment_declarations) {
                Ok(facts) => facts,
                Err(failure) => {
                    assert!(failure.blockers().iter().all(|blocker| matches!(
                        blocker.reason(),
                        crate::DeclarationCheckBlockerReason::MissingDeclarationJudgment(_)
                    )));
                    failure.into_partial()
                }
            };

        let failure =
            check_workspace_bodies_c2(&game, &game_declarations, &environment_checked).unwrap_err();
        assert!(failure.partial().is_empty());
        assert!(game
            .session_brand()
            .same_session(failure.partial().session_brand()));
        assert!(failure.diagnostics().is_none());
        assert_eq!(failure.incompleteness().len(), 1);
        assert_eq!(
            failure.incompleteness()[0].detail(),
            "checked declaration facts belong to another C2 session"
        );
    }

    #[test]
    fn source_invalid_rows_and_incomplete_rows_never_expose_typed_body_facts() {
        let invalid =
            C2Handoff::begin(inline_frontend("pub fn invalid() -> i32 { true }\n")).unwrap();
        let declarations = DeclarationTable::build(&invalid).unwrap();
        let checked = check_declarations_c2(&invalid, &declarations).unwrap();
        let failure = check_workspace_bodies_c2(&invalid, &declarations, &checked).unwrap_err();
        assert!(failure.diagnostics().is_some());
        assert!(failure.incompleteness().is_empty());
        let attempt = failure.partial().attempts().next().unwrap();
        assert!(attempt.authority_complete());
        assert!(attempt.has_source_diagnostics());
        assert!(failure.partial().bodies().next().is_none());
        assert!(failure.partial().all_authority_complete());

        let cascaded = C2Handoff::begin(inline_frontend(concat!(
            "pub fn invalid() -> i32 {\n",
            "    let invalid: i32 = true;\n",
            "    let outer = 2i32;\n",
            "    let closure = move |input: i32|\n",
            "        requires {} throws {} -> i32 { input + outer };\n",
            "    1i32\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&cascaded).unwrap();
        let checked = check_declarations_c2(&cascaded, &declarations).unwrap();
        let failure = check_workspace_bodies_c2(&cascaded, &declarations, &checked).unwrap_err();
        assert!(failure.diagnostics().is_none());
        assert!(!failure.incompleteness().is_empty());
        assert!(failure
            .partial()
            .attempts()
            .any(|attempt| { !attempt.authority_complete() && attempt.has_source_diagnostics() }));
    }

    #[test]
    fn missing_partial_declaration_rows_are_scoped_to_only_their_bodies() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub struct Wrapper<T> { value: T }\n",
            "pub fn good() -> i32 { 1i32 }\n",
            "pub fn pending(value: Wrapper<i32, bool>) -> i32 { 1i32 }\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_failure = check_declarations_c2(&handoff, &declarations).unwrap_err();
        assert!(checked_failure.partial().len() < declarations.len());

        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked_failure.partial())
            .unwrap_err();
        assert_eq!(failure.partial().len(), 2);
        assert_eq!(failure.partial().bodies().count(), 1);
        assert_eq!(
            failure
                .partial()
                .attempts()
                .filter(|attempt| !attempt.authority_complete())
                .count(),
            1
        );
        assert!(failure.diagnostics().is_none());
        assert!(failure.incompleteness().iter().all(|gap| {
            gap.body() != HirBodyId(u64::MAX)
                && gap
                    .detail()
                    .contains("no structurally closed checked declaration row")
        }));
    }

    #[test]
    fn body_expected_types_consume_post_c2_contextual_self() {
        let handoff = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let make = checked
            .declarations()
            .find(|declaration| declaration.name() == "make")
            .unwrap();
        let item = handoff.indexes().item(make.session_item()).unwrap().id();
        let SymbolicDeclarationPayloadSkeleton::Callable(checked_callable) =
            &make.declaration_shape().payload
        else {
            panic!("make must retain a callable checked shape")
        };
        let SymbolicTypeShapeSkeleton::Resolved {
            value: expected, ..
        } = &checked_callable.result
        else {
            panic!("contextual Self must be closed before body checking")
        };

        let body_result = check_workspace_bodies_c2(&handoff, &declarations, checked);
        let bodies = match &body_result {
            Ok(table) => table,
            Err(failure) => failure.partial(),
        };
        let body = bodies
            .bodies()
            .find(|body| body.owner() == item)
            .expect("make body must remain consumable");
        assert!(body
            .expressions()
            .iter()
            .any(|expression| expression.expression().ty() == expected));
    }

    #[test]
    fn checked_body_handles_are_owner_branded() {
        let game = corpus_body_complete("language-game");
        let environment = corpus_body_complete("language-environment");
        let game_body = game.bodies().next().unwrap();
        let environment_body = environment.bodies().next().unwrap();
        let game_handle = game.handle(game_body.id()).unwrap();
        let environment_handle = environment.handle(environment_body.id()).unwrap();

        assert!(game.body(&game_handle).is_some());
        assert!(environment.body(&environment_handle).is_some());
        assert!(game.body(&environment_handle).is_none());
        assert!(environment.body(&game_handle).is_none());
    }

    #[test]
    fn signed_pattern_literals_use_the_existing_fixed_width_literal_checker() {
        use arche_frontend::lexer::{IntegerLiteral, NumericBase};

        let literal = IntegerLiteral {
            base: NumericBase::Decimal,
            digits: "128".into(),
            suffix: None,
            raw: "128".into(),
        };
        assert_eq!(
            integer_pattern_literal(
                &literal,
                true,
                &PatternType::Integer(PatternIntegerType::Signed(8)),
            )
            .unwrap(),
            PatternLiteral::Signed(-128)
        );
        assert!(integer_pattern_literal(
            &literal,
            false,
            &PatternType::Integer(PatternIntegerType::Signed(8)),
        )
        .is_err());
    }

    #[test]
    fn embedded_record_construction_never_adopts_a_foreign_expected_type() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn cross() -> ProcessSpec {\n",
            "    ProcessOutput {\n",
            "        status: 0i32,\n",
            "        stdout: Vec::new(),\n",
            "        stderr: Vec::new(),\n",
            "    }\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        let diagnostics = format!("{:?}", failure.diagnostics());
        assert!(
            failure.diagnostics().is_some(),
            "a well-formed ProcessOutput literal must never satisfy a ProcessSpec context: {:?}",
            failure.incompleteness()
        );
        assert!(diagnostics.contains("TYPE00"), "diagnostics={diagnostics}");
    }

    #[test]
    fn embedded_variant_construction_with_a_foreign_expected_type_fails_closed() {
        let handoff = C2Handoff::begin(inline_frontend(
            "pub fn cross() -> Option<i32> { Result::Ok(1i32) }\n",
        ))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        assert!(
            failure.diagnostics().is_none(),
            "the refused adoption is an authority gap, not a source rejection: {:?}",
            failure.diagnostics()
        );
        assert!(
            failure
                .incompleteness()
                .iter()
                .any(|gap| gap.kind() == BodyCheckIncompletenessKind::MissingGenericInference),
            "gaps={:?}",
            failure.incompleteness()
        );
    }

    #[test]
    fn matching_generic_expected_types_still_close_embedded_variant_constructions() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn some() -> Option<i32> { Option::Some(1i32) }\n",
            "pub fn none() -> Option<i32> { Option::None }\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("matching adoption must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
        let results = bodies
            .bodies()
            .flat_map(C2BodyView::calls)
            .map(CheckedBodyCall::result)
            .collect::<Vec<_>>();
        assert!(
            results.iter().any(|result| matches!(
                result,
                SymbolicType::NominalPath { declaration, arguments }
                    if declaration.name == "Option"
                        && arguments == &[GenericArgumentShape::Type(SymbolicType::I32)]
            )),
            "results={results:#?}"
        );
    }

    #[test]
    fn prelude_function_names_are_not_values() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn first() -> i32 {\n",
            "    let f = panic;\n",
            "    1i32\n",
            "}\n",
            "pub fn second() -> i32 {\n",
            "    let g = include_bytes;\n",
            "    2i32\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        assert!(
            failure.diagnostics().is_none(),
            "unfinished value semantics must stay gaps: {:?}",
            failure.diagnostics()
        );
        let gaps = failure
            .incompleteness()
            .iter()
            .filter(|gap| gap.kind() == BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable)
            .count();
        assert_eq!(gaps, 2, "gaps={:?}", failure.incompleteness());
    }

    #[test]
    fn panic_calls_type_never_without_value_placeholders() {
        let handoff =
            C2Handoff::begin(inline_frontend("pub fn boom() { panic(\"boom\"); }\n")).unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("a direct panic call must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
        assert!(bodies
            .bodies()
            .flat_map(C2BodyView::calls)
            .any(|call| call.result() == &SymbolicType::Never));
    }

    #[test]
    fn panic_and_include_argument_mismatches_are_source_errors() {
        let handoff =
            C2Handoff::begin(inline_frontend("pub fn extra() { panic(\"a\", \"b\"); }\n")).unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        let diagnostics = format!("{:?}", failure.diagnostics());
        assert!(failure.diagnostics().is_some());
        assert!(diagnostics.contains("TYPE002"), "diagnostics={diagnostics}");
    }

    #[test]
    fn diverging_let_else_blocks_are_accepted_semantically() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub enum Choice2 {\n",
            "    One(i32),\n",
            "    Two,\n",
            "}\n",
            "pub fn by_panic(value: Choice2) -> i32 {\n",
            "    let Choice2::One(inner) = value else { panic(\"no\") };\n",
            "    inner\n",
            "}\n",
            "pub fn by_branching_returns(value: Choice2, flag: bool) -> i32 {\n",
            "    let Choice2::One(inner) = value else {\n",
            "        if flag {\n",
            "            return 0i32;\n",
            "        } else {\n",
            "            return 1i32;\n",
            "        }\n",
            "    };\n",
            "    inner\n",
            "}\n",
            "pub fn by_nested_block(value: Choice2) -> i32 {\n",
            "    let Choice2::One(inner) = value else {\n",
            "        {\n",
            "            return 0i32;\n",
            "        }\n",
            "    };\n",
            "    inner\n",
            "}\n",
            "pub fn by_loop(value: Choice2) -> i32 {\n",
            "    let Choice2::One(inner) = value else {\n",
            "        loop {\n",
            "        }\n",
            "    };\n",
            "    inner\n",
            "}\n",
            "pub fn by_dead_tail_statement(value: Choice2) -> i32 {\n",
            "    let Choice2::One(inner) = value else {\n",
            "        return 0i32;\n",
            "        let _unused = 1i32;\n",
            "    };\n",
            "    inner\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("diverging else blocks must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
    }

    #[test]
    fn non_diverging_let_else_blocks_stay_type002() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub enum Choice2 {\n",
            "    One(i32),\n",
            "    Two,\n",
            "}\n",
            "pub fn broken(value: Choice2) -> i32 {\n",
            "    let Choice2::One(inner) = value else {\n",
            "        1i32;\n",
            "    };\n",
            "    inner\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        let diagnostics = format!("{:?}", failure.diagnostics());
        assert!(failure.diagnostics().is_some());
        assert!(
            diagnostics.contains("must diverge"),
            "diagnostics={diagnostics}"
        );
    }

    #[test]
    fn pending_impl_candidates_force_the_selection_gap() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub trait Speak {\n",
            "    fn speak(&self) -> i32;\n",
            "}\n",
            "pub struct Talker {\n",
            "    pub n: i32,\n",
            "}\n",
            "impl Speak for Talker {\n",
            "    fn speak(&self) -> i32 {\n",
            "        self.n\n",
            "    }\n",
            "}\n",
            "pub struct Holder<T> {\n",
            "    pub value: T,\n",
            "}\n",
            "impl<T> Holder<T> {\n",
            "    pub fn get(&self) -> i32 {\n",
            "        1i32\n",
            "    }\n",
            "}\n",
            "impl<T> Holder<T> where T: Speak {\n",
            "    pub fn get(&self) -> char {\n",
            "        'a'\n",
            "    }\n",
            "}\n",
            "pub fn call(holder: &Holder<Talker>) -> char {\n",
            "    holder.get()\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        assert!(
            failure.diagnostics().is_none(),
            "unresolved viability must not select or reject: {:?}",
            failure.diagnostics()
        );
        assert!(failure
            .incompleteness()
            .iter()
            .any(|gap| gap.kind() == BodyCheckIncompletenessKind::MissingMethodSelection));
    }

    #[test]
    fn receiver_mode_mismatches_are_recorded_gaps_not_silent_rows() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub struct Counter {\n",
            "    pub n: i32,\n",
            "}\n",
            "impl Counter {\n",
            "    pub fn make(value: i32) -> Counter {\n",
            "        Counter { n: value }\n",
            "    }\n",
            "}\n",
            "pub fn through_receiver(c: &Counter) -> i32 {\n",
            "    c.make(1i32).n\n",
            "}\n",
            "pub fn bound_mutable<T>(it: &mut T) -> i32 where T: Iterator<T, i32> {\n",
            "    it.next();\n",
            "    0i32\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        assert!(
            failure.diagnostics().is_none(),
            "receiver-mode holes are gaps, not rejections: {:?}",
            failure.diagnostics()
        );
        let gaps = failure
            .incompleteness()
            .iter()
            .filter(|gap| gap.kind() == BodyCheckIncompletenessKind::MissingMethodSelection)
            .count();
        assert!(gaps >= 2, "gaps={:?}", failure.incompleteness());
    }

    #[test]
    fn raw_pointer_mutability_never_erases_between_body_types() {
        let handoff = C2Handoff::begin(inline_frontend(
            "pub fn cast(pointer: *mut i32) -> *const i32 { pointer }\n",
        ))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        assert!(
            failure.diagnostics().is_some(),
            "a mut-to-const raw pointer conversion has no coercion authority: {:?}",
            failure.incompleteness()
        );

        let handoff = C2Handoff::begin(inline_frontend(
            "pub fn keep(pointer: *const i32) -> *const i32 { pointer }\n",
        ))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("identity raw pointer must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
    }

    #[test]
    fn diverging_match_arms_join_to_never_without_fabricated_mismatches() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub enum ChoiceP {\n",
            "    One(i32),\n",
            "    Two,\n",
            "}\n",
            "pub fn else_match(value: ChoiceP, flag: bool) -> i32 {\n",
            "    let ChoiceP::One(inner) = value else {\n",
            "        match flag {\n",
            "            true => {\n",
            "                return 0i32;\n",
            "            },\n",
            "            false => {\n",
            "                return 1i32;\n",
            "            },\n",
            "        }\n",
            "    };\n",
            "    inner\n",
            "}\n",
            "pub fn mixed_statement(flag: bool) -> i32 {\n",
            "    match flag {\n",
            "        true => return 0i32,\n",
            "        false => {\n",
            "            return 1i32;\n",
            "        },\n",
            "    };\n",
            "    2i32\n",
            "}\n",
            "pub fn never_coerces(flag: bool) -> i32 {\n",
            "    let chosen: i32 = match flag {\n",
            "        true => return 1i32,\n",
            "        false => return 2i32,\n",
            "    };\n",
            "    chosen\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("diverging match arms must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
    }

    #[test]
    fn mismatched_match_arms_are_still_type002() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn broken(flag: bool) -> i32 {\n",
            "    match flag {\n",
            "        true => 1i32,\n",
            "        false => 'x',\n",
            "    }\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        let diagnostics = format!("{:?}", failure.diagnostics());
        assert!(failure.diagnostics().is_some());
        assert!(diagnostics.contains("TYPE002"), "diagnostics={diagnostics}");
    }

    #[test]
    fn never_typed_arms_do_not_seed_the_join() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn statement_position(flag: bool) -> i32 {\n",
            "    match flag {\n",
            "        true => loop {\n",
            "        },\n",
            "        false => {},\n",
            "    };\n",
            "    0i32\n",
            "}\n",
            "pub fn value_position(flag: bool) -> i32 {\n",
            "    let chosen = match flag {\n",
            "        true => loop {\n",
            "        },\n",
            "        false => 5i32,\n",
            "    };\n",
            "    chosen\n",
            "}\n",
            "pub fn all_loops(flag: bool) -> i32 {\n",
            "    let spun: i32 = match flag {\n",
            "        true => loop {\n",
            "        },\n",
            "        false => loop {\n",
            "        },\n",
            "    };\n",
            "    spun\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("never-typed arms must not seed: {failure:#?}"));
        assert!(bodies.all_authority_complete());
    }

    #[test]
    fn float_fields_are_bindable_without_poisoning_the_match_domain() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub struct Location {\n",
            "    pub x: f32,\n",
            "    pub y: f32,\n",
            "}\n",
            "pub fn pick(pair: (Location, i32)) -> i32 {\n",
            "    match pair {\n",
            "        (position, n) => n,\n",
            "    }\n",
            "}\n",
            "pub fn keep(value: f64) -> f64 {\n",
            "    let held = value;\n",
            "    held\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("float bindings must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
    }

    #[test]
    fn ordinary_for_selects_the_unique_iterator_impl_pair() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub struct Src {\n",
            "    pub start: i32,\n",
            "}\n",
            "pub struct It {\n",
            "    current: i32,\n",
            "}\n",
            "impl IntoIterator<Src, It> for Src {\n",
            "    fn into_iter(self) -> It {\n",
            "        It { current: self.start }\n",
            "    }\n",
            "}\n",
            "impl Iterator<It, i32> for It {\n",
            "    fn next(&mut self) -> Option<i32> {\n",
            "        Option::None\n",
            "    }\n",
            "}\n",
            "pub fn total(source: Src) -> i32 {\n",
            "    let mut sum = 0i32;\n",
            "    for element in source {\n",
            "        sum += element;\n",
            "    }\n",
            "    sum\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("for selection must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
        let calls = bodies
            .bodies()
            .flat_map(C2BodyView::calls)
            .filter_map(|call| match call.callee() {
                CheckedBodyCallee::TraitMethod { trait_path, method } => {
                    Some((trait_path.name.clone(), method.as_ref(), call.result()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            calls.iter().any(|(trait_name, method, result)| {
                trait_name == "IntoIterator"
                    && *method == "into_iter"
                    && matches!(
                        result,
                        SymbolicType::NominalPath { declaration, .. } if declaration.name == "It"
                    )
            }),
            "calls={calls:#?}"
        );
        assert!(
            calls.iter().any(|(trait_name, method, result)| {
                trait_name == "Iterator"
                    && *method == "next"
                    && matches!(
                        result,
                        SymbolicType::NominalPath { declaration, arguments }
                            if declaration.name == "Option"
                                && arguments
                                    == &[GenericArgumentShape::Type(SymbolicType::I32)]
                    )
            }),
            "calls={calls:#?}"
        );
    }

    #[test]
    fn ordinary_for_without_an_impl_pair_stays_an_honest_gap() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn spin(limit: i32) -> i32 {\n",
            "    let mut sum = 0i32;\n",
            "    for element in limit {\n",
            "        sum += element;\n",
            "    }\n",
            "    sum\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        assert!(
            failure.diagnostics().is_none(),
            "an absent impl pair is a recorded gap, not a rejection: {:?}",
            failure.diagnostics()
        );
        assert!(failure
            .incompleteness()
            .iter()
            .any(|gap| gap.kind() == BodyCheckIncompletenessKind::MissingMethodSelection));
    }

    #[test]
    fn compound_for_sources_are_never_silently_dropped() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn raw_array() -> i32 {\n",
            "    for _ in [1i32, 2i32, 3i32] {\n",
            "    }\n",
            "    0i32\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        assert!(
            failure.diagnostics().is_none(),
            "a raw-array for keeps its selection gap until the negative matrix: {:?}",
            failure.diagnostics()
        );
        assert!(failure
            .incompleteness()
            .iter()
            .any(|gap| gap.kind() == BodyCheckIncompletenessKind::MissingMethodSelection));

        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub struct Src {\n",
            "    pub start: i32,\n",
            "}\n",
            "pub struct It {\n",
            "    current: i32,\n",
            "}\n",
            "impl IntoIterator<Src, It> for Src {\n",
            "    fn into_iter(self) -> It {\n",
            "        It { current: self.start }\n",
            "    }\n",
            "}\n",
            "impl Iterator<It, i32> for It {\n",
            "    fn next(&mut self) -> Option<i32> {\n",
            "        Option::None\n",
            "    }\n",
            "}\n",
            "pub fn block_source(source: Src) -> i32 {\n",
            "    let mut sum = 0i32;\n",
            "    for element in { source } {\n",
            "        sum += element;\n",
            "    }\n",
            "    sum\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("a block source must select: {failure:#?}"));
        assert!(bodies.all_authority_complete());
    }

    #[test]
    fn ambiguous_iterator_impls_reject_even_when_the_binding_is_used() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub struct Src {\n",
            "    pub start: i32,\n",
            "}\n",
            "pub struct It {\n",
            "    current: i32,\n",
            "}\n",
            "pub struct It2 {\n",
            "    current: i32,\n",
            "}\n",
            "impl IntoIterator<Src, It> for Src {\n",
            "    fn into_iter(self) -> It {\n",
            "        It { current: self.start }\n",
            "    }\n",
            "}\n",
            "impl IntoIterator<Src, It2> for Src {\n",
            "    fn into_iter(self) -> It2 {\n",
            "        It2 { current: self.start }\n",
            "    }\n",
            "}\n",
            "impl Iterator<It, i32> for It {\n",
            "    fn next(&mut self) -> Option<i32> {\n",
            "        Option::None\n",
            "    }\n",
            "}\n",
            "impl Iterator<It2, i32> for It2 {\n",
            "    fn next(&mut self) -> Option<i32> {\n",
            "        Option::None\n",
            "    }\n",
            "}\n",
            "pub fn used_binding(source: Src) -> i32 {\n",
            "    let mut total = 0i32;\n",
            "    for element in source {\n",
            "        total += element;\n",
            "    }\n",
            "    total\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        let diagnostics = format!("{:?}", failure.diagnostics());
        assert!(
            failure.diagnostics().is_some(),
            "gaps={:?}",
            failure.incompleteness()
        );
        assert!(
            diagnostics.contains("TRAIT002"),
            "diagnostics={diagnostics}"
        );
    }

    #[test]
    fn catch_types_arms_against_the_declared_singleton_throws_set() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub enum Fault {\n",
            "    Soft,\n",
            "    Hard(i32),\n",
            "}\n",
            "pub fn risky(x: i32) throws { Fault } -> i32 {\n",
            "    if x <= 0i32 {\n",
            "        throw Fault::Soft;\n",
            "    }\n",
            "    x\n",
            "}\n",
            "pub fn direct(x: i32) -> i32 {\n",
            "    catch risky(x) {\n",
            "        Fault::Soft => 0i32,\n",
            "        Fault::Hard(code) => code,\n",
            "    }\n",
            "}\n",
            "pub fn through_pointer(\n",
            "    callback: fn(i32) requires {} throws { Fault } -> i32,\n",
            "    x: i32,\n",
            ") -> i32 {\n",
            "    catch callback(x) {\n",
            "        Fault::Soft => 0i32,\n",
            "        Fault::Hard(code) => code,\n",
            "    }\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("declared singleton catch must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
    }

    #[test]
    fn catch_over_an_unknown_throws_set_stays_an_honest_gap() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn opaque(x: i32) -> i32 {\n",
            "    catch x {\n",
            "        _ => 0i32,\n",
            "    }\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        assert!(
            failure.diagnostics().is_none(),
            "a non-call catch operand is a recorded gap: {:?}",
            failure.diagnostics()
        );
        assert!(failure
            .incompleteness()
            .iter()
            .any(|gap| gap.kind() == BodyCheckIncompletenessKind::MissingEffectAuthority));
    }

    #[test]
    fn nonexhaustive_catch_arms_are_pattern002() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub enum Fault {\n",
            "    Soft,\n",
            "    Hard(i32),\n",
            "}\n",
            "pub fn risky(x: i32) throws { Fault } -> i32 {\n",
            "    if x <= 0i32 {\n",
            "        throw Fault::Soft;\n",
            "    }\n",
            "    x\n",
            "}\n",
            "pub fn partial_arms(x: i32) -> i32 {\n",
            "    catch risky(x) {\n",
            "        Fault::Soft => 0i32,\n",
            "    }\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        let diagnostics = format!("{:?}", failure.diagnostics());
        assert!(failure.diagnostics().is_some());
        assert!(
            diagnostics.contains("PATTERN002"),
            "diagnostics={diagnostics}"
        );
    }

    #[test]
    fn catch_joins_the_operand_result_with_the_arms() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub enum Fault {\n",
            "    Soft,\n",
            "    Hard(i32),\n",
            "}\n",
            "pub fn risky(x: i32) throws { Fault } -> i32 {\n",
            "    if x <= 0i32 {\n",
            "        throw Fault::Soft;\n",
            "    }\n",
            "    x\n",
            "}\n",
            "pub fn leaked(x: i32) -> i32 {\n",
            "    let v = catch risky(x) {\n",
            "        Fault::Soft => 'a',\n",
            "        Fault::Hard(_) => 'b',\n",
            "    };\n",
            "    x\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        let diagnostics = format!("{:?}", failure.diagnostics());
        assert!(failure.diagnostics().is_some());
        assert!(diagnostics.contains("TYPE002"), "diagnostics={diagnostics}");

        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub enum Fault {\n",
            "    Soft,\n",
            "    Hard(i32),\n",
            "}\n",
            "pub fn risky(x: i32) throws { Fault } -> i32 {\n",
            "    if x <= 0i32 {\n",
            "        throw Fault::Soft;\n",
            "    }\n",
            "    x\n",
            "}\n",
            "pub fn takes_char(c: char) -> i32 {\n",
            "    0i32\n",
            "}\n",
            "pub fn feed(x: i32) -> i32 {\n",
            "    let v = catch risky(x) {\n",
            "        Fault::Soft => return 7i32,\n",
            "        Fault::Hard(_) => return 9i32,\n",
            "    };\n",
            "    takes_char(v)\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        let diagnostics = format!("{:?}", failure.diagnostics());
        assert!(
            failure.diagnostics().is_some(),
            "the seeded operand result must survive all-diverging arms: {:?}",
            failure.incompleteness()
        );
        assert!(
            diagnostics.contains("expected char, found i32"),
            "diagnostics={diagnostics}"
        );
    }

    #[test]
    fn generic_declared_throws_keep_the_effect_gap() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub enum Fault {\n",
            "    Soft,\n",
            "}\n",
            "pub fn risky<E>(x: i32, seed: E) throws { E } -> i32 {\n",
            "    x\n",
            "}\n",
            "pub fn caught(x: i32) -> i32 {\n",
            "    catch risky(x, Fault::Soft) {\n",
            "        _ => 0i32,\n",
            "    }\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        assert!(
            failure.diagnostics().is_none(),
            "an unsubstituted throws set is later authority: {:?}",
            failure.diagnostics()
        );
        assert!(failure
            .incompleteness()
            .iter()
            .any(|gap| gap.kind() == BodyCheckIncompletenessKind::MissingEffectAuthority));
    }

    #[test]
    fn never_operands_are_the_catch_join_identity() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub enum Fault {\n",
            "    Soft,\n",
            "    Hard(i32),\n",
            "}\n",
            "pub fn boom(x: i32) throws { Fault } -> ! {\n",
            "    loop {\n",
            "    }\n",
            "}\n",
            "pub fn consume(x: i32) -> i32 {\n",
            "    let v = catch boom(x) {\n",
            "        Fault::Soft => 0i32,\n",
            "        Fault::Hard(code) => code,\n",
            "    };\n",
            "    v\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("a never operand constrains nothing: {failure:#?}"));
        assert!(bodies.all_authority_complete());
    }

    #[test]
    fn generic_control_flow_bindings_close_end_to_end() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub enum Choice<T> {\n",
            "    None,\n",
            "    One(T),\n",
            "    Pair { left: T, right: T },\n",
            "}\n",
            "pub fn walk(seed: i32, bits: i32) -> i32 {\n",
            "    let mut destination = seed;\n",
            "    if destination > 0i32 {\n",
            "        destination += 1i32;\n",
            "    } else if let Choice::One(inner) = Choice::One(destination) {\n",
            "        destination = inner;\n",
            "    }\n",
            "    while let Choice::One(inner) = Choice::One(destination) {\n",
            "        destination = inner;\n",
            "        break;\n",
            "    }\n",
            "    let loop_value = loop {\n",
            "        break destination;\n",
            "    };\n",
            "    let matched = match (Choice::Pair { left: loop_value, right: bits }) {\n",
            "        Choice::None => 0i32,\n",
            "        Choice::One(0i32..=3i32) => 1i32,\n",
            "        Choice::One(name @ 4i32..8i32) if name != 6i32 => name,\n",
            "        Choice::Pair {\n",
            "            left: left @ -10i32..0i32 | left @ 8i32..16i32,\n",
            "            right: _,\n",
            "        } => left,\n",
            "        _ => -1i32,\n",
            "    };\n",
            "    matched\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies =
            check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_or_else(|failure| {
                panic!("generic control-flow bindings must close: {failure:#?}")
            });
        assert!(bodies.all_authority_complete());
    }

    #[test]
    fn while_and_for_loops_accept_only_bare_break() {
        for (body, line, column) in [
            (
                "pub fn probe(flag: bool) -> i32 {\n    while flag {\n        break ();\n    }\n    0i32\n}\n",
                3u32,
                9u32,
            ),
            (
                "pub fn probe(flag: bool) -> i32 {\n    while flag {\n        break 5i32;\n    }\n    0i32\n}\n",
                3,
                9,
            ),
            (
                "pub fn probe(flag: bool) -> i32 {\n    loop {\n        while flag {\n            break 9i32;\n        }\n    }\n}\n",
                4,
                13,
            ),
        ] {
            let handoff = C2Handoff::begin(inline_frontend(body)).unwrap();
            let declarations = DeclarationTable::build(&handoff).unwrap();
            let checked = check_declarations_c2(&handoff, &declarations).unwrap();
            let failure =
                check_workspace_bodies_c2(&handoff, &declarations, &checked).unwrap_err();
            let diagnostics = format!("{:?}", failure.diagnostics());
            assert!(
                diagnostics.contains("accept only a bare `break`")
                    && diagnostics.contains("TYPE002"),
                "body={body} diagnostics={diagnostics}"
            );
            assert!(
                diagnostics.contains(&format!("line: {line}, column: {column}")),
                "the rejection must sit at the break's own span; body={body} diagnostics={diagnostics}"
            );
            assert!(failure.incompleteness().is_empty(), "body={body}");
        }

        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub struct Src {\n",
            "    pub start: i32,\n",
            "}\n",
            "pub struct It {\n",
            "    current: i32,\n",
            "}\n",
            "impl IntoIterator<Src, It> for Src {\n",
            "    fn into_iter(self) -> It {\n",
            "        It { current: self.start }\n",
            "    }\n",
            "}\n",
            "impl Iterator<It, i32> for It {\n",
            "    fn next(&mut self) -> Option<i32> {\n",
            "        Option::None\n",
            "    }\n",
            "}\n",
            "pub fn probe(source: Src) -> i32 {\n",
            "    for element in source {\n",
            "        break 2i32;\n",
            "    }\n",
            "    0i32\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        let diagnostics = format!("{:?}", failure.diagnostics());
        assert!(
            diagnostics.contains("accept only a bare `break`")
                && diagnostics.contains("TYPE002")
                && diagnostics.contains("line: 19, column: 9"),
            "diagnostics={diagnostics}"
        );
    }

    #[test]
    fn bare_breaks_close_in_while_and_for_loops() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn probe(flag: bool) -> i32 {\n",
            "    while flag {\n",
            "        break;\n",
            "    }\n",
            "    let joined = loop {\n",
            "        while flag {\n",
            "            break;\n",
            "        }\n",
            "        break 7i32;\n",
            "    };\n",
            "    joined\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked = check_declarations_c2(&handoff, &declarations).unwrap();
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, &checked)
            .unwrap_or_else(|failure| panic!("bare breaks must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());

        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub struct Src {\n",
            "    pub start: i32,\n",
            "}\n",
            "pub struct It {\n",
            "    current: i32,\n",
            "}\n",
            "impl IntoIterator<Src, It> for Src {\n",
            "    fn into_iter(self) -> It {\n",
            "        It { current: self.start }\n",
            "    }\n",
            "}\n",
            "impl Iterator<It, i32> for It {\n",
            "    fn next(&mut self) -> Option<i32> {\n",
            "        Option::None\n",
            "    }\n",
            "}\n",
            "pub fn probe(source: Src) -> i32 {\n",
            "    for element in source {\n",
            "        break;\n",
            "    }\n",
            "    0i32\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("bare for break must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
    }

    #[test]
    fn nested_bare_breaks_close_in_while_and_for_subtrees() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn probe(flag: bool) -> i32 {\n",
            "    while flag {\n",
            "        match flag {\n",
            "            true => break,\n",
            "            false => {}\n",
            "        }\n",
            "    }\n",
            "    0i32\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked = check_declarations_c2(&handoff, &declarations).unwrap();
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, &checked)
            .unwrap_or_else(|failure| panic!("nested while break must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());

        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub struct Src {\n",
            "    pub start: i32,\n",
            "}\n",
            "pub struct It {\n",
            "    current: i32,\n",
            "}\n",
            "impl IntoIterator<Src, It> for Src {\n",
            "    fn into_iter(self) -> It {\n",
            "        It { current: self.start }\n",
            "    }\n",
            "}\n",
            "impl Iterator<It, i32> for It {\n",
            "    fn next(&mut self) -> Option<i32> {\n",
            "        Option::None\n",
            "    }\n",
            "}\n",
            "pub fn probe(source: Src, flag: bool) -> i32 {\n",
            "    for element in source {\n",
            "        match flag {\n",
            "            true => break,\n",
            "            false => {}\n",
            "        }\n",
            "    }\n",
            "    0i32\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("nested for break must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
    }

    #[test]
    fn nested_breaks_under_loop_stay_honest_gaps() {
        for body in [
            concat!(
                "pub fn probe(flag: bool) -> i32 {\n",
                "    loop {\n",
                "        match flag {\n",
                "            true => break,\n",
                "            false => {}\n",
                "        }\n",
                "    }\n",
                "    0i32\n",
                "}\n",
            ),
            concat!(
                "pub enum Choice {\n",
                "    One(i32),\n",
                "    Two,\n",
                "}\n",
                "pub fn probe(value: Choice) -> i32 {\n",
                "    loop {\n",
                "        let Choice::One(number) = value else {\n",
                "            break;\n",
                "        };\n",
                "        return number;\n",
                "    }\n",
                "    0i32\n",
                "}\n",
            ),
            concat!(
                "pub fn probe(flag: bool) -> i32 {\n",
                "    let mut total = 0i32;\n",
                "    loop {\n",
                "        total = if flag { 1i32 } else { break };\n",
                "    }\n",
                "    total\n",
                "}\n",
            ),
            concat!(
                "pub fn probe(flag: bool) -> i32 {\n",
                "    loop {\n",
                "        match flag {\n",
                "            true => break 1i32,\n",
                "            false => 0i32,\n",
                "        };\n",
                "    }\n",
                "    0i32\n",
                "}\n",
            ),
            concat!(
                "pub fn probe(flag: bool) -> i32 {\n",
                "    loop {\n",
                "        match flag {\n",
                "            true => break 1,\n",
                "            false => {}\n",
                "        }\n",
                "    }\n",
                "}\n",
            ),
            concat!(
                "pub fn probe(flag: bool) -> f64 {\n",
                "    loop {\n",
                "        match flag {\n",
                "            true => break 1.0,\n",
                "            false => {}\n",
                "        }\n",
                "    }\n",
                "}\n",
            ),
            concat!(
                "pub fn probe(flag: bool) -> i32 {\n",
                "    loop {\n",
                "        match flag {\n",
                "            true => break \"text\",\n",
                "            false => {}\n",
                "        }\n",
                "    }\n",
                "}\n",
            ),
            concat!(
                "pub fn probe(kind: i32) -> i32 {\n",
                "    loop {\n",
                "        match kind {\n",
                "            0i32 => break 1i32,\n",
                "            1i32 => break true,\n",
                "            _ => {}\n",
                "        }\n",
                "    }\n",
                "}\n",
            ),
        ] {
            let handoff = C2Handoff::begin(inline_frontend(body)).unwrap();
            let declarations = DeclarationTable::build(&handoff).unwrap();
            let checked_result = check_declarations_c2(&handoff, &declarations);
            let checked = match &checked_result {
                Ok(facts) => facts,
                Err(failure) => failure.partial(),
            };
            let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
            assert!(
                failure.diagnostics().is_none(),
                "a swallowed loop break must gap, not reject; body={body} diagnostics={:?}",
                failure.diagnostics()
            );
            let incompleteness = format!("{:?}", failure.incompleteness());
            assert!(
                incompleteness.contains("does not retain the enclosing loop join"),
                "body={body} incompleteness={incompleteness}"
            );
        }
    }

    #[test]
    fn direct_break_against_annotated_loop_still_rejects() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn probe() -> bool {\n",
            "    loop {\n",
            "        break 1i32;\n",
            "    }\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked = check_declarations_c2(&handoff, &declarations).unwrap();
        let failure = check_workspace_bodies_c2(&handoff, &declarations, &checked).unwrap_err();
        let diagnostics = format!("{:?}", failure.diagnostics());
        assert!(
            diagnostics.contains("TYPE002") && diagnostics.contains("expected bool, found i32"),
            "diagnostics={diagnostics} incompleteness={:?}",
            failure.incompleteness()
        );
    }

    #[test]
    fn noncapturing_annotated_closures_type_as_closure_values() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn holder(seed: i32) -> i32 {\n",
            "    let doubler = move |value: i32|\n",
            "        requires {} throws {} -> i32 {\n",
            "            value + value\n",
            "        };\n",
            "    let factory = gen move |start: i32|\n",
            "        resume i32 yields i32 requires {} throws {} -> i32 {\n",
            "            let next = yield start;\n",
            "            next\n",
            "        };\n",
            "    seed\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("noncapturing values must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
    }

    #[test]
    fn capturing_closures_keep_the_capture_authority_gap() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn holder(seed: i32) -> i32 {\n",
            "    let outer = seed;\n",
            "    let adder = move |value: i32|\n",
            "        requires {} throws {} -> i32 {\n",
            "            value + outer\n",
            "        };\n",
            "    seed\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        assert!(
            failure.diagnostics().is_none(),
            "captures are C4 authority, not rejections: {:?}",
            failure.diagnostics()
        );
        assert!(failure
            .incompleteness()
            .iter()
            .any(|gap| gap.kind() == BodyCheckIncompletenessKind::MissingClosureType));
    }

    #[test]
    fn closure_and_factory_values_are_callable_with_exact_signatures() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn drive(seed: i32) -> i32 {\n",
            "    let doubler = move |value: i32|\n",
            "        requires {} throws {} -> i32 {\n",
            "            value + value\n",
            "        };\n",
            "    let factory = gen move |start: i32|\n",
            "        resume i32 yields i32 requires {} throws {} -> i32 {\n",
            "            let next = yield start;\n",
            "            next\n",
            "        };\n",
            "    let state = factory(seed);\n",
            "    doubler(seed)\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("callable values must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
        let callees = bodies
            .bodies()
            .flat_map(C2BodyView::calls)
            .map(CheckedBodyCall::callee)
            .collect::<Vec<_>>();
        assert!(callees.contains(&&CheckedBodyCallee::ClosureValue));
        assert!(callees.contains(&&CheckedBodyCallee::GeneratorFactoryValue));
    }

    #[test]
    fn closure_call_argument_mismatches_are_type002() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn drive(seed: i32) -> i32 {\n",
            "    let doubler = move |value: i32|\n",
            "        requires {} throws {} -> i32 {\n",
            "            value + value\n",
            "        };\n",
            "    doubler('x')\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        let diagnostics = format!("{:?}", failure.diagnostics());
        assert!(failure.diagnostics().is_some());
        assert!(diagnostics.contains("TYPE002"), "diagnostics={diagnostics}");
    }

    #[test]
    fn generic_owners_carry_identity_arguments_on_closure_values() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub enum Fault2 {\n",
            "    Soft,\n",
            "}\n",
            "pub fn owner<'a, T: Clone + 'a, const N: usize>(seed: i32) -> i32 {\n",
            "    let doubler = move |value: i32|\n",
            "        requires {} throws {} -> i32 {\n",
            "            value + value\n",
            "        };\n",
            "    let factory = gen move |start: i32|\n",
            "        resume i32 yields i32 requires {} throws { Fault2 } -> i32 {\n",
            "            let next = yield start;\n",
            "            next\n",
            "        };\n",
            "    let state = factory(seed);\n",
            "    doubler(seed)\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies =
            check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_or_else(|failure| {
                panic!("generic-owner closure values must close: {failure:#?}")
            });
        assert!(bodies.all_authority_complete());
        let closure_locals: Vec<String> = bodies
            .bodies()
            .flat_map(C2BodyView::locals)
            .map(|local| format!("{local:?}"))
            .filter(|rendered| rendered.contains("ty: Closure {"))
            .collect();
        assert!(
            closure_locals.iter().any(|rendered| {
                rendered.contains(concat!(
                    "arguments: [Lifetime(Bound { depth: 0, index: 0 }), ",
                    "Type(BoundType { depth: 0, index: 1 }), ",
                    "IntegerConst(SymbolicConstExpression { integer_type: Usize, ",
                    "node: Bound { depth: 0, index: 2 } })]",
                )) && rendered.contains("expression_ordinal: 1")
                    && rendered.contains("captures: []")
            }),
            "closure locals must carry the owner's exact identity arguments: \
             {closure_locals:#?}"
        );
    }

    #[test]
    fn unchecked_pin_construction_infers_from_its_argument() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn drive(seed: i32) -> i32 {\n",
            "    let factory = gen move |start: i32|\n",
            "        resume i32 yields i32 requires {} throws {} -> i32 {\n",
            "            let next = yield start;\n",
            "            next\n",
            "        };\n",
            "    let mut state = factory(seed);\n",
            "    let pinned = unsafe { Pin::new_unchecked(&mut state) };\n",
            "    seed\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("unchecked pin must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
    }

    #[test]
    fn checked_pin_keeps_the_unpin_bound_gap_and_unsafe_stays_gated() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn checked(seed: i32) -> i32 {\n",
            "    let factory = gen move |start: i32|\n",
            "        resume i32 yields i32 requires {} throws {} -> i32 {\n",
            "            let next = yield start;\n",
            "            next\n",
            "        };\n",
            "    let mut state = factory(seed);\n",
            "    let pinned = Pin::new(&mut state);\n",
            "    seed\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        assert!(
            failure.diagnostics().is_none(),
            "the Unpin bound awaits its structural judgment: {:?}",
            failure.diagnostics()
        );
        assert!(!failure.incompleteness().is_empty());

        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn no_unsafe(seed: i32) -> i32 {\n",
            "    let factory = gen move |start: i32|\n",
            "        resume i32 yields i32 requires {} throws {} -> i32 {\n",
            "            let next = yield start;\n",
            "            next\n",
            "        };\n",
            "    let mut state = factory(seed);\n",
            "    let pinned = Pin::new_unchecked(&mut state);\n",
            "    seed\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        let diagnostics = format!("{:?}", failure.diagnostics());
        assert!(failure.diagnostics().is_some());
        assert!(
            diagnostics.contains("unsafe context"),
            "diagnostics={diagnostics}"
        );
    }

    #[test]
    fn pinned_resume_types_the_generator_state() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn drive(seed: i32) -> i32 {\n",
            "    let factory = gen move |start: i32|\n",
            "        resume i32 yields i32 requires {} throws {} -> i32 {\n",
            "            let next = yield start;\n",
            "            next\n",
            "        };\n",
            "    let mut state = factory(seed);\n",
            "    let resumed = unsafe { Pin::new_unchecked(&mut state) }.resume(1i32);\n",
            "    seed\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let bodies = check_workspace_bodies_c2(&handoff, &declarations, checked)
            .unwrap_or_else(|failure| panic!("pinned resume must close: {failure:#?}"));
        assert!(bodies.all_authority_complete());
        assert!(bodies
            .bodies()
            .flat_map(C2BodyView::calls)
            .any(|call| call.callee() == &CheckedBodyCallee::GeneratorResume
                && matches!(
                    call.result(),
                    SymbolicType::NominalPath { declaration, arguments }
                        if declaration.name == "GeneratorState"
                            && arguments.len() == 2
                )));
    }

    #[test]
    fn resume_off_the_pin_stays_the_suspension_gap() {
        let handoff = C2Handoff::begin(inline_frontend(concat!(
            "pub fn drive(seed: i32) -> i32 {\n",
            "    let factory = gen move |start: i32|\n",
            "        resume i32 yields i32 requires {} throws {} -> i32 {\n",
            "            let next = yield start;\n",
            "            next\n",
            "        };\n",
            "    let mut state = factory(seed);\n",
            "    let resumed = state.resume(1i32);\n",
            "    seed\n",
            "}\n",
        )))
        .unwrap();
        let declarations = DeclarationTable::build(&handoff).unwrap();
        let checked_result = check_declarations_c2(&handoff, &declarations);
        let checked = match &checked_result {
            Ok(facts) => facts,
            Err(failure) => failure.partial(),
        };
        let failure = check_workspace_bodies_c2(&handoff, &declarations, checked).unwrap_err();
        assert!(
            failure.diagnostics().is_none(),
            "unpinned resume is later authority, not a rejection: {:?}",
            failure.diagnostics()
        );
        assert!(failure
            .incompleteness()
            .iter()
            .any(|gap| gap.kind() == BodyCheckIncompletenessKind::MissingGeneratorType));
    }
}
