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
    CompilerMethodGenericArgumentPattern, CompilerMethodLifetimePattern,
    CompilerMethodSelectorKind, CompilerMethodTypePattern, CompilerNominalKind,
    CompilerNominalMethodReceiverMode, VirtualDefinitionId, VirtualEnumVariantId, VirtualMethodId,
    VirtualNamespace, VirtualPreludeTarget, VirtualTypeFlavor,
};
use arche_frontend::{
    AssociatedPathCandidate, BuiltinResTarget, DeclarationKind, Diagnostic, FileId,
    GenericArgumentShape, HirBodyId, HirBodySource, HirItemId, HirItemRes, HirItemSource, LocalId,
    Mutability, PathResolution, Res, ResolvedGenericArgument, ResolvedSymbolicBody,
    ResolvedSymbolicItem, ResolvedSymbolicTargetHir, ResolvedSymbolicType, SemanticBodyKind,
    SemanticDeclarationPath, SemanticDefinitionInventorySkeleton, Span, SymbolicConstExpression,
    SymbolicConstNode, SymbolicDeclarationPayloadSkeleton, SymbolicDeclarationShapeSkeleton,
    SymbolicDefinitionOwnerSkeleton, SymbolicLifetime, SymbolicPredicate,
    SymbolicPredicateShapeSkeleton, SymbolicRecordForm, SymbolicType, SymbolicTypeEffectSet,
    SymbolicTypeShapeSkeleton, TargetId, TargetRoot, UnresolvedPathKind,
};
use arche_package::PortablePath;

use crate::declaration_check::CheckedDeclarationFacts;
use crate::declarations::DeclarationTable;
use crate::diagnostic::{
    CompilationPhase, NonEmptySemanticDiagnostics, ScopedPackageBytes, SemanticDiagnostic,
};
use crate::model::{
    C2Handoff, C2Resolution, NeedsCtfeObligation, NeedsCtfeObligations, PendingC4Dependencies,
    PendingC4Dependency, SessionBrand,
};
use crate::pattern::{
    analyze_pattern_match, check_irrefutable_pattern, BindingAnnotation, BindingMode, EnumType,
    EnumVariant, IntegerType as PatternIntegerType, IrrefutablePatternAnalysis, Pattern,
    PatternArm, PatternBinding, PatternConst, PatternErrors, PatternLiteral, PatternMatchAnalysis,
    PatternScrutinee, PatternType, PlaceMutability, RangeEndpoint, RecordField, RecordPatternField,
    RecordType, ReferenceMutability, TypedPattern, TypedPatternKind,
};
use crate::typing::{
    check_typed_expression, BinaryTypeOperator, CheckedExpression, TypeCheckError,
    TypeCheckErrorKind, TypedExpressionInput, TypingContext, UnaryTypeOperator,
};
use crate::{BinderFrame, BinderStack, LifetimeOutlives};

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
    EmbeddedMethod(VirtualMethodId),
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
    PendingDirectFunction(HirItemId),
    PendingAssociatedFunction(HirItemId),
    Constructor(ConstructorSelection),
    Query { item: SymbolicType },
    Commands,
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
    EmbeddedRecord(VirtualDefinitionId),
    EmbeddedVariant(VirtualEnumVariantId),
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
    source_loop_depth: usize,
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
            source_loop_depth: 0,
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
        self.source_error(span, error.code().as_str(), error.to_string());
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
        match check_typed_expression(&lowered.input, expected, &self.typing) {
            Ok(checked) => {
                self.expressions.push(CheckedBodyExpression {
                    span,
                    expression: checked.clone(),
                });
                Some(checked)
            }
            Err(error) => {
                if let TypeCheckErrorKind::Mismatch { expected, actual } = error.kind() {
                    if types_match_with_erased_body_lifetime(expected, actual) {
                        self.gap(
                            span,
                            BodyCheckIncompletenessKind::MissingBodyLocalLifetime,
                            "body-local borrow lifetime cannot yet be related to the declaration-bound lifetime",
                        );
                        return self.materialize(lowered, None, span);
                    }
                }
                if matches!(error.kind(), TypeCheckErrorKind::BreakOutsideLoop) {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::UnsupportedC2AdapterSurface,
                        "the expression typing algebra does not retain while/for loop frames for nested break expressions",
                    );
                    return None;
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
                let body = self.lower_loop_body(block)?;
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
            AstExpressionKind::Unsafe(block) => self.lower_block(block, expected),
            AstExpressionKind::Closure(_closure) => {
                self.gap(
                    expression.span,
                    BodyCheckIncompletenessKind::MissingClosureType,
                    "closure expression type requires the C4 capture/Fn-category authority",
                );
                None
            }
            AstExpressionKind::GeneratorClosure(_generator) => {
                self.gap(
                    expression.span,
                    BodyCheckIncompletenessKind::MissingGeneratorType,
                    "generator-closure factory type requires the C4 capture authority",
                );
                None
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
                if self.source_loop_depth == 0 {
                    self.source_error(expression.span, "TYPE002", "break used outside a loop");
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
                if self.source_loop_depth == 0 {
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
            ValueCategory::Constructor(ConstructorSelection::EmbeddedVariant(variant)) => {
                for field in fields {
                    self.check_expression(&field.value, None);
                }
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                    format!("embedded enum variant {variant:?} has no typed C2 field descriptor"),
                );
                return None;
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
        let mut value = self.lower_expression(base, expected)?;
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
                    if owner == Some(CompilerNominalKind::App)
                        && matches!(name, arche_frontend::ast::AstMethodName::Run)
                    {
                        let receiver = receiver?;
                        self.lower_app_run_method(
                            receiver.ty(),
                            generic_arguments.as_ref(),
                            arguments,
                            part.span,
                        )?
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
                    self.check_expression(resume, None);
                    self.gap(
                        part.span,
                        BodyCheckIncompletenessKind::MissingGeneratorType,
                        "resume postfix requires finalized generator suspension-state typing",
                    );
                    return None;
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

    fn lower_app_run_method(
        &mut self,
        receiver_ty: &SymbolicType,
        generic_arguments: Option<&arche_frontend::ast::AstGenericArguments>,
        arguments: &[AstExpression],
        span: Span,
    ) -> Option<LoweredValue> {
        let Some((
            method,
            receiver_mode,
            receiver_pattern,
            parameter_patterns,
            selector_kinds,
            result_pattern,
            has_effects,
        )) = (|| {
            let core = &self.catalog.handoff.frontend().inventory().embedded_core;
            let authority = core.compiler_nominal_method(CompilerNominalKind::App, "run")?;
            Some((
                authority.c1_method(),
                authority.receiver(),
                authority.receiver_type()?.clone(),
                authority.parameters().to_vec(),
                authority
                    .selectors()
                    .iter()
                    .map(|selector| selector.kind())
                    .collect::<Vec<_>>(),
                authority.result().clone(),
                !authority.requires().is_empty() || !authority.throws().is_empty(),
            ))
        })()
        else {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                "verified App.run intrinsic method authority is absent",
            );
            return None;
        };

        let mut complete = true;
        if receiver_mode != CompilerNominalMethodReceiverMode::Mutable
            || !self.intrinsic_shape_matches(&receiver_pattern, receiver_ty)
        {
            self.source_error(span, "TYPE002", "App.run requires a mutable App receiver");
            complete = false;
        }
        if let Some(generic_arguments) = generic_arguments {
            let _ = self.postfix_actuals(generic_arguments.span);
            self.gap(
                generic_arguments.span,
                BodyCheckIncompletenessKind::MissingGenericInference,
                "explicit App.run intrinsic generics are not yet substituted into verified method patterns",
            );
            complete = false;
        }
        if arguments.len() != selector_kinds.len() + parameter_patterns.len() {
            self.source_error(
                span,
                "TYPE002",
                format!(
                    "App.run expects {} selector/runtime arguments, found {}",
                    selector_kinds.len() + parameter_patterns.len(),
                    arguments.len()
                ),
            );
            complete = false;
        }

        let mut selected_items = Vec::new();
        for (index, selector_kind) in selector_kinds.iter().enumerate() {
            let Some(argument) = arguments.get(index) else {
                continue;
            };
            if *selector_kind != CompilerMethodSelectorKind::DefinitionId {
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

        for (index, pattern) in parameter_patterns.iter().enumerate() {
            let Some(argument) = arguments.get(selector_kinds.len() + index) else {
                continue;
            };
            let Some(checked) = self.check_expression(argument, None) else {
                complete = false;
                continue;
            };
            if !self.intrinsic_shape_matches(pattern, checked.ty()) {
                self.source_error(
                    argument.span,
                    "TYPE002",
                    format!(
                        "App.run runtime argument {:?} does not match verified intrinsic pattern {pattern:?}",
                        checked.ty()
                    ),
                );
                complete = false;
            }
        }

        let result = match &result_pattern {
            CompilerMethodTypePattern::Tuple(fields) if fields.is_empty() => SymbolicType::Unit,
            _ => {
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                    "App.run verified result is outside the currently lowered intrinsic result algebra",
                );
                return None;
            }
        };
        if !complete {
            return None;
        }
        if has_effects {
            let mut bytes = b"ARCHE-C2-APP-RUN-EFFECTS\0".to_vec();
            bytes.extend_from_slice(&method.ordinal().to_le_bytes());
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
            callee: CheckedBodyCallee::EmbeddedMethod(method),
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

    fn intrinsic_shape_matches(
        &self,
        pattern: &CompilerMethodTypePattern,
        actual: &SymbolicType,
    ) -> bool {
        match pattern {
            CompilerMethodTypePattern::Generic(_) => true,
            CompilerMethodTypePattern::Definition {
                definition,
                arguments,
            } => {
                let SymbolicType::NominalPath {
                    declaration,
                    arguments: actual_arguments,
                } = actual
                else {
                    return false;
                };
                if self.embedded_core_definition_for_path(declaration) != Some(*definition) {
                    return false;
                }
                if self.is_exact_caps_pack_pattern(*definition, arguments, actual_arguments) {
                    return true;
                }
                if arguments.len() != actual_arguments.len() {
                    return false;
                }
                arguments
                    .iter()
                    .zip(actual_arguments)
                    .all(|(pattern, actual)| match (pattern, actual) {
                        (
                            CompilerMethodGenericArgumentPattern::Type(pattern),
                            GenericArgumentShape::Type(actual),
                        ) => self.intrinsic_shape_matches(pattern, actual),
                        (
                            CompilerMethodGenericArgumentPattern::Lifetime(_),
                            GenericArgumentShape::Lifetime(_),
                        ) => true,
                        _ => false,
                    })
            }
            CompilerMethodTypePattern::SharedReference { lifetime, referent } => match actual {
                SymbolicType::Reference {
                    mutability,
                    lifetime: actual_lifetime,
                    pointee,
                } => {
                    (*mutability == Mutability::Shared || *mutability == Mutability::Mutable)
                        && self.intrinsic_lifetime_matches(lifetime, actual_lifetime)
                        && self.intrinsic_shape_matches(referent, pointee)
                }
                _ => self.intrinsic_shape_matches(referent, actual),
            },
            CompilerMethodTypePattern::MutableReference { lifetime, referent } => match actual {
                SymbolicType::Reference {
                    mutability: Mutability::Mutable,
                    lifetime: actual_lifetime,
                    pointee,
                } => {
                    self.intrinsic_lifetime_matches(lifetime, actual_lifetime)
                        && self.intrinsic_shape_matches(referent, pointee)
                }
                _ => self.intrinsic_shape_matches(referent, actual),
            },
            CompilerMethodTypePattern::Slice(element) => matches!(
                actual,
                SymbolicType::Slice(actual)
                    if self.intrinsic_shape_matches(element, actual)
            ),
            CompilerMethodTypePattern::Tuple(fields) => matches!(
                actual,
                SymbolicType::Tuple(actual)
                    if fields.len() == actual.len()
                        && fields
                            .iter()
                            .zip(actual)
                            .all(|(pattern, actual)| self.intrinsic_shape_matches(pattern, actual))
            ),
        }
    }

    fn intrinsic_lifetime_matches(
        &self,
        pattern: &CompilerMethodLifetimePattern,
        _actual: &SymbolicLifetime,
    ) -> bool {
        matches!(
            pattern,
            CompilerMethodLifetimePattern::Elided | CompilerMethodLifetimePattern::Generic(_)
        )
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
            ValueCategory::PendingDirectFunction(item)
            | ValueCategory::PendingAssociatedFunction(item) => {
                for argument in arguments {
                    self.check_expression(argument, None);
                }
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingGenericInference,
                    format!(
                        "generic callable {item:?} has no explicit actuals and argument inference is not yet authoritative"
                    ),
                );
                return None;
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
        let SymbolicType::FunctionPointer {
            parameters,
            result,
            requires,
            throws,
            ..
        } = function_ty
        else {
            self.source_error(span, "TYPE002", "value is not a function pointer");
            return None;
        };
        self.check_call_arguments(&parameters, arguments, span);
        if !requires.members().is_empty() || !throws.members().is_empty() {
            self.pending_c4(
                span,
                b"call-effect-membership".to_vec(),
                "call effect membership is finalized by C4",
            );
        }
        let result = *result;
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
        let constructed = match &value.input {
            TypedExpressionInput::Known(ty) => ty.clone(),
            _ => return None,
        };
        let actuals = match &constructed {
            SymbolicType::NominalPath { arguments, .. } => arguments.clone(),
            _ => Vec::new(),
        };
        let (callee, form, fields) = match selection {
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
                for argument in arguments {
                    self.check_expression(argument, None);
                }
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                    format!("embedded constructor {definition:?} lacks typed field signatures"),
                );
                return None;
            }
            ConstructorSelection::EmbeddedVariant(variant) => {
                for argument in arguments {
                    self.check_expression(argument, None);
                }
                self.gap(
                    span,
                    BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                    format!("embedded variant {variant:?} lacks typed field signatures"),
                );
                return None;
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
                    "`as` supports only raw-pointer/address reconstruction, not {:?} to {target:?}",
                    checked.ty()
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
                    let actuals = self.path_actuals_or_expected(path_use, entry, expected, span)?;
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
                            "{:?} declaration `{}` is not a value",
                            entry.definition.key.kind, entry.definition.key.name
                        ),
                    );
                    None
                }
            },
            HirItemRes::NominalConstructor { owner } => {
                let actuals = self.path_actuals_or_expected(path_use, entry, expected, span)?;
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
                let actuals = self.path_actuals_or_expected(path_use, entry, expected, span)?;
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
        _path_use: &arche_frontend::HirPathUse,
        span: Span,
        expected: Option<&SymbolicType>,
    ) -> Option<LoweredValue> {
        match target {
            BuiltinResTarget::Method(method) => {
                let typed = self
                    .catalog
                    .handoff
                    .frontend()
                    .inventory()
                    .embedded_core
                    .typed_c2()
                    .compiler_trait_method_for_c1_method(method);
                if typed.is_some() {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingEmbeddedTraitIdentity,
                        format!(
                            "compiler-trait method {method:?} cannot be selected before its stable embedded trait DefinitionId exists"
                        ),
                    );
                } else {
                    self.gap(
                        span,
                        BodyCheckIncompletenessKind::MissingTypedEmbeddedCallable,
                        format!(
                            "embedded method {method:?} has only a string C1 signature, not a typed C2 callable"
                        ),
                    );
                }
                None
            }
            BuiltinResTarget::EnumVariant(variant) => {
                let Some(expected) = expected.cloned() else {
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
                    input: TypedExpressionInput::Known(expected),
                    category: ValueCategory::Constructor(ConstructorSelection::EmbeddedVariant(
                        variant,
                    )),
                })
            }
            BuiltinResTarget::RecordConstructor(definition) => {
                let Some(expected) = expected.cloned() else {
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
                    input: TypedExpressionInput::Known(expected),
                    category: ValueCategory::Constructor(ConstructorSelection::EmbeddedRecord(
                        definition,
                    )),
                })
            }
            BuiltinResTarget::Prelude(prelude) => match prelude {
                VirtualPreludeTarget::Definition(definition) => {
                    if let Some(expected) = expected.cloned() {
                        Some(LoweredValue {
                            input: TypedExpressionInput::Known(expected),
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

    fn path_actuals_or_expected(
        &mut self,
        path_use: &arche_frontend::HirPathUse,
        entry: &DefinitionEntry<'_>,
        expected: Option<&SymbolicType>,
        span: Span,
    ) -> Option<Vec<GenericArgumentShape>> {
        let actuals = self.path_actuals(path_use, span)?;
        let declaration_shape =
            checked_entry_shape(entry, self.scope.body.id, span, &mut self.gaps)?;
        let formal_count = declaration_shape.generic_parameters.len();
        if actuals.len() == formal_count {
            return Some(actuals);
        }
        if actuals.is_empty() {
            if let Some(SymbolicType::NominalPath {
                declaration,
                arguments,
            }) = expected
            {
                if declaration == &entry.semantic_path() && arguments.len() == formal_count {
                    return Some(arguments.clone());
                }
            }
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
                        let _ = self.lower_block(else_block, None);
                        self.gap(
                            else_block.span,
                            BodyCheckIncompletenessKind::UnsupportedC2AdapterSurface,
                            "let-else divergence is not represented by TypedExpressionInput",
                        );
                        complete = false;
                    }
                    if let Some(lowered) = lowered {
                        statements.push(lowered.input);
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
                            Some(item.clone())
                        }
                        Some(_) => {
                            self.gap(
                                iterator.span,
                                BodyCheckIncompletenessKind::MissingEmbeddedTraitIdentity,
                                "ordinary `for` requires IntoIterator/Iterator selection, whose compiler-known stable identities are not yet exposed",
                            );
                            complete = false;
                            None
                        }
                        None => {
                            complete = false;
                            None
                        }
                    };
                    if let Some(item_type) = &item_type {
                        self.check_and_bind_irrefutable(
                            pattern,
                            item_type,
                            PlaceMutability::Mutable,
                        );
                    }
                    let lowered_body = self.lower_loop_body(body);
                    if lowered_body.is_none() {
                        complete = false;
                    }
                    if let (Some(_), Some(body)) = (item_type, lowered_body) {
                        // `for` has while-like break/continue typing and unit result.
                        statements.push(TypedExpressionInput::While {
                            condition: Box::new(TypedExpressionInput::Boolean(true)),
                            body: Box::new(body.input),
                        });
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
                        statements.push(match operator {
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
                        });
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

    fn lower_loop_body(&mut self, block: &AstBlock) -> Option<LoweredValue> {
        self.source_loop_depth += 1;
        let lowered = self.lower_block(block, None);
        self.source_loop_depth -= 1;
        lowered
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
        let body = self.lower_loop_body(&while_.body);
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
        if catch {
            self.gap(
                span,
                BodyCheckIncompletenessKind::MissingEffectAuthority,
                "catch pattern scrutinee requires the C4 canonical throws-set authority",
            );
        } else {
            self.check_match_patterns(operand_checked.ty(), arms, operand.span);
        }

        let mut joined = expected.cloned();
        let mut complete = !catch;
        for arm in arms {
            if !catch {
                self.bind_refutable_arm(&arm.pattern, operand_checked.ty());
            }
            if let Some(guard) = &arm.guard {
                let bool_type = SymbolicType::Bool;
                if self.check_expression(guard, Some(&bool_type)).is_none() {
                    complete = false;
                }
            }
            let value = self.lower_expression(&arm.value, joined.as_ref());
            let checked = value
                .as_ref()
                .and_then(|value| self.materialize(value, joined.as_ref(), arm.value.span));
            if let Some(checked) = checked {
                if joined.is_none() {
                    joined = Some(checked.ty().clone());
                }
            } else {
                complete = false;
            }
        }
        if !complete {
            return None;
        }
        joined.map(|ty| LoweredValue::ordinary(TypedExpressionInput::Known(ty)))
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
            encode_declaration_path_debug(&mut bytes, &path);
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
            PatternArm::new(lowered, false),
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
            Err(errors) => self.pattern_errors_simple(pattern.span, errors),
        }
    }

    fn check_match_patterns(&mut self, ty: &SymbolicType, arms: &[AstMatchArm], span: Span) {
        self.reset_pattern_symbolic();
        let Some(pattern_ty) = self.pattern_type(ty, span) else {
            return;
        };
        let mut lowered = Vec::new();
        for arm in arms {
            let Some(pattern) = self.lower_pattern(&arm.pattern, &pattern_ty) else {
                continue;
            };
            lowered.push(PatternArm::new(pattern, arm.guard.is_some()));
        }
        if lowered.len() != arms.len() {
            return;
        }
        let scrutinee = PatternScrutinee::new(pattern_ty, PlaceMutability::Mutable);
        match analyze_pattern_match(&scrutinee, &lowered) {
            Ok(analysis) => {
                self.collect_match_ctfe(&analysis);
                self.patterns.push(CheckedBodyPattern {
                    span,
                    analysis: CheckedBodyPatternAnalysis::Refutable(analysis),
                });
            }
            Err(errors) => self.pattern_errors(span, arms, errors),
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
            PatternArm::new(lowered, false),
            PatternArm::new(Pattern::Wildcard, false),
        ];
        if let Ok(analysis) = analyze_pattern_match(
            &PatternScrutinee::new(pattern_ty, PlaceMutability::Mutable),
            &arms,
        ) {
            if let Some(typed) = match_first_typed_pattern(&analysis) {
                self.bind_pattern_locals(pattern, typed);
            }
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
        | PatternType::Unsupported(_) => None,
    }
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
        (SymbolicType::Slice(left), SymbolicType::Slice(right))
        | (
            SymbolicType::RawPointer { pointee: left, .. },
            SymbolicType::RawPointer { pointee: right, .. },
        ) => types_match_with_erased_body_lifetime(left, right) && left == right,
        _ => left == right,
    }
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
    encode_declaration_path_debug(&mut bytes, path);
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn symbolic_const_dependency_string(value: &SymbolicConstExpression) -> String {
    let mut bytes = b"ARCHE-C2-PATTERN-SYMBOLIC-CONST\0".to_vec();
    encode_debug_string(&mut bytes, &format!("{:?}", value.integer_type));
    encode_symbolic_const_node(&mut bytes, &value.node);
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
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
            encode_declaration_path_debug(output, path);
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
    encode_debug_string(output, &format!("{:?}", left.integer_type));
    encode_symbolic_const_node(output, &left.node);
    encode_debug_string(output, &format!("{:?}", right.integer_type));
    encode_symbolic_const_node(output, &right.node);
}

fn encode_declaration_path_debug(output: &mut Vec<u8>, path: &SemanticDeclarationPath) {
    output.extend_from_slice(b"ARCHE-SEMANTIC-PATH-DEBUG\0");
    encode_debug_string(output, &path.registry_origin);
    encode_debug_string(output, &path.package_name);
    encode_debug_string(output, &format!("{:?}", path.target));
    output.extend_from_slice(
        &u64::try_from(path.modules.len())
            .expect("module count fits u64")
            .to_le_bytes(),
    );
    for module in &path.modules {
        encode_debug_string(output, module);
    }
    encode_debug_string(output, &format!("{:?}", path.kind));
    encode_debug_string(output, &path.name);
}

fn encode_debug_string(output: &mut Vec<u8>, value: &str) {
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

    fn corpus_body_failure(name: &str) -> C2BodyCheckFailure {
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
        let failure =
            check_workspace_bodies_c2(&handoff, &declarations, checked_declarations).unwrap_err();
        assert_eq!(
            failure.partial().len(),
            expected_bodies,
            "corpus={name}, gaps={:?}",
            failure.incompleteness()
        );
        failure
    }

    #[test]
    fn real_v1_bodies_retain_every_body_and_never_turn_adapter_gaps_into_diagnostics() {
        for corpus in ["language-game", "language-environment"] {
            let failure = corpus_body_failure(corpus);
            assert!(
                failure.diagnostics().is_none(),
                "corpus={corpus}, diagnostics={:?}",
                failure.diagnostics()
            );
            assert!(!failure.incompleteness().is_empty(), "corpus={corpus}");
            let complete = failure.partial().bodies().count();
            assert!(complete > 0, "corpus={corpus}");
            assert!(complete < failure.partial().len(), "corpus={corpus}");
            assert!(!failure.partial().all_authority_complete());
        }
    }

    #[test]
    fn checked_declarations_from_another_session_fail_before_body_traversal() {
        let game = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let game_declarations = DeclarationTable::build(&game).unwrap();
        let environment = C2Handoff::begin(corpus_frontend("language-environment")).unwrap();
        let environment_declarations = DeclarationTable::build(&environment).unwrap();
        let environment_checked =
            check_declarations_c2(&environment, &environment_declarations).unwrap();

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
            "    let closure = move |input: i32| requires {} throws {} -> i32 { input };\n",
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
        let game = corpus_body_failure("language-game");
        let environment = corpus_body_failure("language-environment");
        let game_body = game.partial().bodies().next().unwrap();
        let environment_body = environment.partial().bodies().next().unwrap();
        let game_handle = game.partial().handle(game_body.id()).unwrap();
        let environment_handle = environment.partial().handle(environment_body.id()).unwrap();

        assert!(game.partial().body(&game_handle).is_some());
        assert!(environment.partial().body(&environment_handle).is_some());
        assert!(game.partial().body(&environment_handle).is_none());
        assert!(environment.partial().body(&game_handle).is_none());
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
}
