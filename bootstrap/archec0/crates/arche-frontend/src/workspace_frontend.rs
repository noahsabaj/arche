//! Workspace-wide M27-C1 acquisition and resolved symbolic HIR construction.
//!
//! This is deliberately a separate entry point from the M27-B checker. It
//! snapshots every explicit module into one source authority before resolving
//! names, retains the complete AST, and constructs only session-local HIR and
//! an unverified inventory skeleton. It does not mint a stable semantic ID.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arche_foundation::identity::PackageId;
use arche_package::{
    source_tree_digest, Capability, IntegrityDigest, LockDependencyKind, Manifest, PackageNodeId,
    PortablePath, ResolvedGraph, ResolvedPackage, ResolvedSource, Target, Workspace,
    WorkspaceMember,
};

use crate::ast::{
    AstBlock, AstConstExpression, AstDeclaration, AstDeclarationKind, AstExpression, AstFile,
    AstGeneratorClosure, AstImpl, AstImplMethod, AstImport, AstItem, AstMethodParameter, AstModule,
    AstPattern, AstPatternKind, AstSchedule, AstTraitMethod, AstVisibility, AstVisibilityKind,
    AstWorldInitBlock,
};
use crate::embedded_core::{
    verified_embedded_core_authority, VerifiedEmbeddedCoreAuthority, VirtualDeclarationKind,
    VirtualDefinitionId, VirtualEnumVariantId, VirtualMethodId, VirtualNamespace,
    VirtualPreludeTarget,
};
use crate::include_inputs::{find_include_input_candidates, IncludeInput, IncludeInputKind};
use crate::modules::{FrontendError, FrontendErrorCode};
use crate::parser::parse_reader;
use crate::source::{
    Diagnostic, FileId, SourceDatabase, SourceDatabaseBuilder, SourcePosition, SourceRole, Span,
};
use crate::symbol::Symbol;

use super::arena::{ArenaExhausted, HirArenaAllocators, HirModuleIdAllocator};
use super::inventory::{
    CtfeBudgetsSkeleton, DependencyKind, ManifestCapability, MemberVisibilityPath, ModuleRef,
    Namespace, PackageDependencySkeleton, PackageProvenanceSkeleton, PackageSourceSkeleton,
    SemanticBindingInventorySkeleton, SemanticBindingOrigin, SemanticBindingPath,
    SemanticBindingTarget, SemanticBodyInventorySkeleton, SemanticBodyKey, SemanticBodyKind,
    SemanticDefinitionInventorySkeleton, SemanticDefinitionKey, SemanticInventoryBuilder,
    SemanticInventorySkeleton, SemanticMemberVisibility, SemanticModuleInventorySkeleton,
    SemanticPackageInventorySkeleton, SemanticTargetContractSkeleton,
    SemanticTargetInventorySkeleton, Visibility,
};
use super::shape::{
    encode_symbolic_declaration_shape_skeleton_c1, encode_symbolic_definition_owner_skeleton_c1,
    DeclarationKind, EffectKind, GenericArgumentShape, GenericParameterKind, GenericParameterShape,
    IntegerType, Mutability, PendingShapeKind, SemanticDeclarationPath, SymbolicCallableKind,
    SymbolicCallableParameterMode, SymbolicCallableParameterSkeleton,
    SymbolicCallableShapeSkeleton, SymbolicCapabilityAccessMode, SymbolicConstExpression,
    SymbolicConstNode, SymbolicDeclarationPayloadSkeleton, SymbolicDeclarationShapeSkeleton,
    SymbolicDefinitionOwnerSkeleton, SymbolicEffectAtom, SymbolicEffectSetsSkeleton,
    SymbolicEffectShapeSkeleton, SymbolicFieldShapeSkeleton,
    SymbolicImpliedCapabilityRequirementSkeleton, SymbolicLifetime, SymbolicMethodShapeSkeleton,
    SymbolicPendingShape, SymbolicPredicate, SymbolicPredicateShapeSkeleton, SymbolicQueryTermKind,
    SymbolicQueryTermShapeSkeleton, SymbolicRecordForm, SymbolicRecordShapeSkeleton,
    SymbolicShapeReadiness, SymbolicSourceSpan, SymbolicSystemAccessShapeSkeleton, SymbolicType,
    SymbolicTypeShapeSkeleton, SymbolicVariantShapeSkeleton, TargetRoot,
};
use super::{HirBodyId, HirItemId, HirModuleId, TargetId};
use crate::package::TargetIdAllocator;

pub type WorkspaceInventorySkeleton = SemanticInventorySkeleton<Arc<VerifiedEmbeddedCoreAuthority>>;

/// One C1 result owns the HIR and the exact immutable snapshots that produced
/// it. The inventory remains unverified until the later semantic gate.
#[derive(Debug)]
pub struct FrontendOutput {
    hir: ResolvedSymbolicWorkspaceHir,
    sources: Arc<SourceDatabase>,
    inventory: WorkspaceInventorySkeleton,
}

impl FrontendOutput {
    /// Returns the complete session-local symbolic HIR retained by C1.
    pub fn hir(&self) -> &ResolvedSymbolicWorkspaceHir {
        &self.hir
    }

    /// Returns the immutable source snapshots that produced this result.
    pub fn sources(&self) -> &Arc<SourceDatabase> {
        &self.sources
    }

    /// Returns the unverified semantic inventory skeleton retained by C1.
    pub fn inventory(&self) -> &WorkspaceInventorySkeleton {
        &self.inventory
    }
}

/// One unpacked registry package supplied by the later package-cache adapter.
///
/// This is untrusted input. `check_workspace_c1` binds it to the resolved
/// package node, revalidates its package name and selected version, retains its
/// source bytes, and rejects a source-tree digest mismatch before returning a
/// C1 result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializedRegistryPackage {
    pub package_node: PackageNodeId,
    /// Canonical or canonicalizable unpacked package directory. Host paths do
    /// not enter HIR or inventory identity.
    pub directory: PathBuf,
    /// Manifest parsed from the immutable archive bytes by the cache adapter.
    pub manifest: Manifest,
    /// The same immutable manifest bytes. C1 revalidates the snapshot-committed
    /// package-header span and the parsed manifest commitment against them.
    pub manifest_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSymbolicWorkspaceHir {
    pub packages: Vec<ResolvedSymbolicPackageHir>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSymbolicPackageHir {
    pub package_node: PackageNodeId,
    pub package: PackageId,
    pub targets: Vec<ResolvedSymbolicTargetHir>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSymbolicTargetHir {
    pub id: TargetId,
    pub target: TargetRoot,
    pub modules: Vec<ResolvedSymbolicModule>,
    pub items: Vec<ResolvedSymbolicItem>,
    pub bodies: Vec<ResolvedSymbolicBody>,
    pub path_resolutions: Vec<PathResolution>,
    pub contract: ResolvedTargetContract,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSymbolicModule {
    pub id: HirModuleId,
    pub parent: Option<HirModuleId>,
    pub name: Option<Symbol>,
    pub path: Vec<Symbol>,
    pub file: FileId,
    pub declared_visibility: Visibility,
    pub bindings: Vec<HirBinding>,
    /// Complete syntax, including every accepted signature and body.
    pub ast: AstFile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSymbolicItem {
    pub id: HirItemId,
    pub module: HirModuleId,
    pub owner: Option<HirItemId>,
    pub name: Option<String>,
    pub kind: DeclarationKind,
    pub declared_visibility: Visibility,
    pub member_visibilities: Vec<SemanticMemberVisibility>,
    pub span: Span,
    /// Resolved symbolic inputs that are part of the declaration signature.
    pub symbolic_shape: ResolvedSymbolicShape,
    /// Resolved symbolic inputs retained from accepted bodies. These never
    /// contribute to the declaration identity input.
    pub body_symbolic_shape: ResolvedSymbolicShape,
    /// Ordered resolved generic-argument lists owned by postfix call forms.
    /// Path-owned lists live directly on `HirPathUse`; together these are the
    /// normative C1 generic-actual authority. Flattened symbolic buckets are
    /// derived compatibility/completeness views only.
    pub postfix_generic_argument_uses: Vec<HirGenericArgumentsUse>,
    pub path_uses: Vec<HirPathUse>,
    /// Dedicated lowercase `self` expressions resolved to the receiver's
    /// checked local identity. `self::name` remains an ordinary path use.
    pub self_uses: Vec<HirSelfUse>,
    pub locals: Vec<HirLocalBinding>,
    pub source: HirItemSource,
    symbolic_inputs: SymbolicShapeInputs,
    body_symbolic_inputs: SymbolicShapeInputs,
    owner_shape: SymbolicDefinitionOwnerSkeleton,
    definition_shape: SymbolicDeclarationShapeSkeleton,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SymbolicShapeInputs {
    types: Vec<crate::ast::AstType>,
    consts: Vec<(AstConstExpression, IntegerType)>,
    effects: Vec<AstSymbolicEffect>,
    // Type-namespace paths are retained only for noncanonical C2 template
    // projection (not the encoded flattened C1 compatibility buckets).
    c2_type_roots: Vec<crate::ast::AstType>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AstSymbolicEffect {
    Requires(crate::ast::AstPath),
    Throws(crate::ast::AstType),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SymbolicInputDomain {
    Declaration,
    Body,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BarePatternResolution {
    Binding,
    Path,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PatternBindingFact {
    name: Symbol,
    span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirPathUse {
    pub path: crate::ast::AstPath,
    pub namespace: Option<Namespace>,
    pub lexical_local: Option<LocalId>,
    /// Mixed type/lifetime/integer-const actuals in exact source order, with
    /// their resolved formal kinds. This is authoritative over the derived
    /// flattened symbolic input buckets.
    pub generic_arguments: Vec<HirGenericArgumentUse>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirGenericArgumentsUse {
    pub span: Span,
    pub arguments: Vec<HirGenericArgumentUse>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirGenericArgumentUse {
    pub span: Span,
    pub formal_kind: Option<GenericParameterKind>,
    pub value: ResolvedGenericArgument,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedGenericArgument {
    Type(ResolvedSymbolicType),
    Lifetime(ResolvedSymbolicLifetime),
    IntegerConst(ResolvedSymbolicConst),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirLocalBinding {
    pub id: LocalId,
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HirSelfUse {
    pub span: Span,
    pub receiver: LocalId,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedSymbolicShape {
    pub generic_parameters: Vec<GenericParameterShape>,
    /// Compiler-synthesized declaration lifetime binders in deterministic
    /// receiver-first/input-preorder. They are not source generic arity.
    pub hidden_lifetime_binders: Vec<HiddenLifetimeBinder>,
    pub types: Vec<ResolvedSymbolicType>,
    pub consts: Vec<ResolvedSymbolicConst>,
    pub effects: Vec<ResolvedSymbolicEffect>,
    // Noncanonical C1-to-C2 authority. This sidecar is deliberately omitted
    // from every HIR/inventory/identity encoder.
    c2_contextual_self_templates: Vec<C2ContextualSelfTypeProjection>,
}

impl ResolvedSymbolicShape {
    /// Returns the single-authority structured template for one collapsed C1
    /// contextual-`Self` type leaf.
    ///
    /// The lookup joins on the exact retained pending span and debug spelling.
    /// It fails closed when no unique template was retained or relowering met
    /// an independent pending/error condition.
    pub fn contextual_self_type_template(
        &self,
        pending: &SymbolicPendingShape,
    ) -> Result<&C2ContextualSelfTypeTemplate, C2TypeTemplateLookupError> {
        if pending.kind != PendingShapeKind::ContextualSelf {
            return Err(C2TypeTemplateLookupError::NotContextualSelf);
        }
        let mut matches = self
            .c2_contextual_self_templates
            .iter()
            .filter(|projection| {
                projection.pending_span == pending.source_span
                    && projection.debug_spelling == pending.debug_spelling
            });
        let Some(first) = matches.next() else {
            return Err(C2TypeTemplateLookupError::Missing);
        };
        if matches.any(|candidate| candidate.result != first.result) {
            return Err(C2TypeTemplateLookupError::Ambiguous);
        }
        match &first.result {
            Ok(template) => Ok(template),
            Err(blocker) => Err(C2TypeTemplateLookupError::Blocked(blocker.clone())),
        }
    }
}

/// Immutable, noncanonical C1-to-C2 type template whose reserved leaves mark
/// contextual `Self`. Construction remains private to frontend lowering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct C2ContextualSelfTypeTemplate {
    root_span: SymbolicSourceSpan,
    debug_spelling: String,
    template: Box<SymbolicType>,
    hole_count: u64,
}

impl C2ContextualSelfTypeTemplate {
    pub const fn root_span(&self) -> SymbolicSourceSpan {
        self.root_span
    }

    pub fn debug_spelling(&self) -> &str {
        &self.debug_spelling
    }

    pub const fn hole_count(&self) -> u64 {
        self.hole_count
    }

    /// Replaces every retained contextual-`Self` leaf while preserving the
    /// exact C1-resolved enclosing structure and binder coordinates.
    pub fn instantiate_contextual_self(
        &self,
        self_type: &SymbolicType,
    ) -> Result<SymbolicType, C2TypeTemplateInstantiationError> {
        if symbolic_type_contains_c2_self_marker(self_type) {
            return Err(C2TypeTemplateInstantiationError::ReservedTemplateCoordinate);
        }
        Ok(replace_c2_self_markers(&self.template, self_type))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum C2TypeTemplateBlocker {
    AdditionalPending {
        source_span: SymbolicSourceSpan,
        kind: PendingShapeKind,
        debug_spelling: String,
    },
    FrontendInvariant {
        source_span: Option<SymbolicSourceSpan>,
        code: String,
        message: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum C2TypeTemplateLookupError {
    NotContextualSelf,
    Missing,
    Ambiguous,
    Blocked(C2TypeTemplateBlocker),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C2TypeTemplateInstantiationError {
    ReservedTemplateCoordinate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct C2ContextualSelfTypeProjection {
    pending_span: SymbolicSourceSpan,
    debug_spelling: String,
    result: Result<C2ContextualSelfTypeTemplate, C2TypeTemplateBlocker>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HiddenLifetimeBinder {
    pub index: u64,
    pub span: Span,
    pub source: HiddenLifetimeBinderSource,
    pub generator_state: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HiddenLifetimeBinderSource {
    Receiver,
    Input,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedSymbolicType {
    Resolved(Box<SymbolicType>),
    Pending {
        span: Span,
        reason: UnresolvedPathKind,
        canonical: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedSymbolicConst {
    Resolved(SymbolicConstExpression),
    Pending {
        span: Span,
        reason: UnresolvedPathKind,
        canonical: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedSymbolicEffect {
    Resolved(Box<SymbolicEffectAtom>),
    Pending {
        span: Span,
        reason: UnresolvedPathKind,
        canonical: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedSymbolicLifetime {
    Resolved(SymbolicLifetime),
    Pending {
        span: Span,
        reason: UnresolvedPathKind,
        canonical: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HirItemSource {
    Declaration(Box<AstDeclaration>),
    Impl(Box<AstImpl>),
    TraitMethod(Box<AstTraitMethod>),
    ImplMethod(Box<AstImplMethod>),
    QueryParameter {
        name: Symbol,
        terms: Vec<crate::ast::AstQueryTerm>,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSymbolicBody {
    pub id: HirBodyId,
    pub owner: HirItemId,
    pub kind: SemanticBodyKind,
    pub ordinal: u64,
    pub span: Span,
    pub source: HirBodySource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HirBodySource {
    Block(Box<AstBlock>),
    Expression(Box<AstExpression>),
    WorldInitializer(Box<AstWorldInitBlock>),
    Schedule(Box<AstSchedule>),
    ConstExpression(Box<AstConstExpression>),
    Closure(Box<crate::ast::AstClosure>),
    GeneratorClosure(Box<AstGeneratorClosure>),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GenericParameterId {
    pub owner: HirItemId,
    pub index: u64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LocalId {
    pub owner: HirItemId,
    pub ordinal: u64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BuiltinResTarget {
    Prelude(VirtualPreludeTarget),
    Method(VirtualMethodId),
    EnumVariant(VirtualEnumVariantId),
    RecordConstructor(VirtualDefinitionId),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BuiltinRes {
    pub target: BuiltinResTarget,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Res {
    Module(HirModuleId),
    Item(HirItemRes),
    Generic(GenericParameterId),
    Local(LocalId),
    Builtin(BuiltinRes),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum HirItemRes {
    Definition(HirItemId),
    NominalConstructor { owner: HirItemId },
    EnumVariant { owner: HirItemId, ordinal: u64 },
}

impl HirItemRes {
    pub const fn owner(self) -> HirItemId {
        match self {
            Self::Definition(owner)
            | Self::NominalConstructor { owner }
            | Self::EnumVariant { owner, .. } => owner,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum UnresolvedPathKind {
    UnknownName,
    AmbiguousNamespace,
    /// The path identity is resolved, but generic arity or actual/formal kind
    /// formation is intentionally deferred to C2. This is never a raw name
    /// lookup failure.
    GenericFormationPendingC2,
    AssociatedItemPendingC2,
    SelfTypePendingC2,
    ShadowedLocalNeedsLexicalResolution,
    DependencyHasNoLibraryTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathResolution {
    pub span: Span,
    /// An import can intentionally resolve the same spelling in more than one
    /// namespace. All other C1 paths normally have one result.
    pub resolutions: Vec<Res>,
    pub unresolved: Option<UnresolvedPathKind>,
    /// Exact C1 authority for a user-associated path whose type/method
    /// viability is intentionally deferred. C2 filters these checked session
    /// candidates; it never re-searches the retained source spelling.
    pub associated: Option<AssociatedPathResolution>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AssociatedPathOwner {
    Nominal(HirItemId),
    Generic(GenericParameterId),
    ContextualSelf { context: HirItemId },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssociatedPathResolution {
    pub owner: AssociatedPathOwner,
    pub member: String,
    pub member_span: Span,
    pub path_span: Span,
    pub candidates: Vec<AssociatedPathCandidate>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AssociatedPathCandidate {
    Item(HirItemRes),
    Builtin(BuiltinRes),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HirBindingTarget {
    Module(HirModuleId),
    Item(HirItemRes),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HirBindingOrigin {
    Declaration,
    ReExport {
        source_module: HirModuleId,
        /// Canonical nonempty path relative to `source_module`. A direct
        /// module binding has one segment; a nominal member carries its owner
        /// and member segments so later phases never have to recover the
        /// source from spelling alone.
        source_segments: Vec<String>,
        target: HirBindingTarget,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirBinding {
    pub name: String,
    pub namespace: Namespace,
    pub target: HirBindingTarget,
    pub declared_visibility: Visibility,
    pub origin: HirBindingOrigin,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedTargetContract {
    Library,
    Binary {
        root_world: HirItemId,
        main: HirItemId,
        capabilities: Vec<ManifestCapability>,
    },
    Environment {
        root_world: HirItemId,
        profile: String,
        reset: HirItemId,
        step: HirItemId,
        self_play: HirItemId,
    },
    /// Used only between acquisition and dependency-first linking.
    Pending,
}

#[derive(Clone)]
struct PackageDraft<'a> {
    resolved: &'a ResolvedPackage,
    source: PackageSourceAuthority<'a>,
    targets: Vec<TargetDraft>,
}

#[derive(Clone, Copy)]
enum PackageSourceAuthority<'a> {
    Workspace(&'a WorkspaceMember),
    Registry(&'a MaterializedRegistryPackage),
}

impl PackageSourceAuthority<'_> {
    fn directory(&self) -> &Path {
        match self {
            Self::Workspace(member) => &member.directory,
            Self::Registry(package) => &package.directory,
        }
    }

    fn manifest(&self) -> &Manifest {
        match self {
            Self::Workspace(member) => &member.manifest,
            Self::Registry(package) => &package.manifest,
        }
    }
}

#[derive(Clone)]
struct TargetDraft {
    manifest_ordinal: u64,
    manifest_target: Target,
    target: ResolvedSymbolicTargetHir,
    pending_imports: Vec<PendingImport>,
}

#[derive(Clone)]
struct PendingImport {
    module: HirModuleId,
    import: AstImport,
}

#[derive(Clone, Copy)]
enum ModuleAllocationOrigin<'a> {
    ManifestTarget {
        manifest: &'a Manifest,
        target: &'a Target,
    },
    ModuleDeclaration(Span),
}

/// Runs the C1 workspace frontend without changing the M27-B public check path.
pub fn check_workspace_c1(
    workspace: &Workspace,
    graph: &ResolvedGraph,
    registry_packages: &[MaterializedRegistryPackage],
) -> Result<FrontendOutput, FrontendError> {
    let embedded_core = verified_embedded_core_authority().map_err(|error| {
        frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            format!("compiled embedded-Core authority is invalid: {error}"),
        )
    })?;
    graph.validate().map_err(|diagnostics| {
        frontend_path_error(
            FrontendErrorCode::Target,
            "TARGET012",
            format!("resolved package graph is invalid: {diagnostics}"),
        )
    })?;
    let members = match_workspace_members(workspace, graph)?;
    let registry_packages = match_registry_packages(graph, registry_packages)?;

    let mut sources = SourceDatabaseBuilder::new();
    let mut arenas = HirArenaAllocators::default();
    let mut packages = Vec::with_capacity(graph.packages.len());
    for resolved in &graph.packages {
        let source = match &resolved.source {
            ResolvedSource::Workspace { .. } => members
                .get(&resolved.id)
                .copied()
                .map(PackageSourceAuthority::Workspace),
            ResolvedSource::Registry { .. } => registry_packages
                .get(&resolved.id)
                .copied()
                .map(PackageSourceAuthority::Registry),
        }
        .ok_or_else(|| {
            frontend_path_error(
                FrontendErrorCode::Source,
                "SOURCE001",
                format!(
                    "C1 source integration requires materialized source for package `{}`",
                    resolved.name
                ),
            )
        })?;
        let manifest = source.manifest();
        sources
            .bind_package_manifest(resolved.id, source.directory(), &manifest.source_entry)
            .map_err(|diagnostic| frontend_error(FrontendErrorCode::Source, diagnostic))?;
        let mut targets = Vec::new();
        let mut target_ids = TargetIdAllocator::new();
        for target in manifest.targets() {
            let target_id = target_ids.allocate(manifest, &target)?;
            let manifest_ordinal = target_id.0;
            let mut draft = acquire_target(
                &mut sources,
                resolved,
                source,
                target,
                target_id,
                manifest_ordinal,
            )?;
            collect_target_items(&mut draft, &mut arenas)?;
            targets.push(draft);
        }
        packages.push(PackageDraft {
            resolved,
            source,
            targets,
        });
    }

    resolve_workspace_names(&mut packages, graph)?;
    collect_workspace_bodies_and_paths(&mut packages, graph, &embedded_core, &mut arenas)?;
    finish_symbolic_hir(&mut packages, graph, &embedded_core)?;
    validate_final_c1_resolutions(&packages, graph, &embedded_core)?;
    acquire_include_inputs(&mut sources, &packages, &embedded_core)?;
    sources
        .install_embedded_core(&embedded_core)
        .map_err(|diagnostic| frontend_error(FrontendErrorCode::Source, diagnostic))?;

    let sources = sources.seal();
    let inventory = build_inventory(
        workspace,
        graph,
        &packages,
        &sources,
        Arc::clone(&embedded_core),
    )?;
    let hir = ResolvedSymbolicWorkspaceHir {
        packages: packages
            .into_iter()
            .map(|package| ResolvedSymbolicPackageHir {
                package_node: package.resolved.id,
                package: package.resolved.package_id,
                targets: package
                    .targets
                    .into_iter()
                    .map(|target| target.target)
                    .collect(),
            })
            .collect(),
    };
    Ok(FrontendOutput {
        hir,
        sources,
        inventory,
    })
}

#[derive(Clone)]
struct ResolvedIncludeUse {
    package_name: String,
    package: PackageNodeId,
    package_root: PathBuf,
    input: IncludeInput,
}

fn acquire_include_inputs(
    sources: &mut SourceDatabaseBuilder,
    packages: &[PackageDraft<'_>],
    embedded_core: &VerifiedEmbeddedCoreAuthority,
) -> Result<(), FrontendError> {
    let mut uses = Vec::new();
    for package in packages {
        for target in &package.targets {
            for module in &target.target.modules {
                for candidate in find_include_input_candidates(&module.ast) {
                    let resolution = target
                        .target
                        .path_resolutions
                        .iter()
                        .find(|resolution| resolution.span == candidate.callee_span)
                        .ok_or_else(|| {
                            frontend_error(
                                FrontendErrorCode::Target,
                                Diagnostic::at(
                                    "IDENTITY001",
                                    candidate.callee_span,
                                    "include candidate has no C1 name-resolution authority",
                                ),
                            )
                        })?;
                    let expected = embedded_core
                        .lookup_prelude(candidate.kind.source_name(), VirtualNamespace::Value)
                        .ok_or_else(|| {
                            frontend_path_error(
                                FrontendErrorCode::Target,
                                "IDENTITY001",
                                format!(
                                    "embedded-Core has no `{}` value binding",
                                    candidate.kind.source_name()
                                ),
                            )
                        })?;
                    let resolves_to_builtin = resolution.unresolved.is_none()
                        && matches!(
                            resolution.resolutions.as_slice(),
                            [Res::Builtin(BuiltinRes {
                                target: BuiltinResTarget::Prelude(target),
                            })] if *target == expected
                        );
                    if !resolves_to_builtin {
                        continue;
                    }
                    let input = candidate.validate().map_err(|diagnostic| {
                        frontend_error(FrontendErrorCode::Source, diagnostic)
                    })?;
                    uses.push(ResolvedIncludeUse {
                        package_name: package.resolved.name.to_string(),
                        package: package.resolved.id,
                        package_root: package.source.directory().to_path_buf(),
                        input,
                    });
                }
            }
        }
    }

    uses.sort_by(|left, right| {
        (
            left.package_name.as_bytes(),
            &left.input.path,
            left.input.kind,
            left.input.literal_span.file.0,
            left.input.literal_span.start.byte,
            left.input.literal_span.end.byte,
        )
            .cmp(&(
                right.package_name.as_bytes(),
                &right.input.path,
                right.input.kind,
                right.input.literal_span.file.0,
                right.input.literal_span.start.byte,
                right.input.literal_span.end.byte,
            ))
    });

    let mut cursor = 0;
    while cursor < uses.len() {
        let first = &uses[cursor];
        let mut end = cursor + 1;
        while end < uses.len()
            && uses[end].package == first.package
            && uses[end].input.path == first.input.path
        {
            end += 1;
        }
        let acquisition_span = first.input.literal_span;
        let utf8_span = uses[cursor..end]
            .iter()
            .find(|entry| entry.input.kind == IncludeInputKind::Str)
            .map(|entry| entry.input.literal_span);
        let file = sources
            .acquire(
                first.package,
                &first.package_root,
                first.input.path.clone(),
                SourceRole::Include,
            )
            .map_err(|diagnostic| {
                frontend_error(
                    FrontendErrorCode::Source,
                    Diagnostic::at(
                        "CTFE005",
                        acquisition_span,
                        format!(
                            "could not acquire included input `{}`: {}",
                            first.input.path, diagnostic.message
                        ),
                    ),
                )
            })?;
        if let Some(span) = utf8_span {
            sources.validate_utf8(file).map_err(|error| {
                frontend_error(
                    FrontendErrorCode::Source,
                    Diagnostic::at(
                        "CTFE005",
                        span,
                        format!(
                            "included string `{}` is not exact UTF-8: {error}",
                            first.input.path
                        ),
                    ),
                )
            })?;
        }
        cursor = end;
    }
    Ok(())
}

/// Canonical, host-path-free C1 HIR debug envelope. The module AST payloads
/// make every retained signature and body visible in the same deterministic
/// text contract as the resolved session IDs and name-resolution rows.
pub fn dump_hir_c1(hir: &ResolvedSymbolicWorkspaceHir) -> Result<String, FrontendError> {
    let mut output = String::from("ARCHE-HIR-TEXT 1\n(workspace");
    for package in &hir.packages {
        write!(
            output,
            " (package (node {}) (id {})",
            package.package_node.get(),
            uppercase_hex(package.package.as_bytes())
        )
        .expect("writing to String is infallible");
        for target in &package.targets {
            output.push_str(" (target (id ");
            write!(output, "{}", target.id.0).expect("writing to String is infallible");
            output.push_str(") (root ");
            write_target_root(&mut output, &target.target);
            output.push(')');
            for module in &target.modules {
                write!(
                    output,
                    " (module (id {} {} {}) (parent ",
                    module.id.package().get(),
                    module.id.target().0,
                    module.id.local()
                )
                .expect("writing to String is infallible");
                match module.parent {
                    Some(parent) => write!(
                        output,
                        "({} {} {})",
                        parent.package().get(),
                        parent.target().0,
                        parent.local()
                    )
                    .expect("writing to String is infallible"),
                    None => output.push_str("none"),
                }
                write!(output, ") (file {}) (path", module.file.0)
                    .expect("writing to String is infallible");
                for segment in &module.path {
                    output.push(' ');
                    push_json_string(&mut output, segment.as_str());
                }
                output.push_str(") (visibility ");
                write_visibility(&mut output, &module.declared_visibility);
                output.push(')');
                for binding in &module.bindings {
                    output.push_str(" (binding (name ");
                    push_json_string(&mut output, &binding.name);
                    output.push_str(") (namespace ");
                    output.push_str(namespace_name(binding.namespace));
                    output.push_str(") (target ");
                    write_binding_target(&mut output, &binding.target);
                    output.push_str(") (visibility ");
                    write_visibility(&mut output, &binding.declared_visibility);
                    output.push_str(") (origin ");
                    match &binding.origin {
                        HirBindingOrigin::Declaration => output.push_str("declaration"),
                        HirBindingOrigin::ReExport {
                            source_module,
                            source_segments,
                            target,
                        } => {
                            write!(
                                output,
                                "(re-export (source-module {} {} {}) (source-path",
                                source_module.package().get(),
                                source_module.target().0,
                                source_module.local()
                            )
                            .expect("writing to String is infallible");
                            for segment in source_segments {
                                output.push(' ');
                                push_json_string(&mut output, segment);
                            }
                            output.push_str(") (target ");
                            write_binding_target(&mut output, target);
                            output.push_str("))");
                        }
                    }
                    output.push_str(") (span ");
                    write_span(&mut output, binding.span);
                    output.push_str("))");
                }
                output.push_str(" (ast ");
                push_json_string(&mut output, &crate::ast::dump_ast(&module.ast));
                output.push_str("))");
            }
            for item in &target.items {
                write!(
                    output,
                    " (item (id {}) (module {} {} {}) (owner ",
                    item.id.0,
                    item.module.package().get(),
                    item.module.target().0,
                    item.module.local()
                )
                .expect("writing to String is infallible");
                match item.owner {
                    Some(owner) => {
                        write!(output, "{}", owner.0).expect("writing to String is infallible")
                    }
                    None => output.push_str("none"),
                }
                output.push_str(") (name ");
                match &item.name {
                    Some(name) => push_json_string(&mut output, name),
                    None => output.push_str("none"),
                }
                output.push_str(") (kind ");
                output.push_str(declaration_kind_name(item.kind));
                output.push_str(") (visibility ");
                write_visibility(&mut output, &item.declared_visibility);
                output.push_str(") (span ");
                write_span(&mut output, item.span);
                output.push_str(") (owner-shape ");
                output.push_str(&uppercase_hex(
                    &encode_symbolic_definition_owner_skeleton_c1(&item.owner_shape)
                        .map_err(shape_error)?,
                ));
                output.push_str(") (shape ");
                output.push_str(&uppercase_hex(
                    &encode_symbolic_declaration_shape_skeleton_c1(&item.definition_shape)
                        .map_err(shape_error)?,
                ));
                output.push_str(") (body-symbolic-shape ");
                output.push_str(&uppercase_hex(&encode_alpha_symbolic_shape(
                    &item.body_symbolic_shape,
                )?));
                output.push(')');
                let mut generic_uses = item
                    .path_uses
                    .iter()
                    .filter(|path_use| !path_use.generic_arguments.is_empty())
                    .map(|path_use| {
                        (
                            path_use.path.span,
                            Some(&path_use.path),
                            path_use.generic_arguments.as_slice(),
                        )
                    })
                    .chain(
                        item.postfix_generic_argument_uses
                            .iter()
                            .filter(|generic_use| !generic_use.arguments.is_empty())
                            .map(|generic_use| {
                                (generic_use.span, None, generic_use.arguments.as_slice())
                            }),
                    )
                    .collect::<Vec<_>>();
                generic_uses.sort_by_key(|(span, path, _)| {
                    (span.file.0, span.start.byte, span.end.byte, path.is_none())
                });
                for (span, path, arguments) in generic_uses {
                    output.push_str(" (generic-arguments-use (owner ");
                    if let Some(path) = path {
                        output.push_str("(path ");
                        push_json_string(&mut output, &canonical_path(path));
                        output.push(')');
                    } else {
                        output.push_str("postfix");
                    }
                    output.push_str(") (span ");
                    write_span(&mut output, span);
                    output.push_str(") (arguments");
                    for argument in arguments {
                        output.push(' ');
                        write_generic_argument_use(&mut output, argument)?;
                    }
                    output.push_str("))");
                }
                for binder in &item.symbolic_shape.hidden_lifetime_binders {
                    write!(
                        output,
                        " (hidden-lifetime-binder (index {}) (source {}) (generator-state {}) (span ",
                        binder.index,
                        match binder.source {
                            HiddenLifetimeBinderSource::Receiver => "receiver",
                            HiddenLifetimeBinderSource::Input => "input",
                        },
                        u8::from(binder.generator_state)
                    )
                    .expect("writing to String is infallible");
                    write_span(&mut output, binder.span);
                    output.push_str("))");
                }
                for local in &item.locals {
                    write!(
                        output,
                        " (local (owner {}) (ordinal {}) (name ",
                        local.id.owner.0, local.id.ordinal
                    )
                    .expect("writing to String is infallible");
                    push_json_string(&mut output, &local.name);
                    output.push_str(") (span ");
                    write_span(&mut output, local.span);
                    output.push_str("))");
                }
                for self_use in &item.self_uses {
                    output.push_str(" (self-use (span ");
                    write_span(&mut output, self_use.span);
                    write!(
                        output,
                        ") (receiver {} {}))",
                        self_use.receiver.owner.0, self_use.receiver.ordinal
                    )
                    .expect("writing to String is infallible");
                }
                output.push(')');
            }
            for body in &target.bodies {
                write!(
                    output,
                    " (body (id {}) (owner {}) (kind {}) (ordinal {}) (span ",
                    body.id.0,
                    body.owner.0,
                    body_kind_name(body.kind),
                    body.ordinal
                )
                .expect("writing to String is infallible");
                write_span(&mut output, body.span);
                output.push_str("))");
            }
            for resolution in &target.path_resolutions {
                output.push_str(" (path-resolution (span ");
                write_span(&mut output, resolution.span);
                output.push_str(") (resolutions");
                for resolution in &resolution.resolutions {
                    output.push(' ');
                    write_resolution(&mut output, resolution);
                }
                output.push_str(") (unresolved ");
                match &resolution.unresolved {
                    Some(reason) => output.push_str(unresolved_path_name(reason)),
                    None => output.push_str("none"),
                }
                output.push_str(") (associated ");
                match &resolution.associated {
                    None => output.push_str("none"),
                    Some(associated) => {
                        output.push_str("(owner ");
                        match associated.owner {
                            AssociatedPathOwner::Nominal(item) => {
                                write!(output, "(nominal {})", item.0)
                                    .expect("writing to String is infallible");
                            }
                            AssociatedPathOwner::Generic(parameter) => {
                                write!(
                                    output,
                                    "(generic {} {})",
                                    parameter.owner.0, parameter.index
                                )
                                .expect("writing to String is infallible");
                            }
                            AssociatedPathOwner::ContextualSelf { context } => {
                                write!(output, "(self {})", context.0)
                                    .expect("writing to String is infallible");
                            }
                        }
                        output.push_str(") (member ");
                        push_json_string(&mut output, &associated.member);
                        output.push_str(") (member-span ");
                        write_span(&mut output, associated.member_span);
                        output.push_str(") (path-span ");
                        write_span(&mut output, associated.path_span);
                        output.push_str(") (candidates");
                        for candidate in &associated.candidates {
                            match candidate {
                                AssociatedPathCandidate::Item(item) => {
                                    output.push_str(" (item ");
                                    write_item_res(&mut output, *item);
                                    output.push(')');
                                }
                                AssociatedPathCandidate::Builtin(builtin) => {
                                    output.push_str(" (builtin ");
                                    write_builtin_res(&mut output, *builtin);
                                    output.push(')');
                                }
                            }
                        }
                        output.push(')');
                    }
                }
                output.push_str("))");
            }
            output.push_str(" (contract ");
            write_contract(&mut output, &target.contract);
            output.push_str("))");
        }
        output.push(')');
    }
    output.push_str(")\n");
    Ok(output)
}

fn match_workspace_members<'a>(
    workspace: &'a Workspace,
    graph: &ResolvedGraph,
) -> Result<BTreeMap<PackageNodeId, &'a WorkspaceMember>, FrontendError> {
    let mut output = BTreeMap::new();
    for member in &workspace.members {
        let package = member
            .manifest
            .package
            .as_ref()
            .expect("validated workspace member is a package");
        let matches = graph
            .packages
            .iter()
            .filter(|resolved| resolved.name == package.name)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(frontend_path_error(
                FrontendErrorCode::Target,
                "TARGET012",
                format!(
                    "resolved graph contains {} nodes for workspace package `{}`",
                    matches.len(),
                    package.name
                ),
            ));
        }
        let resolved = matches[0];
        let ResolvedSource::Workspace { relative_path } = &resolved.source else {
            return Err(frontend_path_error(
                FrontendErrorCode::Target,
                "TARGET012",
                format!(
                    "workspace package `{}` is not a workspace source",
                    package.name
                ),
            ));
        };
        if relative_path != &member.relative_path || resolved.version != package.version {
            return Err(frontend_path_error(
                FrontendErrorCode::Target,
                "TARGET012",
                format!(
                    "resolved graph identity for workspace package `{}` does not match its manifest",
                    package.name
                ),
            ));
        }
        if output.insert(resolved.id, member).is_some() {
            return Err(frontend_path_error(
                FrontendErrorCode::Target,
                "TARGET012",
                "resolved workspace package nodes are not unique",
            ));
        }
    }
    Ok(output)
}

fn match_registry_packages<'a>(
    graph: &ResolvedGraph,
    registry_packages: &'a [MaterializedRegistryPackage],
) -> Result<BTreeMap<PackageNodeId, &'a MaterializedRegistryPackage>, FrontendError> {
    let mut output = BTreeMap::new();
    for source in registry_packages {
        let resolved = graph.package(source.package_node).ok_or_else(|| {
            frontend_path_error(
                FrontendErrorCode::Source,
                "SOURCE005",
                format!(
                    "materialized registry source names unknown package node {}",
                    source.package_node.get()
                ),
            )
        })?;
        if !matches!(&resolved.source, ResolvedSource::Registry { .. }) {
            return Err(frontend_path_error(
                FrontendErrorCode::Source,
                "SOURCE005",
                format!(
                    "materialized registry source for `{}` targets a workspace package",
                    resolved.name
                ),
            ));
        }
        let package = source.manifest.package.as_ref().ok_or_else(|| {
            frontend_path_error(
                FrontendErrorCode::Source,
                "SOURCE005",
                format!(
                    "materialized registry source for `{}` has no package manifest",
                    resolved.name
                ),
            )
        })?;
        if package.name != resolved.name || package.version != resolved.version {
            return Err(frontend_path_error(
                FrontendErrorCode::Source,
                "SOURCE005",
                format!(
                    "materialized registry source identity `{}` version `{}` does not match selected package `{}` version `{}`",
                    package.name, package.version, resolved.name, resolved.version
                ),
            ));
        }
        let ResolvedSource::Registry { manifest_span, .. } = &resolved.source else {
            unreachable!("registry source kind was checked above");
        };
        let byte_length = u64::try_from(source.manifest_bytes.len()).map_err(|_| {
            frontend_path_error(
                FrontendErrorCode::Source,
                "SOURCE005",
                format!(
                    "materialized registry manifest for `{}` exceeds the u64 source domain",
                    resolved.name
                ),
            )
        })?;
        let manifest_commitment_matches = source.manifest.source_entry.path.as_str()
            == "Arche.toml"
            && source.manifest.source_entry.byte_length == byte_length
            && source.manifest.source_entry.content_digest
                == IntegrityDigest::of_bytes(&source.manifest_bytes);
        let retained_package_span = source.manifest.package_span();
        if std::str::from_utf8(&source.manifest_bytes).is_err()
            || !manifest_commitment_matches
            || retained_package_span != Some(*manifest_span)
        {
            return Err(frontend_path_error(
                FrontendErrorCode::Source,
                "SOURCE005",
                format!(
                    "materialized registry manifest for `{}` does not match its committed package-header span and bytes",
                    resolved.name
                ),
            ));
        }
        if output.insert(source.package_node, source).is_some() {
            return Err(frontend_path_error(
                FrontendErrorCode::Source,
                "SOURCE005",
                format!(
                    "materialized registry source repeats package `{}`",
                    resolved.name
                ),
            ));
        }
    }
    Ok(output)
}

fn acquire_target(
    sources: &mut SourceDatabaseBuilder,
    resolved: &ResolvedPackage,
    source: PackageSourceAuthority<'_>,
    manifest_target: Target,
    target_id: TargetId,
    manifest_ordinal: u64,
) -> Result<TargetDraft, FrontendError> {
    let target_root = target_root(&manifest_target);
    let mut module_ids = HirModuleIdAllocator::new(resolved.id, target_id);
    let mut modules = Vec::new();
    acquire_module(
        sources,
        &mut module_ids,
        resolved.id,
        source.directory(),
        manifest_target.path().clone(),
        ModuleAllocationOrigin::ManifestTarget {
            manifest: source.manifest(),
            target: &manifest_target,
        },
        None,
        None,
        Vec::new(),
        Visibility::Public,
        &mut modules,
    )?;
    Ok(TargetDraft {
        manifest_ordinal,
        manifest_target,
        target: ResolvedSymbolicTargetHir {
            id: target_id,
            target: target_root,
            modules,
            items: Vec::new(),
            bodies: Vec::new(),
            path_resolutions: Vec::new(),
            contract: ResolvedTargetContract::Pending,
        },
        pending_imports: Vec::new(),
    })
}

#[allow(clippy::too_many_arguments)]
fn acquire_module(
    sources: &mut SourceDatabaseBuilder,
    module_ids: &mut HirModuleIdAllocator,
    package: PackageNodeId,
    package_root: &std::path::Path,
    portable_path: PortablePath,
    allocation_origin: ModuleAllocationOrigin<'_>,
    parent: Option<HirModuleId>,
    name: Option<Symbol>,
    logical_path: Vec<Symbol>,
    declared_visibility: Visibility,
    modules: &mut Vec<ResolvedSymbolicModule>,
) -> Result<HirModuleId, FrontendError> {
    let id = module_ids
        .next_module()
        .map_err(|error| module_allocation_error(error, allocation_origin))?;
    let file = sources
        .acquire(
            package,
            package_root,
            portable_path.clone(),
            SourceRole::Module,
        )
        .map_err(|diagnostic| frontend_error(FrontendErrorCode::Source, diagnostic))?;
    let reader = sources.reader(file).map_err(|error| {
        frontend_path_error(
            FrontendErrorCode::Source,
            "SOURCE002",
            format!("could not read retained source `{portable_path}`: {error}"),
        )
    })?;
    let ast = parse_reader(file, reader)
        .map_err(|diagnostic| frontend_error(FrontendErrorCode::Syntax, diagnostic))?;
    let declarations = ast
        .items
        .iter()
        .filter_map(|item| match item {
            AstItem::Module(module) => Some(module.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut seen = BTreeMap::new();
    for declaration in &declarations {
        if let Some(previous) =
            seen.insert(declaration.name.as_str().to_owned(), declaration.name_span)
        {
            return Err(frontend_error(
                FrontendErrorCode::Name,
                Diagnostic::at(
                    "MODULE008",
                    declaration.name_span,
                    format!("duplicate module declaration `{}`", declaration.name),
                )
                .with_secondary(previous, "first declared here"),
            ));
        }
    }
    modules.push(ResolvedSymbolicModule {
        id,
        parent,
        name,
        path: logical_path.clone(),
        file,
        declared_visibility,
        bindings: Vec::new(),
        ast,
    });

    let source_base = portable_parent(&portable_path)?;
    for declaration in declarations {
        let mut child_logical = logical_path.clone();
        child_logical.push(declaration.name.clone());
        let child_path = child_module_path(&source_base, &logical_path, &declaration)?;
        let visibility = lexical_visibility(&declaration.visibility, &logical_path, modules, id)?;
        acquire_module(
            sources,
            module_ids,
            package,
            package_root,
            child_path,
            ModuleAllocationOrigin::ModuleDeclaration(declaration.name_span),
            Some(id),
            Some(declaration.name),
            child_logical,
            visibility,
            modules,
        )?;
    }
    Ok(id)
}

fn module_allocation_error(
    error: ArenaExhausted,
    origin: ModuleAllocationOrigin<'_>,
) -> FrontendError {
    match origin {
        ModuleAllocationOrigin::ManifestTarget { manifest, target } => {
            let Some(manifest_span) = manifest.target_span(target) else {
                return FrontendError {
                    kind: FrontendErrorCode::Target,
                    diagnostic: Box::new(Diagnostic::path(
                        error.code(),
                        format!("{error}; originating target manifest span is unavailable"),
                    )),
                    files: vec![manifest.path.clone()],
                };
            };
            FrontendError {
                kind: FrontendErrorCode::Target,
                diagnostic: Box::new(Diagnostic::at(
                    error.code(),
                    Span {
                        file: FileId(0),
                        start: SourcePosition {
                            byte: manifest_span.start_byte,
                            line: manifest_span.start_line,
                            column: manifest_span.start_column,
                        },
                        end: SourcePosition {
                            byte: manifest_span.end_byte,
                            line: manifest_span.end_line,
                            column: manifest_span.end_column,
                        },
                    },
                    error.to_string(),
                )),
                files: vec![manifest.path.clone()],
            }
        }
        ModuleAllocationOrigin::ModuleDeclaration(span) => frontend_error(
            FrontendErrorCode::Target,
            Diagnostic::at(error.code(), span, error.to_string()),
        ),
    }
}

fn portable_parent(path: &PortablePath) -> Result<PortablePath, FrontendError> {
    let parent = path
        .as_str()
        .rsplit_once('/')
        .map_or(".", |(parent, _)| parent);
    PortablePath::new(parent).map_err(|diagnostics| {
        frontend_path_error(
            FrontendErrorCode::Source,
            "SOURCE004",
            format!("target source parent is not portable: {diagnostics}"),
        )
    })
}

fn child_module_path(
    source_base: &PortablePath,
    parent: &[Symbol],
    declaration: &AstModule,
) -> Result<PortablePath, FrontendError> {
    let mut segments = Vec::new();
    if source_base.as_str() != "." {
        segments.push(source_base.as_str().to_owned());
    }
    segments.extend(parent.iter().map(|segment| segment.as_str().to_owned()));
    segments.push(format!("{}.arc", declaration.name));
    PortablePath::new(&segments.join("/")).map_err(|diagnostics| {
        frontend_error(
            FrontendErrorCode::Module,
            Diagnostic::at(
                "MODULE002",
                declaration.name_span,
                format!("module path is not portable: {diagnostics}"),
            ),
        )
    })
}

fn target_root(target: &Target) -> TargetRoot {
    match target {
        Target::Library(_) => TargetRoot::Library,
        Target::Binary(target) => TargetRoot::Binary(target.name.as_str().to_owned()),
        Target::Environment(target) => TargetRoot::Environment(target.name.as_str().to_owned()),
    }
}

fn collect_target_items(
    draft: &mut TargetDraft,
    arenas: &mut HirArenaAllocators,
) -> Result<(), FrontendError> {
    let modules = draft.target.modules.clone();
    let mut top_level_items = BTreeMap::<SpanKey, HirItemId>::new();
    for module in &modules {
        for item in &module.ast.items {
            match item {
                AstItem::Module(_) => {}
                AstItem::Import(import) => draft.pending_imports.push(PendingImport {
                    module: module.id,
                    import: import.clone(),
                }),
                AstItem::Declaration(declaration) => {
                    validate_declaration_scope_names(declaration)?;
                    let id = allocate_item(arenas)?;
                    let visibility = lexical_visibility(
                        &declaration.visibility,
                        &module.path,
                        &draft.target.modules,
                        module.id,
                    )?;
                    let kind = declaration_kind(&declaration.kind);
                    draft.target.items.push(ResolvedSymbolicItem {
                        id,
                        module: module.id,
                        owner: None,
                        name: Some(declaration.name.as_str().to_owned()),
                        kind,
                        declared_visibility: visibility.clone(),
                        member_visibilities: member_visibilities(
                            declaration,
                            visibility.clone(),
                            &module.path,
                            &draft.target.modules,
                            module.id,
                        )?,
                        span: declaration.span,
                        symbolic_shape: ResolvedSymbolicShape::default(),
                        body_symbolic_shape: ResolvedSymbolicShape::default(),
                        postfix_generic_argument_uses: Vec::new(),
                        path_uses: Vec::new(),
                        self_uses: Vec::new(),
                        locals: Vec::new(),
                        source: HirItemSource::Declaration(declaration.clone()),
                        symbolic_inputs: SymbolicShapeInputs::default(),
                        body_symbolic_inputs: SymbolicShapeInputs::default(),
                        owner_shape: SymbolicDefinitionOwnerSkeleton::TopLevel,
                        definition_shape: SymbolicDeclarationShapeSkeleton::default(),
                    });
                    top_level_items.insert(SpanKey::from(declaration.span), id);
                    collect_owned_items(draft, arenas, module, id, declaration)?;
                }
                AstItem::Impl(implementation) => {
                    validate_impl_scope_names(implementation)?;
                    let id = allocate_item(arenas)?;
                    draft.target.items.push(ResolvedSymbolicItem {
                        id,
                        module: module.id,
                        owner: None,
                        name: None,
                        kind: DeclarationKind::Impl,
                        declared_visibility: Visibility::DeclaringModule,
                        member_visibilities: implementation
                            .methods
                            .iter()
                            .enumerate()
                            .map(|(ordinal, method)| {
                                let ordinal = checked_u64(ordinal, "impl method count")?;
                                Ok(SemanticMemberVisibility {
                                    path: MemberVisibilityPath::Method { ordinal },
                                    declared_visibility: lexical_visibility(
                                        &method.visibility,
                                        &module.path,
                                        &draft.target.modules,
                                        module.id,
                                    )?,
                                })
                            })
                            .collect::<Result<Vec<_>, FrontendError>>()?,
                        span: implementation.span,
                        symbolic_shape: ResolvedSymbolicShape::default(),
                        body_symbolic_shape: ResolvedSymbolicShape::default(),
                        postfix_generic_argument_uses: Vec::new(),
                        path_uses: Vec::new(),
                        self_uses: Vec::new(),
                        locals: Vec::new(),
                        source: HirItemSource::Impl(implementation.clone()),
                        symbolic_inputs: SymbolicShapeInputs::default(),
                        body_symbolic_inputs: SymbolicShapeInputs::default(),
                        owner_shape: SymbolicDefinitionOwnerSkeleton::TopLevel,
                        definition_shape: SymbolicDeclarationShapeSkeleton::default(),
                    });
                    top_level_items.insert(SpanKey::from(implementation.span), id);
                    for method in &implementation.methods {
                        let method_id = allocate_item(arenas)?;
                        let visibility = lexical_visibility(
                            &method.visibility,
                            &module.path,
                            &draft.target.modules,
                            module.id,
                        )?;
                        draft.target.items.push(ResolvedSymbolicItem {
                            id: method_id,
                            module: module.id,
                            owner: Some(id),
                            name: Some(method.name.as_str().to_owned()),
                            kind: DeclarationKind::Function,
                            declared_visibility: visibility,
                            member_visibilities: Vec::new(),
                            span: method.span,
                            symbolic_shape: ResolvedSymbolicShape::default(),
                            body_symbolic_shape: ResolvedSymbolicShape::default(),
                            postfix_generic_argument_uses: Vec::new(),
                            path_uses: Vec::new(),
                            self_uses: Vec::new(),
                            locals: Vec::new(),
                            source: HirItemSource::ImplMethod(Box::new(method.clone())),
                            symbolic_inputs: SymbolicShapeInputs::default(),
                            body_symbolic_inputs: SymbolicShapeInputs::default(),
                            owner_shape: SymbolicDefinitionOwnerSkeleton::TopLevel,
                            definition_shape: SymbolicDeclarationShapeSkeleton::default(),
                        });
                    }
                }
            }
        }
    }

    collect_direct_bindings(draft, &top_level_items)?;
    Ok(())
}

fn validate_declaration_scope_names(declaration: &AstDeclaration) -> Result<(), FrontendError> {
    use crate::ast::{AstStructForm, AstVariantForm};

    let validate_record_fields = |fields: &[crate::ast::AstRecordField]| {
        validate_unique_names(
            fields.iter().map(|field| (field.name.as_str(), field.span)),
            "field",
        )
    };
    match &declaration.kind {
        AstDeclarationKind::Component(record) | AstDeclarationKind::Resource(record) => {
            validate_generic_parameter_names(record.generics.as_ref())?;
            validate_record_fields(&record.fields)?;
        }
        AstDeclarationKind::Struct(structure) => {
            validate_generic_parameter_names(structure.generics.as_ref())?;
            if let AstStructForm::Record(fields) = &structure.form {
                validate_record_fields(fields)?;
            }
        }
        AstDeclarationKind::Enum(enumeration) => {
            validate_generic_parameter_names(enumeration.generics.as_ref())?;
            validate_unique_names(
                enumeration
                    .variants
                    .iter()
                    .map(|variant| (variant.name.as_str(), variant.span)),
                "enum variant",
            )?;
            for variant in &enumeration.variants {
                if let AstVariantForm::Record(fields) = &variant.form {
                    validate_unique_names(
                        fields.iter().map(|field| (field.name.as_str(), field.span)),
                        "variant field",
                    )?;
                }
            }
        }
        AstDeclarationKind::TypeAlias(alias) => {
            validate_generic_parameter_names(alias.generics.as_ref())?;
        }
        AstDeclarationKind::Function(function) => {
            validate_generic_parameter_names(function.signature.generics.as_ref())?;
        }
        AstDeclarationKind::Generator(generator) => {
            validate_generic_parameter_names(generator.generics.as_ref())?;
        }
        AstDeclarationKind::System(system) => {
            validate_generic_parameter_names(system.generics.as_ref())?;
            validate_unique_names(
                system
                    .parameters
                    .iter()
                    .map(|parameter| (parameter.name.as_str(), parameter.span)),
                "system parameter",
            )?;
        }
        AstDeclarationKind::Trait(trait_) => {
            validate_generic_parameter_names(trait_.generics.as_ref())?;
            validate_unique_names(
                trait_
                    .methods
                    .iter()
                    .map(|method| (method.name.as_str(), method.span)),
                "trait method",
            )?;
            for method in &trait_.methods {
                validate_generic_parameter_names(method.signature.generics.as_ref())?;
            }
        }
        AstDeclarationKind::World { .. }
        | AstDeclarationKind::Tag
        | AstDeclarationKind::Const(_)
        | AstDeclarationKind::Static(_)
        | AstDeclarationKind::Schedule(_) => {}
    }
    Ok(())
}

fn validate_impl_scope_names(implementation: &AstImpl) -> Result<(), FrontendError> {
    validate_generic_parameter_names(implementation.generics.as_ref())?;
    validate_unique_names(
        implementation
            .methods
            .iter()
            .map(|method| (method.name.as_str(), method.span)),
        "impl method",
    )?;
    for method in &implementation.methods {
        validate_generic_parameter_names(method.signature.generics.as_ref())?;
    }
    Ok(())
}

fn validate_generic_parameter_names(
    parameters: Option<&crate::ast::AstGenericParameters>,
) -> Result<(), FrontendError> {
    let Some(parameters) = parameters else {
        return Ok(());
    };
    validate_unique_names(
        parameters.parameters.iter().map(|parameter| {
            let name = match &parameter.kind {
                crate::ast::AstGenericParameterKind::Lifetime { name, .. }
                | crate::ast::AstGenericParameterKind::Type { name, .. }
                | crate::ast::AstGenericParameterKind::IntegerConst { name, .. } => name,
            };
            (name.as_str(), parameter.span)
        }),
        "generic parameter",
    )
}

fn validate_unique_names<'a>(
    names: impl IntoIterator<Item = (&'a str, Span)>,
    category: &str,
) -> Result<(), FrontendError> {
    let mut first_spans = BTreeMap::<&str, Span>::new();
    for (name, span) in names {
        if let Some(previous) = first_spans.get(name) {
            return Err(frontend_error(
                FrontendErrorCode::Name,
                Diagnostic::at("NAME001", span, format!("duplicate {category} `{name}`"))
                    .with_secondary(*previous, "first declared here"),
            ));
        }
        first_spans.insert(name, span);
    }
    Ok(())
}

fn collect_owned_items(
    draft: &mut TargetDraft,
    arenas: &mut HirArenaAllocators,
    module: &ResolvedSymbolicModule,
    owner: HirItemId,
    declaration: &AstDeclaration,
) -> Result<(), FrontendError> {
    match &declaration.kind {
        AstDeclarationKind::Trait(trait_) => {
            for method in &trait_.methods {
                let id = allocate_item(arenas)?;
                draft.target.items.push(ResolvedSymbolicItem {
                    id,
                    module: module.id,
                    owner: Some(owner),
                    name: Some(method.name.as_str().to_owned()),
                    kind: DeclarationKind::Function,
                    declared_visibility: draft
                        .target
                        .items
                        .iter()
                        .find(|item| item.id == owner)
                        .expect("trait owner was just inserted")
                        .declared_visibility
                        .clone(),
                    member_visibilities: Vec::new(),
                    span: method.span,
                    symbolic_shape: ResolvedSymbolicShape::default(),
                    body_symbolic_shape: ResolvedSymbolicShape::default(),
                    postfix_generic_argument_uses: Vec::new(),
                    path_uses: Vec::new(),
                    self_uses: Vec::new(),
                    locals: Vec::new(),
                    source: HirItemSource::TraitMethod(Box::new(method.clone())),
                    symbolic_inputs: SymbolicShapeInputs::default(),
                    body_symbolic_inputs: SymbolicShapeInputs::default(),
                    owner_shape: SymbolicDefinitionOwnerSkeleton::TopLevel,
                    definition_shape: SymbolicDeclarationShapeSkeleton::default(),
                });
            }
        }
        AstDeclarationKind::System(system) => {
            let owner_visibility = draft
                .target
                .items
                .iter()
                .find(|item| item.id == owner)
                .expect("system owner was just inserted")
                .declared_visibility
                .clone();
            for parameter in &system.parameters {
                let crate::ast::AstSystemParameterKind::Query(terms) = &parameter.kind else {
                    continue;
                };
                let id = allocate_item(arenas)?;
                draft.target.items.push(ResolvedSymbolicItem {
                    id,
                    module: module.id,
                    owner: Some(owner),
                    name: Some(parameter.name.as_str().to_owned()),
                    kind: DeclarationKind::Query,
                    declared_visibility: owner_visibility.clone(),
                    member_visibilities: Vec::new(),
                    span: parameter.span,
                    symbolic_shape: ResolvedSymbolicShape::default(),
                    body_symbolic_shape: ResolvedSymbolicShape::default(),
                    postfix_generic_argument_uses: Vec::new(),
                    path_uses: Vec::new(),
                    self_uses: Vec::new(),
                    locals: Vec::new(),
                    source: HirItemSource::QueryParameter {
                        name: parameter.name.clone(),
                        terms: terms.clone(),
                        span: parameter.span,
                    },
                    symbolic_inputs: SymbolicShapeInputs::default(),
                    body_symbolic_inputs: SymbolicShapeInputs::default(),
                    owner_shape: SymbolicDefinitionOwnerSkeleton::TopLevel,
                    definition_shape: SymbolicDeclarationShapeSkeleton::default(),
                });
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_direct_bindings(
    draft: &mut TargetDraft,
    top_level_items: &BTreeMap<SpanKey, HirItemId>,
) -> Result<(), FrontendError> {
    for module_index in 0..draft.target.modules.len() {
        let module_id = draft.target.modules[module_index].id;
        let module_path = draft.target.modules[module_index].path.clone();
        let ast_items = draft.target.modules[module_index].ast.items.clone();
        for ast_item in ast_items {
            let bindings = match ast_item {
                AstItem::Module(module) => {
                    let child = draft
                        .target
                        .modules
                        .iter()
                        .find(|candidate| {
                            candidate.parent == Some(module_id)
                                && candidate.name.as_ref() == Some(&module.name)
                        })
                        .expect("module acquisition created every declared child");
                    vec![HirBinding {
                        name: module.name.as_str().to_owned(),
                        namespace: Namespace::Module,
                        target: HirBindingTarget::Module(child.id),
                        declared_visibility: lexical_visibility(
                            &module.visibility,
                            &module_path,
                            &draft.target.modules,
                            module_id,
                        )?,
                        origin: HirBindingOrigin::Declaration,
                        span: module.name_span,
                    }]
                }
                AstItem::Declaration(declaration) => {
                    let id = top_level_items[&SpanKey::from(declaration.span)];
                    let declared_visibility = draft
                        .target
                        .items
                        .iter()
                        .find(|item| item.id == id)
                        .expect("top-level declaration has a HIR item")
                        .declared_visibility
                        .clone();
                    let mut bindings = vec![HirBinding {
                        name: declaration.name.as_str().to_owned(),
                        namespace: namespace_for_declaration(&declaration.kind),
                        target: HirBindingTarget::Item(HirItemRes::Definition(id)),
                        declared_visibility: declared_visibility.clone(),
                        origin: HirBindingOrigin::Declaration,
                        span: declaration.name_span,
                    }];
                    if declaration_has_nominal_constructor(&declaration.kind) {
                        bindings.push(HirBinding {
                            name: declaration.name.as_str().to_owned(),
                            namespace: Namespace::Value,
                            target: HirBindingTarget::Item(HirItemRes::NominalConstructor {
                                owner: id,
                            }),
                            declared_visibility,
                            origin: HirBindingOrigin::Declaration,
                            span: declaration.name_span,
                        });
                    }
                    bindings
                }
                AstItem::Import(_) | AstItem::Impl(_) => Vec::new(),
            };
            for binding in bindings {
                push_binding(&mut draft.target.modules[module_index], binding)?;
            }
        }
    }
    Ok(())
}

fn declaration_has_nominal_constructor(kind: &AstDeclarationKind) -> bool {
    matches!(
        kind,
        AstDeclarationKind::Component(_)
            | AstDeclarationKind::Resource(_)
            | AstDeclarationKind::Tag
            | AstDeclarationKind::Struct(_)
    )
}

fn push_binding(
    module: &mut ResolvedSymbolicModule,
    binding: HirBinding,
) -> Result<(), FrontendError> {
    if let Some(previous) = module.bindings.iter().find(|candidate| {
        candidate.name == binding.name && candidate.namespace == binding.namespace
    }) {
        return Err(frontend_error(
            FrontendErrorCode::Name,
            Diagnostic::at(
                "NAME001",
                binding.span,
                format!(
                    "duplicate {:?} binding `{}`",
                    binding.namespace, binding.name
                ),
            )
            .with_secondary(previous.span, "first bound here"),
        ));
    }
    module.bindings.push(binding);
    module.bindings.sort_by(|left, right| {
        left.name
            .as_bytes()
            .cmp(right.name.as_bytes())
            .then_with(|| left.namespace.cmp(&right.namespace))
    });
    Ok(())
}

#[derive(Clone, Copy)]
struct PatternResolutionContext<'a, 'workspace> {
    packages: &'a [PackageDraft<'workspace>],
    graph: &'a ResolvedGraph,
    location: PathLocation,
    generics: &'a BTreeMap<String, GenericParameterId>,
    embedded_core: &'a VerifiedEmbeddedCoreAuthority,
}

fn collect_workspace_bodies_and_paths<'workspace>(
    packages: &mut [PackageDraft<'workspace>],
    graph: &ResolvedGraph,
    embedded_core: &VerifiedEmbeddedCoreAuthority,
    arenas: &mut HirArenaAllocators,
) -> Result<(), FrontendError> {
    // Imports and re-exports must be linked before a bare pattern identifier can
    // be distinguished from a binding by ordinary value-namespace lookup.
    let snapshot = packages.to_vec();
    for package_index in 0..packages.len() {
        for target_index in 0..packages[package_index].targets.len() {
            let item_count = packages[package_index].targets[target_index]
                .target
                .items
                .len();
            for item_index in 0..item_count {
                let snapshot_item =
                    &snapshot[package_index].targets[target_index].target.items[item_index];
                let (_, generics) = generic_environment(&snapshot, snapshot_item)?;
                let context = PatternResolutionContext {
                    packages: &snapshot,
                    graph,
                    location: PathLocation {
                        package_index,
                        target_index,
                        module: snapshot_item.module,
                        context_item: Some(snapshot_item.id),
                    },
                    generics: &generics,
                    embedded_core,
                };
                let target = &mut packages[package_index].targets[target_index].target;
                let (items, bodies) = (&mut target.items, &mut target.bodies);
                collect_item_bodies(bodies, arenas, &mut items[item_index], context)?;
            }
        }
    }
    Ok(())
}

fn collect_item_bodies(
    bodies: &mut Vec<ResolvedSymbolicBody>,
    arenas: &mut HirArenaAllocators,
    item: &mut ResolvedSymbolicItem,
    pattern_resolution: PatternResolutionContext<'_, '_>,
) -> Result<(), FrontendError> {
    let primary = match &item.source {
        HirItemSource::Declaration(declaration) => match &declaration.kind {
            AstDeclarationKind::World { initializer } => Some((
                SemanticBodyKind::WorldInitializer,
                initializer.span,
                HirBodySource::WorldInitializer(Box::new((**initializer).clone())),
            )),
            AstDeclarationKind::Const(item) => Some((
                SemanticBodyKind::Declaration,
                item.value.span,
                HirBodySource::Expression(Box::new(item.value.clone())),
            )),
            AstDeclarationKind::Static(item) => Some((
                SemanticBodyKind::Declaration,
                item.value.span,
                HirBodySource::Expression(Box::new(item.value.clone())),
            )),
            AstDeclarationKind::Function(function) => Some((
                SemanticBodyKind::Declaration,
                function.body.span,
                HirBodySource::Block(Box::new(function.body.clone())),
            )),
            AstDeclarationKind::Generator(generator) => Some((
                SemanticBodyKind::Declaration,
                generator.body.span,
                HirBodySource::Block(Box::new(generator.body.clone())),
            )),
            AstDeclarationKind::System(system) => Some((
                SemanticBodyKind::Declaration,
                system.body.span,
                HirBodySource::Block(Box::new(system.body.clone())),
            )),
            AstDeclarationKind::Schedule(schedule) => Some((
                SemanticBodyKind::Declaration,
                item.span,
                HirBodySource::Schedule(Box::new((**schedule).clone())),
            )),
            _ => None,
        },
        HirItemSource::ImplMethod(method) => Some((
            SemanticBodyKind::Declaration,
            method.body.span,
            HirBodySource::Block(Box::new(method.body.clone())),
        )),
        HirItemSource::Impl(_)
        | HirItemSource::TraitMethod(_)
        | HirItemSource::QueryParameter { .. } => None,
    };
    if let Some((kind, span, source)) = primary {
        bodies.push(ResolvedSymbolicBody {
            id: allocate_body(arenas)?,
            owner: item.id,
            kind,
            ordinal: 0,
            span,
            source,
        });
    }
    collect_nested_bodies(bodies, arenas, item, pattern_resolution)
}

fn collect_nested_bodies(
    bodies: &mut Vec<ResolvedSymbolicBody>,
    arenas: &mut HirArenaAllocators,
    item: &mut ResolvedSymbolicItem,
    pattern_resolution: PatternResolutionContext<'_, '_>,
) -> Result<(), FrontendError> {
    let source = item.source.clone();
    let mut collector = NestedBodyCollector {
        bodies,
        arenas,
        owner: item.id,
        ordinals: BTreeMap::new(),
        anonymous_ordinal: 0,
        postfix_generic_argument_uses: &mut item.postfix_generic_argument_uses,
        path_uses: &mut item.path_uses,
        self_uses: &mut item.self_uses,
        locals: &mut item.locals,
        declaration_symbolic_inputs: &mut item.symbolic_inputs,
        body_symbolic_inputs: &mut item.body_symbolic_inputs,
        symbolic_input_domain: SymbolicInputDomain::Declaration,
        scopes: vec![BTreeMap::new()],
        pattern_resolution,
    };
    match &source {
        HirItemSource::Declaration(declaration) => collector.declaration(declaration)?,
        HirItemSource::Impl(implementation) => collector.implementation_header(implementation)?,
        HirItemSource::TraitMethod(method) => collector.method_signature(&method.signature)?,
        HirItemSource::ImplMethod(method) => {
            collector.method_signature(&method.signature)?;
            collector.begin_body();
            collector.block(&method.body)?;
        }
        HirItemSource::QueryParameter { terms, .. } => {
            for term in terms {
                collector.ty(&term.ty)?;
            }
        }
    }
    Ok(())
}

struct NestedBodyCollector<'a, 'context, 'workspace> {
    bodies: &'a mut Vec<ResolvedSymbolicBody>,
    arenas: &'a mut HirArenaAllocators,
    owner: HirItemId,
    ordinals: BTreeMap<SemanticBodyKind, u64>,
    anonymous_ordinal: u64,
    postfix_generic_argument_uses: &'a mut Vec<HirGenericArgumentsUse>,
    path_uses: &'a mut Vec<HirPathUse>,
    self_uses: &'a mut Vec<HirSelfUse>,
    locals: &'a mut Vec<HirLocalBinding>,
    declaration_symbolic_inputs: &'a mut SymbolicShapeInputs,
    body_symbolic_inputs: &'a mut SymbolicShapeInputs,
    symbolic_input_domain: SymbolicInputDomain,
    scopes: Vec<BTreeMap<String, LocalId>>,
    pattern_resolution: PatternResolutionContext<'context, 'workspace>,
}

impl NestedBodyCollector<'_, '_, '_> {
    fn begin_body(&mut self) {
        self.symbolic_input_domain = SymbolicInputDomain::Body;
    }

    fn current_symbolic_inputs(&mut self) -> &mut SymbolicShapeInputs {
        match self.symbolic_input_domain {
            SymbolicInputDomain::Declaration => self.declaration_symbolic_inputs,
            SymbolicInputDomain::Body => self.body_symbolic_inputs,
        }
    }

    fn push(
        &mut self,
        kind: SemanticBodyKind,
        span: Span,
        source: HirBodySource,
    ) -> Result<(), FrontendError> {
        let ordinal = if matches!(
            kind,
            SemanticBodyKind::Closure | SemanticBodyKind::Generator
        ) {
            self.anonymous_ordinal = self.anonymous_ordinal.checked_add(1).ok_or_else(|| {
                frontend_path_error(
                    FrontendErrorCode::Target,
                    "IDENTITY001",
                    "anonymous semantic-body ordinal exceeds u64",
                )
            })?;
            self.anonymous_ordinal
        } else {
            let ordinal = self.ordinals.entry(kind).or_insert(0);
            *ordinal = ordinal.checked_add(1).ok_or_else(|| {
                frontend_path_error(
                    FrontendErrorCode::Target,
                    "IDENTITY001",
                    "nested semantic-body ordinal exceeds u64",
                )
            })?;
            *ordinal
        };
        self.bodies.push(ResolvedSymbolicBody {
            id: allocate_body(self.arenas)?,
            owner: self.owner,
            kind,
            ordinal,
            span,
            source,
        });
        Ok(())
    }

    fn declaration(&mut self, declaration: &AstDeclaration) -> Result<(), FrontendError> {
        use crate::ast::{AstStructForm, AstVariantForm};
        match &declaration.kind {
            AstDeclarationKind::World { initializer } => {
                self.begin_body();
                self.world_initializer(initializer)
            }
            AstDeclarationKind::Component(record) | AstDeclarationKind::Resource(record) => {
                self.generic_parameters(record.generics.as_ref())?;
                self.where_clause(record.where_clause.as_ref())?;
                for field in &record.fields {
                    self.ty(&field.ty)?;
                }
                Ok(())
            }
            AstDeclarationKind::Tag => Ok(()),
            AstDeclarationKind::Struct(structure) => {
                self.generic_parameters(structure.generics.as_ref())?;
                self.where_clause(structure.where_clause.as_ref())?;
                match &structure.form {
                    AstStructForm::Unit => {}
                    AstStructForm::Tuple(fields) => {
                        for field in fields {
                            self.ty(&field.ty)?;
                        }
                    }
                    AstStructForm::Record(fields) => {
                        for field in fields {
                            self.ty(&field.ty)?;
                        }
                    }
                }
                Ok(())
            }
            AstDeclarationKind::Enum(enumeration) => {
                self.generic_parameters(enumeration.generics.as_ref())?;
                self.where_clause(enumeration.where_clause.as_ref())?;
                for variant in &enumeration.variants {
                    match &variant.form {
                        AstVariantForm::Unit => {}
                        AstVariantForm::Tuple(fields) => {
                            for field in fields {
                                self.ty(field)?;
                            }
                        }
                        AstVariantForm::Record(fields) => {
                            for field in fields {
                                self.ty(&field.ty)?;
                            }
                        }
                    }
                }
                Ok(())
            }
            AstDeclarationKind::TypeAlias(alias) => {
                self.generic_parameters(alias.generics.as_ref())?;
                self.ty(&alias.target)?;
                self.where_clause(alias.where_clause.as_ref())
            }
            AstDeclarationKind::Const(item) => {
                self.ty(&item.ty)?;
                self.begin_body();
                self.expression(&item.value)
            }
            AstDeclarationKind::Static(item) => {
                self.ty(&item.ty)?;
                self.begin_body();
                self.expression(&item.value)
            }
            AstDeclarationKind::Function(function) => {
                self.function_signature(&function.signature)?;
                self.begin_body();
                self.block(&function.body)
            }
            AstDeclarationKind::Generator(generator) => {
                self.generic_parameters(generator.generics.as_ref())?;
                for parameter in &generator.parameters {
                    self.pattern(&parameter.pattern)?;
                    self.ty(&parameter.ty)?;
                }
                self.ty(&generator.resume)?;
                self.ty(&generator.yields)?;
                self.effects(&generator.effects)?;
                if let Some(result) = &generator.result {
                    self.ty(result)?;
                }
                self.where_clause(generator.where_clause.as_ref())?;
                self.begin_body();
                self.block(&generator.body)
            }
            AstDeclarationKind::System(system) => {
                self.generic_parameters(system.generics.as_ref())?;
                for parameter in &system.parameters {
                    match &parameter.kind {
                        crate::ast::AstSystemParameterKind::ResourceRead(ty)
                        | crate::ast::AstSystemParameterKind::ResourceWrite(ty)
                        | crate::ast::AstSystemParameterKind::Capability(ty) => self.ty(ty)?,
                        // The package-qualified Query child item is the sole
                        // HIR/path/body authority for these exact source
                        // terms. The parent System definition retains the
                        // same interleaved types in its typed declaration
                        // skeleton, so revisiting them here would mint two
                        // path-use/body rows for one source span.
                        crate::ast::AstSystemParameterKind::Query(_) => {}
                        crate::ast::AstSystemParameterKind::Commands => {}
                    }
                    self.binding(&parameter.name, parameter.span)?;
                }
                self.effects(&system.effects)?;
                self.where_clause(system.where_clause.as_ref())?;
                self.begin_body();
                self.block(&system.body)
            }
            AstDeclarationKind::Schedule(schedule) => {
                self.begin_body();
                for run in &schedule.runs {
                    self.path_in_namespace(&run.target, Some(Namespace::Value))?;
                    if let Some(arguments) = &run.arguments {
                        for argument in &arguments.arguments {
                            match argument {
                                crate::ast::AstSystemGenericArgument::Type(ty) => self.ty(ty)?,
                                crate::ast::AstSystemGenericArgument::IntegerConst(value) => {
                                    self.type_const(
                                        value,
                                        SemanticBodyKind::IntegerGenericArgument,
                                    )?;
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
            AstDeclarationKind::Trait(trait_) => {
                self.generic_parameters(trait_.generics.as_ref())?;
                self.where_clause(trait_.where_clause.as_ref())
            }
        }
    }

    fn implementation_header(&mut self, implementation: &AstImpl) -> Result<(), FrontendError> {
        self.generic_parameters(implementation.generics.as_ref())?;
        if let Some(path) = &implementation.trait_path {
            self.path_in_namespace(path, Some(Namespace::Type))?;
        }
        self.ty(&implementation.target)?;
        self.where_clause(implementation.where_clause.as_ref())
    }

    fn function_signature(
        &mut self,
        signature: &crate::ast::AstFunctionSignature,
    ) -> Result<(), FrontendError> {
        self.generic_parameters(signature.generics.as_ref())?;
        for parameter in &signature.parameters {
            self.pattern(&parameter.pattern)?;
            self.ty(&parameter.ty)?;
        }
        self.effects(&signature.effects)?;
        if let Some(result) = &signature.result {
            self.ty(result)?;
        }
        self.where_clause(signature.where_clause.as_ref())
    }

    fn method_signature(
        &mut self,
        signature: &crate::ast::AstMethodSignature,
    ) -> Result<(), FrontendError> {
        self.generic_parameters(signature.generics.as_ref())?;
        for parameter in &signature.parameters {
            match parameter {
                AstMethodParameter::Receiver(receiver) => {
                    self.binding_name("self", receiver.span)?;
                    self.ty(&receiver_type(receiver))?;
                }
                AstMethodParameter::Parameter(parameter) => {
                    self.pattern(&parameter.pattern)?;
                    self.ty(&parameter.ty)?;
                }
            }
        }
        self.effects(&signature.effects)?;
        if let Some(result) = &signature.result {
            self.ty(result)?;
        }
        self.where_clause(signature.where_clause.as_ref())
    }

    fn generic_parameters(
        &mut self,
        parameters: Option<&crate::ast::AstGenericParameters>,
    ) -> Result<(), FrontendError> {
        let Some(parameters) = parameters else {
            return Ok(());
        };
        for parameter in &parameters.parameters {
            if let crate::ast::AstGenericParameterKind::Type { bounds, .. } = &parameter.kind {
                for bound in bounds {
                    if let crate::ast::AstTypeBoundKind::Trait(path) = &bound.kind {
                        self.path_in_namespace(path, Some(Namespace::Type))?;
                    }
                }
            }
        }
        Ok(())
    }

    fn where_clause(
        &mut self,
        clause: Option<&crate::ast::AstWhereClause>,
    ) -> Result<(), FrontendError> {
        let Some(clause) = clause else {
            return Ok(());
        };
        for predicate in &clause.predicates {
            if let crate::ast::AstWherePredicateKind::Type { ty, bounds } = &predicate.kind {
                self.ty(ty)?;
                for bound in bounds {
                    if let crate::ast::AstTypeBoundKind::Trait(path) = &bound.kind {
                        self.path_in_namespace(path, Some(Namespace::Type))?;
                    }
                }
            }
        }
        Ok(())
    }

    fn effects(&mut self, effects: &crate::ast::AstEffectSets) -> Result<(), FrontendError> {
        if let Some(requires) = &effects.requires {
            for path in &requires.members {
                self.current_symbolic_inputs()
                    .effects
                    .push(AstSymbolicEffect::Requires(path.clone()));
                self.path_in_namespace(path, Some(Namespace::Type))?;
            }
        }
        if let Some(throws) = &effects.throws {
            for ty in &throws.members {
                self.current_symbolic_inputs()
                    .effects
                    .push(AstSymbolicEffect::Throws(ty.clone()));
                self.ty(ty)?;
            }
        }
        Ok(())
    }

    fn ty(&mut self, ty: &crate::ast::AstType) -> Result<(), FrontendError> {
        self.current_symbolic_inputs().types.push(ty.clone());
        match &ty.kind {
            crate::ast::AstTypeKind::Path(path) => {
                self.path_with_generic_namespace(path, Some(Namespace::Type), Some(Namespace::Type))
            }
            crate::ast::AstTypeKind::Tuple(types) => {
                for ty in types {
                    self.ty(ty)?;
                }
                Ok(())
            }
            crate::ast::AstTypeKind::Array { element, length } => {
                self.ty(element)?;
                self.type_const(length, SemanticBodyKind::ArrayLength)
            }
            crate::ast::AstTypeKind::Slice(element) => self.ty(element),
            crate::ast::AstTypeKind::Reference { pointee, .. }
            | crate::ast::AstTypeKind::RawPointer { pointee, .. } => self.ty(pointee),
            crate::ast::AstTypeKind::FunctionPointer {
                parameters,
                effects,
                result,
                ..
            } => {
                for parameter in parameters {
                    self.ty(parameter)?;
                }
                self.effects(effects)?;
                if let Some(result) = result {
                    self.ty(result)?;
                }
                Ok(())
            }
            crate::ast::AstTypeKind::Scalar(_)
            | crate::ast::AstTypeKind::Never
            | crate::ast::AstTypeKind::Unit
            | crate::ast::AstTypeKind::Str
            | crate::ast::AstTypeKind::SelfType => Ok(()),
        }
    }

    fn path_in_namespace(
        &mut self,
        path: &crate::ast::AstPath,
        namespace: Option<Namespace>,
    ) -> Result<(), FrontendError> {
        self.path_with_generic_namespace(path, namespace, namespace)
    }

    fn path_with_generic_namespace(
        &mut self,
        path: &crate::ast::AstPath,
        namespace: Option<Namespace>,
        generic_namespace: Option<Namespace>,
    ) -> Result<(), FrontendError> {
        if namespace == Some(Namespace::Type) {
            self.current_symbolic_inputs()
                .c2_type_roots
                .push(crate::ast::AstType::new(
                    crate::ast::AstTypeKind::Path(path.clone()),
                    path.span,
                ));
        }
        let formal_parameters = self.resolved_path_formal_parameters(path, generic_namespace)?;
        let lexical_local = (namespace == Some(Namespace::Value))
            .then(|| {
                unqualified_path_name(path).and_then(|name| {
                    self.scopes
                        .iter()
                        .rev()
                        .find_map(|scope| scope.get(name.as_str()).copied())
                })
            })
            .flatten();
        let mut parameter_index = 0;
        let mut resolved_arguments = Vec::new();
        if let Some(arguments) = &path.generic_arguments {
            self.generic_arguments_with_formals(
                arguments,
                formal_parameters.as_deref(),
                &mut parameter_index,
                &mut resolved_arguments,
            )?;
        }
        for segment in &path.segments {
            if let Some(arguments) = &segment.generic_arguments {
                self.generic_arguments_with_formals(
                    arguments,
                    formal_parameters.as_deref(),
                    &mut parameter_index,
                    &mut resolved_arguments,
                )?;
            }
        }
        self.path_uses.push(HirPathUse {
            path: path.clone(),
            namespace,
            lexical_local,
            generic_arguments: resolved_arguments,
        });
        Ok(())
    }

    fn resolved_path_formal_parameters(
        &self,
        path: &crate::ast::AstPath,
        namespace: Option<Namespace>,
    ) -> Result<Option<Vec<crate::ast::AstGenericParameter>>, FrontendError> {
        let empty_locals = BTreeMap::new();
        let resolution = resolve_general_path(
            self.pattern_resolution.packages,
            self.pattern_resolution.graph,
            self.pattern_resolution.location,
            path,
            namespace,
            LexicalPathEnvironment {
                generics: self.pattern_resolution.generics,
                locals: &empty_locals,
            },
            self.pattern_resolution.embedded_core,
        )?;
        let [Res::Item(item)] = resolution.resolutions.as_slice() else {
            return Ok(None);
        };
        let item = find_item(self.pattern_resolution.packages, item.owner()).ok_or_else(|| {
            frontend_error(
                FrontendErrorCode::Target,
                Diagnostic::at(
                    "IDENTITY001",
                    path.span,
                    "resolved generic-argument owner is missing from the C1 item arena",
                ),
            )
        })?;
        Ok(Some(
            item_generic_parameters(item)
                .map(|parameters| parameters.parameters.clone())
                .unwrap_or_default(),
        ))
    }

    fn generic_arguments(
        &mut self,
        arguments: &crate::ast::AstGenericArguments,
    ) -> Result<(), FrontendError> {
        self.generic_arguments_with_known_formals(arguments, None)
    }

    fn generic_arguments_with_known_formals(
        &mut self,
        arguments: &crate::ast::AstGenericArguments,
        formal_parameters: Option<&[crate::ast::AstGenericParameter]>,
    ) -> Result<(), FrontendError> {
        let mut parameter_index = 0;
        let mut resolved = Vec::new();
        self.generic_arguments_with_formals(
            arguments,
            formal_parameters,
            &mut parameter_index,
            &mut resolved,
        )?;
        self.postfix_generic_argument_uses
            .push(HirGenericArgumentsUse {
                span: arguments.span,
                arguments: resolved,
            });
        Ok(())
    }

    fn generic_arguments_with_formals(
        &mut self,
        arguments: &crate::ast::AstGenericArguments,
        formal_parameters: Option<&[crate::ast::AstGenericParameter]>,
        parameter_index: &mut usize,
        output: &mut Vec<HirGenericArgumentUse>,
    ) -> Result<(), FrontendError> {
        for argument in &arguments.arguments {
            let formal_parameter =
                formal_parameters.and_then(|parameters| parameters.get(*parameter_index));
            *parameter_index = parameter_index.checked_add(1).ok_or_else(|| {
                frontend_path_error(
                    FrontendErrorCode::Target,
                    "IDENTITY001",
                    "generic argument count exceeds the host index width",
                )
            })?;
            let formal_kind = formal_parameter.map(ast_generic_parameter_kind);
            let value = match &argument.kind {
                crate::ast::AstGenericArgumentKind::Type(ty) => {
                    self.ty(ty)?;
                    let resolved = if formal_kind
                        .as_ref()
                        .is_none_or(|kind| matches!(kind, GenericParameterKind::Type))
                    {
                        resolve_symbolic_type(&self.symbolic_lowering_context(), ty)?
                    } else {
                        ResolvedSymbolicType::Pending {
                            span: argument.span,
                            reason: UnresolvedPathKind::GenericFormationPendingC2,
                            canonical: canonical_type(ty),
                        }
                    };
                    ResolvedGenericArgument::Type(resolved)
                }
                crate::ast::AstGenericArgumentKind::IntegerConst(value) => {
                    let integer_type = match formal_kind.as_ref() {
                        Some(GenericParameterKind::IntegerConst(ty)) => *ty,
                        Some(GenericParameterKind::Lifetime | GenericParameterKind::Type)
                        | None => const_expression_integer_type(value),
                    };
                    self.type_const_with_integer_type(
                        value,
                        SemanticBodyKind::IntegerGenericArgument,
                        integer_type,
                    )?;
                    let resolved = if formal_kind
                        .as_ref()
                        .is_none_or(|kind| matches!(kind, GenericParameterKind::IntegerConst(_)))
                    {
                        resolve_symbolic_const(
                            &self.symbolic_lowering_context(),
                            value,
                            integer_type,
                        )?
                    } else {
                        ResolvedSymbolicConst::Pending {
                            span: argument.span,
                            reason: UnresolvedPathKind::GenericFormationPendingC2,
                            canonical: canonical_const_expression(value),
                        }
                    };
                    ResolvedGenericArgument::IntegerConst(resolved)
                }
                crate::ast::AstGenericArgumentKind::Lifetime(lifetime) => {
                    let resolved = if formal_kind
                        .as_ref()
                        .is_none_or(|kind| matches!(kind, GenericParameterKind::Lifetime))
                    {
                        resolve_symbolic_lifetime(
                            &self.symbolic_lowering_context(),
                            lifetime,
                            argument.span,
                        )?
                    } else {
                        ResolvedSymbolicLifetime::Pending {
                            span: argument.span,
                            reason: UnresolvedPathKind::GenericFormationPendingC2,
                            canonical: format!("'{}", lifetime.as_str()),
                        }
                    };
                    ResolvedGenericArgument::Lifetime(resolved)
                }
            };
            output.push(HirGenericArgumentUse {
                span: argument.span,
                formal_kind,
                value,
            });
        }
        Ok(())
    }

    fn symbolic_lowering_context(&self) -> SymbolicLoweringContext<'_, '_> {
        SymbolicLoweringContext {
            packages: self.pattern_resolution.packages,
            graph: self.pattern_resolution.graph,
            item: self.owner,
            location: self.pattern_resolution.location,
            generics: self.pattern_resolution.generics,
            embedded_core: self.pattern_resolution.embedded_core,
            lifetime_domain: LifetimeDomain::BodyLocal,
            contextual_self_template: false,
        }
    }

    fn type_const(
        &mut self,
        expression: &AstConstExpression,
        kind: SemanticBodyKind,
    ) -> Result<(), FrontendError> {
        self.type_const_with_integer_type(
            expression,
            kind,
            const_expression_integer_type(expression),
        )
    }

    fn type_const_with_integer_type(
        &mut self,
        expression: &AstConstExpression,
        kind: SemanticBodyKind,
        integer_type: IntegerType,
    ) -> Result<(), FrontendError> {
        self.current_symbolic_inputs()
            .consts
            .push((expression.clone(), integer_type));
        self.push(
            kind,
            expression.span,
            HirBodySource::ConstExpression(Box::new(expression.clone())),
        )?;
        self.const_expression(expression)
    }

    fn const_expression(&mut self, expression: &AstConstExpression) -> Result<(), FrontendError> {
        match &expression.kind {
            crate::ast::AstConstExpressionKind::Path(path) => {
                self.path_in_namespace(path, Some(Namespace::Value))
            }
            crate::ast::AstConstExpressionKind::Group(child)
            | crate::ast::AstConstExpressionKind::Unary { operand: child, .. } => {
                self.const_expression(child)
            }
            crate::ast::AstConstExpressionKind::Binary { left, right, .. } => {
                self.const_expression(left)?;
                self.const_expression(right)
            }
            crate::ast::AstConstExpressionKind::Integer(_) => Ok(()),
        }
    }

    fn world_initializer(&mut self, initializer: &AstWorldInitBlock) -> Result<(), FrontendError> {
        for entry in &initializer.entries {
            match &entry.kind {
                crate::ast::AstWorldInitKind::Resource { ty, value } => {
                    self.ty(ty)?;
                    self.expression(value)?;
                }
                crate::ast::AstWorldInitKind::Spawn { values } => {
                    for value in values {
                        self.expression(value)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn block(&mut self, block: &AstBlock) -> Result<(), FrontendError> {
        self.scopes.push(BTreeMap::new());
        for statement in &block.statements {
            match &statement.kind {
                crate::ast::AstStatementKind::Let {
                    pattern,
                    ty,
                    value,
                    else_block,
                } => {
                    if let Some(ty) = ty {
                        self.ty(ty)?;
                    }
                    self.expression(value)?;
                    if let Some(block) = else_block {
                        self.block(block)?;
                    }
                    self.pattern(pattern)?;
                }
                crate::ast::AstStatementKind::For {
                    pattern,
                    iterator,
                    body,
                    ..
                } => {
                    self.expression(iterator)?;
                    self.scopes.push(BTreeMap::new());
                    self.pattern(pattern)?;
                    self.block(body)?;
                    self.scopes.pop();
                }
                crate::ast::AstStatementKind::Assignment { place, value, .. } => {
                    self.expression(place)?;
                    self.expression(value)?;
                }
                crate::ast::AstStatementKind::Expression { expression, .. } => {
                    self.expression(expression)?;
                }
            }
        }
        if let Some(tail) = &block.tail {
            self.expression(tail)?;
        }
        self.scopes.pop();
        Ok(())
    }

    fn expression(&mut self, expression: &AstExpression) -> Result<(), FrontendError> {
        use crate::ast::AstExpressionKind;
        match &expression.kind {
            AstExpressionKind::Path(path) => self.path_in_namespace(path, Some(Namespace::Value)),
            AstExpressionKind::SelfValue => {
                let Some(receiver) = self
                    .scopes
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get("self").copied())
                else {
                    return Err(frontend_error(
                        FrontendErrorCode::Name,
                        Diagnostic::at(
                            "NAME001",
                            expression.span,
                            "lowercase `self` is only available in a method with a receiver",
                        ),
                    ));
                };
                self.self_uses.push(HirSelfUse {
                    span: expression.span,
                    receiver,
                });
                Ok(())
            }
            AstExpressionKind::Group(child)
            | AstExpressionKind::Unary { operand: child, .. }
            | AstExpressionKind::Yield(child) => self.expression(child),
            AstExpressionKind::Tuple(values) | AstExpressionKind::Array(values) => {
                for value in values {
                    self.expression(value)?;
                }
                Ok(())
            }
            AstExpressionKind::ArrayRepeat { value, count } => {
                self.expression(value)?;
                self.type_const(count, SemanticBodyKind::RepeatCount)
            }
            AstExpressionKind::Record {
                constructor,
                fields,
            } => {
                self.path_in_namespace(constructor, Some(Namespace::Value))?;
                for field in fields {
                    self.expression(&field.value)?;
                }
                Ok(())
            }
            AstExpressionKind::Block(block)
            | AstExpressionKind::Loop(block)
            | AstExpressionKind::Unsafe(block) => self.block(block),
            AstExpressionKind::If(if_) => {
                self.scopes.push(BTreeMap::new());
                self.condition(&if_.condition)?;
                self.block(&if_.then_block)?;
                self.scopes.pop();
                if let Some(branch) = &if_.else_branch {
                    match branch {
                        crate::ast::AstElseBranch::Block(block) => self.block(block)?,
                        crate::ast::AstElseBranch::If(expression) => self.expression(expression)?,
                    }
                }
                Ok(())
            }
            AstExpressionKind::While(while_) => {
                self.scopes.push(BTreeMap::new());
                self.condition(&while_.condition)?;
                self.block(&while_.body)?;
                self.scopes.pop();
                Ok(())
            }
            AstExpressionKind::Match { operand, arms }
            | AstExpressionKind::Catch { operand, arms } => {
                self.expression(operand)?;
                for arm in arms {
                    self.scopes.push(BTreeMap::new());
                    self.pattern(&arm.pattern)?;
                    if let Some(guard) = &arm.guard {
                        self.expression(guard)?;
                    }
                    self.expression(&arm.value)?;
                    self.scopes.pop();
                }
                Ok(())
            }
            AstExpressionKind::Closure(closure) => {
                self.push(
                    SemanticBodyKind::Closure,
                    expression.span,
                    HirBodySource::Closure(Box::new((**closure).clone())),
                )?;
                self.scopes.push(BTreeMap::new());
                for parameter in &closure.parameters {
                    self.pattern(&parameter.pattern)?;
                    if let Some(ty) = &parameter.ty {
                        self.ty(ty)?;
                    }
                }
                self.effects(&closure.effects)?;
                if let Some(result) = &closure.result {
                    self.ty(result)?;
                }
                self.expression(&closure.body)?;
                self.scopes.pop();
                Ok(())
            }
            AstExpressionKind::GeneratorClosure(generator) => {
                self.push(
                    SemanticBodyKind::Generator,
                    expression.span,
                    HirBodySource::GeneratorClosure(Box::new((**generator).clone())),
                )?;
                self.scopes.push(BTreeMap::new());
                for parameter in &generator.parameters {
                    self.pattern(&parameter.pattern)?;
                    if let Some(ty) = &parameter.ty {
                        self.ty(ty)?;
                    }
                }
                self.ty(&generator.resume)?;
                self.ty(&generator.yields)?;
                self.effects(&generator.effects)?;
                if let Some(result) = &generator.result {
                    self.ty(result)?;
                }
                self.expression(&generator.body)?;
                self.scopes.pop();
                Ok(())
            }
            AstExpressionKind::Return(value)
            | AstExpressionKind::Break(value)
            | AstExpressionKind::Throw(value) => {
                if let Some(value) = value {
                    self.expression(value)?;
                }
                Ok(())
            }
            AstExpressionKind::Binary { left, right, .. } => {
                self.expression(left)?;
                self.expression(right)
            }
            AstExpressionKind::Cast { value, ty } => {
                self.expression(value)?;
                self.ty(ty)
            }
            AstExpressionKind::Postfix { base, parts } => {
                let base_formal_parameters = match &base.kind {
                    AstExpressionKind::Path(path) => {
                        self.resolved_path_formal_parameters(path, Some(Namespace::Value))?
                    }
                    _ => None,
                };
                self.expression(base)?;
                for part in parts {
                    match &part.kind {
                        crate::ast::AstPostfixKind::Call(arguments)
                        | crate::ast::AstPostfixKind::CommandSpawn(arguments) => {
                            for argument in arguments {
                                self.expression(argument)?;
                            }
                        }
                        crate::ast::AstPostfixKind::Index(index)
                        | crate::ast::AstPostfixKind::Resume(index) => self.expression(index)?,
                        crate::ast::AstPostfixKind::Method {
                            generic_arguments,
                            arguments,
                            ..
                        } => {
                            if let Some(arguments_) = generic_arguments {
                                self.generic_arguments(arguments_)?;
                            }
                            for argument in arguments {
                                self.expression(argument)?;
                            }
                        }
                        crate::ast::AstPostfixKind::TurbofishCall {
                            generic_arguments,
                            arguments,
                        } => {
                            self.generic_arguments_with_known_formals(
                                generic_arguments,
                                base_formal_parameters.as_deref(),
                            )?;
                            for argument in arguments {
                                self.expression(argument)?;
                            }
                        }
                        crate::ast::AstPostfixKind::Field(_)
                        | crate::ast::AstPostfixKind::TupleField(_) => {}
                    }
                }
                Ok(())
            }
            AstExpressionKind::Literal(_)
            | AstExpressionKind::Unit
            | AstExpressionKind::Continue => Ok(()),
        }
    }

    fn condition(&mut self, condition: &crate::ast::AstCondition) -> Result<(), FrontendError> {
        match condition {
            crate::ast::AstCondition::Expression(expression) => self.expression(expression),
            crate::ast::AstCondition::Let { pattern, value } => {
                self.expression(value)?;
                self.pattern(pattern)
            }
        }
    }

    fn pattern(&mut self, pattern: &AstPattern) -> Result<(), FrontendError> {
        match &pattern.kind {
            AstPatternKind::BarePathOrBinding(path) => match self.classify_bare_pattern(path)? {
                BarePatternResolution::Binding => self.binding(
                    unqualified_path_name(path)
                        .expect("only an unqualified bare pattern can classify as a binding"),
                    pattern.span,
                ),
                BarePatternResolution::Path => self.path_in_namespace(path, Some(Namespace::Value)),
            },
            AstPatternKind::Reference { pattern, .. } => self.pattern(pattern),
            AstPatternKind::Tuple(patterns) => {
                for pattern in patterns {
                    self.pattern(pattern)?;
                }
                Ok(())
            }
            AstPatternKind::Or(alternatives) => self.or_pattern(alternatives),
            AstPatternKind::Slice(parts) => {
                for part in parts {
                    if let crate::ast::AstSlicePatternPart::Pattern(pattern) = part {
                        self.pattern(pattern)?;
                    }
                }
                Ok(())
            }
            AstPatternKind::Constructor { path, payload } => {
                self.path_in_namespace(path, Some(Namespace::Value))?;
                match payload {
                    crate::ast::AstConstructorPatternPayload::Unit => {}
                    crate::ast::AstConstructorPatternPayload::Tuple(patterns) => {
                        for pattern in patterns {
                            self.pattern(pattern)?;
                        }
                    }
                    crate::ast::AstConstructorPatternPayload::Record(fields) => {
                        for field in fields {
                            self.pattern(&field.pattern)?;
                        }
                    }
                }
                Ok(())
            }
            AstPatternKind::Range { start, end, .. } => {
                for endpoint in [start, end] {
                    if let crate::ast::AstRangeEndpoint::Const(path) = endpoint {
                        self.path_in_namespace(path, Some(Namespace::Value))?;
                    }
                }
                Ok(())
            }
            AstPatternKind::At { binding, pattern } => {
                let fact = Self::at_binding_fact(binding);
                self.binding(&fact.name, fact.span)?;
                self.pattern(pattern)
            }
            AstPatternKind::Binding { name, .. } => self.binding(name, pattern.span),
            AstPatternKind::Wildcard | AstPatternKind::Unit | AstPatternKind::Literal(_) => Ok(()),
        }
    }

    fn or_pattern(&mut self, alternatives: &[AstPattern]) -> Result<(), FrontendError> {
        let Some(first) = alternatives.first() else {
            return Ok(());
        };
        let expected = self.pattern_binding_facts(first)?;
        let expected_names = expected
            .iter()
            .map(|fact| fact.name.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        for alternative in &alternatives[1..] {
            let actual = self.pattern_binding_facts(alternative)?;
            let actual_names = actual
                .iter()
                .map(|fact| fact.name.as_str().to_owned())
                .collect::<BTreeSet<_>>();
            if actual_names != expected_names {
                return Err(frontend_error(
                    FrontendErrorCode::Name,
                    Diagnostic::at(
                        "PATTERN001",
                        alternative.span,
                        format!(
                            "or-pattern alternative binds {actual_names:?}, expected {expected_names:?}"
                        ),
                    )
                    .with_secondary(first.span, "first alternative establishes the binding set"),
                ));
            }
        }
        for alternative in alternatives {
            self.pattern_paths(alternative)?;
        }
        for fact in expected {
            self.binding(&fact.name, fact.span)?;
        }
        Ok(())
    }

    fn pattern_binding_facts(
        &self,
        pattern: &AstPattern,
    ) -> Result<Vec<PatternBindingFact>, FrontendError> {
        let mut output = Vec::new();
        match &pattern.kind {
            AstPatternKind::BarePathOrBinding(path) => {
                if self.classify_bare_pattern(path)? == BarePatternResolution::Binding {
                    Self::push_pattern_binding_fact(
                        &mut output,
                        PatternBindingFact {
                            name: unqualified_path_name(path)
                                .expect("a classified binding is an unqualified path")
                                .clone(),
                            span: pattern.span,
                        },
                    )?;
                }
            }
            AstPatternKind::Binding { name, .. } => {
                Self::push_pattern_binding_fact(
                    &mut output,
                    PatternBindingFact {
                        name: name.clone(),
                        span: pattern.span,
                    },
                )?;
            }
            AstPatternKind::Reference { pattern, .. } => {
                Self::merge_pattern_binding_facts(
                    &mut output,
                    self.pattern_binding_facts(pattern)?,
                )?;
            }
            AstPatternKind::Tuple(patterns) | AstPatternKind::Or(patterns) => {
                if matches!(pattern.kind, AstPatternKind::Or(_)) {
                    let Some(first) = patterns.first() else {
                        return Ok(output);
                    };
                    let expected = self.pattern_binding_facts(first)?;
                    let expected_names = expected
                        .iter()
                        .map(|fact| fact.name.as_str().to_owned())
                        .collect::<BTreeSet<_>>();
                    for alternative in &patterns[1..] {
                        let actual = self.pattern_binding_facts(alternative)?;
                        let actual_names = actual
                            .iter()
                            .map(|fact| fact.name.as_str().to_owned())
                            .collect::<BTreeSet<_>>();
                        if actual_names != expected_names {
                            return Err(frontend_error(
                                FrontendErrorCode::Name,
                                Diagnostic::at(
                                    "PATTERN001",
                                    alternative.span,
                                    format!(
                                        "or-pattern alternative binds {actual_names:?}, expected {expected_names:?}"
                                    ),
                                )
                                .with_secondary(
                                    first.span,
                                    "first alternative establishes the binding set",
                                ),
                            ));
                        }
                    }
                    Self::merge_pattern_binding_facts(&mut output, expected)?;
                } else {
                    for pattern in patterns {
                        Self::merge_pattern_binding_facts(
                            &mut output,
                            self.pattern_binding_facts(pattern)?,
                        )?;
                    }
                }
            }
            AstPatternKind::Slice(parts) => {
                for part in parts {
                    if let crate::ast::AstSlicePatternPart::Pattern(pattern) = part {
                        Self::merge_pattern_binding_facts(
                            &mut output,
                            self.pattern_binding_facts(pattern)?,
                        )?;
                    }
                }
            }
            AstPatternKind::Constructor { payload, .. } => match payload {
                crate::ast::AstConstructorPatternPayload::Unit => {}
                crate::ast::AstConstructorPatternPayload::Tuple(patterns) => {
                    for pattern in patterns {
                        Self::merge_pattern_binding_facts(
                            &mut output,
                            self.pattern_binding_facts(pattern)?,
                        )?;
                    }
                }
                crate::ast::AstConstructorPatternPayload::Record(fields) => {
                    for field in fields {
                        Self::merge_pattern_binding_facts(
                            &mut output,
                            self.pattern_binding_facts(&field.pattern)?,
                        )?;
                    }
                }
            },
            AstPatternKind::At { binding, pattern } => {
                Self::push_pattern_binding_fact(&mut output, Self::at_binding_fact(binding))?;
                Self::merge_pattern_binding_facts(
                    &mut output,
                    self.pattern_binding_facts(pattern)?,
                )?;
            }
            AstPatternKind::Range { .. }
            | AstPatternKind::Wildcard
            | AstPatternKind::Unit
            | AstPatternKind::Literal(_) => {}
        }
        Ok(output)
    }

    fn pattern_paths(&mut self, pattern: &AstPattern) -> Result<(), FrontendError> {
        match &pattern.kind {
            AstPatternKind::BarePathOrBinding(path) => {
                if self.classify_bare_pattern(path)? == BarePatternResolution::Path {
                    self.path_in_namespace(path, Some(Namespace::Value))?;
                }
            }
            AstPatternKind::Reference { pattern, .. } => self.pattern_paths(pattern)?,
            AstPatternKind::Tuple(patterns) | AstPatternKind::Or(patterns) => {
                for pattern in patterns {
                    self.pattern_paths(pattern)?;
                }
            }
            AstPatternKind::Slice(parts) => {
                for part in parts {
                    if let crate::ast::AstSlicePatternPart::Pattern(pattern) = part {
                        self.pattern_paths(pattern)?;
                    }
                }
            }
            AstPatternKind::Constructor { path, payload } => {
                self.path_in_namespace(path, Some(Namespace::Value))?;
                match payload {
                    crate::ast::AstConstructorPatternPayload::Unit => {}
                    crate::ast::AstConstructorPatternPayload::Tuple(patterns) => {
                        for pattern in patterns {
                            self.pattern_paths(pattern)?;
                        }
                    }
                    crate::ast::AstConstructorPatternPayload::Record(fields) => {
                        for field in fields {
                            self.pattern_paths(&field.pattern)?;
                        }
                    }
                }
            }
            AstPatternKind::Range { start, end, .. } => {
                for endpoint in [start, end] {
                    if let crate::ast::AstRangeEndpoint::Const(path) = endpoint {
                        self.path_in_namespace(path, Some(Namespace::Value))?;
                    }
                }
            }
            AstPatternKind::At { pattern, .. } => self.pattern_paths(pattern)?,
            AstPatternKind::Binding { .. }
            | AstPatternKind::Wildcard
            | AstPatternKind::Unit
            | AstPatternKind::Literal(_) => {}
        }
        Ok(())
    }

    fn classify_bare_pattern(
        &self,
        path: &crate::ast::AstPath,
    ) -> Result<BarePatternResolution, FrontendError> {
        if unqualified_path_name(path).is_none() {
            return Ok(BarePatternResolution::Path);
        }
        let empty_locals = BTreeMap::new();
        let resolution = resolve_general_path(
            self.pattern_resolution.packages,
            self.pattern_resolution.graph,
            self.pattern_resolution.location,
            path,
            Some(Namespace::Value),
            LexicalPathEnvironment {
                generics: self.pattern_resolution.generics,
                locals: &empty_locals,
            },
            self.pattern_resolution.embedded_core,
        )?;
        if resolution.unresolved.is_some() || resolution.resolutions.is_empty() {
            return Ok(BarePatternResolution::Binding);
        }
        if resolution.resolutions.len() != 1 {
            return Err(frontend_error(
                FrontendErrorCode::Name,
                Diagnostic::at(
                    "PATTERN001",
                    path.span,
                    "bare pattern identifier has ambiguous value-namespace lookup",
                ),
            ));
        }
        let is_const_or_unit_variant = match resolution.resolutions[0] {
            Res::Item(HirItemRes::Definition(item)) => {
                find_item(self.pattern_resolution.packages, item)
                    .is_some_and(|item| item.kind == DeclarationKind::Const)
            }
            Res::Item(HirItemRes::EnumVariant { owner, ordinal }) => {
                enum_variant(self.pattern_resolution.packages, owner, ordinal)
                    .is_some_and(|variant| matches!(variant.form, crate::ast::AstVariantForm::Unit))
            }
            Res::Item(HirItemRes::NominalConstructor { .. }) => false,
            Res::Generic(parameter) => matches!(
                generic_parameter_kind(self.pattern_resolution.packages, parameter),
                Some(GenericParameterKind::IntegerConst(_))
            ),
            Res::Module(_) | Res::Local(_) | Res::Builtin(_) => false,
        };
        Ok(if is_const_or_unit_variant {
            BarePatternResolution::Path
        } else {
            BarePatternResolution::Binding
        })
    }

    fn at_binding_fact(pattern: &AstPattern) -> PatternBindingFact {
        let name = match &pattern.kind {
            AstPatternKind::BarePathOrBinding(path) => unqualified_path_name(path)
                .expect("parser admits only an unqualified identifier before `@`")
                .clone(),
            AstPatternKind::Binding { name, .. } => name.clone(),
            _ => unreachable!("parser admits only binding patterns before `@`"),
        };
        PatternBindingFact {
            name,
            span: pattern.span,
        }
    }

    fn merge_pattern_binding_facts(
        output: &mut Vec<PatternBindingFact>,
        facts: Vec<PatternBindingFact>,
    ) -> Result<(), FrontendError> {
        for fact in facts {
            Self::push_pattern_binding_fact(output, fact)?;
        }
        Ok(())
    }

    fn push_pattern_binding_fact(
        output: &mut Vec<PatternBindingFact>,
        fact: PatternBindingFact,
    ) -> Result<(), FrontendError> {
        if let Some(previous) = output.iter().find(|existing| existing.name == fact.name) {
            return Err(frontend_error(
                FrontendErrorCode::Name,
                Diagnostic::at(
                    "NAME001",
                    fact.span,
                    format!("duplicate pattern binding `{}`", fact.name.as_str()),
                )
                .with_secondary(previous.span, "first bound here"),
            ));
        }
        output.push(fact);
        Ok(())
    }

    fn binding(&mut self, name: &Symbol, span: Span) -> Result<(), FrontendError> {
        self.binding_name(name.as_str(), span)
    }

    fn binding_name(&mut self, name: &str, span: Span) -> Result<(), FrontendError> {
        if let Some(previous_id) = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
        {
            let previous = self
                .locals
                .iter()
                .find(|binding| binding.id == previous_id)
                .expect("every active lexical binding has a retained local row");
            return Err(frontend_error(
                FrontendErrorCode::Name,
                Diagnostic::at(
                    "NAME001",
                    span,
                    format!("duplicate active lexical binding `{name}`"),
                )
                .with_secondary(previous.span, "first bound here"),
            ));
        }
        let ordinal = checked_u64(self.locals.len(), "local binding count")?;
        let id = LocalId {
            owner: self.owner,
            ordinal,
        };
        self.locals.push(HirLocalBinding {
            id,
            name: name.to_owned(),
            span,
        });
        self.scopes
            .last_mut()
            .expect("nested-body collector always has a lexical scope")
            .insert(name.to_owned(), id);
        Ok(())
    }
}

fn receiver_type(receiver: &crate::ast::AstReceiver) -> crate::ast::AstType {
    let self_type = crate::ast::AstType::new(crate::ast::AstTypeKind::SelfType, receiver.span);
    match &receiver.kind {
        crate::ast::AstReceiverKind::Value { .. } => self_type,
        crate::ast::AstReceiverKind::Reference { lifetime, mutable } => crate::ast::AstType::new(
            crate::ast::AstTypeKind::Reference {
                lifetime: lifetime.clone(),
                mutable: *mutable,
                pointee: Box::new(self_type),
            },
            receiver.span,
        ),
    }
}

fn member_visibilities(
    declaration: &AstDeclaration,
    inherited: Visibility,
    module_path: &[Symbol],
    modules: &[ResolvedSymbolicModule],
    module: HirModuleId,
) -> Result<Vec<SemanticMemberVisibility>, FrontendError> {
    let mut output = Vec::new();
    match &declaration.kind {
        AstDeclarationKind::Component(record) | AstDeclarationKind::Resource(record) => {
            for (ordinal, field) in record.fields.iter().enumerate() {
                output.push(SemanticMemberVisibility {
                    path: MemberVisibilityPath::Field {
                        ordinal: checked_u64(ordinal, "record field count")?,
                    },
                    declared_visibility: lexical_visibility(
                        &field.visibility,
                        module_path,
                        modules,
                        module,
                    )?,
                });
            }
        }
        AstDeclarationKind::Struct(structure) => match &structure.form {
            crate::ast::AstStructForm::Unit => {}
            crate::ast::AstStructForm::Tuple(fields) => {
                for (ordinal, field) in fields.iter().enumerate() {
                    output.push(SemanticMemberVisibility {
                        path: MemberVisibilityPath::Field {
                            ordinal: checked_u64(ordinal, "tuple field count")?,
                        },
                        declared_visibility: lexical_visibility(
                            &field.visibility,
                            module_path,
                            modules,
                            module,
                        )?,
                    });
                }
            }
            crate::ast::AstStructForm::Record(fields) => {
                for (ordinal, field) in fields.iter().enumerate() {
                    output.push(SemanticMemberVisibility {
                        path: MemberVisibilityPath::Field {
                            ordinal: checked_u64(ordinal, "record field count")?,
                        },
                        declared_visibility: lexical_visibility(
                            &field.visibility,
                            module_path,
                            modules,
                            module,
                        )?,
                    });
                }
            }
        },
        AstDeclarationKind::Enum(enumeration) => {
            for (variant_ordinal, variant) in enumeration.variants.iter().enumerate() {
                let variant_ordinal = checked_u64(variant_ordinal, "enum variant count")?;
                output.push(SemanticMemberVisibility {
                    path: MemberVisibilityPath::Variant {
                        ordinal: variant_ordinal,
                    },
                    declared_visibility: inherited.clone(),
                });
                let field_count = match &variant.form {
                    crate::ast::AstVariantForm::Unit => 0,
                    crate::ast::AstVariantForm::Tuple(fields) => fields.len(),
                    crate::ast::AstVariantForm::Record(fields) => fields.len(),
                };
                for field_ordinal in 0..field_count {
                    output.push(SemanticMemberVisibility {
                        path: MemberVisibilityPath::VariantField {
                            variant_ordinal,
                            field_ordinal: checked_u64(field_ordinal, "variant field count")?,
                        },
                        declared_visibility: inherited.clone(),
                    });
                }
            }
        }
        AstDeclarationKind::Trait(trait_) => {
            for (ordinal, _) in trait_.methods.iter().enumerate() {
                output.push(SemanticMemberVisibility {
                    path: MemberVisibilityPath::Method {
                        ordinal: checked_u64(ordinal, "trait method count")?,
                    },
                    declared_visibility: inherited.clone(),
                });
            }
        }
        _ => {}
    }
    Ok(output)
}

fn declaration_kind(kind: &AstDeclarationKind) -> DeclarationKind {
    match kind {
        AstDeclarationKind::World { .. } => DeclarationKind::World,
        AstDeclarationKind::Component(_) => DeclarationKind::Component,
        AstDeclarationKind::Resource(_) => DeclarationKind::Resource,
        AstDeclarationKind::Tag => DeclarationKind::Tag,
        AstDeclarationKind::Struct(_) => DeclarationKind::Struct,
        AstDeclarationKind::Enum(_) => DeclarationKind::Enum,
        AstDeclarationKind::TypeAlias(_) => DeclarationKind::TypeAlias,
        AstDeclarationKind::Const(_) => DeclarationKind::Const,
        AstDeclarationKind::Static(_) => DeclarationKind::Static,
        AstDeclarationKind::Function(_) => DeclarationKind::Function,
        AstDeclarationKind::Generator(_) => DeclarationKind::Generator,
        AstDeclarationKind::System(_) => DeclarationKind::System,
        AstDeclarationKind::Schedule(_) => DeclarationKind::Schedule,
        AstDeclarationKind::Trait(_) => DeclarationKind::Trait,
    }
}

fn namespace_for_declaration(kind: &AstDeclarationKind) -> Namespace {
    match kind {
        AstDeclarationKind::World { .. }
        | AstDeclarationKind::Component(_)
        | AstDeclarationKind::Resource(_)
        | AstDeclarationKind::Tag
        | AstDeclarationKind::Struct(_)
        | AstDeclarationKind::Enum(_)
        | AstDeclarationKind::TypeAlias(_)
        | AstDeclarationKind::Trait(_) => Namespace::Type,
        AstDeclarationKind::Const(_)
        | AstDeclarationKind::Static(_)
        | AstDeclarationKind::Function(_)
        | AstDeclarationKind::Generator(_)
        | AstDeclarationKind::System(_)
        | AstDeclarationKind::Schedule(_) => Namespace::Value,
    }
}

fn allocate_item(arenas: &mut HirArenaAllocators) -> Result<HirItemId, FrontendError> {
    arenas.next_item().map_err(|error| {
        frontend_path_error(FrontendErrorCode::Target, error.code(), error.to_string())
    })
}

fn allocate_body(arenas: &mut HirArenaAllocators) -> Result<HirBodyId, FrontendError> {
    arenas.next_body().map_err(|error| {
        frontend_path_error(FrontendErrorCode::Target, error.code(), error.to_string())
    })
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SpanKey {
    file: u64,
    start: u64,
    end: u64,
}

impl From<Span> for SpanKey {
    fn from(span: Span) -> Self {
        Self {
            file: span.file.0,
            start: span.start.byte,
            end: span.end.byte,
        }
    }
}

fn lexical_visibility(
    visibility: &AstVisibility,
    module_path: &[Symbol],
    modules: &[ResolvedSymbolicModule],
    module: HirModuleId,
) -> Result<Visibility, FrontendError> {
    match &visibility.kind {
        AstVisibilityKind::Private => Ok(Visibility::DeclaringModule),
        AstVisibilityKind::Public => Ok(Visibility::Public),
        AstVisibilityKind::Package => Ok(Visibility::Package),
        AstVisibilityKind::Super => {
            let mut path = module_path
                .iter()
                .map(|segment| segment.as_str().to_owned())
                .collect::<Vec<_>>();
            if path.pop().is_none()
                && modules
                    .iter()
                    .find(|candidate| candidate.id == module)
                    .is_some_and(|module| module.parent.is_none())
            {
                return Err(frontend_error(
                    FrontendErrorCode::Visibility,
                    Diagnostic::at(
                        "VISIBILITY001",
                        visibility.span,
                        "`pub(super)` is invalid in the target root module",
                    ),
                ));
            }
            Ok(Visibility::AncestorModule { path })
        }
        AstVisibilityKind::In(path) => {
            let mut resolved = match &path.root {
                crate::ast::AstPathRoot::Package => Vec::new(),
                crate::ast::AstPathRoot::SelfValue => module_path
                    .iter()
                    .map(|segment| segment.as_str().to_owned())
                    .collect(),
                crate::ast::AstPathRoot::Super(count) => {
                    let mut output = module_path
                        .iter()
                        .map(|segment| segment.as_str().to_owned())
                        .collect::<Vec<_>>();
                    for _ in 0..*count {
                        if output.pop().is_none() {
                            return Err(frontend_error(
                                FrontendErrorCode::Visibility,
                                Diagnostic::at(
                                    "VISIBILITY001",
                                    path.span,
                                    "`pub(in path)` uses `super` above the target root",
                                ),
                            ));
                        }
                    }
                    output
                }
                crate::ast::AstPathRoot::Bare
                | crate::ast::AstPathRoot::Identifier(_)
                | crate::ast::AstPathRoot::SelfType => {
                    return Err(frontend_error(
                        FrontendErrorCode::Visibility,
                        Diagnostic::at(
                            "NAME002",
                            path.span,
                            "`pub(in path)` must begin with `package::`, `self::`, or `super::`",
                        ),
                    ));
                }
            };
            resolved.extend(
                path.segments
                    .iter()
                    .map(|segment| segment.name.as_str().to_owned()),
            );
            let current = module_path
                .iter()
                .map(|segment| segment.as_str().to_owned())
                .collect::<Vec<_>>();
            if resolved.len() > current.len() || current[..resolved.len()] != resolved {
                return Err(frontend_error(
                    FrontendErrorCode::Visibility,
                    Diagnostic::at(
                        "VISIBILITY002",
                        path.span,
                        "`pub(in path)` must name the declaring module or one of its ancestors",
                    ),
                ));
            }
            if modules.iter().all(|candidate| {
                candidate
                    .path
                    .iter()
                    .map(|segment| segment.as_str())
                    .ne(resolved.iter().map(String::as_str))
            }) {
                return Err(frontend_error(
                    FrontendErrorCode::Visibility,
                    Diagnostic::at(
                        "NAME002",
                        path.span,
                        "`pub(in path)` names an unknown module",
                    ),
                ));
            }
            Ok(Visibility::AncestorModule { path: resolved })
        }
    }
}

fn resolve_workspace_names(
    packages: &mut [PackageDraft<'_>],
    graph: &ResolvedGraph,
) -> Result<(), FrontendError> {
    let mut pending = graph
        .packages
        .iter()
        .map(|package| package.id)
        .collect::<BTreeSet<_>>();
    while !pending.is_empty() {
        let ready = pending.iter().copied().find(|node| {
            graph
                .dependencies
                .iter()
                .filter(|dependency| {
                    dependency.from == *node && dependency.kind == LockDependencyKind::Normal
                })
                .all(|dependency| !pending.contains(&dependency.to))
        });
        let node = ready.ok_or_else(|| {
            frontend_path_error(
                FrontendErrorCode::Target,
                "TARGET014",
                "workspace dependency graph is cyclic during C1 name resolution",
            )
        })?;
        let package_index = packages
            .iter()
            .position(|package| package.resolved.id == node)
            .expect("validated graph package has a draft");
        for target_index in 0..packages[package_index].targets.len() {
            resolve_target_imports(packages, graph, package_index, target_index)?;
            link_target_contract(packages, package_index, target_index)?;
        }
        pending.remove(&node);
    }
    Ok(())
}

fn resolve_target_imports(
    packages: &mut [PackageDraft<'_>],
    graph: &ResolvedGraph,
    package_index: usize,
    target_index: usize,
) -> Result<(), FrontendError> {
    let mut pending =
        std::mem::take(&mut packages[package_index].targets[target_index].pending_imports);
    while !pending.is_empty() {
        let snapshot = packages.to_vec();
        let mut next = Vec::new();
        let mut additions = Vec::new();
        for pending_import in pending {
            let lookup = resolve_binding_path(
                &snapshot,
                graph,
                package_index,
                target_index,
                pending_import.module,
                &pending_import.import.path,
                None,
            )?;
            if lookup.is_empty() {
                next.push(pending_import);
                continue;
            }
            let name = import_name(&pending_import.import)?;
            let module_path = snapshot[package_index].targets[target_index]
                .target
                .modules
                .iter()
                .find(|module| module.id == pending_import.module)
                .expect("pending import module exists")
                .path
                .clone();
            let visibility = lexical_visibility(
                &pending_import.import.visibility,
                &module_path,
                &snapshot[package_index].targets[target_index].target.modules,
                pending_import.module,
            )?;
            for resolved in lookup {
                if !visibility_is_subset(
                    &snapshot,
                    &visibility,
                    pending_import.module,
                    &resolved.binding.declared_visibility,
                    resolved.declaring_module,
                ) {
                    return Err(frontend_error(
                        FrontendErrorCode::Visibility,
                        Diagnostic::at(
                            "VISIBILITY004",
                            pending_import.import.span,
                            "an import visibility cannot widen the declaration it exposes",
                        ),
                    ));
                }
                additions.push((
                    pending_import.module,
                    HirBinding {
                        name: name.clone(),
                        namespace: resolved.binding.namespace,
                        target: resolved.binding.target.clone(),
                        declared_visibility: visibility.clone(),
                        origin: HirBindingOrigin::ReExport {
                            source_module: resolved.declaring_module,
                            source_segments: resolved.source_segments,
                            target: resolved.binding.target.clone(),
                        },
                        span: pending_import.import.span,
                    },
                ));
            }
        }
        if additions.is_empty() {
            let first = &next[0];
            return Err(frontend_error(
                FrontendErrorCode::Name,
                Diagnostic::at(
                    "NAME002",
                    first.import.path.span,
                    "unresolved import in C1 workspace name resolution",
                ),
            ));
        }
        for (module, binding) in additions {
            let target = &mut packages[package_index].targets[target_index].target;
            let module = target
                .modules
                .iter_mut()
                .find(|candidate| candidate.id == module)
                .expect("resolved import module exists");
            push_binding(module, binding)?;
        }
        pending = next;
    }
    Ok(())
}

#[derive(Clone)]
struct BindingLookup {
    declaring_module: HirModuleId,
    source_segments: Vec<String>,
    binding: HirBinding,
}

fn resolve_binding_path(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    package_index: usize,
    target_index: usize,
    from: HirModuleId,
    path: &crate::ast::AstPath,
    namespace: Option<Namespace>,
) -> Result<Vec<BindingLookup>, FrontendError> {
    #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
    enum Scope {
        Module(HirModuleId),
        Nominal(HirItemId),
    }

    let mut remaining = path
        .segments
        .iter()
        .map(|segment| segment.name.as_str().to_owned())
        .collect::<Vec<_>>();
    let mut current = match &path.root {
        crate::ast::AstPathRoot::Package => {
            Scope::Module(root_module(packages, package_index, target_index))
        }
        crate::ast::AstPathRoot::SelfValue => Scope::Module(from),
        crate::ast::AstPathRoot::Super(count) => {
            let mut module = from;
            for _ in 0..*count {
                module = find_module(packages, module)
                    .and_then(|module| module.parent)
                    .ok_or_else(|| {
                        frontend_error(
                            FrontendErrorCode::Name,
                            Diagnostic::at(
                                "NAME002",
                                path.span,
                                "path uses `super` above the target root",
                            ),
                        )
                    })?;
            }
            Scope::Module(module)
        }
        crate::ast::AstPathRoot::Identifier(name) => {
            if let Some(dependency) = normal_dependency(graph, from.package(), name.as_str()) {
                let dependency_index = packages
                    .iter()
                    .position(|package| package.resolved.id == dependency)
                    .expect("validated dependency has a package draft");
                let module = packages[dependency_index]
                    .targets
                    .iter()
                    .find(|target| target.target.target == TargetRoot::Library)
                    .and_then(|target| target.target.modules.first())
                    .map(|module| module.id)
                    .ok_or_else(|| {
                        frontend_error(
                            FrontendErrorCode::Name,
                            Diagnostic::at(
                                "NAME008",
                                path.span,
                                format!("dependency alias `{name}` has no checked library target"),
                            ),
                        )
                    })?;
                Scope::Module(module)
            } else {
                remaining.insert(0, name.as_str().to_owned());
                Scope::Module(from)
            }
        }
        crate::ast::AstPathRoot::Bare => Scope::Module(from),
        crate::ast::AstPathRoot::SelfType => return Ok(Vec::new()),
    };

    if remaining.is_empty() {
        let Scope::Module(module) = current else {
            return Ok(Vec::new());
        };
        if namespace.is_some_and(|namespace| namespace != Namespace::Module) {
            return Ok(Vec::new());
        }
        return Ok(vec![BindingLookup {
            declaring_module: module,
            source_segments: vec![canonical_path(path)],
            binding: HirBinding {
                name: canonical_path(path),
                namespace: Namespace::Module,
                target: HirBindingTarget::Module(module),
                declared_visibility: Visibility::Public,
                origin: HirBindingOrigin::Declaration,
                span: path.span,
            },
        }]);
    }
    for (index, segment) in remaining.iter().enumerate() {
        let terminal = index + 1 == remaining.len();
        let mut matches = match current {
            Scope::Module(module_id) => {
                let module = find_module(packages, module_id).expect("resolved HIR module exists");
                module
                    .bindings
                    .iter()
                    .filter(|binding| binding.name == *segment)
                    .filter(|binding| {
                        namespace.is_none_or(|expected| !terminal || binding.namespace == expected)
                    })
                    .cloned()
                    .map(|binding| BindingLookup {
                        declaring_module: module_id,
                        source_segments: vec![binding.name.clone()],
                        binding,
                    })
                    .collect::<Vec<_>>()
            }
            Scope::Nominal(owner) => nominal_scope_bindings(packages, owner, segment, namespace)?,
        };
        matches.retain(|resolved| {
            if resolved.declaring_module.package() != from.package() {
                resolved.binding.declared_visibility == Visibility::Public
            } else {
                visibility_allows(
                    packages,
                    &resolved.binding.declared_visibility,
                    from,
                    resolved.declaring_module,
                )
            }
        });
        if terminal {
            return Ok(matches);
        }
        let mut scopes = matches
            .into_iter()
            .filter_map(|resolved| match resolved.binding.target {
                HirBindingTarget::Module(module) => Some(Scope::Module(module)),
                HirBindingTarget::Item(HirItemRes::Definition(item))
                    if nominal_scope_owner(packages, item) =>
                {
                    Some(Scope::Nominal(item))
                }
                HirBindingTarget::Item(
                    HirItemRes::Definition(_)
                    | HirItemRes::NominalConstructor { .. }
                    | HirItemRes::EnumVariant { .. },
                ) => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter();
        let Some(next) = scopes.next() else {
            return Ok(Vec::new());
        };
        if scopes.next().is_some() {
            return Err(frontend_error(
                FrontendErrorCode::Name,
                Diagnostic::at(
                    "NAME002",
                    path.span,
                    format!(
                        "path segment `{segment}` has more than one viable module/type partition"
                    ),
                ),
            ));
        }
        current = next;
    }
    Ok(Vec::new())
}

fn nominal_scope_owner(packages: &[PackageDraft<'_>], item: HirItemId) -> bool {
    find_item(packages, item).is_some_and(|item| item.kind == DeclarationKind::Enum)
}

fn enum_variant<'a>(
    packages: &'a [PackageDraft<'_>],
    owner: HirItemId,
    ordinal: u64,
) -> Option<&'a crate::ast::AstVariant> {
    let item = find_item(packages, owner)?;
    let HirItemSource::Declaration(declaration) = &item.source else {
        return None;
    };
    let AstDeclarationKind::Enum(enumeration) = &declaration.kind else {
        return None;
    };
    enumeration.variants.get(usize::try_from(ordinal).ok()?)
}

fn nominal_scope_bindings(
    packages: &[PackageDraft<'_>],
    owner: HirItemId,
    name: &str,
    namespace: Option<Namespace>,
) -> Result<Vec<BindingLookup>, FrontendError> {
    if namespace.is_some_and(|namespace| namespace != Namespace::Value) {
        return Ok(Vec::new());
    }
    let item = find_item(packages, owner).ok_or_else(|| {
        frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            "nominal path scope owner is missing from the C1 item arena",
        )
    })?;
    let HirItemSource::Declaration(declaration) = &item.source else {
        return Ok(Vec::new());
    };
    let AstDeclarationKind::Enum(enumeration) = &declaration.kind else {
        return Ok(Vec::new());
    };
    let mut output = Vec::new();
    for (index, variant) in enumeration.variants.iter().enumerate() {
        if variant.name.as_str() != name {
            continue;
        }
        let ordinal = checked_u64(index, "enum variant count")?;
        let declared_visibility = item
            .member_visibilities
            .iter()
            .find_map(|visibility| match visibility.path {
                MemberVisibilityPath::Variant { ordinal: candidate } if candidate == ordinal => {
                    Some(visibility.declared_visibility.clone())
                }
                MemberVisibilityPath::Field { .. }
                | MemberVisibilityPath::Variant { .. }
                | MemberVisibilityPath::VariantField { .. }
                | MemberVisibilityPath::Method { .. } => None,
            })
            .unwrap_or_else(|| item.declared_visibility.clone());
        output.push(BindingLookup {
            declaring_module: item.module,
            source_segments: vec![
                item.name.clone().ok_or_else(|| {
                    frontend_path_error(
                        FrontendErrorCode::Target,
                        "IDENTITY001",
                        "nominal path scope owner has no declaration name",
                    )
                })?,
                name.to_owned(),
            ],
            binding: HirBinding {
                name: name.to_owned(),
                namespace: Namespace::Value,
                target: HirBindingTarget::Item(HirItemRes::EnumVariant { owner, ordinal }),
                declared_visibility,
                origin: HirBindingOrigin::Declaration,
                span: variant.span,
            },
        });
    }
    Ok(output)
}

fn root_module(
    packages: &[PackageDraft<'_>],
    package_index: usize,
    target_index: usize,
) -> HirModuleId {
    packages[package_index].targets[target_index].target.modules[0].id
}

fn find_module<'a>(
    packages: &'a [PackageDraft<'_>],
    id: HirModuleId,
) -> Option<&'a ResolvedSymbolicModule> {
    let package = packages
        .iter()
        .find(|package| package.resolved.id == id.package())?;
    let target = package
        .targets
        .iter()
        .find(|target| target.target.id == id.target())?;
    target.target.modules.iter().find(|module| module.id == id)
}

fn normal_dependency(
    graph: &ResolvedGraph,
    from: PackageNodeId,
    alias: &str,
) -> Option<PackageNodeId> {
    graph
        .dependencies
        .iter()
        .find(|dependency| {
            dependency.from == from
                && dependency.kind == LockDependencyKind::Normal
                && dependency.alias.as_str() == alias
        })
        .map(|dependency| dependency.to)
}

fn visibility_allows(
    packages: &[PackageDraft<'_>],
    visibility: &Visibility,
    from: HirModuleId,
    declaring: HirModuleId,
) -> bool {
    match visibility {
        Visibility::Public => true,
        Visibility::Package => from.package() == declaring.package(),
        Visibility::DeclaringModule => is_module_descendant(packages, from, declaring),
        Visibility::AncestorModule { path } => {
            if from.package() != declaring.package() || from.target() != declaring.target() {
                return false;
            }
            let package = packages
                .iter()
                .find(|package| package.resolved.id == declaring.package())
                .expect("declaring package exists");
            let target = package
                .targets
                .iter()
                .find(|target| target.target.id == declaring.target())
                .expect("declaring target exists");
            let Some(boundary) = target.target.modules.iter().find(|module| {
                module
                    .path
                    .iter()
                    .map(Symbol::as_str)
                    .eq(path.iter().map(String::as_str))
            }) else {
                return false;
            };
            is_module_descendant(packages, from, boundary.id)
        }
    }
}

fn is_module_descendant(
    packages: &[PackageDraft<'_>],
    mut module: HirModuleId,
    boundary: HirModuleId,
) -> bool {
    loop {
        if module == boundary {
            return true;
        }
        let Some(parent) = find_module(packages, module).and_then(|module| module.parent) else {
            return false;
        };
        module = parent;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VisibilityAudience {
    Public,
    Package(PackageNodeId),
    Module(HirModuleId),
}

fn visibility_audience(
    packages: &[PackageDraft<'_>],
    visibility: &Visibility,
    declaring: HirModuleId,
) -> Option<VisibilityAudience> {
    match visibility {
        Visibility::Public => Some(VisibilityAudience::Public),
        Visibility::Package => Some(VisibilityAudience::Package(declaring.package())),
        Visibility::DeclaringModule => Some(VisibilityAudience::Module(declaring)),
        Visibility::AncestorModule { path } => {
            let package = packages
                .iter()
                .find(|package| package.resolved.id == declaring.package())?;
            let target = package
                .targets
                .iter()
                .find(|target| target.target.id == declaring.target())?;
            target
                .target
                .modules
                .iter()
                .find(|module| {
                    module
                        .path
                        .iter()
                        .map(Symbol::as_str)
                        .eq(path.iter().map(String::as_str))
                })
                .map(|module| VisibilityAudience::Module(module.id))
        }
    }
}

fn visibility_is_subset(
    packages: &[PackageDraft<'_>],
    requested: &Visibility,
    requested_declaring: HirModuleId,
    source: &Visibility,
    source_declaring: HirModuleId,
) -> bool {
    let Some(requested) = visibility_audience(packages, requested, requested_declaring) else {
        return false;
    };
    let Some(source) = visibility_audience(packages, source, source_declaring) else {
        return false;
    };
    match (requested, source) {
        (_, VisibilityAudience::Public) => true,
        (VisibilityAudience::Public, _) => false,
        (VisibilityAudience::Package(requested), VisibilityAudience::Package(source)) => {
            requested == source
        }
        (VisibilityAudience::Package(_), VisibilityAudience::Module(_)) => false,
        (VisibilityAudience::Module(requested), VisibilityAudience::Package(source)) => {
            requested.package() == source
        }
        (VisibilityAudience::Module(requested), VisibilityAudience::Module(source)) => {
            is_module_descendant(packages, requested, source)
        }
    }
}

fn import_name(import: &AstImport) -> Result<String, FrontendError> {
    import
        .path
        .segments
        .last()
        .map(|segment| segment.name.as_str().to_owned())
        .or_else(|| match &import.path.root {
            crate::ast::AstPathRoot::Identifier(name) => Some(name.as_str().to_owned()),
            _ => None,
        })
        .ok_or_else(|| {
            frontend_error(
                FrontendErrorCode::Name,
                Diagnostic::at(
                    "NAME002",
                    import.path.span,
                    "import path has no binding name",
                ),
            )
        })
}

fn link_target_contract(
    packages: &mut [PackageDraft<'_>],
    package_index: usize,
    target_index: usize,
) -> Result<(), FrontendError> {
    let snapshot = packages.to_vec();
    let draft = &snapshot[package_index].targets[target_index];
    let contract = match &draft.manifest_target {
        Target::Library(_) => {
            if let Some(world) = draft
                .target
                .items
                .iter()
                .find(|item| item.kind == DeclarationKind::World)
            {
                return Err(frontend_error(
                    FrontendErrorCode::Target,
                    Diagnostic::at(
                        "TARGET001",
                        world.span,
                        "library targets cannot define a root world",
                    ),
                ));
            }
            if let Some(main) = root_named_item(&snapshot, package_index, target_index, "main") {
                return Err(frontend_error(
                    FrontendErrorCode::Target,
                    Diagnostic::at(
                        "TARGET002",
                        main.span,
                        "library targets cannot define process `main`",
                    ),
                ));
            }
            ResolvedTargetContract::Library
        }
        Target::Binary(binary) => {
            let root_world = resolve_manifest_item(
                &snapshot,
                package_index,
                target_index,
                &binary.world.canonical(),
                DeclarationKind::World,
                "root world",
            )?;
            let main = root_named_item(&snapshot, package_index, target_index, "main")
                .filter(|item| item.kind == DeclarationKind::Function)
                .ok_or_else(|| {
                    frontend_path_error(
                        FrontendErrorCode::Target,
                        "TARGET003",
                        "binary target requires exactly one root-exported `pub fn main`",
                    )
                })?;
            let main_binding = draft.target.modules[0]
                .bindings
                .iter()
                .find(|binding| {
                    binding.name == "main"
                        && binding.namespace == Namespace::Value
                        && binding.target == HirBindingTarget::Item(HirItemRes::Definition(main.id))
                })
                .expect("root named item has a root binding");
            if main_binding.declared_visibility != Visibility::Public {
                return Err(frontend_error(
                    FrontendErrorCode::Target,
                    Diagnostic::at(
                        "TARGET003",
                        main_binding.span,
                        "binary entrypoint must be exported from the target root as `pub fn main`",
                    ),
                ));
            }
            let mut capabilities = binary
                .capabilities
                .iter()
                .copied()
                .map(manifest_capability)
                .collect::<Vec<_>>();
            capabilities.sort_by_key(|capability| capability_key(*capability));
            ResolvedTargetContract::Binary {
                root_world,
                main: main.id,
                capabilities,
            }
        }
        Target::Environment(environment) => {
            if let Some(main) = snapshot[package_index].targets[target_index]
                .target
                .items
                .iter()
                .find(|item| item.name.as_deref() == Some("main"))
            {
                return Err(frontend_error(
                    FrontendErrorCode::Target,
                    Diagnostic::at(
                        "TARGET007",
                        main.span,
                        "environment targets forbid a source `main` declaration",
                    ),
                ));
            }
            let profile = snapshot[package_index]
                .source
                .manifest()
                .environment_profiles
                .get(&environment.profile)
                .ok_or_else(|| {
                    frontend_path_error(
                        FrontendErrorCode::Target,
                        "TARGET006",
                        format!(
                            "environment target `{}` names missing profile `{}`",
                            environment.name, environment.profile
                        ),
                    )
                })?;
            ResolvedTargetContract::Environment {
                root_world: resolve_manifest_item(
                    &snapshot,
                    package_index,
                    target_index,
                    &environment.world.canonical(),
                    DeclarationKind::World,
                    "root world",
                )?,
                profile: environment.profile.as_str().to_owned(),
                reset: resolve_manifest_item(
                    &snapshot,
                    package_index,
                    target_index,
                    &profile.reset.canonical(),
                    DeclarationKind::Schedule,
                    "reset schedule",
                )?,
                step: resolve_manifest_item(
                    &snapshot,
                    package_index,
                    target_index,
                    &profile.step.canonical(),
                    DeclarationKind::Schedule,
                    "step schedule",
                )?,
                self_play: resolve_manifest_item(
                    &snapshot,
                    package_index,
                    target_index,
                    &profile.self_play.canonical(),
                    DeclarationKind::Schedule,
                    "self-play schedule",
                )?,
            }
        }
    };
    packages[package_index].targets[target_index]
        .target
        .contract = contract;
    Ok(())
}

fn root_named_item<'a>(
    packages: &'a [PackageDraft<'_>],
    package_index: usize,
    target_index: usize,
    name: &str,
) -> Option<&'a ResolvedSymbolicItem> {
    let binding = packages[package_index].targets[target_index].target.modules[0]
        .bindings
        .iter()
        .find(|binding| binding.name == name && binding.namespace == Namespace::Value)?;
    let HirBindingTarget::Item(HirItemRes::Definition(item)) = binding.target else {
        return None;
    };
    find_item(packages, item)
}

fn resolve_manifest_item(
    packages: &[PackageDraft<'_>],
    package_index: usize,
    target_index: usize,
    canonical: &str,
    expected: DeclarationKind,
    label: &str,
) -> Result<HirItemId, FrontendError> {
    let Some(path) = canonical.strip_prefix("package::") else {
        return Err(frontend_path_error(
            FrontendErrorCode::Target,
            "TARGET004",
            format!("{label} must be a package-rooted item path"),
        ));
    };
    let segments = path.split("::").collect::<Vec<_>>();
    let mut module = root_module(packages, package_index, target_index);
    for (index, segment) in segments.iter().enumerate() {
        let terminal = index + 1 == segments.len();
        let current = find_module(packages, module).expect("manifest path module exists");
        if terminal {
            let matches = current
                .bindings
                .iter()
                .filter(|binding| binding.name == *segment)
                .filter_map(|binding| match binding.target {
                    HirBindingTarget::Item(HirItemRes::Definition(item)) => {
                        find_item(packages, item)
                    }
                    HirBindingTarget::Item(
                        HirItemRes::NominalConstructor { .. } | HirItemRes::EnumVariant { .. },
                    ) => None,
                    HirBindingTarget::Module(_) => None,
                })
                .filter(|item| item.kind == expected)
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                return Err(frontend_path_error(
                    FrontendErrorCode::Target,
                    "TARGET004",
                    format!(
                        "{label} `{canonical}` resolves to {} matching declarations",
                        matches.len()
                    ),
                ));
            }
            return Ok(matches[0].id);
        }
        module = *current
            .bindings
            .iter()
            .find_map(|binding| (binding.name == *segment).then_some(&binding.target))
            .and_then(|target| match target {
                HirBindingTarget::Module(module) => Some(module),
                HirBindingTarget::Item(_) => None,
            })
            .ok_or_else(|| {
                frontend_path_error(
                    FrontendErrorCode::Target,
                    "TARGET004",
                    format!("{label} `{canonical}` traverses an unknown module `{segment}`"),
                )
            })?;
    }
    Err(frontend_path_error(
        FrontendErrorCode::Target,
        "TARGET004",
        format!("{label} path is empty"),
    ))
}

fn find_item<'a>(
    packages: &'a [PackageDraft<'_>],
    id: HirItemId,
) -> Option<&'a ResolvedSymbolicItem> {
    packages
        .iter()
        .flat_map(|package| &package.targets)
        .flat_map(|target| &target.target.items)
        .find(|item| item.id == id)
}

fn manifest_capability(capability: Capability) -> ManifestCapability {
    match capability {
        Capability::Args => ManifestCapability::Args,
        Capability::Environment => ManifestCapability::Environment,
        Capability::Stdio => ManifestCapability::Stdio,
        Capability::Files => ManifestCapability::Files,
        Capability::Subprocess => ManifestCapability::Subprocess,
        Capability::WallClock => ManifestCapability::WallClock,
        Capability::MonotonicClock => ManifestCapability::MonotonicClock,
        Capability::Tcp => ManifestCapability::Tcp,
        Capability::Udp => ManifestCapability::Udp,
        Capability::Threads => ManifestCapability::Threads,
        Capability::Atomics => ManifestCapability::Atomics,
        Capability::Synchronization => ManifestCapability::Synchronization,
    }
}

fn capability_key(capability: ManifestCapability) -> &'static str {
    match capability {
        ManifestCapability::Args => "args",
        ManifestCapability::Atomics => "atomics",
        ManifestCapability::Environment => "environment",
        ManifestCapability::Files => "files",
        ManifestCapability::MonotonicClock => "monotonic-clock",
        ManifestCapability::Stdio => "stdio",
        ManifestCapability::Subprocess => "subprocess",
        ManifestCapability::Synchronization => "synchronization",
        ManifestCapability::Tcp => "tcp",
        ManifestCapability::Threads => "threads",
        ManifestCapability::Udp => "udp",
        ManifestCapability::WallClock => "wall-clock",
    }
}

fn finish_symbolic_hir(
    packages: &mut [PackageDraft<'_>],
    graph: &ResolvedGraph,
    embedded_core: &VerifiedEmbeddedCoreAuthority,
) -> Result<(), FrontendError> {
    let snapshot = packages.to_vec();
    let mut resolved_shapes = BTreeMap::new();
    let mut body_shapes = BTreeMap::new();
    for package_index in 0..snapshot.len() {
        for target_index in 0..snapshot[package_index].targets.len() {
            for item in &snapshot[package_index].targets[target_index].target.items {
                let (parameters, generics) = generic_environment(&snapshot, item)?;
                let body_context = SymbolicLoweringContext {
                    packages: &snapshot,
                    graph,
                    item: item.id,
                    location: PathLocation {
                        package_index,
                        target_index,
                        module: item.module,
                        context_item: Some(item.id),
                    },
                    generics: &generics,
                    embedded_core,
                    lifetime_domain: LifetimeDomain::BodyLocal,
                    contextual_self_template: false,
                };
                let elision_plan = declaration_elision_plan(&body_context, item)?;
                let context = SymbolicLoweringContext {
                    lifetime_domain: LifetimeDomain::Declaration(&elision_plan),
                    ..body_context
                };
                let symbolic_shape = resolve_symbolic_inputs(
                    &context,
                    parameters,
                    elision_plan.hidden_binders.clone(),
                    &item.symbolic_inputs,
                )?;
                assert_declaration_shape_has_no_erased_local(item, &symbolic_shape)?;
                let body_symbolic_shape = resolve_symbolic_inputs(
                    &body_context,
                    Vec::new(),
                    Vec::new(),
                    &item.body_symbolic_inputs,
                )?;
                resolved_shapes.insert(item.id, symbolic_shape);
                body_shapes.insert(item.id, body_symbolic_shape);
            }
        }
    }

    // Child callables and ordinary declarations are lowered first. Parent
    // trait/impl entries then reuse these exact typed child shapes rather than
    // re-deriving a parallel signature.
    let mut definition_shapes = BTreeMap::new();
    for item in snapshot
        .iter()
        .flat_map(|package| &package.targets)
        .flat_map(|target| &target.target.items)
    {
        let parent = matches!(
            &item.source,
            HirItemSource::Declaration(declaration)
                if matches!(&declaration.kind, AstDeclarationKind::Trait(_))
        ) || matches!(&item.source, HirItemSource::Impl(_));
        if !parent {
            definition_shapes.insert(
                item.id,
                resolve_item_declaration_skeleton(
                    &snapshot,
                    graph,
                    embedded_core,
                    item,
                    resolved_shapes
                        .get(&item.id)
                        .expect("resolved declaration inputs exist"),
                    &definition_shapes,
                )?,
            );
        }
    }
    for item in snapshot
        .iter()
        .flat_map(|package| &package.targets)
        .flat_map(|target| &target.target.items)
    {
        let parent = matches!(
            &item.source,
            HirItemSource::Declaration(declaration)
                if matches!(&declaration.kind, AstDeclarationKind::Trait(_))
        ) || matches!(&item.source, HirItemSource::Impl(_));
        if parent {
            definition_shapes.insert(
                item.id,
                resolve_item_declaration_skeleton(
                    &snapshot,
                    graph,
                    embedded_core,
                    item,
                    resolved_shapes
                        .get(&item.id)
                        .expect("resolved declaration inputs exist"),
                    &definition_shapes,
                )?,
            );
        }
    }

    let mut owner_shapes = BTreeMap::new();
    for item in snapshot
        .iter()
        .flat_map(|package| &package.targets)
        .flat_map(|target| &target.target.items)
    {
        owner_shapes.insert(
            item.id,
            resolve_item_owner_skeleton(
                &snapshot,
                graph,
                embedded_core,
                item,
                definition_shapes
                    .get(&item.id)
                    .expect("typed declaration shape exists"),
            )?,
        );
    }

    for package in packages.iter_mut() {
        for target in &mut package.targets {
            for item in &mut target.target.items {
                item.symbolic_shape = resolved_shapes
                    .remove(&item.id)
                    .expect("resolved declaration inputs are assigned once");
                item.body_symbolic_shape = body_shapes
                    .remove(&item.id)
                    .expect("resolved body inputs are assigned once");
                item.definition_shape = definition_shapes
                    .remove(&item.id)
                    .expect("typed declaration shape is assigned once");
                item.owner_shape = owner_shapes
                    .remove(&item.id)
                    .expect("typed owner shape is assigned once");
            }
        }
    }

    drop(snapshot);
    let resolution_snapshot = packages.to_vec();
    for (package_index, package) in packages.iter_mut().enumerate() {
        for (target_index, target) in package.targets.iter_mut().enumerate() {
            let mut resolutions = Vec::new();
            collect_target_path_resolutions(
                &resolution_snapshot,
                graph,
                package_index,
                target_index,
                embedded_core,
                &mut resolutions,
            )?;
            resolutions.sort_by_key(|resolution| {
                (
                    resolution.span.file.0,
                    resolution.span.start.byte,
                    resolution.span.end.byte,
                )
            });
            if let Some(duplicate) = resolutions
                .windows(2)
                .find(|pair| pair[0].span == pair[1].span)
            {
                return Err(c1_resolution_identity_error(
                    duplicate[1].span,
                    "two C1 path-resolution rows claim the same retained source span",
                ));
            }
            target.target.path_resolutions = resolutions;
        }
    }
    Ok(())
}

fn validate_final_c1_resolutions(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    embedded_core: &VerifiedEmbeddedCoreAuthority,
) -> Result<(), FrontendError> {
    for target in packages
        .iter()
        .flat_map(|package| &package.targets)
        .map(|target| &target.target)
    {
        for resolution in &target.path_resolutions {
            validate_final_path_resolution(packages, graph, target, resolution, embedded_core)?;
        }
        for item in &target.items {
            validate_item_pending_authority(packages, target, item)?;
        }
    }
    Ok(())
}

fn validate_final_path_resolution(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    target: &ResolvedSymbolicTargetHir,
    resolution: &PathResolution,
    embedded_core: &VerifiedEmbeddedCoreAuthority,
) -> Result<(), FrontendError> {
    match &resolution.unresolved {
        None => {
            if resolution.associated.is_some() || resolution.resolutions.is_empty() {
                return Err(c1_resolution_identity_error(
                    resolution.span,
                    "resolved C1 path row has an invalid result/associated shape",
                ));
            }
            if resolution.resolutions.len() > 1 && !is_import_path_span(target, resolution.span) {
                return Err(c1_resolution_name_error(
                    resolution.span,
                    "path has more than one viable namespace partition",
                ));
            }
            Ok(())
        }
        Some(UnresolvedPathKind::AssociatedItemPendingC2) => {
            let Some(associated) = &resolution.associated else {
                return Err(c1_resolution_identity_error(
                    resolution.span,
                    "associated C2 pending row has no typed owner/candidate authority",
                ));
            };
            if !resolution.resolutions.is_empty()
                || associated.path_span != resolution.span
                || associated.candidates.is_empty()
                || associated
                    .candidates
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
            {
                return Err(c1_resolution_identity_error(
                    resolution.span,
                    "associated C2 pending row is not a canonical nonempty candidate set",
                ));
            }
            let uses = target
                .items
                .iter()
                .flat_map(|item| {
                    item.path_uses
                        .iter()
                        .filter(move |path_use| path_use.path.span == resolution.span)
                        .map(move |path_use| (item, path_use))
                })
                .collect::<Vec<_>>();
            let [(context, path_use)] = uses.as_slice() else {
                return Err(c1_resolution_identity_error(
                    resolution.span,
                    "associated C2 pending row does not join to exactly one retained HIR path use",
                ));
            };
            let Some(member) = path_use.path.segments.last() else {
                return Err(c1_resolution_identity_error(
                    resolution.span,
                    "associated C2 pending path has no member segment",
                ));
            };
            if associated.member != member.name.as_str()
                || associated.member_span != member.span
                || !associated_owner_is_valid(packages, context, associated.owner)
                || associated.candidates.iter().any(|candidate| {
                    !associated_candidate_is_valid(
                        packages,
                        embedded_core,
                        associated.member.as_str(),
                        *candidate,
                    )
                })
            {
                return Err(c1_resolution_identity_error(
                    resolution.span,
                    "associated C2 pending owner/member/candidate identity is invalid",
                ));
            }
            let package_index = packages
                .iter()
                .position(|package| package.resolved.id == context.module.package())
                .expect("associated context package exists");
            let target_index = packages[package_index]
                .targets
                .iter()
                .position(|candidate| candidate.target.id == context.module.target())
                .expect("associated context target exists");
            let (_, generics) = generic_environment(packages, context)?;
            let empty_locals = BTreeMap::new();
            let expected = resolve_associated_path(
                packages,
                graph,
                PathLocation {
                    package_index,
                    target_index,
                    module: context.module,
                    context_item: Some(context.id),
                },
                &path_use.path,
                path_use.namespace,
                LexicalPathEnvironment {
                    generics: &generics,
                    locals: &empty_locals,
                },
                embedded_core,
            )?;
            if !matches!(
                expected,
                Some(AssociatedLookup::Pending(expected)) if expected == *associated
            ) {
                return Err(c1_resolution_identity_error(
                    resolution.span,
                    "associated C2 pending row differs from the canonical checked candidate partition",
                ));
            }
            Ok(())
        }
        Some(UnresolvedPathKind::SelfTypePendingC2) => {
            if !resolution.resolutions.is_empty() || resolution.associated.is_some() {
                return Err(c1_resolution_identity_error(
                    resolution.span,
                    "contextual Self pending row carries an incompatible result",
                ));
            }
            let contexts = target
                .items
                .iter()
                .filter(|item| {
                    item.path_uses
                        .iter()
                        .any(|path_use| path_use.path.span == resolution.span)
                })
                .collect::<Vec<_>>();
            if !matches!(contexts.as_slice(), [context] if has_contextual_self_authority(packages, context))
            {
                return Err(c1_resolution_name_error(
                    resolution.span,
                    "`Self` is not available outside a trait or implementation context",
                ));
            }
            Ok(())
        }
        Some(
            UnresolvedPathKind::UnknownName
            | UnresolvedPathKind::AmbiguousNamespace
            | UnresolvedPathKind::GenericFormationPendingC2
            | UnresolvedPathKind::ShadowedLocalNeedsLexicalResolution
            | UnresolvedPathKind::DependencyHasNoLibraryTarget,
        ) => Err(c1_resolution_name_error(
            resolution.span,
            "path has no single checked C1 name-resolution authority",
        )),
    }
}

fn is_import_path_span(target: &ResolvedSymbolicTargetHir, span: Span) -> bool {
    target.modules.iter().any(|module| {
        module
            .ast
            .items
            .iter()
            .any(|item| matches!(item, AstItem::Import(import) if import.path.span == span))
    })
}

fn associated_owner_is_valid(
    packages: &[PackageDraft<'_>],
    context: &ResolvedSymbolicItem,
    owner: AssociatedPathOwner,
) -> bool {
    match owner {
        AssociatedPathOwner::Nominal(item) => find_item(packages, item).is_some(),
        AssociatedPathOwner::Generic(parameter) => {
            matches!(
                generic_parameter_kind(packages, parameter),
                Some(GenericParameterKind::Type)
            ) && generic_owner_is_active(packages, context.id, parameter.owner)
        }
        AssociatedPathOwner::ContextualSelf { context: owner } => {
            owner == context.id && has_contextual_self_authority(packages, context)
        }
    }
}

fn generic_owner_is_active(
    packages: &[PackageDraft<'_>],
    mut context: HirItemId,
    owner: HirItemId,
) -> bool {
    loop {
        if context == owner {
            return true;
        }
        let Some(parent) = find_item(packages, context).and_then(|item| item.owner) else {
            return false;
        };
        context = parent;
    }
}

fn has_contextual_self_authority(
    packages: &[PackageDraft<'_>],
    context: &ResolvedSymbolicItem,
) -> bool {
    match &context.source {
        HirItemSource::TraitMethod(_) | HirItemSource::ImplMethod(_) => {
            context.owner.is_some_and(|owner| {
                find_item(packages, owner).is_some_and(|owner| {
                    owner.kind == DeclarationKind::Trait || owner.kind == DeclarationKind::Impl
                })
            })
        }
        HirItemSource::Declaration(declaration) => {
            matches!(declaration.kind, AstDeclarationKind::Trait(_))
        }
        HirItemSource::Impl(_) => true,
        HirItemSource::QueryParameter { .. } => false,
    }
}

fn associated_candidate_is_valid(
    packages: &[PackageDraft<'_>],
    embedded_core: &VerifiedEmbeddedCoreAuthority,
    member: &str,
    candidate: AssociatedPathCandidate,
) -> bool {
    match candidate {
        AssociatedPathCandidate::Item(HirItemRes::Definition(item)) => find_item(packages, item)
            .is_some_and(|item| {
                item.name.as_deref() == Some(member)
                    && matches!(
                        item.source,
                        HirItemSource::TraitMethod(_) | HirItemSource::ImplMethod(_)
                    )
            }),
        AssociatedPathCandidate::Item(
            HirItemRes::NominalConstructor { .. } | HirItemRes::EnumVariant { .. },
        ) => false,
        AssociatedPathCandidate::Builtin(BuiltinRes {
            target: BuiltinResTarget::Method(method),
        }) => embedded_core
            .method(method)
            .is_some_and(|method| method.source_name() == member),
        AssociatedPathCandidate::Builtin(BuiltinRes {
            target:
                BuiltinResTarget::Prelude(_)
                | BuiltinResTarget::EnumVariant(_)
                | BuiltinResTarget::RecordConstructor(_),
        }) => false,
    }
}

fn c1_resolution_name_error(span: Span, message: &str) -> FrontendError {
    frontend_error(
        FrontendErrorCode::Name,
        Diagnostic::at("NAME002", span, message),
    )
}

fn c1_resolution_identity_error(span: Span, message: &str) -> FrontendError {
    frontend_error(
        FrontendErrorCode::Target,
        Diagnostic::at("IDENTITY001", span, message),
    )
}

fn validate_item_pending_authority(
    packages: &[PackageDraft<'_>],
    target: &ResolvedSymbolicTargetHir,
    item: &ResolvedSymbolicItem,
) -> Result<(), FrontendError> {
    validate_resolved_symbolic_shape(packages, target, item, &item.symbolic_shape)?;
    validate_resolved_symbolic_shape(packages, target, item, &item.body_symbolic_shape)?;
    for argument in item
        .path_uses
        .iter()
        .flat_map(|path| &path.generic_arguments)
        .chain(
            item.postfix_generic_argument_uses
                .iter()
                .flat_map(|arguments| &arguments.arguments),
        )
    {
        match &argument.value {
            ResolvedGenericArgument::Type(value) => {
                validate_resolved_type_pending(packages, target, item, value)?;
            }
            ResolvedGenericArgument::Lifetime(value) => {
                if let ResolvedSymbolicLifetime::Pending {
                    span,
                    reason,
                    canonical,
                } = value
                {
                    validate_pending_reason(
                        packages,
                        target,
                        item,
                        *span,
                        reason,
                        canonical,
                        PendingSymbolicDomain::Lifetime,
                    )?;
                }
            }
            ResolvedGenericArgument::IntegerConst(value) => {
                validate_resolved_const_pending(packages, target, item, value)?;
            }
        }
    }
    validate_declaration_skeleton_pending(packages, target, item, &item.definition_shape)
}

fn validate_resolved_symbolic_shape(
    packages: &[PackageDraft<'_>],
    target: &ResolvedSymbolicTargetHir,
    item: &ResolvedSymbolicItem,
    shape: &ResolvedSymbolicShape,
) -> Result<(), FrontendError> {
    for value in &shape.types {
        validate_resolved_type_pending(packages, target, item, value)?;
    }
    for value in &shape.consts {
        validate_resolved_const_pending(packages, target, item, value)?;
    }
    for value in &shape.effects {
        if let ResolvedSymbolicEffect::Pending {
            span,
            reason,
            canonical,
        } = value
        {
            validate_pending_reason(
                packages,
                target,
                item,
                *span,
                reason,
                canonical,
                PendingSymbolicDomain::Type,
            )?;
        }
    }
    Ok(())
}

fn validate_resolved_type_pending(
    packages: &[PackageDraft<'_>],
    target: &ResolvedSymbolicTargetHir,
    item: &ResolvedSymbolicItem,
    value: &ResolvedSymbolicType,
) -> Result<(), FrontendError> {
    if let ResolvedSymbolicType::Pending {
        span,
        reason,
        canonical,
    } = value
    {
        validate_pending_reason(
            packages,
            target,
            item,
            *span,
            reason,
            canonical,
            PendingSymbolicDomain::Type,
        )?;
    }
    Ok(())
}

fn validate_resolved_const_pending(
    packages: &[PackageDraft<'_>],
    target: &ResolvedSymbolicTargetHir,
    item: &ResolvedSymbolicItem,
    value: &ResolvedSymbolicConst,
) -> Result<(), FrontendError> {
    if let ResolvedSymbolicConst::Pending {
        span,
        reason,
        canonical,
    } = value
    {
        validate_pending_reason(
            packages,
            target,
            item,
            *span,
            reason,
            canonical,
            PendingSymbolicDomain::IntegerConst,
        )?;
    }
    Ok(())
}

fn validate_pending_reason(
    packages: &[PackageDraft<'_>],
    target: &ResolvedSymbolicTargetHir,
    item: &ResolvedSymbolicItem,
    span: Span,
    reason: &UnresolvedPathKind,
    canonical: &str,
    domain: PendingSymbolicDomain,
) -> Result<(), FrontendError> {
    if canonical.is_empty() {
        return Err(c1_resolution_identity_error(
            span,
            "pending symbolic row has no diagnostic spelling",
        ));
    }
    match reason {
        UnresolvedPathKind::GenericFormationPendingC2 => {
            if has_generic_formation_authority(packages, target, item, span, domain) {
                Ok(())
            } else {
                Err(c1_resolution_identity_error(
                    span,
                    "generic-formation pending row has no retained path/formal authority",
                ))
            }
        }
        UnresolvedPathKind::AssociatedItemPendingC2 => {
            if matching_associated_rows(target, item, span) == 1 {
                Ok(())
            } else {
                Err(c1_resolution_identity_error(
                    span,
                    "associated symbolic pending row does not join to one typed path authority",
                ))
            }
        }
        UnresolvedPathKind::SelfTypePendingC2 => {
            if has_contextual_self_authority(packages, item) {
                Ok(())
            } else {
                Err(c1_resolution_name_error(
                    span,
                    "`Self` is not available outside a trait or implementation context",
                ))
            }
        }
        UnresolvedPathKind::UnknownName
        | UnresolvedPathKind::AmbiguousNamespace
        | UnresolvedPathKind::ShadowedLocalNeedsLexicalResolution
        | UnresolvedPathKind::DependencyHasNoLibraryTarget => Err(c1_resolution_name_error(
            span,
            "symbolic row has no single checked C1 resolution authority",
        )),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingSymbolicDomain {
    Type,
    Lifetime,
    IntegerConst,
}

fn matching_associated_rows(
    target: &ResolvedSymbolicTargetHir,
    item: &ResolvedSymbolicItem,
    span: Span,
) -> usize {
    target
        .path_resolutions
        .iter()
        .filter(|resolution| {
            resolution.associated.is_some()
                && span == resolution.span
                && item
                    .path_uses
                    .iter()
                    .any(|path_use| path_use.path.span == resolution.span)
        })
        .count()
}

fn has_generic_formation_authority(
    packages: &[PackageDraft<'_>],
    target: &ResolvedSymbolicTargetHir,
    item: &ResolvedSymbolicItem,
    span: Span,
    domain: PendingSymbolicDomain,
) -> bool {
    let argument_witnesses = item
        .path_uses
        .iter()
        .flat_map(|path_use| &path_use.generic_arguments)
        .chain(
            item.postfix_generic_argument_uses
                .iter()
                .flat_map(|arguments| &arguments.arguments),
        )
        .filter(|argument| argument.span == span)
        .filter(|argument| {
            argument
                .formal_kind
                .as_ref()
                .is_some_and(|formal| !generic_actual_matches_formal(&argument.value, formal))
        })
        .count();
    let path_witnesses = item
        .path_uses
        .iter()
        .filter(|path_use| path_use.path.span == span)
        .filter(|path_use| path_has_generic_formation_mismatch(packages, target, path_use, domain))
        .count();
    argument_witnesses + path_witnesses == 1
}

fn generic_actual_matches_formal(
    actual: &ResolvedGenericArgument,
    formal: &GenericParameterKind,
) -> bool {
    matches!(
        (actual, formal),
        (ResolvedGenericArgument::Type(_), GenericParameterKind::Type)
            | (
                ResolvedGenericArgument::Lifetime(_),
                GenericParameterKind::Lifetime
            )
            | (
                ResolvedGenericArgument::IntegerConst(_),
                GenericParameterKind::IntegerConst(_)
            )
    )
}

fn path_has_generic_formation_mismatch(
    packages: &[PackageDraft<'_>],
    target: &ResolvedSymbolicTargetHir,
    path_use: &HirPathUse,
    domain: PendingSymbolicDomain,
) -> bool {
    let Some(resolution) = target
        .path_resolutions
        .iter()
        .find(|resolution| resolution.span == path_use.path.span)
    else {
        return false;
    };
    let [resolution] = resolution.resolutions.as_slice() else {
        return false;
    };
    match resolution {
        Res::Generic(parameter) => {
            !generic_kind_matches_domain(
                generic_parameter_kind(packages, *parameter).as_ref(),
                domain,
            ) || !path_use.generic_arguments.is_empty()
        }
        Res::Item(HirItemRes::Definition(owner)) => {
            if domain != PendingSymbolicDomain::Type {
                return false;
            }
            let Some(owner) = find_item(packages, *owner) else {
                return false;
            };
            let formals = item_generic_parameters(owner)
                .map_or(&[][..], |parameters| parameters.parameters.as_slice());
            formals.len() != path_use.generic_arguments.len()
                || path_use
                    .generic_arguments
                    .iter()
                    .zip(formals)
                    .any(|(argument, formal)| {
                        !generic_actual_matches_formal(
                            &argument.value,
                            &ast_generic_parameter_kind(formal),
                        )
                    })
        }
        Res::Builtin(BuiltinRes {
            target: BuiltinResTarget::Prelude(VirtualPreludeTarget::SemanticType(_)),
        }) => {
            domain != PendingSymbolicDomain::Type
                || path_use.generic_arguments.is_empty()
                || path_use
                    .generic_arguments
                    .iter()
                    .any(|argument| !matches!(argument.value, ResolvedGenericArgument::Type(_)))
        }
        Res::Module(_)
        | Res::Item(HirItemRes::NominalConstructor { .. } | HirItemRes::EnumVariant { .. })
        | Res::Local(_)
        | Res::Builtin(BuiltinRes {
            target:
                BuiltinResTarget::Prelude(VirtualPreludeTarget::Definition(_))
                | BuiltinResTarget::Method(_)
                | BuiltinResTarget::EnumVariant(_)
                | BuiltinResTarget::RecordConstructor(_),
        }) => false,
    }
}

fn generic_kind_matches_domain(
    kind: Option<&GenericParameterKind>,
    domain: PendingSymbolicDomain,
) -> bool {
    matches!(
        (kind, domain),
        (
            Some(GenericParameterKind::Type),
            PendingSymbolicDomain::Type
        ) | (
            Some(GenericParameterKind::Lifetime),
            PendingSymbolicDomain::Lifetime
        ) | (
            Some(GenericParameterKind::IntegerConst(_)),
            PendingSymbolicDomain::IntegerConst
        )
    )
}

fn validate_declaration_skeleton_pending(
    packages: &[PackageDraft<'_>],
    target: &ResolvedSymbolicTargetHir,
    item: &ResolvedSymbolicItem,
    shape: &SymbolicDeclarationShapeSkeleton,
) -> Result<(), FrontendError> {
    for predicate in &shape.predicates {
        validate_predicate_shape_pending(packages, target, item, predicate)?;
    }
    match &shape.payload {
        SymbolicDeclarationPayloadSkeleton::World | SymbolicDeclarationPayloadSkeleton::Tag => {
            Ok(())
        }
        SymbolicDeclarationPayloadSkeleton::Schedule { effects, .. } => {
            validate_effect_sets_pending(packages, target, item, effects)
        }
        SymbolicDeclarationPayloadSkeleton::Record(record) => {
            for field in &record.fields {
                validate_type_shape_pending(packages, target, item, &field.ty)?;
            }
            Ok(())
        }
        SymbolicDeclarationPayloadSkeleton::Enum(variants) => {
            for field in variants.iter().flat_map(|variant| &variant.fields) {
                validate_type_shape_pending(packages, target, item, &field.ty)?;
            }
            Ok(())
        }
        SymbolicDeclarationPayloadSkeleton::Callable(callable) => {
            validate_callable_shape_pending(packages, target, item, callable)
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
                        validate_type_shape_pending(packages, target, item, ty)?;
                    }
                    SymbolicSystemAccessShapeSkeleton::Query(terms) => {
                        for term in terms {
                            validate_type_shape_pending(packages, target, item, &term.ty)?;
                        }
                    }
                    SymbolicSystemAccessShapeSkeleton::Commands => {}
                }
            }
            for implied in implied_requires {
                validate_type_shape_pending(packages, target, item, &implied.referent)?;
            }
            validate_type_shape_pending(packages, target, item, result)?;
            validate_effect_sets_pending(packages, target, item, effects)
        }
        SymbolicDeclarationPayloadSkeleton::Trait { methods } => {
            validate_owned_method_shape_reuse(target, item, methods)
        }
        SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref,
            target: ty,
            methods,
            ..
        } => {
            if let Some(trait_ref) = trait_ref {
                validate_type_shape_pending(packages, target, item, trait_ref)?;
            }
            validate_type_shape_pending(packages, target, item, ty)?;
            validate_owned_method_shape_reuse(target, item, methods)
        }
        SymbolicDeclarationPayloadSkeleton::Alias { target: ty }
        | SymbolicDeclarationPayloadSkeleton::Const { ty }
        | SymbolicDeclarationPayloadSkeleton::Static { ty, .. } => {
            validate_type_shape_pending(packages, target, item, ty)
        }
        SymbolicDeclarationPayloadSkeleton::Query { terms } => {
            for term in terms {
                validate_type_shape_pending(packages, target, item, &term.ty)?;
            }
            Ok(())
        }
    }
}

fn validate_owned_method_shape_reuse(
    target: &ResolvedSymbolicTargetHir,
    owner: &ResolvedSymbolicItem,
    methods: &[SymbolicMethodShapeSkeleton],
) -> Result<(), FrontendError> {
    let children = target
        .items
        .iter()
        .filter(|item| item.owner == Some(owner.id))
        .collect::<Vec<_>>();
    if children.len() != methods.len()
        || children.iter().zip(methods).any(|(child, method)| {
            child.name.as_deref() != Some(method.name.as_str())
                || child.definition_shape != *method.shape
        })
    {
        return Err(c1_resolution_identity_error(
            owner.span,
            "parent declaration does not reuse its exact checked child callable shapes",
        ));
    }
    Ok(())
}

fn validate_callable_shape_pending(
    packages: &[PackageDraft<'_>],
    target: &ResolvedSymbolicTargetHir,
    item: &ResolvedSymbolicItem,
    callable: &SymbolicCallableShapeSkeleton,
) -> Result<(), FrontendError> {
    for parameter in &callable.parameters {
        validate_type_shape_pending(packages, target, item, &parameter.ty)?;
    }
    validate_type_shape_pending(packages, target, item, &callable.result)?;
    if let Some(resume) = &callable.resume {
        validate_type_shape_pending(packages, target, item, resume)?;
    }
    if let Some(yields) = &callable.yields {
        validate_type_shape_pending(packages, target, item, yields)?;
    }
    validate_effect_sets_pending(packages, target, item, &callable.effects)
}

fn validate_effect_sets_pending(
    packages: &[PackageDraft<'_>],
    target: &ResolvedSymbolicTargetHir,
    item: &ResolvedSymbolicItem,
    effects: &SymbolicEffectSetsSkeleton,
) -> Result<(), FrontendError> {
    for effect in effects.requires.iter().chain(&effects.throws) {
        match effect {
            SymbolicEffectShapeSkeleton::Resolved { .. } => {}
            SymbolicEffectShapeSkeleton::Pending(pending) => {
                validate_skeleton_pending(packages, target, item, pending)?;
            }
        }
    }
    Ok(())
}

fn validate_predicate_shape_pending(
    packages: &[PackageDraft<'_>],
    target: &ResolvedSymbolicTargetHir,
    item: &ResolvedSymbolicItem,
    predicate: &SymbolicPredicateShapeSkeleton,
) -> Result<(), FrontendError> {
    if let SymbolicPredicateShapeSkeleton::Pending(pending) = predicate {
        validate_skeleton_pending(packages, target, item, pending)?;
    }
    Ok(())
}

fn validate_type_shape_pending(
    packages: &[PackageDraft<'_>],
    target: &ResolvedSymbolicTargetHir,
    item: &ResolvedSymbolicItem,
    ty: &SymbolicTypeShapeSkeleton,
) -> Result<(), FrontendError> {
    if let SymbolicTypeShapeSkeleton::Pending(pending) = ty {
        validate_skeleton_pending(packages, target, item, pending)?;
    }
    Ok(())
}

fn validate_skeleton_pending(
    packages: &[PackageDraft<'_>],
    target: &ResolvedSymbolicTargetHir,
    item: &ResolvedSymbolicItem,
    pending: &SymbolicPendingShape,
) -> Result<(), FrontendError> {
    let span = Span {
        file: FileId(pending.source_span.file),
        start: SourcePosition {
            byte: pending.source_span.start_byte,
            line: pending.source_span.start_line,
            column: pending.source_span.start_column,
        },
        end: SourcePosition {
            byte: pending.source_span.end_byte,
            line: pending.source_span.end_line,
            column: pending.source_span.end_column,
        },
    };
    match pending.kind {
        PendingShapeKind::ContextualSelf if has_contextual_self_authority(packages, item) => Ok(()),
        PendingShapeKind::GenericFormation
            if has_generic_formation_authority(
                packages,
                target,
                item,
                span,
                PendingSymbolicDomain::Type,
            ) =>
        {
            Ok(())
        }
        PendingShapeKind::PathUse
        | PendingShapeKind::EffectMember
        | PendingShapeKind::Predicate
            if matching_associated_rows(target, item, span) == 1 =>
        {
            Ok(())
        }
        PendingShapeKind::PathUse
        | PendingShapeKind::ContextualSelf
        | PendingShapeKind::GenericFormation
        | PendingShapeKind::EffectMember
        | PendingShapeKind::Predicate => Err(c1_resolution_name_error(
            span,
            "pending declaration-shape leaf has no typed C1 continuation authority",
        )),
    }
}

#[derive(Clone, Copy)]
struct SymbolicLoweringContext<'a, 'workspace> {
    packages: &'a [PackageDraft<'workspace>],
    graph: &'a ResolvedGraph,
    item: HirItemId,
    location: PathLocation,
    generics: &'a BTreeMap<String, GenericParameterId>,
    embedded_core: &'a VerifiedEmbeddedCoreAuthority,
    lifetime_domain: LifetimeDomain<'a>,
    contextual_self_template: bool,
}

#[derive(Clone, Copy)]
enum LifetimeDomain<'a> {
    Declaration(&'a DeclarationElisionPlan),
    BodyLocal,
}

#[derive(Clone, Debug, Default)]
struct DeclarationElisionPlan {
    by_span: BTreeMap<SpanKey, SymbolicLifetime>,
    hidden_binders: Vec<HiddenLifetimeBinder>,
}

struct DeclarationElisionPlanner {
    plan: DeclarationElisionPlan,
    next_index: u64,
    generator_state: bool,
}

impl DeclarationElisionPlanner {
    fn new(explicit_parameter_count: usize, generator_state: bool) -> Result<Self, FrontendError> {
        Ok(Self {
            plan: DeclarationElisionPlan::default(),
            next_index: checked_u64(
                explicit_parameter_count,
                "explicit generic parameter count before hidden lifetime binders",
            )?,
            generator_state,
        })
    }

    fn allocate(
        &mut self,
        span: Span,
        source: HiddenLifetimeBinderSource,
    ) -> Result<SymbolicLifetime, FrontendError> {
        let index = self.next_index;
        self.next_index = self.next_index.checked_add(1).ok_or_else(|| {
            frontend_error(
                FrontendErrorCode::Target,
                Diagnostic::at(
                    "IDENTITY001",
                    span,
                    "hidden declaration lifetime-binder index exceeds u64",
                ),
            )
        })?;
        let lifetime = SymbolicLifetime::Bound { depth: 0, index };
        if self
            .plan
            .by_span
            .insert(SpanKey::from(span), lifetime.clone())
            .is_some()
        {
            return Err(frontend_error(
                FrontendErrorCode::Target,
                Diagnostic::at(
                    "IDENTITY001",
                    span,
                    "one declaration reference span was assigned two hidden lifetime binders",
                ),
            ));
        }
        self.plan.hidden_binders.push(HiddenLifetimeBinder {
            index,
            span,
            source,
            generator_state: self.generator_state,
        });
        Ok(lifetime)
    }

    fn bind_output(
        &mut self,
        span: Span,
        lifetime: &SymbolicLifetime,
    ) -> Result<(), FrontendError> {
        if let Some(previous) = self
            .plan
            .by_span
            .insert(SpanKey::from(span), lifetime.clone())
        {
            if previous != *lifetime {
                return Err(frontend_error(
                    FrontendErrorCode::Target,
                    Diagnostic::at(
                        "IDENTITY001",
                        span,
                        "one declaration output span acquired inconsistent elided lifetimes",
                    ),
                ));
            }
        }
        Ok(())
    }

    fn receiver(
        &mut self,
        context: &SymbolicLoweringContext<'_, '_>,
        receiver: &crate::ast::AstReceiver,
    ) -> Result<Option<SymbolicLifetime>, FrontendError> {
        match &receiver.kind {
            crate::ast::AstReceiverKind::Value { .. } => Ok(None),
            crate::ast::AstReceiverKind::Reference { lifetime, .. } => match lifetime {
                Some(lifetime) => Ok(Some(resolve_declared_lifetime(context, lifetime)?)),
                None => Ok(Some(
                    self.allocate(receiver.span, HiddenLifetimeBinderSource::Receiver)?,
                )),
            },
        }
    }

    fn input_type(
        &mut self,
        context: &SymbolicLoweringContext<'_, '_>,
        ty: &crate::ast::AstType,
        lifetimes: &mut BTreeSet<SymbolicLifetime>,
    ) -> Result<(), FrontendError> {
        use crate::ast::{AstGenericArgumentKind, AstTypeKind};
        match &ty.kind {
            AstTypeKind::Reference {
                lifetime, pointee, ..
            } => {
                let lifetime = match lifetime {
                    Some(lifetime) => resolve_declared_lifetime(context, lifetime)?,
                    None => self.allocate(ty.span, HiddenLifetimeBinderSource::Input)?,
                };
                lifetimes.insert(lifetime);
                self.input_type(context, pointee, lifetimes)
            }
            AstTypeKind::Tuple(types) => {
                for ty in types {
                    self.input_type(context, ty, lifetimes)?;
                }
                Ok(())
            }
            AstTypeKind::Array { element, .. }
            | AstTypeKind::Slice(element)
            | AstTypeKind::RawPointer {
                pointee: element, ..
            } => self.input_type(context, element, lifetimes),
            AstTypeKind::Path(path) => {
                for argument in path
                    .generic_arguments
                    .iter()
                    .chain(
                        path.segments
                            .iter()
                            .filter_map(|segment| segment.generic_arguments.as_ref()),
                    )
                    .flat_map(|arguments| &arguments.arguments)
                {
                    match &argument.kind {
                        AstGenericArgumentKind::Type(ty) => {
                            self.input_type(context, ty, lifetimes)?;
                        }
                        AstGenericArgumentKind::Lifetime(lifetime) => {
                            lifetimes.insert(resolve_declared_lifetime(context, lifetime)?);
                        }
                        AstGenericArgumentKind::IntegerConst(_) => {}
                    }
                }
                Ok(())
            }
            AstTypeKind::FunctionPointer {
                parameters, result, ..
            } => self.callable_types(context, None, parameters.iter(), result.as_deref()),
            AstTypeKind::Scalar(_)
            | AstTypeKind::Never
            | AstTypeKind::Unit
            | AstTypeKind::Str
            | AstTypeKind::SelfType => Ok(()),
        }
    }

    fn output_type(
        &mut self,
        context: &SymbolicLoweringContext<'_, '_>,
        ty: &crate::ast::AstType,
        selected: Option<&SymbolicLifetime>,
    ) -> Result<(), FrontendError> {
        use crate::ast::{AstGenericArgumentKind, AstTypeKind};
        match &ty.kind {
            AstTypeKind::Reference {
                lifetime, pointee, ..
            } => {
                if lifetime.is_none() {
                    let selected = selected.ok_or_else(|| {
                        frontend_error(
                            FrontendErrorCode::Target,
                            Diagnostic::at(
                                "TYPE001",
                                ty.span,
                                "elided output reference requires one input lifetime or a borrowed receiver",
                            ),
                        )
                    })?;
                    self.bind_output(ty.span, selected)?;
                }
                self.output_type(context, pointee, selected)
            }
            AstTypeKind::Tuple(types) => {
                for ty in types {
                    self.output_type(context, ty, selected)?;
                }
                Ok(())
            }
            AstTypeKind::Array { element, .. }
            | AstTypeKind::Slice(element)
            | AstTypeKind::RawPointer {
                pointee: element, ..
            } => self.output_type(context, element, selected),
            AstTypeKind::Path(path) => {
                for argument in path
                    .generic_arguments
                    .iter()
                    .chain(
                        path.segments
                            .iter()
                            .filter_map(|segment| segment.generic_arguments.as_ref()),
                    )
                    .flat_map(|arguments| &arguments.arguments)
                {
                    if let AstGenericArgumentKind::Type(ty) = &argument.kind {
                        self.output_type(context, ty, selected)?;
                    }
                }
                Ok(())
            }
            AstTypeKind::FunctionPointer {
                parameters, result, ..
            } => self.callable_types(context, None, parameters.iter(), result.as_deref()),
            AstTypeKind::Scalar(_)
            | AstTypeKind::Never
            | AstTypeKind::Unit
            | AstTypeKind::Str
            | AstTypeKind::SelfType => Ok(()),
        }
    }

    fn noncallable_type(
        &mut self,
        context: &SymbolicLoweringContext<'_, '_>,
        ty: &crate::ast::AstType,
    ) -> Result<(), FrontendError> {
        use crate::ast::{AstGenericArgumentKind, AstTypeKind};
        match &ty.kind {
            AstTypeKind::Reference {
                lifetime, pointee, ..
            } => {
                if lifetime.is_none() {
                    return Err(frontend_error(
                        FrontendErrorCode::Target,
                        Diagnostic::at(
                            "TYPE001",
                            ty.span,
                            "an elided reference lifetime is not legal in a non-callable declaration",
                        ),
                    ));
                }
                self.noncallable_type(context, pointee)
            }
            AstTypeKind::Tuple(types) => {
                for ty in types {
                    self.noncallable_type(context, ty)?;
                }
                Ok(())
            }
            AstTypeKind::Array { element, .. }
            | AstTypeKind::Slice(element)
            | AstTypeKind::RawPointer {
                pointee: element, ..
            } => self.noncallable_type(context, element),
            AstTypeKind::Path(path) => {
                for argument in path
                    .generic_arguments
                    .iter()
                    .chain(
                        path.segments
                            .iter()
                            .filter_map(|segment| segment.generic_arguments.as_ref()),
                    )
                    .flat_map(|arguments| &arguments.arguments)
                {
                    if let AstGenericArgumentKind::Type(ty) = &argument.kind {
                        self.noncallable_type(context, ty)?;
                    }
                }
                Ok(())
            }
            AstTypeKind::FunctionPointer {
                parameters, result, ..
            } => self.callable_types(context, None, parameters.iter(), result.as_deref()),
            AstTypeKind::Scalar(_)
            | AstTypeKind::Never
            | AstTypeKind::Unit
            | AstTypeKind::Str
            | AstTypeKind::SelfType => Ok(()),
        }
    }

    fn callable_types<'a>(
        &mut self,
        context: &SymbolicLoweringContext<'_, '_>,
        receiver_lifetime: Option<SymbolicLifetime>,
        inputs: impl IntoIterator<Item = &'a crate::ast::AstType>,
        output: Option<&crate::ast::AstType>,
    ) -> Result<(), FrontendError> {
        let mut input_lifetimes = BTreeSet::new();
        if let Some(receiver) = &receiver_lifetime {
            input_lifetimes.insert(receiver.clone());
        }
        for input in inputs {
            self.input_type(context, input, &mut input_lifetimes)?;
        }
        let selected = receiver_lifetime.or_else(|| {
            (input_lifetimes.len() == 1).then(|| {
                input_lifetimes
                    .into_iter()
                    .next()
                    .expect("one lifetime exists")
            })
        });
        if let Some(output) = output {
            self.output_type(context, output, selected.as_ref())?;
        }
        Ok(())
    }
}

fn declaration_elision_plan(
    context: &SymbolicLoweringContext<'_, '_>,
    item: &ResolvedSymbolicItem,
) -> Result<DeclarationElisionPlan, FrontendError> {
    use crate::ast::{AstStructForm, AstSystemParameterKind, AstVariantForm};

    let explicit_parameter_count = item_generic_parameters(item)
        .map(|parameters| parameters.parameters.len())
        .unwrap_or(0);
    let mut planner = DeclarationElisionPlanner::new(
        explicit_parameter_count,
        item.kind == DeclarationKind::Generator,
    )?;
    match &item.source {
        HirItemSource::Declaration(declaration) => match &declaration.kind {
            AstDeclarationKind::Function(function) => planner.callable_types(
                context,
                None,
                function
                    .signature
                    .parameters
                    .iter()
                    .map(|parameter| &parameter.ty),
                function.signature.result.as_ref(),
            )?,
            AstDeclarationKind::Generator(generator) => {
                let mut input_lifetimes = BTreeSet::new();
                for parameter in &generator.parameters {
                    planner.input_type(context, &parameter.ty, &mut input_lifetimes)?;
                }
                planner.input_type(context, &generator.resume, &mut input_lifetimes)?;
                let selected = (input_lifetimes.len() == 1).then(|| {
                    input_lifetimes
                        .into_iter()
                        .next()
                        .expect("one lifetime exists")
                });
                planner.output_type(context, &generator.yields, selected.as_ref())?;
                if let Some(result) = &generator.result {
                    planner.output_type(context, result, selected.as_ref())?;
                }
            }
            AstDeclarationKind::System(system) => {
                let mut inputs = Vec::new();
                for parameter in &system.parameters {
                    match &parameter.kind {
                        AstSystemParameterKind::ResourceRead(ty)
                        | AstSystemParameterKind::ResourceWrite(ty)
                        | AstSystemParameterKind::Capability(ty) => inputs.push(ty),
                        AstSystemParameterKind::Query(terms) => {
                            inputs.extend(terms.iter().map(|term| &term.ty));
                        }
                        AstSystemParameterKind::Commands => {}
                    }
                }
                planner.callable_types(context, None, inputs, None)?;
            }
            AstDeclarationKind::Component(record) | AstDeclarationKind::Resource(record) => {
                for field in &record.fields {
                    planner.noncallable_type(context, &field.ty)?;
                }
            }
            AstDeclarationKind::Struct(structure) => match &structure.form {
                AstStructForm::Unit => {}
                AstStructForm::Tuple(fields) => {
                    for field in fields {
                        planner.noncallable_type(context, &field.ty)?;
                    }
                }
                AstStructForm::Record(fields) => {
                    for field in fields {
                        planner.noncallable_type(context, &field.ty)?;
                    }
                }
            },
            AstDeclarationKind::Enum(enumeration) => {
                for variant in &enumeration.variants {
                    match &variant.form {
                        AstVariantForm::Unit => {}
                        AstVariantForm::Tuple(fields) => {
                            for field in fields {
                                planner.noncallable_type(context, field)?;
                            }
                        }
                        AstVariantForm::Record(fields) => {
                            for field in fields {
                                planner.noncallable_type(context, &field.ty)?;
                            }
                        }
                    }
                }
            }
            AstDeclarationKind::TypeAlias(alias) => {
                planner.noncallable_type(context, &alias.target)?;
            }
            AstDeclarationKind::Const(const_) => {
                planner.noncallable_type(context, &const_.ty)?;
            }
            AstDeclarationKind::Static(static_) => {
                planner.noncallable_type(context, &static_.ty)?;
            }
            AstDeclarationKind::World { .. }
            | AstDeclarationKind::Tag
            | AstDeclarationKind::Schedule(_)
            | AstDeclarationKind::Trait(_) => {}
        },
        HirItemSource::Impl(implementation) => {
            planner.noncallable_type(context, &implementation.target)?;
        }
        HirItemSource::TraitMethod(method) => {
            method_elision_plan(context, &mut planner, &method.signature)?;
        }
        HirItemSource::ImplMethod(method) => {
            method_elision_plan(context, &mut planner, &method.signature)?;
        }
        HirItemSource::QueryParameter { terms, .. } => {
            for term in terms {
                planner.noncallable_type(context, &term.ty)?;
            }
        }
    }
    Ok(planner.plan)
}

fn method_elision_plan(
    context: &SymbolicLoweringContext<'_, '_>,
    planner: &mut DeclarationElisionPlanner,
    signature: &crate::ast::AstMethodSignature,
) -> Result<(), FrontendError> {
    let receiver_lifetime = signature
        .parameters
        .iter()
        .find_map(|parameter| match parameter {
            AstMethodParameter::Receiver(receiver) => Some(receiver),
            AstMethodParameter::Parameter(_) => None,
        })
        .map(|receiver| planner.receiver(context, receiver))
        .transpose()?
        .flatten();
    planner.callable_types(
        context,
        receiver_lifetime,
        signature
            .parameters
            .iter()
            .filter_map(|parameter| match parameter {
                AstMethodParameter::Receiver(_) => None,
                AstMethodParameter::Parameter(parameter) => Some(&parameter.ty),
            }),
        signature.result.as_ref(),
    )
}

fn resolve_declared_lifetime(
    context: &SymbolicLoweringContext<'_, '_>,
    lifetime: &Symbol,
) -> Result<SymbolicLifetime, FrontendError> {
    if lifetime.as_str() == "static" {
        return Ok(SymbolicLifetime::Static);
    }
    let parameter = context.generics.get(lifetime.as_str()).ok_or_else(|| {
        frontend_error(
            FrontendErrorCode::Name,
            Diagnostic::at(
                "NAME002",
                find_item(context.packages, context.item)
                    .expect("lowering context item exists")
                    .span,
                format!("unknown declaration lifetime `'{}'", lifetime.as_str()),
            ),
        )
    })?;
    if !matches!(
        generic_parameter_kind(context.packages, *parameter),
        Some(GenericParameterKind::Lifetime)
    ) {
        return Err(frontend_error(
            FrontendErrorCode::Name,
            Diagnostic::at(
                "NAME003",
                find_item(context.packages, context.item)
                    .expect("lowering context item exists")
                    .span,
                format!("`{}` is not a lifetime parameter", lifetime.as_str()),
            ),
        ));
    }
    let depth = generic_parameter_depth(context, *parameter).map_err(lowering_frontend)?;
    Ok(SymbolicLifetime::Bound {
        depth,
        index: parameter.index,
    })
}

enum SymbolicLoweringError {
    Pending {
        reason: UnresolvedPathKind,
        span: Span,
    },
    Frontend(FrontendError),
}

impl SymbolicLoweringError {
    const fn pending(reason: UnresolvedPathKind, span: Span) -> Self {
        Self::Pending { reason, span }
    }
}

impl From<FrontendError> for SymbolicLoweringError {
    fn from(error: FrontendError) -> Self {
        Self::Frontend(error)
    }
}

type SymbolicLoweringResult<T> = Result<T, SymbolicLoweringError>;

fn resolve_symbolic_type(
    context: &SymbolicLoweringContext<'_, '_>,
    ty: &crate::ast::AstType,
) -> Result<ResolvedSymbolicType, FrontendError> {
    match lower_symbolic_type(context, ty) {
        Ok(ty) => Ok(ResolvedSymbolicType::Resolved(Box::new(ty))),
        Err(SymbolicLoweringError::Pending { reason, span }) => Ok(ResolvedSymbolicType::Pending {
            span,
            reason,
            canonical: canonical_type(ty),
        }),
        Err(SymbolicLoweringError::Frontend(error)) => Err(error),
    }
}

fn resolve_symbolic_const(
    context: &SymbolicLoweringContext<'_, '_>,
    expression: &AstConstExpression,
    integer_type: IntegerType,
) -> Result<ResolvedSymbolicConst, FrontendError> {
    match lower_symbolic_const(context, expression, integer_type) {
        Ok(value) => Ok(ResolvedSymbolicConst::Resolved(value)),
        Err(SymbolicLoweringError::Pending { reason, span }) => {
            Ok(ResolvedSymbolicConst::Pending {
                span,
                reason,
                canonical: canonical_const_expression(expression),
            })
        }
        Err(SymbolicLoweringError::Frontend(error)) => Err(error),
    }
}

fn resolve_symbolic_lifetime(
    context: &SymbolicLoweringContext<'_, '_>,
    lifetime: &Symbol,
    span: Span,
) -> Result<ResolvedSymbolicLifetime, FrontendError> {
    match lower_symbolic_lifetime(context, Some(lifetime), span) {
        Ok(value) => Ok(ResolvedSymbolicLifetime::Resolved(value)),
        Err(SymbolicLoweringError::Pending { reason, span }) => {
            Ok(ResolvedSymbolicLifetime::Pending {
                span,
                reason,
                canonical: format!("'{}", lifetime.as_str()),
            })
        }
        Err(SymbolicLoweringError::Frontend(error)) => Err(error),
    }
}

fn resolve_symbolic_effect(
    context: &SymbolicLoweringContext<'_, '_>,
    effect: &AstSymbolicEffect,
) -> Result<ResolvedSymbolicEffect, FrontendError> {
    let (kind, lowered, canonical) = match effect {
        AstSymbolicEffect::Requires(path) => (
            EffectKind::Requires,
            lower_symbolic_path_type(context, path),
            canonical_path(path),
        ),
        AstSymbolicEffect::Throws(ty) => (
            EffectKind::Throws,
            lower_symbolic_type(context, ty),
            canonical_type(ty),
        ),
    };
    match lowered {
        Ok(ty) => Ok(ResolvedSymbolicEffect::Resolved(Box::new(
            SymbolicEffectAtom { kind, ty },
        ))),
        Err(SymbolicLoweringError::Pending { reason, span }) => {
            Ok(ResolvedSymbolicEffect::Pending {
                span,
                reason,
                canonical,
            })
        }
        Err(SymbolicLoweringError::Frontend(error)) => Err(error),
    }
}

fn lower_symbolic_type(
    context: &SymbolicLoweringContext<'_, '_>,
    ty: &crate::ast::AstType,
) -> SymbolicLoweringResult<SymbolicType> {
    use crate::ast::{AstScalarType, AstTypeKind};
    Ok(match &ty.kind {
        AstTypeKind::Scalar(scalar) => match scalar {
            AstScalarType::Integer(integer) => symbolic_integer_type(integer_type(*integer)),
            AstScalarType::F32 => SymbolicType::F32,
            AstScalarType::F64 => SymbolicType::F64,
            AstScalarType::Bool => SymbolicType::Bool,
            AstScalarType::Char => SymbolicType::Char,
            AstScalarType::Entity => SymbolicType::Entity,
        },
        AstTypeKind::Never => SymbolicType::Never,
        AstTypeKind::Unit => SymbolicType::Unit,
        AstTypeKind::Str => SymbolicType::Str,
        AstTypeKind::SelfType => return lower_owner_self_type(context, ty.span),
        AstTypeKind::Path(path) => lower_symbolic_path_type(context, path)?,
        AstTypeKind::Tuple(types) => SymbolicType::Tuple(
            types
                .iter()
                .map(|ty| lower_symbolic_type(context, ty))
                .collect::<SymbolicLoweringResult<Vec<_>>>()?,
        ),
        AstTypeKind::Array { element, length } => SymbolicType::Array {
            element: Box::new(lower_symbolic_type(context, element)?),
            length: lower_symbolic_const(context, length, IntegerType::Usize)?,
        },
        AstTypeKind::Slice(element) => {
            SymbolicType::Slice(Box::new(lower_symbolic_type(context, element)?))
        }
        AstTypeKind::Reference {
            lifetime,
            mutable,
            pointee,
        } => SymbolicType::Reference {
            mutability: symbolic_mutability(*mutable),
            lifetime: lower_symbolic_lifetime(context, lifetime.as_ref(), ty.span)?,
            pointee: Box::new(lower_symbolic_type(context, pointee)?),
        },
        AstTypeKind::RawPointer { mutable, pointee } => SymbolicType::RawPointer {
            mutability: symbolic_mutability(*mutable),
            pointee: Box::new(lower_symbolic_type(context, pointee)?),
        },
        AstTypeKind::FunctionPointer {
            unsafe_,
            parameters,
            effects,
            result,
        } => {
            let requires = effects
                .requires
                .as_ref()
                .map(|set| {
                    set.members
                        .iter()
                        .map(|path| lower_symbolic_path_type(context, path))
                        .collect::<SymbolicLoweringResult<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default();
            let throws = effects
                .throws
                .as_ref()
                .map(|set| {
                    set.members
                        .iter()
                        .map(|ty| lower_symbolic_type(context, ty))
                        .collect::<SymbolicLoweringResult<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default();
            SymbolicType::FunctionPointer {
                unsafe_: *unsafe_,
                parameters: parameters
                    .iter()
                    .map(|ty| lower_symbolic_type(context, ty))
                    .collect::<SymbolicLoweringResult<Vec<_>>>()?,
                result: Box::new(match result {
                    Some(result) => lower_symbolic_type(context, result)?,
                    None => SymbolicType::Unit,
                }),
                requires: super::shape::SymbolicTypeEffectSet::pending_c4(requires),
                throws: super::shape::SymbolicTypeEffectSet::pending_c4(throws),
            }
        }
    })
}

fn lower_owner_self_type(
    context: &SymbolicLoweringContext<'_, '_>,
    span: Span,
) -> SymbolicLoweringResult<SymbolicType> {
    let item = find_item(context.packages, context.item).ok_or_else(|| {
        SymbolicLoweringError::Frontend(frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            "Self type lowering owner is missing from the C1 item arena",
        ))
    })?;
    let owner = item
        .owner
        .and_then(|owner| find_item(context.packages, owner));
    match owner.map(|owner| &owner.source) {
        Some(HirItemSource::Impl(implementation)) => {
            lower_symbolic_type(context, &implementation.target)
        }
        Some(
            HirItemSource::Declaration(_)
            | HirItemSource::TraitMethod(_)
            | HirItemSource::ImplMethod(_)
            | HirItemSource::QueryParameter { .. },
        )
        | None => {
            if context.contextual_self_template {
                Ok(c2_contextual_self_marker())
            } else {
                Err(SymbolicLoweringError::pending(
                    UnresolvedPathKind::SelfTypePendingC2,
                    span,
                ))
            }
        }
    }
}

const C2_CONTEXTUAL_SELF_MARKER_DEPTH: u64 = u64::MAX;
const C2_CONTEXTUAL_SELF_MARKER_INDEX: u64 = u64::MAX;

const fn c2_contextual_self_marker() -> SymbolicType {
    SymbolicType::BoundType {
        depth: C2_CONTEXTUAL_SELF_MARKER_DEPTH,
        index: C2_CONTEXTUAL_SELF_MARKER_INDEX,
    }
}

const fn is_c2_contextual_self_marker(ty: &SymbolicType) -> bool {
    matches!(
        ty,
        SymbolicType::BoundType {
            depth: C2_CONTEXTUAL_SELF_MARKER_DEPTH,
            index: C2_CONTEXTUAL_SELF_MARKER_INDEX,
        }
    )
}

fn count_c2_self_markers(ty: &SymbolicType) -> usize {
    rewrite_c2_self_markers(ty, None).1
}

fn symbolic_type_contains_c2_self_marker(ty: &SymbolicType) -> bool {
    count_c2_self_markers(ty) != 0
}

fn replace_c2_self_markers(ty: &SymbolicType, replacement: &SymbolicType) -> SymbolicType {
    rewrite_c2_self_markers(ty, Some(replacement)).0
}

fn rewrite_c2_self_markers(
    ty: &SymbolicType,
    replacement: Option<&SymbolicType>,
) -> (SymbolicType, usize) {
    if is_c2_contextual_self_marker(ty) {
        return (replacement.unwrap_or(ty).clone(), 1);
    }
    match ty {
        primitive @ (SymbolicType::I8
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
        | SymbolicType::BoundType { .. }) => (primitive.clone(), 0),
        SymbolicType::Slice(element) => {
            let (element, count) = rewrite_c2_self_markers(element, replacement);
            (SymbolicType::Slice(Box::new(element)), count)
        }
        SymbolicType::Array { element, length } => {
            let (element, count) = rewrite_c2_self_markers(element, replacement);
            (
                SymbolicType::Array {
                    element: Box::new(element),
                    length: length.clone(),
                },
                count,
            )
        }
        SymbolicType::Tuple(elements) => {
            let (elements, count) = rewrite_c2_self_type_list(elements, replacement);
            (SymbolicType::Tuple(elements), count)
        }
        SymbolicType::Reference {
            mutability,
            lifetime,
            pointee,
        } => {
            let (pointee, count) = rewrite_c2_self_markers(pointee, replacement);
            (
                SymbolicType::Reference {
                    mutability: *mutability,
                    lifetime: lifetime.clone(),
                    pointee: Box::new(pointee),
                },
                count,
            )
        }
        SymbolicType::RawPointer {
            mutability,
            pointee,
        } => {
            let (pointee, count) = rewrite_c2_self_markers(pointee, replacement);
            (
                SymbolicType::RawPointer {
                    mutability: *mutability,
                    pointee: Box::new(pointee),
                },
                count,
            )
        }
        SymbolicType::NominalPath {
            declaration,
            arguments,
        } => {
            let (arguments, count) = rewrite_c2_self_arguments(arguments, replacement);
            (
                SymbolicType::NominalPath {
                    declaration: declaration.clone(),
                    arguments,
                },
                count,
            )
        }
        SymbolicType::FunctionPointer {
            unsafe_,
            parameters,
            result,
            requires,
            throws,
        } => {
            let (parameters, mut count) = rewrite_c2_self_type_list(parameters, replacement);
            let (result, result_count) = rewrite_c2_self_markers(result, replacement);
            count = count.saturating_add(result_count);
            let (requires, requires_count) = rewrite_c2_self_effect_set(requires, replacement);
            count = count.saturating_add(requires_count);
            let (throws, throws_count) = rewrite_c2_self_effect_set(throws, replacement);
            count = count.saturating_add(throws_count);
            (
                SymbolicType::FunctionPointer {
                    unsafe_: *unsafe_,
                    parameters,
                    result: Box::new(result),
                    requires,
                    throws,
                },
                count,
            )
        }
        SymbolicType::Closure {
            owner,
            expression_ordinal,
            captures,
            parameters,
            result,
            requires,
            throws,
            arguments,
        } => {
            let (captures, mut count) = rewrite_c2_self_captures(captures, replacement);
            let (parameters, parameter_count) = rewrite_c2_self_type_list(parameters, replacement);
            count = count.saturating_add(parameter_count);
            let (result, result_count) = rewrite_c2_self_markers(result, replacement);
            count = count.saturating_add(result_count);
            let (requires, requires_count) = rewrite_c2_self_effect_set(requires, replacement);
            count = count.saturating_add(requires_count);
            let (throws, throws_count) = rewrite_c2_self_effect_set(throws, replacement);
            count = count.saturating_add(throws_count);
            let (arguments, argument_count) = rewrite_c2_self_arguments(arguments, replacement);
            count = count.saturating_add(argument_count);
            (
                SymbolicType::Closure {
                    owner: owner.clone(),
                    expression_ordinal: *expression_ordinal,
                    captures,
                    parameters,
                    result: Box::new(result),
                    requires,
                    throws,
                    arguments,
                },
                count,
            )
        }
        SymbolicType::Generator {
            target,
            captures,
            parameters,
            factory_unsafe,
            resume,
            yields,
            result,
            requires,
            throws,
        } => {
            let (target, mut count) = rewrite_c2_self_generator_target(target, replacement);
            let (captures, capture_count) = rewrite_c2_self_captures(captures, replacement);
            count = count.saturating_add(capture_count);
            let (parameters, parameter_count) = rewrite_c2_self_type_list(parameters, replacement);
            count = count.saturating_add(parameter_count);
            let (resume, resume_count) = rewrite_c2_self_markers(resume, replacement);
            count = count.saturating_add(resume_count);
            let (yields, yields_count) = rewrite_c2_self_markers(yields, replacement);
            count = count.saturating_add(yields_count);
            let (result, result_count) = rewrite_c2_self_markers(result, replacement);
            count = count.saturating_add(result_count);
            let (requires, requires_count) = rewrite_c2_self_effect_set(requires, replacement);
            count = count.saturating_add(requires_count);
            let (throws, throws_count) = rewrite_c2_self_effect_set(throws, replacement);
            count = count.saturating_add(throws_count);
            (
                SymbolicType::Generator {
                    target: Box::new(target),
                    captures,
                    parameters,
                    factory_unsafe: *factory_unsafe,
                    resume: Box::new(resume),
                    yields: Box::new(yields),
                    result: Box::new(result),
                    requires,
                    throws,
                },
                count,
            )
        }
        SymbolicType::JoinHandle { result, throws } => {
            let (result, mut count) = rewrite_c2_self_markers(result, replacement);
            let (throws, throws_count) = rewrite_c2_self_effect_set(throws, replacement);
            count = count.saturating_add(throws_count);
            (
                SymbolicType::JoinHandle {
                    result: Box::new(result),
                    throws,
                },
                count,
            )
        }
        SymbolicType::GeneratorFactory {
            target,
            captures,
            call_trait,
            parameters,
            factory_unsafe,
            produced_generator,
        } => {
            let (target, mut count) = rewrite_c2_self_generator_target(target, replacement);
            let (captures, capture_count) = rewrite_c2_self_captures(captures, replacement);
            count = count.saturating_add(capture_count);
            let (parameters, parameter_count) = rewrite_c2_self_type_list(parameters, replacement);
            count = count.saturating_add(parameter_count);
            let (produced_generator, produced_count) =
                rewrite_c2_self_markers(produced_generator, replacement);
            count = count.saturating_add(produced_count);
            (
                SymbolicType::GeneratorFactory {
                    target: Box::new(target),
                    captures,
                    call_trait: *call_trait,
                    parameters,
                    factory_unsafe: *factory_unsafe,
                    produced_generator: Box::new(produced_generator),
                },
                count,
            )
        }
    }
}

fn rewrite_c2_self_type_list(
    types: &[SymbolicType],
    replacement: Option<&SymbolicType>,
) -> (Vec<SymbolicType>, usize) {
    let mut count = 0_usize;
    let types = types
        .iter()
        .map(|ty| {
            let (ty, child_count) = rewrite_c2_self_markers(ty, replacement);
            count = count.saturating_add(child_count);
            ty
        })
        .collect();
    (types, count)
}

fn rewrite_c2_self_arguments(
    arguments: &[GenericArgumentShape],
    replacement: Option<&SymbolicType>,
) -> (Vec<GenericArgumentShape>, usize) {
    let mut count = 0_usize;
    let arguments = arguments
        .iter()
        .map(|argument| match argument {
            GenericArgumentShape::Type(ty) => {
                let (ty, child_count) = rewrite_c2_self_markers(ty, replacement);
                count = count.saturating_add(child_count);
                GenericArgumentShape::Type(ty)
            }
            GenericArgumentShape::Lifetime(lifetime) => {
                GenericArgumentShape::Lifetime(lifetime.clone())
            }
            GenericArgumentShape::IntegerConst(value) => {
                GenericArgumentShape::IntegerConst(value.clone())
            }
        })
        .collect();
    (arguments, count)
}

fn rewrite_c2_self_effect_set(
    effects: &super::shape::SymbolicTypeEffectSet,
    replacement: Option<&SymbolicType>,
) -> (super::shape::SymbolicTypeEffectSet, usize) {
    let (members, count) = rewrite_c2_self_type_list(effects.members(), replacement);
    let effects = if effects.readiness() == SymbolicShapeReadiness::PendingC4 {
        super::shape::SymbolicTypeEffectSet::pending_c4(members)
    } else {
        super::shape::SymbolicTypeEffectSet::resolved(members)
    };
    (effects, count)
}

fn rewrite_c2_self_captures(
    captures: &[super::shape::SymbolicCapture],
    replacement: Option<&SymbolicType>,
) -> (Vec<super::shape::SymbolicCapture>, usize) {
    let mut count = 0_usize;
    let captures = captures
        .iter()
        .map(|capture| {
            let (ty, child_count) = rewrite_c2_self_markers(&capture.ty, replacement);
            count = count.saturating_add(child_count);
            super::shape::SymbolicCapture {
                ordinal: capture.ordinal,
                mode: capture.mode,
                ty,
            }
        })
        .collect();
    (captures, count)
}

fn rewrite_c2_self_generator_target(
    target: &super::shape::GeneratorTarget,
    replacement: Option<&SymbolicType>,
) -> (super::shape::GeneratorTarget, usize) {
    match target {
        super::shape::GeneratorTarget::Named {
            declaration,
            arguments,
            hidden_lifetime_binders,
        } => {
            let (arguments, count) = rewrite_c2_self_arguments(arguments, replacement);
            (
                super::shape::GeneratorTarget::Named {
                    declaration: declaration.clone(),
                    arguments,
                    hidden_lifetime_binders: hidden_lifetime_binders.clone(),
                },
                count,
            )
        }
        super::shape::GeneratorTarget::Anonymous {
            owner,
            expression_ordinal,
            arguments,
        } => {
            let (arguments, count) = rewrite_c2_self_arguments(arguments, replacement);
            (
                super::shape::GeneratorTarget::Anonymous {
                    owner: owner.clone(),
                    expression_ordinal: *expression_ordinal,
                    arguments,
                },
                count,
            )
        }
    }
}

fn lower_symbolic_path_type(
    context: &SymbolicLoweringContext<'_, '_>,
    path: &crate::ast::AstPath,
) -> SymbolicLoweringResult<SymbolicType> {
    let empty_locals = BTreeMap::new();
    let resolution = resolve_general_path(
        context.packages,
        context.graph,
        context.location,
        path,
        Some(Namespace::Type),
        LexicalPathEnvironment {
            generics: context.generics,
            locals: &empty_locals,
        },
        context.embedded_core,
    )?;
    if let Some(reason) = resolution.unresolved {
        return Err(SymbolicLoweringError::pending(reason, resolution.span));
    }
    let [resolution] = resolution.resolutions.as_slice() else {
        return Err(SymbolicLoweringError::pending(
            UnresolvedPathKind::AmbiguousNamespace,
            path.span,
        ));
    };
    match resolution {
        Res::Generic(parameter) => {
            if path.generic_arguments.is_some()
                || path
                    .segments
                    .iter()
                    .any(|segment| segment.generic_arguments.is_some())
            {
                return Err(SymbolicLoweringError::pending(
                    UnresolvedPathKind::GenericFormationPendingC2,
                    path.span,
                ));
            }
            match generic_parameter_kind(context.packages, *parameter) {
                Some(GenericParameterKind::Type) => Ok(SymbolicType::BoundType {
                    depth: generic_parameter_depth(context, *parameter)?,
                    index: parameter.index,
                }),
                _ => Err(SymbolicLoweringError::pending(
                    UnresolvedPathKind::GenericFormationPendingC2,
                    path.span,
                )),
            }
        }
        Res::Item(HirItemRes::Definition(item)) => {
            let item_row = find_item(context.packages, *item).ok_or_else(|| {
                SymbolicLoweringError::Frontend(frontend_path_error(
                    FrontendErrorCode::Target,
                    "IDENTITY001",
                    "resolved generic-argument owner is missing from the C1 item arena",
                ))
            })?;
            let formal_parameters = item_generic_parameters(item_row)
                .map_or(&[][..], |parameters| parameters.parameters.as_slice());
            Ok(SymbolicType::NominalPath {
                declaration: semantic_declaration_path(context, *item)?,
                arguments: lower_symbolic_generic_arguments(
                    context,
                    path,
                    Some(formal_parameters),
                )?,
            })
        }
        Res::Item(HirItemRes::NominalConstructor { .. } | HirItemRes::EnumVariant { .. }) => Err(
            SymbolicLoweringError::pending(UnresolvedPathKind::AmbiguousNamespace, path.span),
        ),
        Res::Builtin(builtin) => lower_embedded_symbolic_type(context, builtin, path),
        Res::Module(_) | Res::Local(_) => Err(SymbolicLoweringError::pending(
            UnresolvedPathKind::AmbiguousNamespace,
            path.span,
        )),
    }
}

fn lower_symbolic_generic_arguments(
    context: &SymbolicLoweringContext<'_, '_>,
    path: &crate::ast::AstPath,
    formal_parameters: Option<&[crate::ast::AstGenericParameter]>,
) -> SymbolicLoweringResult<Vec<GenericArgumentShape>> {
    let actual_arguments = path
        .generic_arguments
        .iter()
        .chain(
            path.segments
                .iter()
                .filter_map(|segment| segment.generic_arguments.as_ref()),
        )
        .flat_map(|arguments| &arguments.arguments)
        .collect::<Vec<_>>();
    if let Some(formal_parameters) = formal_parameters {
        if actual_arguments.len() != formal_parameters.len() {
            return Err(SymbolicLoweringError::pending(
                UnresolvedPathKind::GenericFormationPendingC2,
                path.span,
            ));
        }
        return actual_arguments
            .into_iter()
            .zip(formal_parameters)
            .map(|(argument, formal)| match (&argument.kind, &formal.kind) {
                (
                    crate::ast::AstGenericArgumentKind::Type(ty),
                    crate::ast::AstGenericParameterKind::Type { .. },
                ) => Ok(GenericArgumentShape::Type(lower_symbolic_type(
                    context, ty,
                )?)),
                (
                    crate::ast::AstGenericArgumentKind::Lifetime(lifetime),
                    crate::ast::AstGenericParameterKind::Lifetime { .. },
                ) => Ok(GenericArgumentShape::Lifetime(lower_symbolic_lifetime(
                    context,
                    Some(lifetime),
                    argument.span,
                )?)),
                (
                    crate::ast::AstGenericArgumentKind::IntegerConst(expression),
                    crate::ast::AstGenericParameterKind::IntegerConst { ty, .. },
                ) => Ok(GenericArgumentShape::IntegerConst(lower_symbolic_const(
                    context,
                    expression,
                    integer_type(*ty),
                )?)),
                _ => Err(SymbolicLoweringError::pending(
                    UnresolvedPathKind::GenericFormationPendingC2,
                    argument.span,
                )),
            })
            .collect();
    }

    let mut output = Vec::new();
    for argument in actual_arguments {
        output.push(match &argument.kind {
            crate::ast::AstGenericArgumentKind::Type(ty) => {
                GenericArgumentShape::Type(lower_symbolic_type(context, ty)?)
            }
            crate::ast::AstGenericArgumentKind::Lifetime(lifetime) => {
                GenericArgumentShape::Lifetime(lower_symbolic_lifetime(
                    context,
                    Some(lifetime),
                    argument.span,
                )?)
            }
            crate::ast::AstGenericArgumentKind::IntegerConst(expression) => {
                GenericArgumentShape::IntegerConst(lower_symbolic_const(
                    context,
                    expression,
                    const_expression_integer_type(expression),
                )?)
            }
        });
    }
    Ok(output)
}

fn lower_symbolic_lifetime(
    context: &SymbolicLoweringContext<'_, '_>,
    lifetime: Option<&Symbol>,
    span: Span,
) -> SymbolicLoweringResult<SymbolicLifetime> {
    let Some(lifetime) = lifetime else {
        return match context.lifetime_domain {
            LifetimeDomain::BodyLocal => Ok(SymbolicLifetime::ErasedLocal),
            LifetimeDomain::Declaration(plan) => plan
                .by_span
                .get(&SpanKey::from(span))
                .cloned()
                .ok_or_else(|| {
                    SymbolicLoweringError::Frontend(frontend_error(
                        FrontendErrorCode::Target,
                        Diagnostic::at(
                            "TYPE001",
                            span,
                            "declaration reference is missing its deterministic elision coordinate",
                        ),
                    ))
                }),
        };
    };
    if lifetime.as_str() == "static" {
        return Ok(SymbolicLifetime::Static);
    }
    let Some(parameter) = context.generics.get(lifetime.as_str()) else {
        return Err(SymbolicLoweringError::Frontend(frontend_error(
            FrontendErrorCode::Name,
            Diagnostic::at(
                "NAME002",
                span,
                format!("unknown lifetime `'{}'", lifetime.as_str()),
            ),
        )));
    };
    match generic_parameter_kind(context.packages, *parameter) {
        Some(GenericParameterKind::Lifetime) => Ok(SymbolicLifetime::Bound {
            depth: generic_parameter_depth(context, *parameter)?,
            index: parameter.index,
        }),
        _ => Err(SymbolicLoweringError::Frontend(frontend_error(
            FrontendErrorCode::Name,
            Diagnostic::at(
                "NAME002",
                span,
                format!("`{}` does not name a lifetime parameter", lifetime.as_str()),
            ),
        ))),
    }
}

fn lower_symbolic_const(
    context: &SymbolicLoweringContext<'_, '_>,
    expression: &AstConstExpression,
    integer_type: IntegerType,
) -> SymbolicLoweringResult<SymbolicConstExpression> {
    use crate::ast::{AstConstBinaryOperator, AstConstExpressionKind, AstConstUnaryOperator};
    let node = match &expression.kind {
        AstConstExpressionKind::Integer(literal) => {
            SymbolicConstNode::IntegerLiteral(integer_literal_bits(literal, integer_type)?)
        }
        AstConstExpressionKind::Path(path) => {
            let empty_locals = BTreeMap::new();
            let resolution = resolve_general_path(
                context.packages,
                context.graph,
                context.location,
                path,
                Some(Namespace::Value),
                LexicalPathEnvironment {
                    generics: context.generics,
                    locals: &empty_locals,
                },
                context.embedded_core,
            )?;
            if let Some(reason) = resolution.unresolved {
                return Err(SymbolicLoweringError::pending(reason, resolution.span));
            }
            let [resolution] = resolution.resolutions.as_slice() else {
                return Err(SymbolicLoweringError::pending(
                    UnresolvedPathKind::AmbiguousNamespace,
                    path.span,
                ));
            };
            match resolution {
                Res::Generic(parameter)
                    if matches!(
                        generic_parameter_kind(context.packages, *parameter),
                        Some(GenericParameterKind::IntegerConst(_))
                    ) =>
                {
                    SymbolicConstNode::Bound {
                        depth: generic_parameter_depth(context, *parameter)?,
                        index: parameter.index,
                    }
                }
                Res::Item(HirItemRes::Definition(item))
                    if find_item(context.packages, *item)
                        .is_some_and(|item| item.kind == DeclarationKind::Const) =>
                {
                    SymbolicConstNode::ConstDefinitionPath(semantic_declaration_path(
                        context, *item,
                    )?)
                }
                Res::Builtin(_) => {
                    return Err(SymbolicLoweringError::pending(
                        UnresolvedPathKind::AmbiguousNamespace,
                        path.span,
                    ));
                }
                Res::Item(
                    HirItemRes::Definition(_)
                    | HirItemRes::NominalConstructor { .. }
                    | HirItemRes::EnumVariant { .. },
                )
                | Res::Generic(_)
                | Res::Module(_)
                | Res::Local(_) => {
                    return Err(SymbolicLoweringError::pending(
                        if matches!(resolution, Res::Generic(_)) {
                            UnresolvedPathKind::GenericFormationPendingC2
                        } else {
                            UnresolvedPathKind::AmbiguousNamespace
                        },
                        path.span,
                    ));
                }
            }
        }
        AstConstExpressionKind::Group(child) => {
            return lower_symbolic_const(context, child, integer_type);
        }
        AstConstExpressionKind::Unary { operator, operand } => {
            let operand = Box::new(lower_symbolic_const(context, operand, integer_type)?);
            match operator {
                AstConstUnaryOperator::Negate => SymbolicConstNode::WrappingNeg(operand),
                AstConstUnaryOperator::BitNot => SymbolicConstNode::BitNot(operand),
            }
        }
        AstConstExpressionKind::Binary {
            operator,
            left,
            right,
        } => {
            let left = Box::new(lower_symbolic_const(context, left, integer_type)?);
            let right = Box::new(lower_symbolic_const(context, right, integer_type)?);
            match operator {
                AstConstBinaryOperator::Multiply => SymbolicConstNode::WrappingMul(left, right),
                AstConstBinaryOperator::Divide => SymbolicConstNode::IntegerDivide(left, right),
                AstConstBinaryOperator::Remainder => {
                    SymbolicConstNode::IntegerRemainder(left, right)
                }
                AstConstBinaryOperator::Add => SymbolicConstNode::WrappingAdd(left, right),
                AstConstBinaryOperator::Subtract => SymbolicConstNode::WrappingSub(left, right),
                AstConstBinaryOperator::ShiftLeft => {
                    SymbolicConstNode::MaskedShiftLeft(left, right)
                }
                AstConstBinaryOperator::ShiftRight => {
                    SymbolicConstNode::MaskedShiftRight(left, right)
                }
                AstConstBinaryOperator::BitAnd => SymbolicConstNode::BitAnd(left, right),
                AstConstBinaryOperator::BitXor => SymbolicConstNode::BitXor(left, right),
                AstConstBinaryOperator::BitOr => SymbolicConstNode::BitOr(left, right),
            }
        }
    };
    Ok(SymbolicConstExpression { integer_type, node })
}

fn integer_literal_bits(
    literal: &crate::lexer::IntegerLiteral,
    integer_type: IntegerType,
) -> SymbolicLoweringResult<Vec<u8>> {
    let radix = match literal.base {
        crate::lexer::NumericBase::Binary => 2,
        crate::lexer::NumericBase::Octal => 8,
        crate::lexer::NumericBase::Decimal => 10,
        crate::lexer::NumericBase::Hexadecimal => 16,
    };
    let value = u128::from_str_radix(&literal.digits, radix).map_err(|_| {
        SymbolicLoweringError::Frontend(frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            "integer literal cannot be represented in the canonical type-const tree",
        ))
    })?;
    let width = integer_type.byte_width();
    let bit_width = width * 8;
    let maximum = (1_u128 << bit_width) - 1;
    if value > maximum {
        return Err(SymbolicLoweringError::Frontend(frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            "integer literal exceeds its contextual canonical type-const width",
        )));
    }
    Ok(value.to_le_bytes()[..width].to_vec())
}

fn semantic_declaration_path(
    context: &SymbolicLoweringContext<'_, '_>,
    item_id: HirItemId,
) -> SymbolicLoweringResult<SemanticDeclarationPath> {
    let item = find_item(context.packages, item_id).ok_or_else(|| {
        SymbolicLoweringError::Frontend(frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            "resolved symbolic item is missing from the C1 session arena",
        ))
    })?;
    let package = context
        .packages
        .iter()
        .find(|package| package.resolved.id == item.module.package())
        .expect("resolved item package exists");
    let target = package
        .targets
        .iter()
        .find(|target| target.target.id == item.module.target())
        .expect("resolved item target exists");
    let module = target
        .target
        .modules
        .iter()
        .find(|module| module.id == item.module)
        .expect("resolved item module exists");
    Ok(SemanticDeclarationPath {
        registry_origin: context.graph.registry_identity.clone(),
        package_name: package.resolved.name.to_string(),
        target: target.target.target.clone(),
        modules: module
            .path
            .iter()
            .map(|segment| segment.as_str().to_owned())
            .collect(),
        kind: item.kind,
        name: item.name.clone().unwrap_or_default(),
    })
}

fn embedded_declaration_path(
    context: &SymbolicLoweringContext<'_, '_>,
    definition: crate::embedded_core::VirtualDefinitionId,
    span: Span,
) -> SymbolicLoweringResult<SemanticDeclarationPath> {
    let row = context
        .embedded_core
        .definition(definition)
        .ok_or_else(|| {
            SymbolicLoweringError::Frontend(frontend_path_error(
                FrontendErrorCode::Target,
                "IDENTITY001",
                "embedded-Core prelude target does not name a verified definition row",
            ))
        })?;
    let kind = match row.declaration_kind() {
        VirtualDeclarationKind::Primitive => {
            return Err(SymbolicLoweringError::pending(
                UnresolvedPathKind::AmbiguousNamespace,
                span,
            ));
        }
        VirtualDeclarationKind::Struct => DeclarationKind::Struct,
        VirtualDeclarationKind::Enum => DeclarationKind::Enum,
        VirtualDeclarationKind::Trait => DeclarationKind::Trait,
        VirtualDeclarationKind::Function => DeclarationKind::Function,
    };
    let projection = context.embedded_core.projection();
    Ok(SemanticDeclarationPath {
        registry_origin: projection.registry_origin().to_owned(),
        package_name: projection.scoped_name().to_owned(),
        target: TargetRoot::Library,
        modules: Vec::new(),
        kind,
        name: row.name().to_owned(),
    })
}

fn lower_embedded_symbolic_type(
    context: &SymbolicLoweringContext<'_, '_>,
    builtin: &BuiltinRes,
    path: &crate::ast::AstPath,
) -> SymbolicLoweringResult<SymbolicType> {
    let arguments = lower_symbolic_generic_arguments(context, path, None)?;
    match builtin.target {
        BuiltinResTarget::Prelude(VirtualPreludeTarget::Definition(definition)) => {
            Ok(SymbolicType::NominalPath {
                declaration: embedded_declaration_path(context, definition, path.span)?,
                arguments,
            })
        }
        BuiltinResTarget::Prelude(VirtualPreludeTarget::SemanticType(semantic_type)) => {
            let row = context
                .embedded_core
                .semantic_type(semantic_type)
                .ok_or_else(|| {
                    SymbolicLoweringError::Frontend(frontend_path_error(
                        FrontendErrorCode::Target,
                        "IDENTITY001",
                        "embedded-Core prelude target does not name a verified semantic type row",
                    ))
                })?;
            if row.semantic_tag() != 30 || row.spelling() != "JoinHandle" {
                return Err(SymbolicLoweringError::Frontend(frontend_path_error(
                    FrontendErrorCode::Target,
                    "IDENTITY001",
                    "embedded-Core semantic type is not the sealed JoinHandle tag-30 row",
                )));
            }
            let mut arguments = arguments.into_iter();
            let Some(GenericArgumentShape::Type(result)) = arguments.next() else {
                return Err(SymbolicLoweringError::pending(
                    UnresolvedPathKind::GenericFormationPendingC2,
                    path.span,
                ));
            };
            let throws = arguments
                .map(|argument| match argument {
                    GenericArgumentShape::Type(ty) => Ok(ty),
                    GenericArgumentShape::Lifetime(_) | GenericArgumentShape::IntegerConst(_) => {
                        Err(SymbolicLoweringError::pending(
                            UnresolvedPathKind::GenericFormationPendingC2,
                            path.span,
                        ))
                    }
                })
                .collect::<SymbolicLoweringResult<Vec<_>>>()?;
            Ok(SymbolicType::JoinHandle {
                result: Box::new(result),
                throws: super::shape::SymbolicTypeEffectSet::pending_c4(throws),
            })
        }
        BuiltinResTarget::Method(_)
        | BuiltinResTarget::EnumVariant(_)
        | BuiltinResTarget::RecordConstructor(_) => Err(SymbolicLoweringError::pending(
            UnresolvedPathKind::AmbiguousNamespace,
            path.span,
        )),
    }
}

fn generic_parameter_kind(
    packages: &[PackageDraft<'_>],
    id: GenericParameterId,
) -> Option<GenericParameterKind> {
    let parameter = generic_parameter(packages, id)?;
    Some(match parameter.kind {
        crate::ast::AstGenericParameterKind::Lifetime { .. } => GenericParameterKind::Lifetime,
        crate::ast::AstGenericParameterKind::Type { .. } => GenericParameterKind::Type,
        crate::ast::AstGenericParameterKind::IntegerConst { ty, .. } => {
            GenericParameterKind::IntegerConst(integer_type(ty))
        }
    })
}

fn generic_parameter<'a>(
    packages: &'a [PackageDraft<'_>],
    id: GenericParameterId,
) -> Option<&'a crate::ast::AstGenericParameter> {
    let owner = find_item(packages, id.owner)?;
    item_generic_parameters(owner)?
        .parameters
        .get(usize::try_from(id.index).ok()?)
}

fn generic_parameter_depth(
    context: &SymbolicLoweringContext<'_, '_>,
    parameter: GenericParameterId,
) -> SymbolicLoweringResult<u64> {
    let mut owner = context.item;
    let mut depth = 0_u64;
    loop {
        if owner == parameter.owner {
            return Ok(depth);
        }
        owner = find_item(context.packages, owner)
            .and_then(|item| item.owner)
            .ok_or_else(|| {
                SymbolicLoweringError::Frontend(frontend_path_error(
                    FrontendErrorCode::Target,
                    "IDENTITY001",
                    "generic parameter is not owned by the resolved declaration chain",
                ))
            })?;
        depth = depth.checked_add(1).ok_or_else(|| {
            SymbolicLoweringError::Frontend(frontend_path_error(
                FrontendErrorCode::Target,
                "IDENTITY001",
                "generic binder depth exceeds u64",
            ))
        })?;
    }
}

fn canonical_type(ty: &crate::ast::AstType) -> String {
    use crate::ast::{AstScalarType, AstTypeKind};
    match &ty.kind {
        AstTypeKind::Scalar(AstScalarType::Integer(ty)) => integer_type_name(integer_type(*ty)),
        AstTypeKind::Scalar(AstScalarType::F32) => "f32".to_owned(),
        AstTypeKind::Scalar(AstScalarType::F64) => "f64".to_owned(),
        AstTypeKind::Scalar(AstScalarType::Bool) => "bool".to_owned(),
        AstTypeKind::Scalar(AstScalarType::Char) => "char".to_owned(),
        AstTypeKind::Scalar(AstScalarType::Entity) => "entity".to_owned(),
        AstTypeKind::Never => "!".to_owned(),
        AstTypeKind::Unit => "()".to_owned(),
        AstTypeKind::Str => "str".to_owned(),
        AstTypeKind::SelfType => "Self".to_owned(),
        AstTypeKind::Path(path) => canonical_path(path),
        AstTypeKind::Tuple(types) => format!(
            "({})",
            types
                .iter()
                .map(canonical_type)
                .collect::<Vec<_>>()
                .join(",")
        ),
        AstTypeKind::Array { element, length } => format!(
            "[{};{}]",
            canonical_type(element),
            canonical_const_expression(length)
        ),
        AstTypeKind::Slice(element) => format!("[{}]", canonical_type(element)),
        AstTypeKind::Reference {
            lifetime,
            mutable,
            pointee,
        } => format!(
            "&{}{}{}",
            lifetime
                .as_ref()
                .map(|lifetime| format!("'{} ", lifetime.as_str()))
                .unwrap_or_default(),
            if *mutable { "mut " } else { "" },
            canonical_type(pointee)
        ),
        AstTypeKind::RawPointer { mutable, pointee } => format!(
            "*{} {}",
            if *mutable { "mut" } else { "const" },
            canonical_type(pointee)
        ),
        AstTypeKind::FunctionPointer {
            unsafe_,
            parameters,
            effects,
            result,
        } => {
            let mut output = String::new();
            if *unsafe_ {
                output.push_str("unsafe ");
            }
            output.push_str("fn(");
            output.push_str(
                &parameters
                    .iter()
                    .map(canonical_type)
                    .collect::<Vec<_>>()
                    .join(","),
            );
            output.push(')');
            if let Some(requires) = &effects.requires {
                output.push_str(" requires{");
                output.push_str(
                    &requires
                        .members
                        .iter()
                        .map(canonical_path)
                        .collect::<Vec<_>>()
                        .join(","),
                );
                output.push('}');
            }
            if let Some(throws) = &effects.throws {
                output.push_str(" throws{");
                output.push_str(
                    &throws
                        .members
                        .iter()
                        .map(canonical_type)
                        .collect::<Vec<_>>()
                        .join(","),
                );
                output.push('}');
            }
            if let Some(result) = result {
                output.push_str("->");
                output.push_str(&canonical_type(result));
            }
            output
        }
    }
}

fn canonical_path(path: &crate::ast::AstPath) -> String {
    use crate::ast::AstPathRoot;
    let mut output = match &path.root {
        AstPathRoot::Bare => String::new(),
        AstPathRoot::Package => "package".to_owned(),
        AstPathRoot::SelfValue => "self".to_owned(),
        AstPathRoot::SelfType => "Self".to_owned(),
        AstPathRoot::Super(count) => (0..*count).map(|_| "super").collect::<Vec<_>>().join("::"),
        AstPathRoot::Identifier(name) => name.as_str().to_owned(),
    };
    if let Some(arguments) = &path.generic_arguments {
        output.push_str(&canonical_generic_arguments(arguments));
    }
    for segment in &path.segments {
        if !output.is_empty() {
            output.push_str("::");
        }
        output.push_str(segment.name.as_str());
        if let Some(arguments) = &segment.generic_arguments {
            output.push_str(&canonical_generic_arguments(arguments));
        }
    }
    output
}

fn canonical_generic_arguments(arguments: &crate::ast::AstGenericArguments) -> String {
    let values = arguments
        .arguments
        .iter()
        .map(|argument| match &argument.kind {
            crate::ast::AstGenericArgumentKind::Type(ty) => canonical_type(ty),
            crate::ast::AstGenericArgumentKind::Lifetime(lifetime) => {
                format!("'{}", lifetime.as_str())
            }
            crate::ast::AstGenericArgumentKind::IntegerConst(expression) => {
                canonical_const_expression(expression)
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("<{values}>")
}

fn canonical_const_expression(expression: &AstConstExpression) -> String {
    use crate::ast::{AstConstBinaryOperator, AstConstExpressionKind, AstConstUnaryOperator};
    match &expression.kind {
        AstConstExpressionKind::Integer(literal) => literal.raw.to_string(),
        AstConstExpressionKind::Path(path) => canonical_path(path),
        AstConstExpressionKind::Group(child) => {
            format!("({})", canonical_const_expression(child))
        }
        AstConstExpressionKind::Unary { operator, operand } => format!(
            "{}{}",
            match operator {
                AstConstUnaryOperator::Negate => "-",
                AstConstUnaryOperator::BitNot => "~",
            },
            canonical_const_expression(operand)
        ),
        AstConstExpressionKind::Binary {
            operator,
            left,
            right,
        } => format!(
            "{}{}{}",
            canonical_const_expression(left),
            match operator {
                AstConstBinaryOperator::BitOr => "|",
                AstConstBinaryOperator::BitXor => "^",
                AstConstBinaryOperator::BitAnd => "&",
                AstConstBinaryOperator::ShiftLeft => "<<",
                AstConstBinaryOperator::ShiftRight => ">>",
                AstConstBinaryOperator::Add => "+",
                AstConstBinaryOperator::Subtract => "-",
                AstConstBinaryOperator::Multiply => "*",
                AstConstBinaryOperator::Divide => "/",
                AstConstBinaryOperator::Remainder => "%",
            },
            canonical_const_expression(right)
        ),
    }
}

fn integer_type_name(integer_type: IntegerType) -> String {
    match integer_type {
        IntegerType::I8 => "i8",
        IntegerType::I16 => "i16",
        IntegerType::I32 => "i32",
        IntegerType::I64 => "i64",
        IntegerType::Isize => "isize",
        IntegerType::U8 => "u8",
        IntegerType::U16 => "u16",
        IntegerType::U32 => "u32",
        IntegerType::U64 => "u64",
        IntegerType::Usize => "usize",
    }
    .to_owned()
}

fn symbolic_integer_type(integer_type: IntegerType) -> SymbolicType {
    match integer_type {
        IntegerType::I8 => SymbolicType::I8,
        IntegerType::I16 => SymbolicType::I16,
        IntegerType::I32 => SymbolicType::I32,
        IntegerType::I64 => SymbolicType::I64,
        IntegerType::Isize => SymbolicType::Isize,
        IntegerType::U8 => SymbolicType::U8,
        IntegerType::U16 => SymbolicType::U16,
        IntegerType::U32 => SymbolicType::U32,
        IntegerType::U64 => SymbolicType::U64,
        IntegerType::Usize => SymbolicType::Usize,
    }
}

fn resolve_item_declaration_skeleton(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    embedded_core: &VerifiedEmbeddedCoreAuthority,
    item: &ResolvedSymbolicItem,
    resolved_shape: &ResolvedSymbolicShape,
    completed: &BTreeMap<HirItemId, SymbolicDeclarationShapeSkeleton>,
) -> Result<SymbolicDeclarationShapeSkeleton, FrontendError> {
    let package_index = packages
        .iter()
        .position(|package| package.resolved.id == item.module.package())
        .expect("item package exists");
    let target_index = packages[package_index]
        .targets
        .iter()
        .position(|target| target.target.id == item.module.target())
        .expect("item target exists");
    let (_, generics) = generic_environment(packages, item)?;
    let body_context = SymbolicLoweringContext {
        packages,
        graph,
        item: item.id,
        location: PathLocation {
            package_index,
            target_index,
            module: item.module,
            context_item: Some(item.id),
        },
        generics: &generics,
        embedded_core,
        lifetime_domain: LifetimeDomain::BodyLocal,
        contextual_self_template: false,
    };
    let elision_plan = declaration_elision_plan(&body_context, item)?;
    let context = SymbolicLoweringContext {
        lifetime_domain: LifetimeDomain::Declaration(&elision_plan),
        ..body_context
    };
    lower_declaration_shape_skeleton(&context, item, resolved_shape, completed)
}

fn resolve_item_owner_skeleton(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    embedded_core: &VerifiedEmbeddedCoreAuthority,
    item: &ResolvedSymbolicItem,
    definition_shape: &SymbolicDeclarationShapeSkeleton,
) -> Result<SymbolicDefinitionOwnerSkeleton, FrontendError> {
    let package_index = packages
        .iter()
        .position(|package| package.resolved.id == item.module.package())
        .expect("item package exists");
    let target_index = packages[package_index]
        .targets
        .iter()
        .position(|target| target.target.id == item.module.target())
        .expect("item target exists");
    let (_, generics) = generic_environment(packages, item)?;
    let body_context = SymbolicLoweringContext {
        packages,
        graph,
        item: item.id,
        location: PathLocation {
            package_index,
            target_index,
            module: item.module,
            context_item: Some(item.id),
        },
        generics: &generics,
        embedded_core,
        lifetime_domain: LifetimeDomain::BodyLocal,
        contextual_self_template: false,
    };
    let elision_plan = declaration_elision_plan(&body_context, item)?;
    let context = SymbolicLoweringContext {
        lifetime_domain: LifetimeDomain::Declaration(&elision_plan),
        ..body_context
    };
    lower_definition_owner_entry(&context, item, definition_shape)
}

fn lower_declaration_shape_skeleton(
    context: &SymbolicLoweringContext<'_, '_>,
    item: &ResolvedSymbolicItem,
    resolved_shape: &ResolvedSymbolicShape,
    completed: &BTreeMap<HirItemId, SymbolicDeclarationShapeSkeleton>,
) -> Result<SymbolicDeclarationShapeSkeleton, FrontendError> {
    let mut generic_parameters = resolved_shape
        .generic_parameters
        .iter()
        .map(|parameter| parameter.kind.clone())
        .collect::<Vec<_>>();
    generic_parameters.extend(
        resolved_shape
            .hidden_lifetime_binders
            .iter()
            .map(|_| GenericParameterKind::Lifetime),
    );
    let predicates = lower_declaration_predicates(context, item)?;
    let payload = match &item.source {
        HirItemSource::Declaration(declaration) => match &declaration.kind {
            AstDeclarationKind::World { .. } => SymbolicDeclarationPayloadSkeleton::World,
            AstDeclarationKind::Component(record) | AstDeclarationKind::Resource(record) => {
                SymbolicDeclarationPayloadSkeleton::Record(lower_record_shape(
                    context,
                    SymbolicRecordForm::Record,
                    record
                        .fields
                        .iter()
                        .map(|field| (Some(&field.name), &field.ty)),
                )?)
            }
            AstDeclarationKind::Tag => SymbolicDeclarationPayloadSkeleton::Tag,
            AstDeclarationKind::Struct(structure) => {
                SymbolicDeclarationPayloadSkeleton::Record(match &structure.form {
                    crate::ast::AstStructForm::Unit => SymbolicRecordShapeSkeleton {
                        form: SymbolicRecordForm::Unit,
                        fields: Vec::new(),
                    },
                    crate::ast::AstStructForm::Tuple(fields) => lower_record_shape(
                        context,
                        SymbolicRecordForm::Tuple,
                        fields.iter().map(|field| (None, &field.ty)),
                    )?,
                    crate::ast::AstStructForm::Record(fields) => lower_record_shape(
                        context,
                        SymbolicRecordForm::Record,
                        fields.iter().map(|field| (Some(&field.name), &field.ty)),
                    )?,
                })
            }
            AstDeclarationKind::Enum(enumeration) => {
                SymbolicDeclarationPayloadSkeleton::Enum(lower_enum_shape(context, enumeration)?)
            }
            AstDeclarationKind::TypeAlias(alias) => SymbolicDeclarationPayloadSkeleton::Alias {
                target: lower_type_shape(context, &alias.target)?,
            },
            AstDeclarationKind::Const(constant) => SymbolicDeclarationPayloadSkeleton::Const {
                ty: lower_type_shape(context, &constant.ty)?,
            },
            AstDeclarationKind::Static(static_) => SymbolicDeclarationPayloadSkeleton::Static {
                mutable: static_.mutable,
                ty: lower_type_shape(context, &static_.ty)?,
            },
            AstDeclarationKind::Function(function) => {
                SymbolicDeclarationPayloadSkeleton::Callable(Box::new(lower_callable_shape(
                    context,
                    &function.signature.parameters,
                    function.signature.result.as_ref(),
                    function.signature.unsafe_,
                    &function.signature.effects,
                )?))
            }
            AstDeclarationKind::Generator(generator) => {
                SymbolicDeclarationPayloadSkeleton::Callable(Box::new(
                    lower_generator_callable_shape(context, generator)?,
                ))
            }
            AstDeclarationKind::System(system) => {
                let (accesses, implied_requires) = lower_system_shapes(context, system)?;
                SymbolicDeclarationPayloadSkeleton::System {
                    accesses,
                    implied_requires,
                    result: SymbolicTypeShapeSkeleton::resolved(SymbolicType::Unit),
                    effects: lower_effect_sets(context, &system.effects)?,
                }
            }
            AstDeclarationKind::Schedule(_) => SymbolicDeclarationPayloadSkeleton::Schedule {
                effects: SymbolicEffectSetsSkeleton::default(),
                readiness: SymbolicShapeReadiness::PendingC4,
            },
            AstDeclarationKind::Trait(_) => SymbolicDeclarationPayloadSkeleton::Trait {
                methods: lower_owned_method_shapes(context, item, completed)?,
            },
        },
        HirItemSource::Impl(implementation) => SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref: implementation
                .trait_path
                .as_ref()
                .map(|path| lower_trait_reference(context, path))
                .transpose()?,
            target: lower_type_shape(context, &implementation.target)?,
            is_default: implementation.is_default,
            methods: lower_owned_method_shapes(context, item, completed)?,
        },
        HirItemSource::TraitMethod(method) => SymbolicDeclarationPayloadSkeleton::Callable(
            Box::new(lower_method_callable_shape(context, &method.signature)?),
        ),
        HirItemSource::ImplMethod(method) => SymbolicDeclarationPayloadSkeleton::Callable(
            Box::new(lower_method_callable_shape(context, &method.signature)?),
        ),
        HirItemSource::QueryParameter { terms, .. } => SymbolicDeclarationPayloadSkeleton::Query {
            terms: lower_query_terms(context, terms)?,
        },
    };
    let skeleton = SymbolicDeclarationShapeSkeleton {
        generic_parameters,
        predicates,
        payload,
    };
    if let Some(canonical) =
        super::shape::try_canonicalize_declaration_shape(&skeleton).map_err(shape_error)?
    {
        super::shape::encode_declaration_shape_preimage(&canonical).map_err(shape_error)?;
        let readiness = canonical.readiness();
        match super::shape::encode_final_declaration_shape_identity(&canonical) {
            Ok(_) if readiness.final_identity_ready() => {}
            Err(super::shape::ShapeEncodingError::FinalIdentityNeedsCtfe)
                if !readiness.final_identity_ready() => {}
            Ok(_) | Err(_) => {
                return Err(frontend_path_error(
                    FrontendErrorCode::Target,
                    "IDENTITY001",
                    "declaration-shape final-identity readiness gate disagrees with its typed tree",
                ));
            }
        }
    }
    Ok(skeleton)
}

fn lower_definition_owner_entry(
    context: &SymbolicLoweringContext<'_, '_>,
    item: &ResolvedSymbolicItem,
    definition_shape: &SymbolicDeclarationShapeSkeleton,
) -> Result<SymbolicDefinitionOwnerSkeleton, FrontendError> {
    let skeleton = match &item.source {
        HirItemSource::Declaration(declaration) => match &declaration.kind {
            AstDeclarationKind::Trait(_) => Ok(SymbolicDefinitionOwnerSkeleton::Trait {
                path: semantic_declaration_path(context, item.id).map_err(lowering_frontend)?,
                shape: Box::new(definition_shape.clone()),
            }),
            AstDeclarationKind::System(_) => Ok(SymbolicDefinitionOwnerSkeleton::SystemQuery {
                path: semantic_declaration_path(context, item.id).map_err(lowering_frontend)?,
                shape: Box::new(definition_shape.clone()),
            }),
            _ => Ok(SymbolicDefinitionOwnerSkeleton::TopLevel),
        },
        HirItemSource::Impl(implementation) => {
            let generic_parameters = definition_shape.generic_parameters.clone();
            let predicates = definition_shape.predicates.clone();
            let target = lower_type_shape(context, &implementation.target)?;
            match &implementation.trait_path {
                Some(trait_path) => Ok(SymbolicDefinitionOwnerSkeleton::TraitImpl {
                    trait_ref: lower_trait_reference(context, trait_path)?,
                    target,
                    generic_parameters,
                    predicates,
                    is_default: implementation.is_default,
                }),
                None => Ok(SymbolicDefinitionOwnerSkeleton::InherentImpl {
                    target,
                    generic_parameters,
                    predicates,
                }),
            }
        }
        HirItemSource::TraitMethod(_)
        | HirItemSource::ImplMethod(_)
        | HirItemSource::QueryParameter { .. } => Ok(SymbolicDefinitionOwnerSkeleton::TopLevel),
    }?;
    if let Some(canonical) =
        super::shape::try_canonicalize_definition_owner(&skeleton).map_err(shape_error)?
    {
        super::shape::encode_definition_owner_entry(&canonical).map_err(shape_error)?;
        let readiness = canonical.readiness();
        match super::shape::encode_final_definition_owner_identity(&canonical) {
            Ok(_) if readiness.final_identity_ready() => {}
            Err(super::shape::ShapeEncodingError::FinalIdentityNeedsCtfe)
                if !readiness.final_identity_ready() => {}
            Ok(_) | Err(_) => {
                return Err(frontend_path_error(
                    FrontendErrorCode::Target,
                    "IDENTITY001",
                    "owner-shape final-identity readiness gate disagrees with its typed tree",
                ));
            }
        }
    }
    Ok(skeleton)
}

fn lower_type_shape(
    context: &SymbolicLoweringContext<'_, '_>,
    ty: &crate::ast::AstType,
) -> Result<SymbolicTypeShapeSkeleton, FrontendError> {
    Ok(match resolve_symbolic_type(context, ty)? {
        ResolvedSymbolicType::Resolved(value) => SymbolicTypeShapeSkeleton::resolved(*value),
        ResolvedSymbolicType::Pending {
            span,
            reason,
            canonical,
        } => SymbolicTypeShapeSkeleton::pending(
            SymbolicShapeReadiness::PendingC2,
            symbolic_source_span(span),
            pending_shape_kind(&reason),
            canonical,
        ),
    })
}

fn lower_trait_reference(
    context: &SymbolicLoweringContext<'_, '_>,
    path: &crate::ast::AstPath,
) -> Result<SymbolicTypeShapeSkeleton, FrontendError> {
    Ok(match lower_symbolic_path_type(context, path) {
        Ok(value) => SymbolicTypeShapeSkeleton::resolved(value),
        Err(SymbolicLoweringError::Pending { reason, span }) => SymbolicTypeShapeSkeleton::pending(
            SymbolicShapeReadiness::PendingC2,
            symbolic_source_span(span),
            pending_shape_kind(&reason),
            canonical_path(path),
        ),
        Err(SymbolicLoweringError::Frontend(error)) => return Err(error),
    })
}

fn lower_record_shape<'a>(
    context: &SymbolicLoweringContext<'_, '_>,
    form: SymbolicRecordForm,
    fields: impl IntoIterator<Item = (Option<&'a Symbol>, &'a crate::ast::AstType)>,
) -> Result<SymbolicRecordShapeSkeleton, FrontendError> {
    Ok(SymbolicRecordShapeSkeleton {
        form,
        fields: fields
            .into_iter()
            .map(|(name, ty)| {
                Ok(SymbolicFieldShapeSkeleton {
                    name: name.map(|name| name.as_str().to_owned()),
                    ty: lower_type_shape(context, ty)?,
                })
            })
            .collect::<Result<Vec<_>, FrontendError>>()?,
    })
}

fn lower_enum_shape(
    context: &SymbolicLoweringContext<'_, '_>,
    enumeration: &crate::ast::AstEnumDeclaration,
) -> Result<Vec<SymbolicVariantShapeSkeleton>, FrontendError> {
    enumeration
        .variants
        .iter()
        .map(|variant| {
            let record = match &variant.form {
                crate::ast::AstVariantForm::Unit => SymbolicRecordShapeSkeleton {
                    form: SymbolicRecordForm::Unit,
                    fields: Vec::new(),
                },
                crate::ast::AstVariantForm::Tuple(fields) => lower_record_shape(
                    context,
                    SymbolicRecordForm::Tuple,
                    fields.iter().map(|field| (None, field)),
                )?,
                crate::ast::AstVariantForm::Record(fields) => lower_record_shape(
                    context,
                    SymbolicRecordForm::Record,
                    fields.iter().map(|field| (Some(&field.name), &field.ty)),
                )?,
            };
            Ok(SymbolicVariantShapeSkeleton {
                name: variant.name.as_str().to_owned(),
                form: record.form,
                fields: record.fields,
            })
        })
        .collect()
}

fn lower_callable_shape(
    context: &SymbolicLoweringContext<'_, '_>,
    parameters: &[crate::ast::AstParameter],
    result: Option<&crate::ast::AstType>,
    unsafe_: bool,
    effects: &crate::ast::AstEffectSets,
) -> Result<SymbolicCallableShapeSkeleton, FrontendError> {
    Ok(SymbolicCallableShapeSkeleton {
        kind: SymbolicCallableKind::Function,
        parameters: parameters
            .iter()
            .map(|parameter| {
                Ok(SymbolicCallableParameterSkeleton {
                    mode: SymbolicCallableParameterMode::Value,
                    ty: lower_type_shape(context, &parameter.ty)?,
                })
            })
            .collect::<Result<Vec<_>, FrontendError>>()?,
        result: match result {
            Some(result) => lower_type_shape(context, result)?,
            None => SymbolicTypeShapeSkeleton::resolved(SymbolicType::Unit),
        },
        unsafe_,
        resume: None,
        yields: None,
        effects: lower_effect_sets(context, effects)?,
    })
}

fn lower_generator_callable_shape(
    context: &SymbolicLoweringContext<'_, '_>,
    generator: &crate::ast::AstGenerator,
) -> Result<SymbolicCallableShapeSkeleton, FrontendError> {
    Ok(SymbolicCallableShapeSkeleton {
        kind: SymbolicCallableKind::Generator,
        parameters: generator
            .parameters
            .iter()
            .map(|parameter| {
                Ok(SymbolicCallableParameterSkeleton {
                    mode: SymbolicCallableParameterMode::Value,
                    ty: lower_type_shape(context, &parameter.ty)?,
                })
            })
            .collect::<Result<Vec<_>, FrontendError>>()?,
        result: match &generator.result {
            Some(result) => lower_type_shape(context, result)?,
            None => SymbolicTypeShapeSkeleton::resolved(SymbolicType::Unit),
        },
        unsafe_: generator.unsafe_,
        resume: Some(lower_type_shape(context, &generator.resume)?),
        yields: Some(lower_type_shape(context, &generator.yields)?),
        effects: lower_effect_sets(context, &generator.effects)?,
    })
}

fn lower_method_callable_shape(
    context: &SymbolicLoweringContext<'_, '_>,
    signature: &crate::ast::AstMethodSignature,
) -> Result<SymbolicCallableShapeSkeleton, FrontendError> {
    let parameters = signature
        .parameters
        .iter()
        .map(|parameter| match parameter {
            AstMethodParameter::Receiver(receiver) => {
                let mode = match receiver.kind {
                    crate::ast::AstReceiverKind::Value { .. } => {
                        SymbolicCallableParameterMode::ReceiverValue
                    }
                    crate::ast::AstReceiverKind::Reference { mutable: false, .. } => {
                        SymbolicCallableParameterMode::ReceiverShared
                    }
                    crate::ast::AstReceiverKind::Reference { mutable: true, .. } => {
                        SymbolicCallableParameterMode::ReceiverMutable
                    }
                };
                Ok(SymbolicCallableParameterSkeleton {
                    mode,
                    ty: lower_type_shape(context, &receiver_type(receiver))?,
                })
            }
            AstMethodParameter::Parameter(parameter) => Ok(SymbolicCallableParameterSkeleton {
                mode: SymbolicCallableParameterMode::Value,
                ty: lower_type_shape(context, &parameter.ty)?,
            }),
        })
        .collect::<Result<Vec<_>, FrontendError>>()?;
    Ok(SymbolicCallableShapeSkeleton {
        kind: SymbolicCallableKind::Function,
        parameters,
        result: match &signature.result {
            Some(result) => lower_type_shape(context, result)?,
            None => SymbolicTypeShapeSkeleton::resolved(SymbolicType::Unit),
        },
        unsafe_: signature.unsafe_,
        resume: None,
        yields: None,
        effects: lower_effect_sets(context, &signature.effects)?,
    })
}

fn lower_query_terms(
    context: &SymbolicLoweringContext<'_, '_>,
    terms: &[crate::ast::AstQueryTerm],
) -> Result<Vec<SymbolicQueryTermShapeSkeleton>, FrontendError> {
    terms
        .iter()
        .map(|term| {
            Ok(SymbolicQueryTermShapeSkeleton {
                kind: match term.kind {
                    crate::ast::AstQueryTermKind::Read => SymbolicQueryTermKind::Read,
                    crate::ast::AstQueryTermKind::Write => SymbolicQueryTermKind::Write,
                    crate::ast::AstQueryTermKind::Exclude => SymbolicQueryTermKind::Exclude,
                },
                ty: lower_type_shape(context, &term.ty)?,
            })
        })
        .collect()
}

fn lower_system_parameter_shape(
    context: &SymbolicLoweringContext<'_, '_>,
    parameter: &crate::ast::AstSystemParameter,
) -> Result<SymbolicSystemAccessShapeSkeleton, FrontendError> {
    Ok(match &parameter.kind {
        crate::ast::AstSystemParameterKind::Capability(ty) => match &ty.kind {
            crate::ast::AstTypeKind::Reference { mutable: false, .. } => {
                SymbolicSystemAccessShapeSkeleton::CapabilityShared(lower_type_shape(context, ty)?)
            }
            crate::ast::AstTypeKind::Reference { mutable: true, .. } => {
                SymbolicSystemAccessShapeSkeleton::CapabilityMutable(lower_type_shape(context, ty)?)
            }
            _ => {
                return Err(frontend_error(
                    FrontendErrorCode::Target,
                    Diagnostic::at(
                        "TYPE001",
                        ty.span,
                        "system capability access requires a shared or mutable reference type",
                    ),
                ));
            }
        },
        crate::ast::AstSystemParameterKind::ResourceRead(ty) => {
            SymbolicSystemAccessShapeSkeleton::ResourceRead(lower_type_shape(context, ty)?)
        }
        crate::ast::AstSystemParameterKind::ResourceWrite(ty) => {
            SymbolicSystemAccessShapeSkeleton::ResourceWrite(lower_type_shape(context, ty)?)
        }
        crate::ast::AstSystemParameterKind::Query(terms) => {
            SymbolicSystemAccessShapeSkeleton::Query(lower_query_terms(context, terms)?)
        }
        crate::ast::AstSystemParameterKind::Commands => SymbolicSystemAccessShapeSkeleton::Commands,
    })
}

fn symbolic_source_span(span: Span) -> SymbolicSourceSpan {
    SymbolicSourceSpan {
        file: span.file.0,
        start_byte: span.start.byte,
        end_byte: span.end.byte,
        start_line: span.start.line,
        start_column: span.start.column,
        end_line: span.end.line,
        end_column: span.end.column,
    }
}

fn lower_system_shapes(
    context: &SymbolicLoweringContext<'_, '_>,
    system: &crate::ast::AstSystem,
) -> Result<
    (
        Vec<SymbolicSystemAccessShapeSkeleton>,
        Vec<SymbolicImpliedCapabilityRequirementSkeleton>,
    ),
    FrontendError,
> {
    let mut accesses = Vec::with_capacity(system.parameters.len());
    let mut implied_requires = Vec::new();
    for (index, parameter) in system.parameters.iter().enumerate() {
        accesses.push(lower_system_parameter_shape(context, parameter)?);
        let crate::ast::AstSystemParameterKind::Capability(ty) = &parameter.kind else {
            continue;
        };
        let crate::ast::AstTypeKind::Reference {
            mutable, pointee, ..
        } = &ty.kind
        else {
            continue;
        };
        implied_requires.push(SymbolicImpliedCapabilityRequirementSkeleton {
            parameter_ordinal: checked_u64(index, "system parameter count")?,
            parameter_span: symbolic_source_span(parameter.span),
            access: if *mutable {
                SymbolicCapabilityAccessMode::Mutable
            } else {
                SymbolicCapabilityAccessMode::Shared
            },
            referent: lower_type_shape(context, pointee)?,
            readiness: SymbolicShapeReadiness::PendingC4,
        });
    }
    Ok((accesses, implied_requires))
}

fn lower_effect_sets(
    context: &SymbolicLoweringContext<'_, '_>,
    effects: &crate::ast::AstEffectSets,
) -> Result<SymbolicEffectSetsSkeleton, FrontendError> {
    let requires = effects
        .requires
        .as_ref()
        .into_iter()
        .flat_map(|set| &set.members)
        .map(|path| {
            lower_effect_shape(
                context,
                &AstSymbolicEffect::Requires(path.clone()),
                canonical_path(path),
            )
        })
        .collect::<Result<Vec<_>, FrontendError>>()?;
    let throws = effects
        .throws
        .as_ref()
        .into_iter()
        .flat_map(|set| &set.members)
        .map(|ty| {
            lower_effect_shape(
                context,
                &AstSymbolicEffect::Throws(ty.clone()),
                canonical_type(ty),
            )
        })
        .collect::<Result<Vec<_>, FrontendError>>()?;
    Ok(SymbolicEffectSetsSkeleton { requires, throws })
}

fn lower_effect_shape(
    context: &SymbolicLoweringContext<'_, '_>,
    effect: &AstSymbolicEffect,
    debug_spelling: String,
) -> Result<SymbolicEffectShapeSkeleton, FrontendError> {
    Ok(match resolve_symbolic_effect(context, effect)? {
        ResolvedSymbolicEffect::Resolved(effect) => {
            SymbolicEffectShapeSkeleton::resolved_pending_c4(effect.ty)
        }
        ResolvedSymbolicEffect::Pending { span, reason, .. } => {
            SymbolicEffectShapeSkeleton::pending(
                SymbolicShapeReadiness::PendingC2,
                symbolic_source_span(span),
                pending_shape_kind(&reason),
                debug_spelling,
            )
        }
    })
}

fn lower_owned_method_shapes(
    context: &SymbolicLoweringContext<'_, '_>,
    owner: &ResolvedSymbolicItem,
    completed: &BTreeMap<HirItemId, SymbolicDeclarationShapeSkeleton>,
) -> Result<Vec<SymbolicMethodShapeSkeleton>, FrontendError> {
    context
        .packages
        .iter()
        .flat_map(|package| &package.targets)
        .flat_map(|target| &target.target.items)
        .filter(|candidate| candidate.owner == Some(owner.id))
        .map(|method| {
            let shape = completed.get(&method.id).ok_or_else(|| {
                frontend_path_error(
                    FrontendErrorCode::Target,
                    "IDENTITY001",
                    "parent declaration was lowered before its child callable shape",
                )
            })?;
            Ok(SymbolicMethodShapeSkeleton {
                name: method.name.clone().unwrap_or_default(),
                shape: Box::new(shape.clone()),
            })
        })
        .collect()
}

fn lower_declaration_predicates(
    context: &SymbolicLoweringContext<'_, '_>,
    item: &ResolvedSymbolicItem,
) -> Result<Vec<SymbolicPredicateShapeSkeleton>, FrontendError> {
    let mut predicates = Vec::new();
    if let Some(parameters) = item_generic_parameters(item) {
        for (index, parameter) in parameters.parameters.iter().enumerate() {
            match &parameter.kind {
                crate::ast::AstGenericParameterKind::Type { bounds, .. } => {
                    let subject = SymbolicTypeShapeSkeleton::resolved(SymbolicType::BoundType {
                        depth: 0,
                        index: checked_u64(index, "declaration predicate generic parameter")?,
                    });
                    for bound in bounds {
                        predicates.push(lower_type_bound_predicate(context, &subject, bound)?);
                    }
                }
                crate::ast::AstGenericParameterKind::Lifetime { name, outlives } => {
                    if let Some(outlives) = outlives {
                        predicates.push(lower_lifetime_outlives_predicate(
                            context,
                            name,
                            outlives,
                            parameter.span,
                        )?);
                    }
                }
                crate::ast::AstGenericParameterKind::IntegerConst { .. } => {}
            }
        }
    }
    if let Some(clause) = item_where_clause(item) {
        for predicate in &clause.predicates {
            match &predicate.kind {
                crate::ast::AstWherePredicateKind::Type { ty, bounds } => {
                    let subject = lower_type_shape(context, ty)?;
                    for bound in bounds {
                        predicates.push(lower_type_bound_predicate(context, &subject, bound)?);
                    }
                }
                crate::ast::AstWherePredicateKind::Lifetime { lifetime, outlives } => {
                    predicates.push(lower_lifetime_outlives_predicate(
                        context,
                        lifetime,
                        outlives,
                        predicate.span,
                    )?);
                }
            }
        }
    }
    // Source predicates are a mathematical set. Repeating the same bound in
    // inline and `where` syntax is idempotent; the unverified skeleton must be
    // canonical before the ready-only wrapper independently rejects corrupt
    // duplicate rows supplied by API callers.
    predicates.sort();
    predicates.dedup();
    Ok(predicates)
}

fn lower_type_bound_predicate(
    context: &SymbolicLoweringContext<'_, '_>,
    subject: &SymbolicTypeShapeSkeleton,
    bound: &crate::ast::AstTypeBound,
) -> Result<SymbolicPredicateShapeSkeleton, FrontendError> {
    match &bound.kind {
        crate::ast::AstTypeBoundKind::Trait(path) => {
            let trait_ref = lower_trait_reference(context, path)?;
            match (subject, &trait_ref) {
                (
                    SymbolicTypeShapeSkeleton::Resolved {
                        value: self_type, ..
                    },
                    SymbolicTypeShapeSkeleton::Resolved {
                        value:
                            SymbolicType::NominalPath {
                                declaration,
                                arguments,
                            },
                        ..
                    },
                ) => Ok(SymbolicPredicateShapeSkeleton::resolved(
                    SymbolicPredicate::Trait {
                        trait_path: declaration.clone(),
                        self_type: self_type.clone(),
                        arguments: arguments.clone(),
                    },
                )),
                (SymbolicTypeShapeSkeleton::Pending(pending), _)
                | (_, SymbolicTypeShapeSkeleton::Pending(pending)) => {
                    Ok(SymbolicPredicateShapeSkeleton::pending(
                        SymbolicShapeReadiness::PendingC2,
                        pending.source_span,
                        pending.kind,
                        canonical_path(path),
                    ))
                }
                _ => Ok(SymbolicPredicateShapeSkeleton::pending(
                    SymbolicShapeReadiness::PendingC2,
                    symbolic_source_span(bound.span),
                    PendingShapeKind::Predicate,
                    canonical_path(path),
                )),
            }
        }
        crate::ast::AstTypeBoundKind::Lifetime(lifetime) => {
            let SymbolicTypeShapeSkeleton::Resolved { value: ty, .. } = subject else {
                let SymbolicTypeShapeSkeleton::Pending(pending) = subject else {
                    unreachable!("symbolic type skeleton is a closed union")
                };
                return Ok(SymbolicPredicateShapeSkeleton::pending(
                    SymbolicShapeReadiness::PendingC2,
                    pending.source_span,
                    pending.kind,
                    lifetime.as_str(),
                ));
            };
            match lower_symbolic_lifetime(context, Some(lifetime), bound.span) {
                Ok(lifetime) => Ok(SymbolicPredicateShapeSkeleton::resolved(
                    SymbolicPredicate::TypeOutlives {
                        ty: ty.clone(),
                        lifetime,
                    },
                )),
                Err(SymbolicLoweringError::Pending { reason, span }) => {
                    Ok(SymbolicPredicateShapeSkeleton::pending(
                        SymbolicShapeReadiness::PendingC2,
                        symbolic_source_span(span),
                        pending_shape_kind(&reason),
                        lifetime.as_str(),
                    ))
                }
                Err(SymbolicLoweringError::Frontend(error)) => Err(error),
            }
        }
    }
}

fn lower_lifetime_outlives_predicate(
    context: &SymbolicLoweringContext<'_, '_>,
    longer: &Symbol,
    shorter: &Symbol,
    span: Span,
) -> Result<SymbolicPredicateShapeSkeleton, FrontendError> {
    let longer_value = lower_symbolic_lifetime(context, Some(longer), span);
    let shorter_value = lower_symbolic_lifetime(context, Some(shorter), span);
    match (longer_value, shorter_value) {
        (Ok(longer), Ok(shorter)) => Ok(SymbolicPredicateShapeSkeleton::resolved(
            SymbolicPredicate::LifetimeOutlives { longer, shorter },
        )),
        (Err(SymbolicLoweringError::Frontend(error)), _)
        | (_, Err(SymbolicLoweringError::Frontend(error))) => Err(error),
        (Err(SymbolicLoweringError::Pending { reason, span }), _)
        | (_, Err(SymbolicLoweringError::Pending { reason, span })) => {
            Ok(SymbolicPredicateShapeSkeleton::pending(
                SymbolicShapeReadiness::PendingC2,
                symbolic_source_span(span),
                pending_shape_kind(&reason),
                format!("'{}: '{}", longer.as_str(), shorter.as_str()),
            ))
        }
    }
}

fn pending_shape_kind(reason: &UnresolvedPathKind) -> PendingShapeKind {
    match reason {
        UnresolvedPathKind::SelfTypePendingC2 => PendingShapeKind::ContextualSelf,
        UnresolvedPathKind::AssociatedItemPendingC2
        | UnresolvedPathKind::UnknownName
        | UnresolvedPathKind::DependencyHasNoLibraryTarget => PendingShapeKind::PathUse,
        UnresolvedPathKind::GenericFormationPendingC2 => PendingShapeKind::GenericFormation,
        UnresolvedPathKind::AmbiguousNamespace
        | UnresolvedPathKind::ShadowedLocalNeedsLexicalResolution => {
            PendingShapeKind::GenericFormation
        }
    }
}

fn symbolic_mutability(mutable: bool) -> Mutability {
    if mutable {
        Mutability::Mutable
    } else {
        Mutability::Shared
    }
}

fn resolve_symbolic_inputs(
    context: &SymbolicLoweringContext<'_, '_>,
    generic_parameters: Vec<GenericParameterShape>,
    hidden_lifetime_binders: Vec<HiddenLifetimeBinder>,
    inputs: &SymbolicShapeInputs,
) -> Result<ResolvedSymbolicShape, FrontendError> {
    let mut types = Vec::with_capacity(inputs.types.len());
    let mut c2_contextual_self_templates = Vec::new();
    for ty in &inputs.types {
        let value = resolve_symbolic_type(context, ty)?;
        if let ResolvedSymbolicType::Pending {
            span,
            reason: UnresolvedPathKind::SelfTypePendingC2,
            canonical,
        } = &value
        {
            c2_contextual_self_templates
                .push(project_contextual_self_type(context, ty, *span, canonical));
        }
        types.push(value);
    }
    for ty in &inputs.c2_type_roots {
        if let Ok(ResolvedSymbolicType::Pending {
            span,
            reason: UnresolvedPathKind::SelfTypePendingC2,
            canonical,
        }) = resolve_symbolic_type(context, ty)
        {
            let projection = project_contextual_self_type(context, ty, span, canonical.as_str());
            if !c2_contextual_self_templates.contains(&projection) {
                c2_contextual_self_templates.push(projection);
            }
        }
    }
    Ok(ResolvedSymbolicShape {
        generic_parameters,
        hidden_lifetime_binders,
        types,
        consts: inputs
            .consts
            .iter()
            .map(|(expression, integer_type)| {
                resolve_symbolic_const(context, expression, *integer_type)
            })
            .collect::<Result<Vec<_>, _>>()?,
        effects: inputs
            .effects
            .iter()
            .map(|effect| resolve_symbolic_effect(context, effect))
            .collect::<Result<Vec<_>, _>>()?,
        c2_contextual_self_templates,
    })
}

fn project_contextual_self_type(
    context: &SymbolicLoweringContext<'_, '_>,
    ty: &crate::ast::AstType,
    pending_span: Span,
    debug_spelling: &str,
) -> C2ContextualSelfTypeProjection {
    let template_context = SymbolicLoweringContext {
        contextual_self_template: true,
        ..*context
    };
    let result = match lower_symbolic_type(&template_context, ty) {
        Ok(template) => match u64::try_from(count_c2_self_markers(&template)) {
            Ok(hole_count) if hole_count > 0 => Ok(C2ContextualSelfTypeTemplate {
                root_span: symbolic_source_span(ty.span),
                debug_spelling: debug_spelling.to_owned(),
                template: Box::new(template),
                hole_count,
            }),
            Ok(_) | Err(_) => Err(C2TypeTemplateBlocker::FrontendInvariant {
                source_span: Some(symbolic_source_span(ty.span)),
                code: "IDENTITY001".to_owned(),
                message: "contextual-Self template relowering retained no representable hole"
                    .to_owned(),
            }),
        },
        Err(SymbolicLoweringError::Pending { reason, span }) => {
            Err(C2TypeTemplateBlocker::AdditionalPending {
                source_span: symbolic_source_span(span),
                kind: pending_shape_kind(&reason),
                debug_spelling: canonical_type(ty),
            })
        }
        Err(SymbolicLoweringError::Frontend(error)) => {
            Err(C2TypeTemplateBlocker::FrontendInvariant {
                source_span: error.diagnostic.primary.span.map(symbolic_source_span),
                code: error.diagnostic.code.to_owned(),
                message: error.diagnostic.message.clone(),
            })
        }
    };
    C2ContextualSelfTypeProjection {
        pending_span: symbolic_source_span(pending_span),
        debug_spelling: debug_spelling.to_owned(),
        result,
    }
}

fn assert_declaration_shape_has_no_erased_local(
    item: &ResolvedSymbolicItem,
    shape: &ResolvedSymbolicShape,
) -> Result<(), FrontendError> {
    let erased = shape.types.iter().any(|ty| match ty {
        ResolvedSymbolicType::Resolved(ty) => symbolic_type_has_erased_local(ty),
        ResolvedSymbolicType::Pending { .. } => false,
    }) || shape.effects.iter().any(|effect| match effect {
        ResolvedSymbolicEffect::Resolved(effect) => symbolic_type_has_erased_local(&effect.ty),
        ResolvedSymbolicEffect::Pending { .. } => false,
    });
    if erased {
        return Err(frontend_error(
            FrontendErrorCode::Target,
            Diagnostic::at(
                "TYPE001",
                item.span,
                "declaration symbolic shape contains the body-local erased lifetime marker",
            ),
        ));
    }
    Ok(())
}

fn symbolic_type_has_erased_local(ty: &SymbolicType) -> bool {
    match ty {
        SymbolicType::Reference {
            lifetime, pointee, ..
        } => *lifetime == SymbolicLifetime::ErasedLocal || symbolic_type_has_erased_local(pointee),
        SymbolicType::Slice(element)
        | SymbolicType::Array { element, .. }
        | SymbolicType::RawPointer {
            pointee: element, ..
        } => symbolic_type_has_erased_local(element),
        SymbolicType::Tuple(types) => types.iter().any(symbolic_type_has_erased_local),
        SymbolicType::NominalPath { arguments, .. } => {
            arguments.iter().any(generic_argument_has_erased_local)
        }
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
            .any(symbolic_type_has_erased_local),
        SymbolicType::Closure {
            captures,
            parameters,
            result,
            requires,
            throws,
            arguments,
            ..
        } => {
            captures
                .iter()
                .any(|capture| symbolic_type_has_erased_local(&capture.ty))
                || parameters
                    .iter()
                    .chain(std::iter::once(result.as_ref()))
                    .chain(requires.members())
                    .chain(throws.members())
                    .any(symbolic_type_has_erased_local)
                || arguments.iter().any(generic_argument_has_erased_local)
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
            generator_target_has_erased_local(target)
                || captures
                    .iter()
                    .any(|capture| symbolic_type_has_erased_local(&capture.ty))
                || parameters
                    .iter()
                    .chain([resume.as_ref(), yields.as_ref(), result.as_ref()])
                    .chain(requires.members())
                    .chain(throws.members())
                    .any(symbolic_type_has_erased_local)
        }
        SymbolicType::JoinHandle { result, throws } => std::iter::once(result.as_ref())
            .chain(throws.members())
            .any(symbolic_type_has_erased_local),
        SymbolicType::GeneratorFactory {
            target,
            captures,
            parameters,
            produced_generator,
            ..
        } => {
            generator_target_has_erased_local(target)
                || captures
                    .iter()
                    .any(|capture| symbolic_type_has_erased_local(&capture.ty))
                || parameters
                    .iter()
                    .chain(std::iter::once(produced_generator.as_ref()))
                    .any(symbolic_type_has_erased_local)
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
        | SymbolicType::BoundType { .. } => false,
    }
}

fn generic_argument_has_erased_local(argument: &GenericArgumentShape) -> bool {
    match argument {
        GenericArgumentShape::Type(ty) => symbolic_type_has_erased_local(ty),
        GenericArgumentShape::Lifetime(lifetime) => *lifetime == SymbolicLifetime::ErasedLocal,
        GenericArgumentShape::IntegerConst(_) => false,
    }
}

fn generator_target_has_erased_local(target: &super::shape::GeneratorTarget) -> bool {
    let arguments = match target {
        super::shape::GeneratorTarget::Named { arguments, .. }
        | super::shape::GeneratorTarget::Anonymous { arguments, .. } => arguments,
    };
    arguments.iter().any(generic_argument_has_erased_local)
}

fn encode_alpha_symbolic_shape(shape: &ResolvedSymbolicShape) -> Result<Vec<u8>, FrontendError> {
    use super::shape::encode_symbolic_const;
    let mut output = encode_alpha_generic_parameters(&shape.generic_parameters)?;
    output.extend_from_slice(
        &checked_u64(
            shape.hidden_lifetime_binders.len(),
            "hidden declaration lifetime binder list",
        )?
        .to_le_bytes(),
    );
    for binder in &shape.hidden_lifetime_binders {
        output.extend_from_slice(&binder.index.to_le_bytes());
        output.push(match binder.source {
            HiddenLifetimeBinderSource::Receiver => 1,
            HiddenLifetimeBinderSource::Input => 2,
        });
        output.push(u8::from(binder.generator_state));
    }
    push_resolution_list(&mut output, &shape.types, |value| match value {
        ResolvedSymbolicType::Resolved(ty) => {
            super::shape::encode_symbolic_type_skeleton_c1(ty).map_err(shape_error)
        }
        ResolvedSymbolicType::Pending {
            reason, canonical, ..
        } => pending_shape_bytes(reason, canonical),
    })?;
    push_resolution_list(&mut output, &shape.consts, |value| match value {
        ResolvedSymbolicConst::Resolved(value) => encode_symbolic_const(value).map_err(shape_error),
        ResolvedSymbolicConst::Pending {
            reason, canonical, ..
        } => pending_shape_bytes(reason, canonical),
    })?;
    push_resolution_list(&mut output, &shape.effects, |value| match value {
        ResolvedSymbolicEffect::Resolved(value) => {
            super::shape::encode_symbolic_effect_skeleton_c1(value).map_err(shape_error)
        }
        ResolvedSymbolicEffect::Pending {
            reason, canonical, ..
        } => pending_shape_bytes(reason, canonical),
    })?;
    Ok(output)
}

fn encode_alpha_generic_parameters(
    parameters: &[GenericParameterShape],
) -> Result<Vec<u8>, FrontendError> {
    let mut output = Vec::new();
    output.extend_from_slice(
        &checked_u64(parameters.len(), "alpha generic parameter list")?.to_le_bytes(),
    );
    for parameter in parameters {
        match parameter.kind {
            GenericParameterKind::Type => output.push(1),
            GenericParameterKind::Lifetime => output.push(2),
            GenericParameterKind::IntegerConst(integer_type) => {
                output.push(3);
                output.push(integer_type.tag());
            }
        }
    }
    Ok(output)
}

fn item_where_clause(item: &ResolvedSymbolicItem) -> Option<&crate::ast::AstWhereClause> {
    match &item.source {
        HirItemSource::Declaration(declaration) => match &declaration.kind {
            AstDeclarationKind::Component(record) | AstDeclarationKind::Resource(record) => {
                record.where_clause.as_ref()
            }
            AstDeclarationKind::Struct(structure) => structure.where_clause.as_ref(),
            AstDeclarationKind::Enum(enumeration) => enumeration.where_clause.as_ref(),
            AstDeclarationKind::TypeAlias(alias) => alias.where_clause.as_ref(),
            AstDeclarationKind::Function(function) => function.signature.where_clause.as_ref(),
            AstDeclarationKind::Generator(generator) => generator.where_clause.as_ref(),
            AstDeclarationKind::System(system) => system.where_clause.as_ref(),
            AstDeclarationKind::Trait(trait_) => trait_.where_clause.as_ref(),
            AstDeclarationKind::World { .. }
            | AstDeclarationKind::Tag
            | AstDeclarationKind::Const(_)
            | AstDeclarationKind::Static(_)
            | AstDeclarationKind::Schedule(_) => None,
        },
        HirItemSource::Impl(implementation) => implementation.where_clause.as_ref(),
        HirItemSource::TraitMethod(method) => method.signature.where_clause.as_ref(),
        HirItemSource::ImplMethod(method) => method.signature.where_clause.as_ref(),
        HirItemSource::QueryParameter { .. } => None,
    }
}

fn lowering_frontend(error: SymbolicLoweringError) -> FrontendError {
    match error {
        SymbolicLoweringError::Pending { reason, span } => frontend_error(
            FrontendErrorCode::Target,
            Diagnostic::at(
                "IDENTITY001",
                span,
                format!(
                    "owner shape contains unresolved symbolic input: {}",
                    unresolved_path_name(&reason)
                ),
            ),
        ),
        SymbolicLoweringError::Frontend(error) => error,
    }
}

fn generic_environment(
    packages: &[PackageDraft<'_>],
    item: &ResolvedSymbolicItem,
) -> Result<
    (
        Vec<GenericParameterShape>,
        BTreeMap<String, GenericParameterId>,
    ),
    FrontendError,
> {
    let mut environment = if let Some(owner) = item.owner {
        let owner = find_item(packages, owner).expect("owned item parent exists");
        generic_environment(packages, owner)?.1
    } else {
        BTreeMap::new()
    };
    let mut shapes = Vec::new();
    if let Some(parameters) = item_generic_parameters(item) {
        let mut same_scope = BTreeMap::<String, Span>::new();
        for (index, parameter) in parameters.parameters.iter().enumerate() {
            let index = checked_u64(index, "generic parameter count")?;
            let (name, kind) = match &parameter.kind {
                crate::ast::AstGenericParameterKind::Lifetime { name, .. } => {
                    (name.as_str(), GenericParameterKind::Lifetime)
                }
                crate::ast::AstGenericParameterKind::Type { name, .. } => {
                    (name.as_str(), GenericParameterKind::Type)
                }
                crate::ast::AstGenericParameterKind::IntegerConst { name, ty } => (
                    name.as_str(),
                    GenericParameterKind::IntegerConst(integer_type(*ty)),
                ),
            };
            if let Some(previous) = same_scope.get(name) {
                return Err(frontend_error(
                    FrontendErrorCode::Name,
                    Diagnostic::at(
                        "NAME001",
                        parameter.span,
                        format!("duplicate generic parameter `{name}`"),
                    )
                    .with_secondary(*previous, "first declared here"),
                ));
            }
            let replaced = same_scope.insert(name.to_owned(), parameter.span);
            debug_assert!(
                replaced.is_none(),
                "same-scope generic insertion was prechecked"
            );
            if let Some(outer) = environment.get(name).copied() {
                let outer_span = generic_parameter(packages, outer)
                    .expect("active owner-chain generic has a retained parameter row")
                    .span;
                return Err(frontend_error(
                    FrontendErrorCode::Name,
                    Diagnostic::at(
                        "NAME001",
                        parameter.span,
                        format!("generic parameter `{name}` shadows an active owner parameter"),
                    )
                    .with_secondary(outer_span, "first declared here"),
                ));
            }
            shapes.push(GenericParameterShape {
                index,
                name: name.to_owned(),
                kind,
            });
            environment.insert(
                name.to_owned(),
                GenericParameterId {
                    owner: item.id,
                    index,
                },
            );
        }
    }
    Ok((shapes, environment))
}

fn ast_generic_parameter_kind(parameter: &crate::ast::AstGenericParameter) -> GenericParameterKind {
    match &parameter.kind {
        crate::ast::AstGenericParameterKind::Lifetime { .. } => GenericParameterKind::Lifetime,
        crate::ast::AstGenericParameterKind::Type { .. } => GenericParameterKind::Type,
        crate::ast::AstGenericParameterKind::IntegerConst { ty, .. } => {
            GenericParameterKind::IntegerConst(integer_type(*ty))
        }
    }
}

fn item_generic_parameters(
    item: &ResolvedSymbolicItem,
) -> Option<&crate::ast::AstGenericParameters> {
    match &item.source {
        HirItemSource::Declaration(declaration) => match &declaration.kind {
            AstDeclarationKind::Component(record) | AstDeclarationKind::Resource(record) => {
                record.generics.as_ref()
            }
            AstDeclarationKind::Struct(structure) => structure.generics.as_ref(),
            AstDeclarationKind::Enum(enumeration) => enumeration.generics.as_ref(),
            AstDeclarationKind::TypeAlias(alias) => alias.generics.as_ref(),
            AstDeclarationKind::Function(function) => function.signature.generics.as_ref(),
            AstDeclarationKind::Generator(generator) => generator.generics.as_ref(),
            AstDeclarationKind::System(system) => system.generics.as_ref(),
            AstDeclarationKind::Trait(trait_) => trait_.generics.as_ref(),
            AstDeclarationKind::World { .. }
            | AstDeclarationKind::Tag
            | AstDeclarationKind::Const(_)
            | AstDeclarationKind::Static(_)
            | AstDeclarationKind::Schedule(_) => None,
        },
        HirItemSource::Impl(implementation) => implementation.generics.as_ref(),
        HirItemSource::TraitMethod(method) => method.signature.generics.as_ref(),
        HirItemSource::ImplMethod(method) => method.signature.generics.as_ref(),
        HirItemSource::QueryParameter { .. } => None,
    }
}

fn integer_type(ty: crate::ast::AstIntegerType) -> IntegerType {
    match ty {
        crate::ast::AstIntegerType::I8 => IntegerType::I8,
        crate::ast::AstIntegerType::I16 => IntegerType::I16,
        crate::ast::AstIntegerType::I32 => IntegerType::I32,
        crate::ast::AstIntegerType::I64 => IntegerType::I64,
        crate::ast::AstIntegerType::Isize => IntegerType::Isize,
        crate::ast::AstIntegerType::U8 => IntegerType::U8,
        crate::ast::AstIntegerType::U16 => IntegerType::U16,
        crate::ast::AstIntegerType::U32 => IntegerType::U32,
        crate::ast::AstIntegerType::U64 => IntegerType::U64,
        crate::ast::AstIntegerType::Usize => IntegerType::Usize,
    }
}

fn const_expression_integer_type(expression: &AstConstExpression) -> IntegerType {
    use crate::ast::AstConstExpressionKind;
    match &expression.kind {
        AstConstExpressionKind::Integer(literal) => literal
            .suffix
            .map(integer_suffix_type)
            .unwrap_or(IntegerType::Usize),
        AstConstExpressionKind::Group(child)
        | AstConstExpressionKind::Unary { operand: child, .. } => {
            const_expression_integer_type(child)
        }
        AstConstExpressionKind::Binary { left, right, .. } => {
            let left = const_expression_integer_type(left);
            let right = const_expression_integer_type(right);
            if left == IntegerType::Usize {
                right
            } else {
                left
            }
        }
        AstConstExpressionKind::Path(_) => IntegerType::Usize,
    }
}

fn integer_suffix_type(suffix: crate::lexer::IntegerSuffix) -> IntegerType {
    use crate::lexer::IntegerSuffix;
    match suffix {
        IntegerSuffix::I8 => IntegerType::I8,
        IntegerSuffix::I16 => IntegerType::I16,
        IntegerSuffix::I32 => IntegerType::I32,
        IntegerSuffix::I64 => IntegerType::I64,
        IntegerSuffix::Isize => IntegerType::Isize,
        IntegerSuffix::U8 => IntegerType::U8,
        IntegerSuffix::U16 => IntegerType::U16,
        IntegerSuffix::U32 => IntegerType::U32,
        IntegerSuffix::U64 => IntegerType::U64,
        IntegerSuffix::Usize => IntegerType::Usize,
    }
}

fn collect_target_path_resolutions(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    package_index: usize,
    target_index: usize,
    embedded_core: &VerifiedEmbeddedCoreAuthority,
    output: &mut Vec<PathResolution>,
) -> Result<(), FrontendError> {
    let target = &packages[package_index].targets[target_index].target;
    let empty_generics = BTreeMap::new();
    let empty_locals = BTreeMap::new();
    let empty_lexical = LexicalPathEnvironment {
        generics: &empty_generics,
        locals: &empty_locals,
    };
    for module in &target.modules {
        for item in &module.ast.items {
            match item {
                AstItem::Module(module_item) => {
                    if let AstVisibilityKind::In(path) = &module_item.visibility.kind {
                        output.push(resolve_general_path(
                            packages,
                            graph,
                            PathLocation {
                                package_index,
                                target_index,
                                module: module.id,
                                context_item: None,
                            },
                            path,
                            Some(Namespace::Module),
                            empty_lexical,
                            embedded_core,
                        )?);
                    }
                }
                AstItem::Import(import) => output.push(resolve_general_path(
                    packages,
                    graph,
                    PathLocation {
                        package_index,
                        target_index,
                        module: module.id,
                        context_item: None,
                    },
                    &import.path,
                    None,
                    empty_lexical,
                    embedded_core,
                )?),
                AstItem::Declaration(declaration) => {
                    if let AstVisibilityKind::In(path) = &declaration.visibility.kind {
                        output.push(resolve_general_path(
                            packages,
                            graph,
                            PathLocation {
                                package_index,
                                target_index,
                                module: module.id,
                                context_item: None,
                            },
                            path,
                            Some(Namespace::Module),
                            empty_lexical,
                            embedded_core,
                        )?);
                    }
                }
                AstItem::Impl(_) => {}
            }
        }
    }
    for item in &target.items {
        let (_, generics) = generic_environment(packages, item)?;
        let locals = BTreeMap::new();
        for self_use in &item.self_uses {
            output.push(PathResolution {
                span: self_use.span,
                resolutions: vec![Res::Local(self_use.receiver)],
                unresolved: None,
                associated: None,
            });
        }
        for path_use in &item.path_uses {
            if let Some(local) = path_use.lexical_local {
                output.push(PathResolution {
                    span: path_use.path.span,
                    resolutions: vec![Res::Local(local)],
                    unresolved: None,
                    associated: None,
                });
                continue;
            }
            output.push(resolve_general_path(
                packages,
                graph,
                PathLocation {
                    package_index,
                    target_index,
                    module: item.module,
                    context_item: Some(item.id),
                },
                &path_use.path,
                path_use.namespace,
                LexicalPathEnvironment {
                    generics: &generics,
                    locals: &locals,
                },
                embedded_core,
            )?);
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct PathLocation {
    package_index: usize,
    target_index: usize,
    module: HirModuleId,
    context_item: Option<HirItemId>,
}

#[derive(Clone, Copy)]
struct LexicalPathEnvironment<'a> {
    generics: &'a BTreeMap<String, GenericParameterId>,
    locals: &'a BTreeMap<String, Option<LocalId>>,
}

fn unqualified_path_name(path: &crate::ast::AstPath) -> Option<&Symbol> {
    if path.generic_arguments.is_some() {
        return None;
    }
    match &path.root {
        crate::ast::AstPathRoot::Identifier(name) if path.segments.is_empty() => Some(name),
        crate::ast::AstPathRoot::Bare if path.segments.len() == 1 => {
            let segment = &path.segments[0];
            segment.generic_arguments.is_none().then_some(&segment.name)
        }
        _ => None,
    }
}

fn unqualified_prelude_name(path: &crate::ast::AstPath) -> Option<&Symbol> {
    match &path.root {
        crate::ast::AstPathRoot::Identifier(name) if path.segments.is_empty() => Some(name),
        crate::ast::AstPathRoot::Bare if path.segments.len() == 1 => Some(&path.segments[0].name),
        _ => None,
    }
}

enum AssociatedLookup {
    Direct(Res),
    Pending(AssociatedPathResolution),
    Missing,
}

fn associated_path_parts(
    path: &crate::ast::AstPath,
) -> Option<(crate::ast::AstPath, &crate::ast::AstPathSegment)> {
    let member = path.segments.last()?;
    let mut owner = path.clone();
    owner.segments.pop();
    Some((owner, member))
}

fn associated_name_error(path: &crate::ast::AstPath, message: &str) -> FrontendError {
    let span = path
        .segments
        .last()
        .map_or(path.span, |segment| segment.span);
    frontend_error(
        FrontendErrorCode::Name,
        Diagnostic::at("NAME002", span, message),
    )
}

fn visible_child_method_candidates(
    packages: &[PackageDraft<'_>],
    owner: HirItemId,
    member: &str,
    from: HirModuleId,
) -> Vec<AssociatedPathCandidate> {
    let mut candidates = packages
        .iter()
        .flat_map(|package| &package.targets)
        .flat_map(|target| &target.target.items)
        .filter(|item| item.owner == Some(owner) && item.name.as_deref() == Some(member))
        .filter(|item| {
            matches!(
                item.source,
                HirItemSource::TraitMethod(_) | HirItemSource::ImplMethod(_)
            )
        })
        .filter(|item| visibility_allows(packages, &item.declared_visibility, from, item.module))
        .map(|item| AssociatedPathCandidate::Item(HirItemRes::Definition(item.id)))
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    candidates
}

fn visible_embedded_trait_candidates(
    packages: &[PackageDraft<'_>],
    from: HirModuleId,
    member: &str,
    embedded_core: &VerifiedEmbeddedCoreAuthority,
) -> Vec<AssociatedPathCandidate> {
    let module = find_module(packages, from).expect("path module exists");
    let mut candidates = Vec::new();
    for trait_row in embedded_core.projection().traits() {
        let owner = trait_row.definition();
        let Some(definition) = embedded_core.definition(owner) else {
            continue;
        };
        if module.bindings.iter().any(|binding| {
            binding.namespace == Namespace::Type && binding.name == definition.name()
        }) {
            continue;
        }
        candidates.extend(trait_row.methods().iter().filter_map(|method| {
            embedded_core
                .method(*method)
                .filter(|row| row.source_name() == member)
                .map(|method| {
                    AssociatedPathCandidate::Builtin(BuiltinRes {
                        target: BuiltinResTarget::Method(method.id()),
                    })
                })
        }));
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn normal_dependency_reachable(
    graph: &ResolvedGraph,
    from: PackageNodeId,
    to: PackageNodeId,
) -> bool {
    if from == to {
        return true;
    }
    let mut pending = vec![from];
    let mut visited = BTreeSet::new();
    while let Some(package) = pending.pop() {
        if !visited.insert(package) {
            continue;
        }
        for dependency in graph.dependencies.iter().filter(|dependency| {
            dependency.from == package && dependency.kind == LockDependencyKind::Normal
        }) {
            if dependency.to == to {
                return true;
            }
            pending.push(dependency.to);
        }
    }
    false
}

fn impl_target_is_nominal_owner(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    package_index: usize,
    target_index: usize,
    item: &ResolvedSymbolicItem,
    implementation: &AstImpl,
    owner: HirItemId,
) -> Result<bool, FrontendError> {
    let crate::ast::AstTypeKind::Path(path) = &implementation.target.kind else {
        return Ok(false);
    };
    let bindings = resolve_binding_path(
        packages,
        graph,
        package_index,
        target_index,
        item.module,
        path,
        Some(Namespace::Type),
    )?;
    Ok(bindings.into_iter().any(|binding| {
        binding.binding.target == HirBindingTarget::Item(HirItemRes::Definition(owner))
    }))
}

fn nominal_associated_candidates(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    location: PathLocation,
    owner: HirItemId,
    member: &str,
    embedded_core: &VerifiedEmbeddedCoreAuthority,
) -> Result<Vec<AssociatedPathCandidate>, FrontendError> {
    let owner_item = find_item(packages, owner).ok_or_else(|| {
        frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            "associated-path nominal owner is missing from the C1 item arena",
        )
    })?;
    if owner_item.kind == DeclarationKind::Trait {
        return Ok(visible_child_method_candidates(
            packages,
            owner,
            member,
            location.module,
        ));
    }

    let mut candidates = Vec::new();
    let in_scope_module = find_module(packages, location.module).expect("path module exists");
    for trait_owner in in_scope_module
        .bindings
        .iter()
        .filter(|binding| binding.namespace == Namespace::Type)
        .filter_map(|binding| match binding.target {
            HirBindingTarget::Item(HirItemRes::Definition(item))
                if find_item(packages, item)
                    .is_some_and(|item| item.kind == DeclarationKind::Trait) =>
            {
                Some(item)
            }
            HirBindingTarget::Module(_)
            | HirBindingTarget::Item(
                HirItemRes::Definition(_)
                | HirItemRes::NominalConstructor { .. }
                | HirItemRes::EnumVariant { .. },
            ) => None,
        })
    {
        candidates.extend(visible_child_method_candidates(
            packages,
            trait_owner,
            member,
            location.module,
        ));
    }
    candidates.extend(visible_embedded_trait_candidates(
        packages,
        location.module,
        member,
        embedded_core,
    ));
    for (package_index, package) in packages.iter().enumerate() {
        if !normal_dependency_reachable(
            graph,
            packages[location.package_index].resolved.id,
            package.resolved.id,
        ) {
            continue;
        }
        for (target_index, target) in package.targets.iter().enumerate() {
            if package_index == location.package_index {
                if target_index != location.target_index {
                    continue;
                }
            } else if target.target.target != TargetRoot::Library {
                continue;
            }
            for item in &target.target.items {
                let HirItemSource::Impl(implementation) = &item.source else {
                    continue;
                };
                if implementation.trait_path.is_some()
                    || !impl_target_is_nominal_owner(
                        packages,
                        graph,
                        package_index,
                        target_index,
                        item,
                        implementation,
                        owner,
                    )?
                {
                    continue;
                }
                candidates.extend(visible_child_method_candidates(
                    packages,
                    item.id,
                    member,
                    location.module,
                ));
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    Ok(candidates)
}

enum AssociatedTraitOwner {
    User(HirItemId),
    Builtin(VirtualDefinitionId),
}

fn trait_owner_from_path(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    owner_item: &ResolvedSymbolicItem,
    path: &crate::ast::AstPath,
    embedded_core: &VerifiedEmbeddedCoreAuthority,
) -> Result<Option<AssociatedTraitOwner>, FrontendError> {
    let package_index = packages
        .iter()
        .position(|package| package.resolved.id == owner_item.module.package())
        .expect("generic owner package exists");
    let target_index = packages[package_index]
        .targets
        .iter()
        .position(|target| target.target.id == owner_item.module.target())
        .expect("generic owner target exists");
    let mut owners = resolve_binding_path(
        packages,
        graph,
        package_index,
        target_index,
        owner_item.module,
        path,
        Some(Namespace::Type),
    )?
    .into_iter()
    .filter_map(|binding| match binding.binding.target {
        HirBindingTarget::Item(HirItemRes::Definition(item))
            if find_item(packages, item)
                .is_some_and(|item| item.kind == DeclarationKind::Trait) =>
        {
            Some(item)
        }
        HirBindingTarget::Module(_)
        | HirBindingTarget::Item(
            HirItemRes::Definition(_)
            | HirItemRes::NominalConstructor { .. }
            | HirItemRes::EnumVariant { .. },
        ) => None,
    })
    .collect::<Vec<_>>();
    owners.sort();
    owners.dedup();
    if owners.len() == 1 {
        return Ok(Some(AssociatedTraitOwner::User(owners[0])));
    }
    if !owners.is_empty() {
        return Ok(None);
    }
    let Some(name) = unqualified_prelude_name(path) else {
        return Ok(None);
    };
    let Some(owner) =
        embedded_core.lookup_prelude_definition(name.as_str(), VirtualNamespace::Type)
    else {
        return Ok(None);
    };
    Ok(embedded_core
        .definition(owner)
        .is_some_and(|row| row.declaration_kind() == VirtualDeclarationKind::Trait)
        .then_some(AssociatedTraitOwner::Builtin(owner)))
}

fn generic_associated_candidates(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    location: PathLocation,
    parameter: GenericParameterId,
    member: &str,
    embedded_core: &VerifiedEmbeddedCoreAuthority,
) -> Result<Vec<AssociatedPathCandidate>, FrontendError> {
    let owner_item = find_item(packages, parameter.owner).ok_or_else(|| {
        frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            "associated-path generic owner is missing from the C1 item arena",
        )
    })?;
    let parameter_row = generic_parameter(packages, parameter).ok_or_else(|| {
        frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            "associated-path generic parameter is missing from its owner",
        )
    })?;
    let (parameter_name, inline_bounds) = match &parameter_row.kind {
        crate::ast::AstGenericParameterKind::Type { name, bounds } => {
            (name.as_str(), bounds.as_slice())
        }
        crate::ast::AstGenericParameterKind::Lifetime { .. }
        | crate::ast::AstGenericParameterKind::IntegerConst { .. } => return Ok(Vec::new()),
    };
    let mut trait_paths = inline_bounds
        .iter()
        .filter_map(|bound| match &bound.kind {
            crate::ast::AstTypeBoundKind::Trait(path) => Some(path.clone()),
            crate::ast::AstTypeBoundKind::Lifetime(_) => None,
        })
        .collect::<Vec<_>>();
    if let Some(where_clause) = item_where_clause(owner_item) {
        for predicate in &where_clause.predicates {
            let crate::ast::AstWherePredicateKind::Type { ty, bounds } = &predicate.kind else {
                continue;
            };
            let crate::ast::AstTypeKind::Path(subject) = &ty.kind else {
                continue;
            };
            if unqualified_path_name(subject).map(Symbol::as_str) != Some(parameter_name) {
                continue;
            }
            trait_paths.extend(bounds.iter().filter_map(|bound| match &bound.kind {
                crate::ast::AstTypeBoundKind::Trait(path) => Some(path.clone()),
                crate::ast::AstTypeBoundKind::Lifetime(_) => None,
            }));
        }
    }

    let mut candidates = Vec::new();
    for trait_path in trait_paths {
        if let Some(trait_owner) =
            trait_owner_from_path(packages, graph, owner_item, &trait_path, embedded_core)?
        {
            match trait_owner {
                AssociatedTraitOwner::User(trait_owner) => {
                    candidates.extend(visible_child_method_candidates(
                        packages,
                        trait_owner,
                        member,
                        location.module,
                    ));
                }
                AssociatedTraitOwner::Builtin(trait_owner) => {
                    if let Some(method) = embedded_core.lookup_method(Some(trait_owner), member) {
                        candidates.push(AssociatedPathCandidate::Builtin(BuiltinRes {
                            target: BuiltinResTarget::Method(method),
                        }));
                    }
                }
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    Ok(candidates)
}

fn contextual_self_candidates(
    packages: &[PackageDraft<'_>],
    context: HirItemId,
    member: &str,
    from: HirModuleId,
) -> Result<Vec<AssociatedPathCandidate>, FrontendError> {
    let context_item = find_item(packages, context).ok_or_else(|| {
        frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            "associated Self context is missing from the C1 item arena",
        )
    })?;
    let owner = match &context_item.source {
        HirItemSource::TraitMethod(_) | HirItemSource::ImplMethod(_) => context_item.owner,
        HirItemSource::Declaration(declaration)
            if matches!(declaration.kind, AstDeclarationKind::Trait(_)) =>
        {
            Some(context_item.id)
        }
        HirItemSource::Impl(_) => Some(context_item.id),
        HirItemSource::Declaration(_) | HirItemSource::QueryParameter { .. } => None,
    };
    let Some(owner) = owner else {
        return Ok(Vec::new());
    };
    let owner_item = find_item(packages, owner).ok_or_else(|| {
        frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            "associated Self owner is missing from the C1 item arena",
        )
    })?;
    if !matches!(owner_item.source, HirItemSource::Impl(_)) {
        return Ok(visible_child_method_candidates(
            packages, owner, member, from,
        ));
    }
    let mut candidates = packages
        .iter()
        .flat_map(|package| &package.targets)
        .flat_map(|target| &target.target.items)
        .filter(|item| {
            matches!(item.source, HirItemSource::ImplMethod(_))
                && item.name.as_deref() == Some(member)
        })
        .filter(|item| {
            item.owner
                .and_then(|candidate_owner| find_item(packages, candidate_owner))
                .is_some_and(|candidate_owner| {
                    candidate_owner.owner_shape == owner_item.owner_shape
                })
        })
        .filter(|item| visibility_allows(packages, &item.declared_visibility, from, item.module))
        .map(|item| AssociatedPathCandidate::Item(HirItemRes::Definition(item.id)))
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    Ok(candidates)
}

fn contextual_self_nominal_owner(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    context: HirItemId,
) -> Result<Option<HirItemId>, FrontendError> {
    let context_item = find_item(packages, context).ok_or_else(|| {
        frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            "associated Self context is missing from the C1 item arena",
        )
    })?;
    let implementation = match &context_item.source {
        HirItemSource::Impl(implementation) => Some((context_item, implementation)),
        HirItemSource::ImplMethod(_) => context_item
            .owner
            .and_then(|owner| find_item(packages, owner))
            .and_then(|owner| match &owner.source {
                HirItemSource::Impl(implementation) => Some((owner, implementation)),
                HirItemSource::Declaration(_)
                | HirItemSource::TraitMethod(_)
                | HirItemSource::ImplMethod(_)
                | HirItemSource::QueryParameter { .. } => None,
            }),
        HirItemSource::Declaration(_)
        | HirItemSource::TraitMethod(_)
        | HirItemSource::QueryParameter { .. } => None,
    };
    let Some((implementation_item, implementation)) = implementation else {
        return Ok(None);
    };
    let crate::ast::AstTypeKind::Path(target) = &implementation.target.kind else {
        return Ok(None);
    };
    let package_index = packages
        .iter()
        .position(|package| package.resolved.id == implementation_item.module.package())
        .expect("implementation package exists");
    let target_index = packages[package_index]
        .targets
        .iter()
        .position(|target| target.target.id == implementation_item.module.target())
        .expect("implementation target exists");
    let mut owners = resolve_binding_path(
        packages,
        graph,
        package_index,
        target_index,
        implementation_item.module,
        target,
        Some(Namespace::Type),
    )?
    .into_iter()
    .filter_map(|binding| match binding.binding.target {
        HirBindingTarget::Item(HirItemRes::Definition(owner)) => Some(owner),
        HirBindingTarget::Module(_)
        | HirBindingTarget::Item(
            HirItemRes::NominalConstructor { .. } | HirItemRes::EnumVariant { .. },
        ) => None,
    })
    .collect::<Vec<_>>();
    owners.sort();
    owners.dedup();
    Ok(match owners.as_slice() {
        [owner] => Some(*owner),
        [] | [_, _, ..] => None,
    })
}

fn contextual_self_enum_variant(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    context: HirItemId,
    member: &str,
) -> Result<Option<Res>, FrontendError> {
    let Some(owner) = contextual_self_nominal_owner(packages, graph, context)? else {
        return Ok(None);
    };
    let Some(owner_item) = find_item(packages, owner) else {
        return Ok(None);
    };
    let HirItemSource::Declaration(declaration) = &owner_item.source else {
        return Ok(None);
    };
    let AstDeclarationKind::Enum(enumeration) = &declaration.kind else {
        return Ok(None);
    };
    let Some((ordinal, _)) = enumeration
        .variants
        .iter()
        .enumerate()
        .find(|(_, variant)| variant.name.as_str() == member)
    else {
        return Ok(None);
    };
    Ok(Some(Res::Item(HirItemRes::EnumVariant {
        owner,
        ordinal: checked_u64(ordinal, "enum variant count")?,
    })))
}

fn resolve_associated_path(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    location: PathLocation,
    path: &crate::ast::AstPath,
    namespace: Option<Namespace>,
    lexical: LexicalPathEnvironment<'_>,
    embedded_core: &VerifiedEmbeddedCoreAuthority,
) -> Result<Option<AssociatedLookup>, FrontendError> {
    if namespace.is_some_and(|namespace| namespace != Namespace::Value) {
        return Ok(None);
    }
    let Some((owner_path, member)) = associated_path_parts(path) else {
        return Ok(None);
    };

    let (owner, candidates) = if matches!(owner_path.root, crate::ast::AstPathRoot::SelfType)
        && owner_path.segments.is_empty()
    {
        let Some(context) = location.context_item else {
            return Err(associated_name_error(
                path,
                "`Self` associated path has no trait or impl context",
            ));
        };
        let candidates =
            contextual_self_candidates(packages, context, member.name.as_str(), location.module)?;
        if let Some(variant) =
            contextual_self_enum_variant(packages, graph, context, member.name.as_str())?
        {
            if candidates.is_empty() {
                return Ok(Some(AssociatedLookup::Direct(variant)));
            }
            return Err(associated_name_error(
                path,
                "contextual Self member has both an enum variant and method candidates",
            ));
        }
        (AssociatedPathOwner::ContextualSelf { context }, candidates)
    } else if let Some(name) = unqualified_path_name(&owner_path) {
        let generic_parameter = lexical.generics.get(name.as_str()).copied();
        let type_parameter = generic_parameter
            .map(|parameter| {
                generic_parameter_kind(packages, parameter)
                    .map(|kind| (kind == GenericParameterKind::Type).then_some(parameter))
                    .ok_or_else(|| {
                        frontend_error(
                            FrontendErrorCode::Target,
                            Diagnostic::at(
                                "IDENTITY001",
                                owner_path.span,
                                "associated-path generic owner has no retained parameter authority",
                            ),
                        )
                    })
            })
            .transpose()?
            .flatten();
        if let Some(parameter) = type_parameter {
            (
                AssociatedPathOwner::Generic(parameter),
                generic_associated_candidates(
                    packages,
                    graph,
                    location,
                    parameter,
                    member.name.as_str(),
                    embedded_core,
                )?,
            )
        } else {
            let bindings = resolve_binding_path(
                packages,
                graph,
                location.package_index,
                location.target_index,
                location.module,
                &owner_path,
                Some(Namespace::Type),
            )?;
            let mut nominal = bindings
                .into_iter()
                .filter_map(|binding| match binding.binding.target {
                    HirBindingTarget::Item(HirItemRes::Definition(item)) => Some(item),
                    HirBindingTarget::Module(_)
                    | HirBindingTarget::Item(
                        HirItemRes::NominalConstructor { .. } | HirItemRes::EnumVariant { .. },
                    ) => None,
                })
                .collect::<Vec<_>>();
            nominal.sort();
            nominal.dedup();
            if nominal.len() == 1 {
                let owner = nominal[0];
                (
                    AssociatedPathOwner::Nominal(owner),
                    nominal_associated_candidates(
                        packages,
                        graph,
                        location,
                        owner,
                        member.name.as_str(),
                        embedded_core,
                    )?,
                )
            } else if nominal.is_empty() {
                let Some(embedded_owner) =
                    embedded_core.lookup_prelude_definition(name.as_str(), VirtualNamespace::Type)
                else {
                    return Ok(None);
                };
                let mut direct = Vec::new();
                if let Some(variant) =
                    embedded_core.lookup_enum_variant(embedded_owner, member.name.as_str())
                {
                    direct.push(Res::Builtin(BuiltinRes {
                        target: BuiltinResTarget::EnumVariant(variant),
                    }));
                }
                if let Some(method) =
                    embedded_core.lookup_method(Some(embedded_owner), member.name.as_str())
                {
                    direct.push(Res::Builtin(BuiltinRes {
                        target: BuiltinResTarget::Method(method),
                    }));
                }
                return match direct.as_slice() {
                    [resolution] => Ok(Some(AssociatedLookup::Direct(resolution.clone()))),
                    [] => Ok(Some(AssociatedLookup::Missing)),
                    _ => Err(associated_name_error(
                        path,
                        "embedded associated path has more than one viable member",
                    )),
                };
            } else {
                return Err(associated_name_error(
                    path,
                    "associated path has more than one viable type owner",
                ));
            }
        }
    } else {
        let bindings = resolve_binding_path(
            packages,
            graph,
            location.package_index,
            location.target_index,
            location.module,
            &owner_path,
            Some(Namespace::Type),
        )?;
        let mut nominal = bindings
            .into_iter()
            .filter_map(|binding| match binding.binding.target {
                HirBindingTarget::Item(HirItemRes::Definition(item)) => Some(item),
                HirBindingTarget::Module(_)
                | HirBindingTarget::Item(
                    HirItemRes::NominalConstructor { .. } | HirItemRes::EnumVariant { .. },
                ) => None,
            })
            .collect::<Vec<_>>();
        nominal.sort();
        nominal.dedup();
        if nominal.len() != 1 {
            return Ok(None);
        }
        let owner = nominal[0];
        (
            AssociatedPathOwner::Nominal(owner),
            nominal_associated_candidates(
                packages,
                graph,
                location,
                owner,
                member.name.as_str(),
                embedded_core,
            )?,
        )
    };

    if candidates.is_empty() {
        return Ok(Some(AssociatedLookup::Missing));
    }
    Ok(Some(AssociatedLookup::Pending(AssociatedPathResolution {
        owner,
        member: member.name.as_str().to_owned(),
        member_span: member.span,
        path_span: path.span,
        candidates,
    })))
}

fn resolve_general_path(
    packages: &[PackageDraft<'_>],
    graph: &ResolvedGraph,
    location: PathLocation,
    path: &crate::ast::AstPath,
    namespace: Option<Namespace>,
    lexical: LexicalPathEnvironment<'_>,
    embedded_core: &VerifiedEmbeddedCoreAuthority,
) -> Result<PathResolution, FrontendError> {
    if let Some(name) = unqualified_path_name(path) {
        if namespace == Some(Namespace::Value) {
            if let Some(local) = lexical.locals.get(name.as_str()) {
                return Ok(match local {
                    Some(local) => PathResolution {
                        span: path.span,
                        resolutions: vec![Res::Local(*local)],
                        unresolved: None,
                        associated: None,
                    },
                    None => PathResolution {
                        span: path.span,
                        resolutions: Vec::new(),
                        unresolved: Some(UnresolvedPathKind::ShadowedLocalNeedsLexicalResolution),
                        associated: None,
                    },
                });
            }
        }
        if let Some(generic) = lexical.generics.get(name.as_str()) {
            let parameter = generic_parameter(packages, *generic).ok_or_else(|| {
                frontend_error(
                    FrontendErrorCode::Target,
                    Diagnostic::at(
                        "IDENTITY001",
                        path.span,
                        "active generic path has no retained parameter authority",
                    ),
                )
            })?;
            let generic_namespace = match ast_generic_parameter_kind(parameter) {
                GenericParameterKind::Type => Some(Namespace::Type),
                GenericParameterKind::IntegerConst(_) => Some(Namespace::Value),
                GenericParameterKind::Lifetime => None,
            };
            if generic_namespace == namespace {
                return Ok(PathResolution {
                    span: path.span,
                    resolutions: vec![Res::Generic(*generic)],
                    unresolved: None,
                    associated: None,
                });
            }
        }
    }
    if matches!(path.root, crate::ast::AstPathRoot::SelfType) && path.segments.is_empty() {
        return Ok(PathResolution {
            span: path.span,
            resolutions: Vec::new(),
            unresolved: Some(UnresolvedPathKind::SelfTypePendingC2),
            associated: None,
        });
    }
    let bindings = resolve_binding_path(
        packages,
        graph,
        location.package_index,
        location.target_index,
        location.module,
        path,
        namespace,
    )?;
    let associated = resolve_associated_path(
        packages,
        graph,
        location,
        path,
        namespace,
        lexical,
        embedded_core,
    )?;
    if !bindings.is_empty() {
        if matches!(
            associated.as_ref(),
            Some(AssociatedLookup::Direct(_) | AssociatedLookup::Pending(_))
        ) {
            return Err(associated_name_error(
                path,
                "path has more than one viable module/type-associated partition",
            ));
        }
        let mut resolutions = bindings
            .into_iter()
            .map(|binding| match binding.binding.target {
                HirBindingTarget::Module(module) => Res::Module(module),
                HirBindingTarget::Item(item) => Res::Item(item),
            })
            .collect::<Vec<_>>();
        resolutions.sort();
        resolutions.dedup();
        return Ok(PathResolution {
            span: path.span,
            resolutions,
            unresolved: None,
            associated: None,
        });
    }
    if let Some(associated) = associated {
        return Ok(match associated {
            AssociatedLookup::Direct(resolution) => PathResolution {
                span: path.span,
                resolutions: vec![resolution],
                unresolved: None,
                associated: None,
            },
            AssociatedLookup::Pending(associated) => PathResolution {
                span: path.span,
                resolutions: Vec::new(),
                unresolved: Some(UnresolvedPathKind::AssociatedItemPendingC2),
                associated: Some(associated),
            },
            AssociatedLookup::Missing => {
                return Err(associated_name_error(
                    path,
                    "type has no visible associated function, static method, or enum variant with this name",
                ));
            }
        });
    }
    if let Some(name) = unqualified_prelude_name(path) {
        let namespaces: &[VirtualNamespace] = match namespace {
            Some(Namespace::Type) => &[VirtualNamespace::Type],
            Some(Namespace::Value) => &[VirtualNamespace::Value],
            Some(Namespace::Module) => &[],
            None => &[VirtualNamespace::Type, VirtualNamespace::Value],
        };
        let mut resolutions = namespaces
            .iter()
            .filter_map(|namespace| embedded_core.lookup_prelude(name.as_str(), *namespace))
            .map(|target| {
                Res::Builtin(BuiltinRes {
                    target: BuiltinResTarget::Prelude(target),
                })
            })
            .collect::<Vec<_>>();
        if namespace.is_none_or(|namespace| namespace == Namespace::Value) {
            if let Some(owner) = embedded_core.lookup_record_constructor(name.as_str()) {
                resolutions.push(Res::Builtin(BuiltinRes {
                    target: BuiltinResTarget::RecordConstructor(owner),
                }));
            }
        }
        resolutions.sort();
        resolutions.dedup();
        if !resolutions.is_empty() {
            return Ok(PathResolution {
                span: path.span,
                resolutions,
                unresolved: None,
                associated: None,
            });
        }
    }
    Ok(PathResolution {
        span: path.span,
        resolutions: Vec::new(),
        unresolved: Some(UnresolvedPathKind::UnknownName),
        associated: None,
    })
}

fn build_inventory(
    _workspace: &Workspace,
    graph: &ResolvedGraph,
    packages: &[PackageDraft<'_>],
    sources: &SourceDatabase,
    embedded_core: Arc<VerifiedEmbeddedCoreAuthority>,
) -> Result<WorkspaceInventorySkeleton, FrontendError> {
    let mut builder = SemanticInventoryBuilder::new(embedded_core);
    let mut source_digests = BTreeMap::new();
    for package in packages {
        let mut entries = sources.source_entries(package.resolved.id);
        let manifest_entry = &package.source.manifest().source_entry;
        match entries.binary_search_by(|entry| entry.path.cmp(&manifest_entry.path)) {
            Ok(index) if entries[index] != *manifest_entry => {
                return Err(frontend_path_error(
                    FrontendErrorCode::Source,
                    "SOURCE005",
                    format!(
                        "retained manifest source `{}` for `{}` differs from its parsed manifest commitment",
                        manifest_entry.path, package.resolved.name
                    ),
                ));
            }
            Ok(_) => {}
            Err(index) => entries.insert(index, manifest_entry.clone()),
        }
        let digest = source_tree_digest(&entries).map_err(|diagnostics| {
            frontend_path_error(
                FrontendErrorCode::Source,
                "SOURCE005",
                format!("could not commit package source tree: {diagnostics}"),
            )
        })?;
        if let ResolvedSource::Registry {
            source_digest: expected,
            ..
        } = &package.resolved.source
        {
            if digest != *expected {
                return Err(frontend_path_error(
                    FrontendErrorCode::Source,
                    "SOURCE005",
                    format!(
                        "retained registry source tree for `{}` has digest `{digest}` instead of selected `{expected}`",
                        package.resolved.name
                    ),
                ));
            }
        }
        source_digests.insert(package.resolved.id, digest);
        builder.push_source_tree(package.resolved.package_id, *digest.as_bytes());
    }
    for root in &graph.roots {
        let package = graph.package(*root).expect("validated graph root exists");
        builder.push_workspace_root(package.package_id);
    }
    for package in packages {
        let source_digest = source_digests[&package.resolved.id];
        let source = match &package.resolved.source {
            ResolvedSource::Workspace { relative_path } => PackageSourceSkeleton::Workspace {
                path: relative_path.as_str().to_owned(),
                source_digest: *source_digest.as_bytes(),
            },
            ResolvedSource::Registry {
                archive_digest,
                source_digest,
                provenance_record_digest,
                inclusion_record_digest,
                manifest_span: _,
            } => PackageSourceSkeleton::Registry {
                archive_digest: *archive_digest.as_bytes(),
                source_digest: *source_digest.as_bytes(),
                provenance_record_digest: *provenance_record_digest.as_bytes(),
                inclusion_record_digest: *inclusion_record_digest.as_bytes(),
            },
        };
        let dependencies = graph
            .dependencies
            .iter()
            .filter(|dependency| dependency.from == package.resolved.id)
            .map(|dependency| {
                let target = graph
                    .package(dependency.to)
                    .expect("validated dependency target exists");
                PackageDependencySkeleton {
                    alias: dependency.alias.as_str().to_owned(),
                    package: target.package_id,
                    requirement: dependency.requirement.as_str().to_owned(),
                    kind: match dependency.kind {
                        LockDependencyKind::Normal => DependencyKind::Normal,
                        LockDependencyKind::Development => DependencyKind::Development,
                    },
                }
            })
            .collect();
        let mut target_rows = Vec::new();
        let mut module_rows = Vec::new();
        let mut definition_rows = Vec::new();
        let mut body_rows = Vec::new();
        for target in &package.targets {
            target_rows.push(SemanticTargetInventorySkeleton {
                manifest_ordinal: target.manifest_ordinal,
                target_id: target.target.id,
                target: target.target.target.clone(),
                root_module: target.target.modules[0].id,
                contract: inventory_contract(packages, &target.target.contract)?,
            });
            for module in &target.target.modules {
                let module_ref = module_ref(packages, module.id)?;
                let bindings = module
                    .bindings
                    .iter()
                    .filter(|binding| {
                        matches!(binding.origin, HirBindingOrigin::Declaration)
                            || binding.declared_visibility == Visibility::Public
                    })
                    .map(|binding| inventory_binding(packages, binding))
                    .collect::<Result<Vec<_>, FrontendError>>()?;
                module_rows.push(SemanticModuleInventorySkeleton {
                    hir_module: module.id,
                    module: module_ref,
                    file: module.file,
                    declared_visibility: module.declared_visibility.clone(),
                    bindings,
                });
            }
            for item in &target.target.items {
                definition_rows.push(SemanticDefinitionInventorySkeleton {
                    hir_item: item.id,
                    key: item_key(packages, item)?,
                    declared_visibility: item.declared_visibility.clone(),
                    member_visibilities: item.member_visibilities.clone(),
                    symbolic_shape: item.definition_shape.clone(),
                });
            }
            for body in &target.target.bodies {
                let owner = find_item(packages, body.owner).expect("body owner item exists");
                let owner_key = item_key(packages, owner)?;
                body_rows.push(SemanticBodyInventorySkeleton {
                    hir_body: body.id,
                    key: SemanticBodyKey {
                        package: package.resolved.package_id,
                        target: target.target.target.clone(),
                        modules: owner_key.module.path.clone(),
                        declaration_kind: owner.kind,
                        declaration_name: owner.name.clone(),
                        declaration_span: owner.span,
                        body_kind: body.kind,
                        ordinal: body.ordinal,
                        body_span: body.span,
                    },
                });
            }
        }
        let budgets = package.source.manifest().const_eval;
        builder.push_package(SemanticPackageInventorySkeleton {
            package_node: package.resolved.id,
            package: package.resolved.package_id,
            provenance: PackageProvenanceSkeleton {
                registry_origin: graph.registry_identity.clone(),
                scoped_name: package.resolved.name.to_string(),
                version: package.resolved.version.to_string(),
                source,
                dependencies,
            },
            ctfe_budgets: CtfeBudgetsSkeleton {
                step_limit: budgets.steps,
                depth_limit: budgets.call_depth,
                heap_limit: budgets.heap_bytes,
            },
            targets: target_rows,
            modules: module_rows,
            definitions: definition_rows,
            bodies: body_rows,
        });
    }
    builder.finish().map_err(|error| {
        frontend_path_error(FrontendErrorCode::Target, "INVENTORY001", error.to_string())
    })
}

fn inventory_contract(
    packages: &[PackageDraft<'_>],
    contract: &ResolvedTargetContract,
) -> Result<SemanticTargetContractSkeleton, FrontendError> {
    Ok(match contract {
        ResolvedTargetContract::Library => SemanticTargetContractSkeleton::Library,
        ResolvedTargetContract::Binary {
            root_world,
            main,
            capabilities,
        } => SemanticTargetContractSkeleton::Binary {
            root_world: Box::new(item_key(
                packages,
                find_item(packages, *root_world).expect("linked world item exists"),
            )?),
            main: Box::new(item_key(
                packages,
                find_item(packages, *main).expect("linked main item exists"),
            )?),
            capabilities: capabilities.clone(),
        },
        ResolvedTargetContract::Environment {
            root_world,
            profile,
            reset,
            step,
            self_play,
        } => SemanticTargetContractSkeleton::Environment {
            root_world: Box::new(item_key(
                packages,
                find_item(packages, *root_world).expect("linked world item exists"),
            )?),
            profile: profile.clone(),
            reset: Box::new(item_key(
                packages,
                find_item(packages, *reset).expect("linked reset item exists"),
            )?),
            step: Box::new(item_key(
                packages,
                find_item(packages, *step).expect("linked step item exists"),
            )?),
            self_play: Box::new(item_key(
                packages,
                find_item(packages, *self_play).expect("linked self-play item exists"),
            )?),
        },
        ResolvedTargetContract::Pending => {
            return Err(frontend_path_error(
                FrontendErrorCode::Target,
                "TARGET015",
                "semantic target contract was not linked",
            ));
        }
    })
}

fn inventory_binding(
    packages: &[PackageDraft<'_>],
    binding: &HirBinding,
) -> Result<SemanticBindingInventorySkeleton, FrontendError> {
    let target = inventory_binding_target(packages, &binding.target)?;
    let origin = match &binding.origin {
        HirBindingOrigin::Declaration => SemanticBindingOrigin::Declaration,
        HirBindingOrigin::ReExport {
            source_module,
            source_segments,
            target: origin_target,
        } => SemanticBindingOrigin::ReExport {
            source: SemanticBindingPath {
                module: module_ref(packages, *source_module)?,
                segments: source_segments.clone(),
                namespace: binding.namespace,
            },
            target: Box::new(inventory_binding_target(packages, origin_target)?),
        },
    };
    Ok(SemanticBindingInventorySkeleton {
        name: binding.name.clone(),
        namespace: binding.namespace,
        target,
        declared_visibility: binding.declared_visibility.clone(),
        origin,
    })
}

fn inventory_binding_target(
    packages: &[PackageDraft<'_>],
    target: &HirBindingTarget,
) -> Result<SemanticBindingTarget, FrontendError> {
    Ok(match target {
        HirBindingTarget::Module(module) => {
            SemanticBindingTarget::Module(module_ref(packages, *module)?)
        }
        HirBindingTarget::Item(HirItemRes::Definition(item)) => {
            SemanticBindingTarget::Definition(binding_owner_key(packages, *item)?)
        }
        HirBindingTarget::Item(HirItemRes::NominalConstructor { owner }) => {
            SemanticBindingTarget::NominalConstructor(binding_owner_key(packages, *owner)?)
        }
        HirBindingTarget::Item(HirItemRes::EnumVariant { owner, ordinal }) => {
            SemanticBindingTarget::EnumVariant {
                owner: binding_owner_key(packages, *owner)?,
                ordinal: *ordinal,
            }
        }
    })
}

fn binding_owner_key(
    packages: &[PackageDraft<'_>],
    owner: HirItemId,
) -> Result<SemanticDefinitionKey, FrontendError> {
    item_key(
        packages,
        find_item(packages, owner).expect("binding target item exists"),
    )
}

fn module_ref(packages: &[PackageDraft<'_>], id: HirModuleId) -> Result<ModuleRef, FrontendError> {
    let package = packages
        .iter()
        .find(|package| package.resolved.id == id.package())
        .expect("module package exists");
    let target = package
        .targets
        .iter()
        .find(|target| target.target.id == id.target())
        .expect("module target exists");
    let module = target
        .target
        .modules
        .iter()
        .find(|module| module.id == id)
        .expect("module exists");
    Ok(ModuleRef {
        package: package.resolved.package_id,
        target: target.target.target.clone(),
        path: module
            .path
            .iter()
            .map(|segment| segment.as_str().to_owned())
            .collect(),
    })
}

fn item_key(
    packages: &[PackageDraft<'_>],
    item: &ResolvedSymbolicItem,
) -> Result<SemanticDefinitionKey, FrontendError> {
    Ok(SemanticDefinitionKey {
        module: module_ref(packages, item.module)?,
        owner_path: owner_path(packages, item.owner)?,
        kind: item.kind,
        name: item.name.clone().unwrap_or_default(),
        span: item.span,
    })
}

fn owner_path(
    packages: &[PackageDraft<'_>],
    owner: Option<HirItemId>,
) -> Result<SymbolicDefinitionOwnerSkeleton, FrontendError> {
    let Some(owner) = owner else {
        return Ok(SymbolicDefinitionOwnerSkeleton::TopLevel);
    };
    let item = find_item(packages, owner).expect("owned item parent exists");
    if matches!(item.owner_shape, SymbolicDefinitionOwnerSkeleton::TopLevel) {
        return Err(frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            "owned definition parent has no typed owner-shape entry",
        ));
    }
    Ok(item.owner_shape.clone())
}

fn push_resolution_list<T>(
    output: &mut Vec<u8>,
    values: &[T],
    mut encode: impl FnMut(&T) -> Result<Vec<u8>, FrontendError>,
) -> Result<(), FrontendError> {
    let count = checked_u64(values.len(), "symbolic shape list length")?;
    output.extend_from_slice(&count.to_le_bytes());
    for value in values {
        let bytes = encode(value)?;
        push_blob(output, &bytes, "symbolic shape entry")?;
    }
    Ok(())
}

fn pending_shape_bytes(
    reason: &UnresolvedPathKind,
    canonical: &str,
) -> Result<Vec<u8>, FrontendError> {
    let mut output = vec![0];
    output.push(match reason {
        UnresolvedPathKind::UnknownName => 1,
        UnresolvedPathKind::AmbiguousNamespace => 2,
        UnresolvedPathKind::AssociatedItemPendingC2 => 3,
        UnresolvedPathKind::SelfTypePendingC2 => 4,
        UnresolvedPathKind::ShadowedLocalNeedsLexicalResolution => 5,
        UnresolvedPathKind::DependencyHasNoLibraryTarget => 6,
        UnresolvedPathKind::GenericFormationPendingC2 => 7,
    });
    push_blob(&mut output, canonical.as_bytes(), "pending symbolic shape")?;
    Ok(output)
}

fn push_blob(output: &mut Vec<u8>, bytes: &[u8], label: &'static str) -> Result<(), FrontendError> {
    output.extend_from_slice(&checked_u64(bytes.len(), label)?.to_le_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

fn shape_error(error: super::shape::ShapeEncodingError) -> FrontendError {
    frontend_path_error(FrontendErrorCode::Target, "IDENTITY001", error.to_string())
}

fn checked_u64(value: usize, label: &'static str) -> Result<u64, FrontendError> {
    u64::try_from(value).map_err(|_| {
        frontend_path_error(
            FrontendErrorCode::Target,
            "IDENTITY001",
            format!("{label} exceeds the checked u64 representation"),
        )
    })
}

fn frontend_error(kind: FrontendErrorCode, diagnostic: Diagnostic) -> FrontendError {
    FrontendError {
        kind,
        diagnostic: Box::new(diagnostic),
        files: Vec::<PathBuf>::new(),
    }
}

fn frontend_path_error(
    kind: FrontendErrorCode,
    code: &'static str,
    message: impl Into<String>,
) -> FrontendError {
    frontend_error(kind, Diagnostic::path(code, message))
}

fn uppercase_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn push_json_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character <= '\u{1f}' => {
                write!(output, "\\u{:04X}", u32::from(character))
                    .expect("writing to String is infallible");
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

fn write_span(output: &mut String, span: Span) {
    write!(
        output,
        "{} {} {} {} {} {} {}",
        span.file.0,
        span.start.byte,
        span.start.line,
        span.start.column,
        span.end.byte,
        span.end.line,
        span.end.column
    )
    .expect("writing to String is infallible");
}

fn write_target_root(output: &mut String, target: &TargetRoot) {
    match target {
        TargetRoot::Library => output.push_str("library"),
        TargetRoot::Binary(name) => {
            output.push_str("(binary ");
            push_json_string(output, name);
            output.push(')');
        }
        TargetRoot::Environment(name) => {
            output.push_str("(environment ");
            push_json_string(output, name);
            output.push(')');
        }
    }
}

fn write_visibility(output: &mut String, visibility: &Visibility) {
    match visibility {
        Visibility::DeclaringModule => output.push_str("declaring-module"),
        Visibility::AncestorModule { path } => {
            output.push_str("(ancestor-module");
            for segment in path {
                output.push(' ');
                push_json_string(output, segment);
            }
            output.push(')');
        }
        Visibility::Package => output.push_str("package"),
        Visibility::Public => output.push_str("public"),
    }
}

fn namespace_name(namespace: Namespace) -> &'static str {
    match namespace {
        Namespace::Module => "module",
        Namespace::Type => "type",
        Namespace::Value => "value",
    }
}

fn write_generic_argument_use(
    output: &mut String,
    argument: &HirGenericArgumentUse,
) -> Result<(), FrontendError> {
    output.push_str("(argument (span ");
    write_span(output, argument.span);
    output.push_str(") (formal ");
    match &argument.formal_kind {
        Some(GenericParameterKind::Type) => output.push_str("type"),
        Some(GenericParameterKind::Lifetime) => output.push_str("lifetime"),
        Some(GenericParameterKind::IntegerConst(integer_type)) => {
            output.push_str("(integer-const ");
            output.push_str(&integer_type_name(*integer_type));
            output.push(')');
        }
        None => output.push_str("unknown"),
    }
    output.push_str(") (value ");
    match &argument.value {
        ResolvedGenericArgument::Type(value) => {
            output.push_str("(type ");
            write_resolved_symbolic_value(output, value, |value| {
                super::shape::encode_symbolic_type_skeleton_c1(value).map_err(shape_error)
            })?;
            output.push(')');
        }
        ResolvedGenericArgument::Lifetime(value) => {
            output.push_str("(lifetime ");
            match value {
                ResolvedSymbolicLifetime::Resolved(SymbolicLifetime::Static) => {
                    output.push_str("(resolved static)");
                }
                ResolvedSymbolicLifetime::Resolved(SymbolicLifetime::Bound { depth, index }) => {
                    write!(output, "(resolved (bound {depth} {index}))")
                        .expect("writing to String is infallible");
                }
                ResolvedSymbolicLifetime::Resolved(SymbolicLifetime::ErasedLocal) => {
                    output.push_str("(resolved erased-local)");
                }
                ResolvedSymbolicLifetime::Pending {
                    reason, canonical, ..
                } => write_pending_symbolic_value(output, reason, canonical),
            }
            output.push(')');
        }
        ResolvedGenericArgument::IntegerConst(value) => {
            output.push_str("(integer-const ");
            write_resolved_symbolic_value(output, value, |value| {
                super::shape::encode_symbolic_const(value).map_err(shape_error)
            })?;
            output.push(')');
        }
    }
    output.push_str("))");
    Ok(())
}

fn write_resolved_symbolic_value<T, V>(
    output: &mut String,
    value: &T,
    encode: impl FnOnce(&V) -> Result<Vec<u8>, FrontendError>,
) -> Result<(), FrontendError>
where
    T: ResolvedSymbolicValue<Value = V>,
{
    match value.resolved_symbolic_value() {
        Ok(value) => {
            output.push_str("(resolved ");
            output.push_str(&uppercase_hex(&encode(value)?));
            output.push(')');
        }
        Err((reason, canonical)) => write_pending_symbolic_value(output, reason, canonical),
    }
    Ok(())
}

trait ResolvedSymbolicValue {
    type Value: ?Sized;

    fn resolved_symbolic_value(&self) -> Result<&Self::Value, (&UnresolvedPathKind, &str)>;
}

impl ResolvedSymbolicValue for ResolvedSymbolicType {
    type Value = SymbolicType;

    fn resolved_symbolic_value(&self) -> Result<&Self::Value, (&UnresolvedPathKind, &str)> {
        match self {
            Self::Resolved(value) => Ok(value),
            Self::Pending {
                reason, canonical, ..
            } => Err((reason, canonical)),
        }
    }
}

impl ResolvedSymbolicValue for ResolvedSymbolicConst {
    type Value = SymbolicConstExpression;

    fn resolved_symbolic_value(&self) -> Result<&Self::Value, (&UnresolvedPathKind, &str)> {
        match self {
            Self::Resolved(value) => Ok(value),
            Self::Pending {
                reason, canonical, ..
            } => Err((reason, canonical)),
        }
    }
}

fn write_pending_symbolic_value(output: &mut String, reason: &UnresolvedPathKind, canonical: &str) {
    output.push_str("(pending ");
    output.push_str(unresolved_path_name(reason));
    output.push(' ');
    push_json_string(output, canonical);
    output.push(')');
}

fn write_binding_target(output: &mut String, target: &HirBindingTarget) {
    match target {
        HirBindingTarget::Module(module) => write!(
            output,
            "(module {} {} {})",
            module.package().get(),
            module.target().0,
            module.local()
        )
        .expect("writing to String is infallible"),
        HirBindingTarget::Item(item) => {
            output.push_str("(item ");
            write_item_res(output, *item);
            output.push(')');
        }
    }
}

fn write_resolution(output: &mut String, resolution: &Res) {
    match resolution {
        Res::Module(module) => write!(
            output,
            "(module {} {} {})",
            module.package().get(),
            module.target().0,
            module.local()
        )
        .expect("writing to String is infallible"),
        Res::Item(item) => {
            output.push_str("(item ");
            write_item_res(output, *item);
            output.push(')');
        }
        Res::Generic(parameter) => write!(
            output,
            "(generic {} {})",
            parameter.owner.0, parameter.index
        )
        .expect("writing to String is infallible"),
        Res::Local(local) => write!(output, "(local {} {})", local.owner.0, local.ordinal)
            .expect("writing to String is infallible"),
        Res::Builtin(builtin) => write_builtin_res(output, *builtin),
    }
}

fn write_builtin_res(output: &mut String, builtin: BuiltinRes) {
    match builtin.target {
        BuiltinResTarget::Prelude(VirtualPreludeTarget::Definition(definition)) => {
            write!(output, "(builtin-definition {})", definition.ordinal())
        }
        BuiltinResTarget::Prelude(VirtualPreludeTarget::SemanticType(semantic_type)) => write!(
            output,
            "(builtin-semantic-type {})",
            semantic_type.ordinal()
        ),
        BuiltinResTarget::Method(method) => {
            write!(output, "(builtin-method {})", method.ordinal())
        }
        BuiltinResTarget::EnumVariant(variant) => {
            write!(output, "(builtin-variant {})", variant.ordinal())
        }
        BuiltinResTarget::RecordConstructor(owner) => {
            write!(output, "(builtin-constructor {})", owner.ordinal())
        }
    }
    .expect("writing to String is infallible");
}

fn write_item_res(output: &mut String, item: HirItemRes) {
    match item {
        HirItemRes::Definition(item) => {
            write!(output, "(definition {})", item.0).expect("writing to String is infallible");
        }
        HirItemRes::NominalConstructor { owner } => {
            write!(output, "(constructor {})", owner.0).expect("writing to String is infallible");
        }
        HirItemRes::EnumVariant { owner, ordinal } => {
            write!(output, "(variant {} {ordinal})", owner.0)
                .expect("writing to String is infallible");
        }
    }
}

fn write_contract(output: &mut String, contract: &ResolvedTargetContract) {
    match contract {
        ResolvedTargetContract::Library => output.push_str("library"),
        ResolvedTargetContract::Binary {
            root_world,
            main,
            capabilities,
        } => {
            write!(
                output,
                "(binary (root-world {}) (main {}) (capabilities",
                root_world.0, main.0
            )
            .expect("writing to String is infallible");
            for capability in capabilities {
                output.push(' ');
                output.push_str(capability_key(*capability));
            }
            output.push_str("))");
        }
        ResolvedTargetContract::Environment {
            root_world,
            profile,
            reset,
            step,
            self_play,
        } => {
            write!(
                output,
                "(environment (root-world {}) (profile ",
                root_world.0
            )
            .expect("writing to String is infallible");
            push_json_string(output, profile);
            write!(
                output,
                ") (reset {}) (step {}) (self-play {}))",
                reset.0, step.0, self_play.0
            )
            .expect("writing to String is infallible");
        }
        ResolvedTargetContract::Pending => output.push_str("pending"),
    }
}

fn declaration_kind_name(kind: DeclarationKind) -> &'static str {
    match kind {
        DeclarationKind::World => "world",
        DeclarationKind::Component => "component",
        DeclarationKind::Resource => "resource",
        DeclarationKind::Tag => "tag",
        DeclarationKind::System => "system",
        DeclarationKind::Schedule => "schedule",
        DeclarationKind::Function => "function",
        DeclarationKind::Generator => "generator",
        DeclarationKind::Struct => "struct",
        DeclarationKind::Enum => "enum",
        DeclarationKind::Trait => "trait",
        DeclarationKind::Impl => "impl",
        DeclarationKind::TypeAlias => "type-alias",
        DeclarationKind::Const => "const",
        DeclarationKind::Static => "static",
        DeclarationKind::Query => "query",
    }
}

fn body_kind_name(kind: SemanticBodyKind) -> &'static str {
    match kind {
        SemanticBodyKind::Declaration => "declaration",
        SemanticBodyKind::Closure => "closure",
        SemanticBodyKind::Generator => "generator",
        SemanticBodyKind::WorldInitializer => "world-initializer",
        SemanticBodyKind::ArrayLength => "array-length",
        SemanticBodyKind::RepeatCount => "repeat-count",
        SemanticBodyKind::IntegerGenericArgument => "integer-generic-argument",
    }
}

fn unresolved_path_name(reason: &UnresolvedPathKind) -> &'static str {
    match reason {
        UnresolvedPathKind::UnknownName => "unknown-name",
        UnresolvedPathKind::AmbiguousNamespace => "ambiguous-namespace",
        UnresolvedPathKind::GenericFormationPendingC2 => "generic-formation-pending-c2",
        UnresolvedPathKind::AssociatedItemPendingC2 => "associated-item-pending-c2",
        UnresolvedPathKind::SelfTypePendingC2 => "self-type-pending-c2",
        UnresolvedPathKind::ShadowedLocalNeedsLexicalResolution => {
            "shadowed-local-needs-lexical-resolution"
        }
        UnresolvedPathKind::DependencyHasNoLibraryTarget => "dependency-has-no-library-target",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use arche_package::{
        canonical_package_id, load_workspace, resolve, source_tree_digest, DependencyRequirement,
        IntegrityDigest, ManifestRequest, ManifestSpan, RegistrySnapshot, ResolvedDependency,
        ResolvedGraph, ResolvedPackage, ResolvedSource, SourceTreeEntry,
        OFFICIAL_REGISTRY_IDENTITY,
    };
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::{
        declaration_shape_readiness, encode_inventory_c1, try_canonicalize_declaration_shape,
    };

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct TemporaryWorkspace(PathBuf);

    struct BodyCorpusExpectation {
        name: &'static str,
        count: usize,
        kind_counts: &'static [(SemanticBodyKind, usize)],
    }

    impl Drop for TemporaryWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn corpus(name: &str) -> FrontendOutput {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../tests/m27c1")
            .join(name);
        let workspace = load_workspace(&ManifestRequest::discover_from(&root)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        check_workspace_c1(&workspace, &graph, &[]).unwrap()
    }

    fn inline_library(source: &str) -> FrontendOutput {
        inline_library_with_files(source, &[]).unwrap()
    }

    fn inline_library_result(source: &str) -> Result<FrontendOutput, FrontendError> {
        inline_library_with_files(source, &[])
    }

    fn inline_library_with_files(
        source: &str,
        extra_files: &[(&str, &str)],
    ) -> Result<FrontendOutput, FrontendError> {
        let ordinal = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let fixture = TemporaryWorkspace(std::env::temp_dir().join(format!(
            "arche-c1-owner-shape-{}-{ordinal}",
            std::process::id()
        )));
        fs::create_dir_all(fixture.0.join("src")).unwrap();
        fs::write(
            fixture.0.join("Arche.toml"),
            concat!(
                "schema = 1\n\n",
                "[package]\n",
                "name = \"example/owners\"\n",
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
        for (relative_path, contents) in extra_files {
            let path = fixture.0.join(relative_path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let workspace = load_workspace(&ManifestRequest::discover_from(&fixture.0)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        check_workspace_c1(&workspace, &graph, &[])
    }

    fn assert_exact_body_inventory(
        corpus_name: &str,
        output: &FrontendOutput,
        expected_count: usize,
        expected_kind_counts: &[(SemanticBodyKind, usize)],
    ) {
        let mut expected_rows = Vec::new();
        let mut kind_counts = BTreeMap::<SemanticBodyKind, usize>::new();
        for package in &output.hir.packages {
            for target in &package.targets {
                for body in &target.bodies {
                    *kind_counts.entry(body.kind).or_default() += 1;
                    let owner = target
                        .items
                        .iter()
                        .find(|item| item.id == body.owner)
                        .expect("accepted body has an owner in its target");
                    let module = target
                        .modules
                        .iter()
                        .find(|module| module.id == owner.module)
                        .expect("accepted body owner has a module in its target");
                    expected_rows.push(SemanticBodyInventorySkeleton {
                        hir_body: body.id,
                        key: SemanticBodyKey {
                            package: package.package,
                            target: target.target.clone(),
                            modules: module
                                .path
                                .iter()
                                .map(|segment| segment.as_str().to_owned())
                                .collect(),
                            declaration_kind: owner.kind,
                            declaration_name: owner.name.clone(),
                            declaration_span: owner.span,
                            body_kind: body.kind,
                            ordinal: body.ordinal,
                            body_span: body.span,
                        },
                    });
                }
            }
        }
        assert_eq!(
            expected_rows.len(),
            expected_count,
            "{corpus_name} accepted-body count changed"
        );
        assert_eq!(
            kind_counts,
            expected_kind_counts.iter().copied().collect(),
            "{corpus_name} accepted-body kind multiset changed"
        );

        let mut unmatched_inventory = output
            .inventory
            .packages
            .iter()
            .flat_map(|package| package.bodies.iter().cloned())
            .collect::<Vec<_>>();
        assert_eq!(
            unmatched_inventory.len(),
            expected_count,
            "{corpus_name} inventory-body count changed"
        );
        for expected in expected_rows {
            let index = unmatched_inventory
                .iter()
                .position(|actual| actual == &expected)
                .unwrap_or_else(|| {
                    panic!("{corpus_name} inventory omitted accepted body {expected:#?}")
                });
            unmatched_inventory.swap_remove(index);
        }
        assert!(
            unmatched_inventory.is_empty(),
            "{corpus_name} inventory retained bodies absent from accepted HIR: {unmatched_inventory:#?}"
        );
    }

    #[test]
    fn materialized_registry_source_reaches_hir_and_is_digest_bound() {
        let ordinal = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let fixture = TemporaryWorkspace(std::env::temp_dir().join(format!(
            "arche-c1-registry-source-{}-{ordinal}",
            std::process::id()
        )));
        let registry_root = fixture.0.join("registry-cache");
        fs::create_dir_all(fixture.0.join("src")).unwrap();
        fs::create_dir_all(registry_root.join("src")).unwrap();

        let workspace_manifest = concat!(
            "schema = 1\n\n",
            "[package]\n",
            "name = \"example/app\"\n",
            "version = \"0.1.0\"\n",
            "edition = \"2026\"\n",
            "arche = \">=0.0.0\"\n",
            "publish = false\n\n",
            "[lib]\n",
            "path = \"src/lib.arc\"\n\n",
            "[dependencies.dep]\n",
            "package = \"example/registry\"\n",
            "version = \"^1.0\"\n",
        );
        let workspace_source = concat!(
            "use dep::RegistryThing;\n",
            "pub fn consume(value: RegistryThing) { let _ = value; }\n",
        );
        let registry_manifest_bytes = concat!(
            "schema = 1\n\n",
            "[ package ]\n",
            "name = \"example/registry\"\n",
            "version = \"1.0.0\"\n",
            "edition = \"2026\"\n",
            "arche = \">=0.0.0\"\n",
            "publish = false\n\n",
            "[lib]\n",
            "path = \"src/lib.arc\"\n",
        );
        let registry_source = "pub struct RegistryThing { pub value: i32 }\n";

        fs::write(fixture.0.join("Arche.toml"), workspace_manifest).unwrap();
        fs::write(fixture.0.join("src/lib.arc"), workspace_source).unwrap();
        fs::write(registry_root.join("Arche.toml"), registry_manifest_bytes).unwrap();
        fs::write(registry_root.join("src/lib.arc"), registry_source).unwrap();

        let workspace = load_workspace(&ManifestRequest::discover_from(&fixture.0)).unwrap();
        let workspace_member = &workspace.members[0];
        let workspace_package = workspace_member.manifest.package.as_ref().unwrap();
        let dependency = workspace_member
            .manifest
            .dependencies
            .values()
            .next()
            .unwrap();
        let registry_manifest = Manifest::load(&registry_root.join("Arche.toml")).unwrap();
        let registry_package = registry_manifest.package.as_ref().unwrap();
        let registry_source_digest = source_tree_digest(&[
            registry_manifest.source_entry.clone(),
            SourceTreeEntry::from_bytes(
                PortablePath::new("src/lib.arc").unwrap(),
                registry_source.as_bytes(),
            )
            .unwrap(),
        ])
        .unwrap();
        let archive_digest = IntegrityDigest::of_bytes(b"registry archive");
        let provenance_record_digest = IntegrityDigest::of_bytes(b"provenance");
        let inclusion_record_digest = IntegrityDigest::of_bytes(b"inclusion");
        let workspace_node = PackageNodeId::new(0);
        let registry_node = PackageNodeId::new(1);
        let package_header_start = u64::try_from(
            registry_manifest_bytes
                .find("[ package ]")
                .expect("registry fixture has a package header"),
        )
        .unwrap();
        let graph = ResolvedGraph {
            packages: vec![
                ResolvedPackage {
                    id: workspace_node,
                    package_id: canonical_package_id(&workspace_package.name),
                    name: workspace_package.name.clone(),
                    version: workspace_package.version.clone(),
                    source: ResolvedSource::Workspace {
                        relative_path: workspace_member.relative_path.clone(),
                    },
                },
                ResolvedPackage {
                    id: registry_node,
                    package_id: canonical_package_id(&registry_package.name),
                    name: registry_package.name.clone(),
                    version: registry_package.version.clone(),
                    source: ResolvedSource::Registry {
                        archive_digest,
                        source_digest: registry_source_digest,
                        provenance_record_digest,
                        inclusion_record_digest,
                        manifest_span: ManifestSpan {
                            start_byte: package_header_start,
                            end_byte: package_header_start + 11,
                            start_line: 3,
                            start_column: 1,
                            end_line: 3,
                            end_column: 12,
                        },
                    },
                },
            ],
            roots: vec![workspace_node],
            dependencies: vec![ResolvedDependency {
                from: workspace_node,
                alias: dependency.alias.clone(),
                to: registry_node,
                requirement: DependencyRequirement::from_version_req(
                    dependency.requirement.as_ref().unwrap(),
                ),
                kind: LockDependencyKind::Normal,
            }],
            registry_identity: OFFICIAL_REGISTRY_IDENTITY.to_owned(),
            registry_snapshot_digest: IntegrityDigest::of_bytes(b"registry snapshot"),
        };
        graph.validate().unwrap();
        let materialized = [MaterializedRegistryPackage {
            package_node: registry_node,
            directory: registry_root,
            manifest: registry_manifest,
            manifest_bytes: registry_manifest_bytes.as_bytes().to_vec(),
        }];

        let output = check_workspace_c1(&workspace, &graph, &materialized).unwrap();
        let registry_hir = output
            .hir
            .packages
            .iter()
            .find(|package| package.package_node == registry_node)
            .expect("registry package reaches package-aware HIR");
        let registry_item = registry_hir
            .targets
            .iter()
            .flat_map(|target| &target.items)
            .find(|item| item.name.as_deref() == Some("RegistryThing"))
            .expect("registry source declaration reaches resolved HIR");
        assert!(output
            .hir
            .packages
            .iter()
            .find(|package| package.package_node == workspace_node)
            .unwrap()
            .targets
            .iter()
            .flat_map(|target| &target.path_resolutions)
            .any(|resolution| {
                resolution.resolutions.iter().any(|resolved| {
                    matches!(
                        resolved,
                        Res::Item(item) if item.owner() == registry_item.id
                    )
                })
            }));
        let registry_inventory = output
            .inventory
            .packages
            .iter()
            .find(|package| package.package_node == registry_node)
            .unwrap();
        assert_eq!(
            registry_inventory.provenance.registry_origin,
            OFFICIAL_REGISTRY_IDENTITY
        );
        assert_eq!(
            registry_inventory.provenance.scoped_name,
            "example/registry"
        );
        assert_eq!(registry_inventory.provenance.version, "1.0.0");
        assert_eq!(
            registry_inventory.provenance.source,
            PackageSourceSkeleton::Registry {
                archive_digest: *archive_digest.as_bytes(),
                source_digest: *registry_source_digest.as_bytes(),
                provenance_record_digest: *provenance_record_digest.as_bytes(),
                inclusion_record_digest: *inclusion_record_digest.as_bytes(),
            }
        );
        assert_eq!(
            registry_inventory.provenance.dependencies,
            Vec::<PackageDependencySkeleton>::new()
        );
        let workspace_inventory = output
            .inventory
            .packages
            .iter()
            .find(|package| package.package_node == workspace_node)
            .unwrap();
        assert_eq!(
            workspace_inventory.provenance.dependencies,
            vec![PackageDependencySkeleton {
                alias: "dep".to_owned(),
                package: registry_inventory.package,
                requirement: "^1.0".to_owned(),
                kind: DependencyKind::Normal,
            }]
        );
        assert!(output
            .inventory
            .source_trees
            .iter()
            .any(|(package, digest)| {
                *package == registry_hir.package && *digest == *registry_source_digest.as_bytes()
            }));
        let encoded_inventory = encode_inventory_c1(&output.inventory).unwrap();
        let encoded_inventory_digest: [u8; 32] = Sha256::digest(&encoded_inventory).into();
        assert_eq!(
            encoded_inventory_digest,
            [
                0x10, 0x7b, 0xb9, 0x3d, 0xe9, 0x8b, 0x8a, 0x22, 0xb7, 0x5b, 0xc3, 0x1f, 0x8f, 0x5d,
                0x06, 0x1d, 0x6b, 0x05, 0x6a, 0xba, 0x76, 0x25, 0x06, 0xb0, 0x51, 0x77, 0x52, 0xca,
                0x1e, 0xb3, 0xf4, 0x09,
            ]
        );
        drop(output);

        let mut wrong_manifest_bytes = materialized.clone();
        let byte = wrong_manifest_bytes[0]
            .manifest_bytes
            .last_mut()
            .expect("registry manifest is nonempty");
        *byte = if *byte == b'\n' { b' ' } else { b'\n' };
        let error = check_workspace_c1(&workspace, &graph, &wrong_manifest_bytes).unwrap_err();
        assert_eq!(error.kind, FrontendErrorCode::Source);
        assert_eq!(error.diagnostic.code, "SOURCE005");
        assert!(error
            .diagnostic
            .message
            .contains("committed package-header span and bytes"));

        let mut wrong_span_graph = graph.clone();
        let ResolvedSource::Registry { manifest_span, .. } =
            &mut wrong_span_graph.packages[usize::try_from(registry_node.get()).unwrap()].source
        else {
            unreachable!();
        };
        manifest_span.start_column += 1;
        manifest_span.end_column += 1;
        wrong_span_graph.validate().unwrap();
        let error = check_workspace_c1(&workspace, &wrong_span_graph, &materialized).unwrap_err();
        assert_eq!(error.kind, FrontendErrorCode::Source);
        assert_eq!(error.diagnostic.code, "SOURCE005");
        assert!(error
            .diagnostic
            .message
            .contains("committed package-header span"));

        let mut stale_graph = graph;
        let ResolvedSource::Registry { source_digest, .. } =
            &mut stale_graph.packages[usize::try_from(registry_node.get()).unwrap()].source
        else {
            unreachable!();
        };
        *source_digest = IntegrityDigest::of_bytes(b"stale registry source");
        stale_graph.validate().unwrap();
        let error = check_workspace_c1(&workspace, &stale_graph, &materialized).unwrap_err();
        assert_eq!(error.kind, FrontendErrorCode::Source);
        assert_eq!(error.diagnostic.code, "SOURCE005");
        assert!(error.diagnostic.message.contains("instead of selected"));
    }

    #[test]
    fn module_id_exhaustion_reports_the_exact_allocation_origin() {
        let manifest_path = PathBuf::from("packages/app/Arche.toml");
        let manifest = Manifest::parse(
            &manifest_path,
            concat!(
                "schema = 1\n\n",
                "[package]\n",
                "name = \"example/app\"\n",
                "version = \"0.1.0\"\n",
                "edition = \"2026\"\n",
                "arche = \">=0.0.0\"\n",
                "publish = false\n\n",
                "[lib]\n",
                "path = \"src/lib.arc\"\n",
            ),
        )
        .unwrap();
        let target = manifest.targets().next().unwrap();
        let exhausted = || {
            let package = PackageNodeId::new(4);
            let target_id = TargetId(2);
            let mut allocator = HirModuleIdAllocator::near_exhaustion(package, target_id);
            assert_eq!(
                allocator.next_module().unwrap(),
                HirModuleId::new(package, target_id, u64::MAX)
            );
            allocator.next_module().unwrap_err()
        };

        let root_error = module_allocation_error(
            exhausted(),
            ModuleAllocationOrigin::ManifestTarget {
                manifest: &manifest,
                target: &target,
            },
        );
        let target_span = manifest.target_span(&target).unwrap();
        assert_eq!(root_error.diagnostic.code, "IDENTITY001");
        assert_eq!(root_error.files, vec![manifest_path.clone()]);
        assert_eq!(
            root_error.diagnostic.primary.span,
            Some(Span {
                file: FileId(0),
                start: SourcePosition {
                    byte: target_span.start_byte,
                    line: target_span.start_line,
                    column: target_span.start_column,
                },
                end: SourcePosition {
                    byte: target_span.end_byte,
                    line: target_span.end_line,
                    column: target_span.end_column,
                },
            })
        );

        let declaration_span = Span {
            file: FileId(27),
            start: SourcePosition {
                byte: 9,
                line: 2,
                column: 3,
            },
            end: SourcePosition {
                byte: 15,
                line: 2,
                column: 9,
            },
        };
        let child_error = module_allocation_error(
            exhausted(),
            ModuleAllocationOrigin::ModuleDeclaration(declaration_span),
        );
        assert_eq!(child_error.diagnostic.code, "IDENTITY001");
        assert_eq!(child_error.diagnostic.primary.span, Some(declaration_span));
        assert!(child_error.files.is_empty());

        let foreign_manifest_path = PathBuf::from("packages/foreign/Arche.toml");
        let foreign_manifest = Manifest::parse(
            &foreign_manifest_path,
            concat!(
                "schema = 1\n\n",
                "[package]\n",
                "name = \"example/foreign\"\n",
                "version = \"0.1.0\"\n",
                "edition = \"2026\"\n",
                "arche = \">=0.0.0\"\n",
                "publish = false\n\n",
                "[lib]\n",
                "path = \"src/foreign.arc\"\n",
            ),
        )
        .unwrap();
        let foreign_target = foreign_manifest.targets().next().unwrap();
        let missing_span = module_allocation_error(
            exhausted(),
            ModuleAllocationOrigin::ManifestTarget {
                manifest: &manifest,
                target: &foreign_target,
            },
        );
        assert_eq!(missing_span.diagnostic.code, "IDENTITY001");
        assert_eq!(missing_span.diagnostic.primary.span, None);
        assert_eq!(missing_span.files, vec![manifest_path]);
        assert_eq!(
            missing_span.diagnostic.message,
            "IDENTITY001: HirModuleId session ID allocator is exhausted; originating target manifest span is unavailable"
        );
    }

    #[test]
    fn both_corpora_have_dense_global_and_package_qualified_session_arenas() {
        for name in ["language-game", "language-environment"] {
            let output = corpus(name);
            let (embedded_core, ordinary_sources) = output.sources.files().split_last().unwrap();
            assert_eq!(embedded_core.id(), crate::EMBEDDED_CORE_FILE_ID);
            assert!(embedded_core.is_embedded_core());
            assert_eq!(
                ordinary_sources
                    .iter()
                    .map(|file| file.id().0)
                    .collect::<Vec<_>>(),
                (0..u64::try_from(ordinary_sources.len()).unwrap()).collect::<Vec<_>>()
            );
            let items = output
                .hir
                .packages
                .iter()
                .flat_map(|package| &package.targets)
                .flat_map(|target| &target.items)
                .map(|item| item.id.0)
                .collect::<Vec<_>>();
            assert_eq!(
                items,
                (0..u64::try_from(items.len()).unwrap()).collect::<Vec<_>>()
            );
            let bodies = output
                .hir
                .packages
                .iter()
                .flat_map(|package| &package.targets)
                .flat_map(|target| &target.bodies)
                .map(|body| body.id.0)
                .collect::<Vec<_>>();
            assert_eq!(
                bodies,
                (0..u64::try_from(bodies.len()).unwrap()).collect::<Vec<_>>()
            );
            for package in &output.hir.packages {
                for (target_ordinal, target) in package.targets.iter().enumerate() {
                    assert_eq!(target.id.0, u64::try_from(target_ordinal).unwrap());
                    assert_eq!(
                        target
                            .modules
                            .iter()
                            .map(|module| module.id.local())
                            .collect::<Vec<_>>(),
                        (0..u64::try_from(target.modules.len()).unwrap()).collect::<Vec<_>>()
                    );
                    assert!(target.modules.iter().all(|module| {
                        module.id.package() == package.package_node
                            && module.id.target() == target.id
                    }));
                }
            }
        }
    }

    #[test]
    fn branded_include_inputs_follow_modules_and_precede_embedded_core() {
        let output = corpus("language-environment");
        let package = output
            .inventory
            .packages
            .iter()
            .find(|package| package.provenance.scoped_name == "fixtures/language-environment")
            .unwrap();
        let includes = output
            .sources
            .files()
            .iter()
            .filter(|source| source.roles().contains(&SourceRole::Include))
            .collect::<Vec<_>>();
        assert_eq!(
            includes
                .iter()
                .map(|source| source.portable_path().as_str())
                .collect::<Vec<_>>(),
            vec!["data/message.txt", "data/table.bin"]
        );
        assert!(includes
            .iter()
            .all(|source| source.package() == Some(package.package_node)));
        let last_module = output
            .sources
            .files()
            .iter()
            .filter(|source| source.roles().contains(&SourceRole::Module))
            .map(|source| source.id().0)
            .max()
            .unwrap();
        assert!(includes.iter().all(|source| source.id().0 > last_module));
        assert!(includes
            .iter()
            .all(|source| source.id() != crate::EMBEDDED_CORE_FILE_ID));
        assert_eq!(
            output.sources.files().last().unwrap().id(),
            crate::EMBEDDED_CORE_FILE_ID
        );

        for name in ["include_bytes", "include_str"] {
            let expected = output
                .inventory
                .embedded_core
                .lookup_prelude(name, VirtualNamespace::Value)
                .unwrap();
            assert!(output
                .hir
                .packages
                .iter()
                .flat_map(|package| &package.targets)
                .flat_map(|target| &target.path_resolutions)
                .any(|resolution| matches!(
                    resolution.resolutions.as_slice(),
                    [Res::Builtin(BuiltinRes {
                        target: BuiltinResTarget::Prelude(target),
                    })] if *target == expected
                )));
        }
    }

    #[test]
    fn exact_package_manifest_include_merges_its_source_tree_commitment() {
        let output =
            inline_library("pub const MANIFEST: &'static str = include_str(\"Arche.toml\");\n");
        let package = &output.hir.packages[0];
        let manifest_sources = output
            .sources
            .files()
            .iter()
            .filter(|source| {
                source.package() == Some(package.package_node)
                    && source.portable_path().as_str() == "Arche.toml"
            })
            .collect::<Vec<_>>();
        assert_eq!(manifest_sources.len(), 1);
        assert_eq!(manifest_sources[0].roles(), &[SourceRole::Include]);

        let ordinary_sources = output
            .sources
            .files()
            .iter()
            .filter(|source| !source.is_embedded_core())
            .collect::<Vec<_>>();
        assert_eq!(
            ordinary_sources
                .iter()
                .map(|source| source.id().0)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );

        let retained_entries = output.sources.source_entries(package.package_node);
        assert_eq!(
            retained_entries
                .iter()
                .filter(|entry| entry.path.as_str() == "Arche.toml")
                .count(),
            1
        );
        let retained_digest = source_tree_digest(&retained_entries).unwrap();
        assert!(output
            .inventory
            .source_trees
            .iter()
            .any(|(inventory_package, digest)| {
                *inventory_package == package.package && *digest == *retained_digest.as_bytes()
            }));
    }

    #[test]
    fn include_cannot_alias_the_package_manifest_under_another_path() {
        let ordinal = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let fixture = TemporaryWorkspace(std::env::temp_dir().join(format!(
            "arche-c1-manifest-alias-{}-{ordinal}",
            std::process::id()
        )));
        fs::create_dir_all(fixture.0.join("src")).unwrap();
        fs::write(
            fixture.0.join("Arche.toml"),
            concat!(
                "schema = 1\n\n",
                "[package]\n",
                "name = \"example/manifest-alias\"\n",
                "version = \"0.1.0\"\n",
                "edition = \"2026\"\n",
                "arche = \">=0.0.0\"\n",
                "publish = false\n\n",
                "[lib]\n",
                "path = \"src/lib.arc\"\n",
            ),
        )
        .unwrap();
        fs::write(
            fixture.0.join("src/lib.arc"),
            "pub const MANIFEST: &'static str = include_str(\"manifest-alias.toml\");\n",
        )
        .unwrap();
        fs::hard_link(
            fixture.0.join("Arche.toml"),
            fixture.0.join("manifest-alias.toml"),
        )
        .unwrap_or_else(|error| panic!("test filesystem must support hard links: {error}"));

        let workspace = load_workspace(&ManifestRequest::discover_from(&fixture.0)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        let error = check_workspace_c1(&workspace, &graph, &[]).unwrap_err();
        assert_eq!(error.kind, FrontendErrorCode::Source);
        assert_eq!(error.diagnostic.code, "CTFE005");
        assert!(error
            .diagnostic
            .message
            .contains("aliases retained manifest Arche.toml"));
    }

    #[test]
    fn shadowed_include_spelling_is_not_validated_or_acquired_as_a_builtin() {
        let output = inline_library(concat!(
            "pub fn include_str(value: i32) -> i32 { value }\n",
            "pub const VALUE: i32 = include_str(0);\n",
        ));
        assert!(output
            .sources
            .files()
            .iter()
            .all(|source| !source.roles().contains(&SourceRole::Include)));
        assert!(output
            .hir
            .packages
            .iter()
            .flat_map(|package| &package.targets)
            .flat_map(|target| &target.path_resolutions)
            .filter(|resolution| resolution
                .resolutions
                .iter()
                .any(|resolution| { matches!(resolution, Res::Item(_)) }))
            .all(|resolution| {
                resolution
                    .resolutions
                    .iter()
                    .all(|resolution| !matches!(resolution, Res::Builtin(_)))
            }));
    }

    #[test]
    fn invalid_included_string_is_ctfe005_at_the_literal() {
        let ordinal = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let fixture = TemporaryWorkspace(std::env::temp_dir().join(format!(
            "arche-c1-invalid-include-{}-{ordinal}",
            std::process::id()
        )));
        fs::create_dir_all(fixture.0.join("src")).unwrap();
        fs::create_dir_all(fixture.0.join("data")).unwrap();
        fs::write(
            fixture.0.join("Arche.toml"),
            concat!(
                "schema = 1\n\n",
                "[package]\n",
                "name = \"example/invalid-include\"\n",
                "version = \"0.1.0\"\n",
                "edition = \"2026\"\n",
                "arche = \">=0.0.0\"\n",
                "publish = false\n\n",
                "[lib]\n",
                "path = \"src/lib.arc\"\n",
            ),
        )
        .unwrap();
        fs::write(
            fixture.0.join("src/lib.arc"),
            "pub const BAD: &'static str = include_str(\"data/bad.bin\");\n",
        )
        .unwrap();
        fs::write(fixture.0.join("data/bad.bin"), [0xff]).unwrap();
        let workspace = load_workspace(&ManifestRequest::discover_from(&fixture.0)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        let error = check_workspace_c1(&workspace, &graph, &[]).unwrap_err();
        assert_eq!(error.diagnostic.code, "CTFE005");
        let span = error.diagnostic.primary.span.unwrap();
        assert_ne!(span.file, crate::EMBEDDED_CORE_FILE_ID);
        assert!(span.start.byte < span.end.byte);
    }

    #[test]
    fn both_corpora_hir_text_is_byte_exact_and_host_path_free() {
        let mut digest = Sha256::new();
        for name in ["language-game", "language-environment"] {
            let output = corpus(name);
            let first = dump_hir_c1(&output.hir).unwrap();
            let second = dump_hir_c1(&output.hir).unwrap();
            assert_eq!(first, second);
            assert!(first.starts_with("ARCHE-HIR-TEXT 1\n(workspace"));
            assert!(first.ends_with(")\n"));
            assert!(!first.contains(env!("CARGO_MANIFEST_DIR")));
            digest.update(u64::try_from(name.len()).unwrap().to_le_bytes());
            digest.update(name.as_bytes());
            digest.update(u64::try_from(first.len()).unwrap().to_le_bytes());
            digest.update(first.as_bytes());
        }
        let digest = uppercase_hex(&digest.finalize());
        assert_eq!(
            digest,
            "7429C71C3AD17941CD845ECBCD4877935F0CCD94B725926D76668A0BADB81D08"
        );
    }

    #[test]
    fn accepted_nested_and_type_const_bodies_are_all_addressable() {
        let cases = [
            BodyCorpusExpectation {
                name: "language-game",
                count: 74,
                kind_counts: &[
                    (SemanticBodyKind::Declaration, 53),
                    (SemanticBodyKind::Closure, 2),
                    (SemanticBodyKind::Generator, 2),
                    (SemanticBodyKind::WorldInitializer, 1),
                    (SemanticBodyKind::ArrayLength, 10),
                    (SemanticBodyKind::RepeatCount, 2),
                    (SemanticBodyKind::IntegerGenericArgument, 4),
                ],
            },
            BodyCorpusExpectation {
                name: "language-environment",
                count: 47,
                kind_counts: &[
                    (SemanticBodyKind::Declaration, 34),
                    (SemanticBodyKind::WorldInitializer, 1),
                    (SemanticBodyKind::ArrayLength, 8),
                    (SemanticBodyKind::RepeatCount, 2),
                    (SemanticBodyKind::IntegerGenericArgument, 2),
                ],
            },
        ];
        for case in cases {
            assert_exact_body_inventory(
                case.name,
                &corpus(case.name),
                case.count,
                case.kind_counts,
            );
        }
    }

    #[test]
    fn query_child_is_the_sole_owner_of_its_const_argument_body() {
        let output = corpus("language-game");
        let mut target_count = 0;
        for target in output
            .hir
            .packages
            .iter()
            .flat_map(|package| &package.targets)
            .filter(|target| {
                target
                    .items
                    .iter()
                    .any(|item| item.name.as_deref() == Some("GenericSystem"))
            })
        {
            target_count += 1;
            let system = target
                .items
                .iter()
                .find(|item| item.name.as_deref() == Some("GenericSystem"))
                .expect("selected target contains GenericSystem");
            let query = target
                .items
                .iter()
                .find(|item| {
                    item.owner == Some(system.id)
                        && item.name.as_deref() == Some("positions")
                        && matches!(&item.source, HirItemSource::QueryParameter { .. })
                })
                .expect("GenericSystem retains its positions Query child");

            assert_eq!(
                target
                    .bodies
                    .iter()
                    .filter(|body| {
                        body.owner == query.id
                            && body.kind == SemanticBodyKind::IntegerGenericArgument
                    })
                    .count(),
                1
            );
            assert_eq!(
                target
                    .bodies
                    .iter()
                    .filter(|body| {
                        body.owner == system.id
                            && body.kind == SemanticBodyKind::IntegerGenericArgument
                    })
                    .count(),
                0
            );
        }
        assert_eq!(target_count, 2);
    }

    #[test]
    fn anonymous_closure_and_generator_ordinals_share_source_preorder() {
        let output = inline_library(concat!(
            "pub fn anonymous_bodies() {\n",
            "    let closure = |value: i32| requires {} throws {} -> i32 { value };\n",
            "    let generator = gen |seed: i32| resume i32 yields i32 requires {} throws {} -> i32 {\n",
            "        let resumed = yield seed;\n",
            "        resumed\n",
            "    };\n",
            "}\n",
        ));
        let owner = output
            .hir
            .packages
            .iter()
            .flat_map(|package| &package.targets)
            .flat_map(|target| &target.items)
            .find(|item| item.name.as_deref() == Some("anonymous_bodies"))
            .unwrap();
        let anonymous = output
            .hir
            .packages
            .iter()
            .flat_map(|package| &package.targets)
            .flat_map(|target| &target.bodies)
            .filter(|body| {
                body.owner == owner.id
                    && matches!(
                        body.kind,
                        SemanticBodyKind::Closure | SemanticBodyKind::Generator
                    )
            })
            .map(|body| (body.kind, body.ordinal, body.span))
            .collect::<Vec<_>>();
        assert_eq!(
            anonymous
                .iter()
                .map(|(kind, ordinal, _)| (*kind, *ordinal))
                .collect::<Vec<_>>(),
            vec![
                (SemanticBodyKind::Closure, 1),
                (SemanticBodyKind::Generator, 2),
            ]
        );
        assert!(anonymous[0].2.start.byte < anonymous[1].2.start.byte);
    }

    #[test]
    fn corpus_locals_resolve_at_their_exact_lexical_program_points() {
        for name in ["language-game", "language-environment"] {
            let output = corpus(name);
            let resolutions = output
                .hir
                .packages
                .iter()
                .flat_map(|package| &package.targets)
                .flat_map(|target| &target.path_resolutions)
                .collect::<Vec<_>>();
            assert!(resolutions.iter().all(|resolution| {
                resolution.unresolved
                    != Some(UnresolvedPathKind::ShadowedLocalNeedsLexicalResolution)
            }));
            assert!(
                resolutions
                    .iter()
                    .flat_map(|resolution| &resolution.resolutions)
                    .any(|resolution| matches!(resolution, Res::Local(_))),
                "{name} must retain resolved local uses"
            );
        }
    }

    #[test]
    fn type_paths_do_not_resolve_through_value_locals_or_const_generics() {
        let output = inline_library(concat!(
            "pub struct Thing {}\n",
            "pub struct N {}\n",
            "pub struct Wrapper {}\n",
            "impl Wrapper { pub fn make() -> i32 { 1i32 } }\n",
            "pub fn local_split(Thing: Thing) { let _ = Thing; }\n",
            "pub fn generic_split<const N: usize>(value: N, array: [i32; N]) {\n",
            "    let _ = value;\n",
            "    let _ = array;\n",
            "}\n",
            "pub fn associated_local(Wrapper: i32) -> i32 {\n",
            "    let _ = Wrapper;\n",
            "    Wrapper::make()\n",
            "}\n",
            "pub fn associated_const<const Wrapper: usize>() -> i32 { Wrapper::make() }\n",
        ));
        let target = &output.hir.packages[0].targets[0];
        let thing = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("Thing"))
            .unwrap();
        let n_type = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("N"))
            .unwrap();
        let local_split = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("local_split"))
            .unwrap();
        let thing_local = local_split
            .locals
            .iter()
            .find(|local| local.name == "Thing")
            .unwrap();
        let thing_paths = local_split
            .path_uses
            .iter()
            .filter(|path_use| {
                unqualified_path_name(&path_use.path).is_some_and(|name| name.as_str() == "Thing")
            })
            .collect::<Vec<_>>();
        assert_eq!(thing_paths.len(), 2);
        for path_use in thing_paths {
            let resolution = target
                .path_resolutions
                .iter()
                .find(|resolution| resolution.span == path_use.path.span)
                .unwrap();
            match path_use.namespace {
                Some(Namespace::Type) => assert_eq!(
                    resolution.resolutions,
                    [Res::Item(HirItemRes::Definition(thing.id))]
                ),
                Some(Namespace::Value) => {
                    assert_eq!(path_use.lexical_local, Some(thing_local.id));
                    assert_eq!(resolution.resolutions, [Res::Local(thing_local.id)]);
                }
                namespace => panic!("unexpected Thing namespace {namespace:?}"),
            }
        }

        let generic_split = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("generic_split"))
            .unwrap();
        let const_generic = generic_split
            .symbolic_shape
            .generic_parameters
            .iter()
            .find(|parameter| parameter.name == "N")
            .unwrap();
        for path_use in generic_split.path_uses.iter().filter(|path_use| {
            unqualified_path_name(&path_use.path).is_some_and(|name| name.as_str() == "N")
        }) {
            let resolution = target
                .path_resolutions
                .iter()
                .find(|resolution| resolution.span == path_use.path.span)
                .unwrap();
            match path_use.namespace {
                Some(Namespace::Type) => assert_eq!(
                    resolution.resolutions,
                    [Res::Item(HirItemRes::Definition(n_type.id))]
                ),
                Some(Namespace::Value) => assert_eq!(
                    resolution.resolutions,
                    [Res::Generic(GenericParameterId {
                        owner: generic_split.id,
                        index: const_generic.index,
                    })]
                ),
                namespace => panic!("unexpected N namespace {namespace:?}"),
            }
        }

        let wrapper = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("Wrapper"))
            .unwrap();
        for function in ["associated_local", "associated_const"] {
            let item = target
                .items
                .iter()
                .find(|item| item.name.as_deref() == Some(function))
                .unwrap();
            let path_use = item
                .path_uses
                .iter()
                .find(|path_use| canonical_path(&path_use.path) == "Wrapper::make")
                .unwrap();
            let resolution = target
                .path_resolutions
                .iter()
                .find(|resolution| resolution.span == path_use.path.span)
                .unwrap();
            assert_eq!(
                resolution.associated.as_ref().unwrap().owner,
                AssociatedPathOwner::Nominal(wrapper.id),
                "{function} must resolve the owner segment in the type namespace"
            );
            assert_eq!(
                resolution.unresolved,
                Some(UnresolvedPathKind::AssociatedItemPendingC2)
            );
        }
    }

    #[test]
    fn development_dependency_alias_is_inventory_visible_and_source_invisible() {
        let ordinal = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let fixture = TemporaryWorkspace(std::env::temp_dir().join(format!(
            "arche-c1-development-visibility-{}-{ordinal}",
            std::process::id()
        )));
        fs::create_dir_all(fixture.0.join("src")).unwrap();
        fs::create_dir_all(fixture.0.join("packages/test-support/src")).unwrap();
        fs::write(
            fixture.0.join("Arche.toml"),
            concat!(
                "schema = 1\n\n",
                "[package]\n",
                "name = \"example/app\"\n",
                "version = \"0.1.0\"\n",
                "edition = \"2026\"\n",
                "arche = \">=0.0.0\"\n",
                "publish = false\n\n",
                "[workspace]\n",
                "members = [\".\", \"packages/test-support\"]\n",
                "default-members = [\".\"]\n\n",
                "[lib]\n",
                "path = \"src/lib.arc\"\n\n",
                "[dev-dependencies.test_support]\n",
                "path = \"packages/test-support\"\n",
            ),
        )
        .unwrap();
        fs::write(
            fixture.0.join("packages/test-support/Arche.toml"),
            concat!(
                "schema = 1\n\n",
                "[package]\n",
                "name = \"example/test-support\"\n",
                "version = \"0.1.0\"\n",
                "edition = \"2026\"\n",
                "arche = \">=0.0.0\"\n",
                "publish = false\n\n",
                "[lib]\n",
                "path = \"src/lib.arc\"\n",
            ),
        )
        .unwrap();
        fs::write(fixture.0.join("src/lib.arc"), "pub fn okay() {}\n").unwrap();
        fs::write(
            fixture.0.join("packages/test-support/src/lib.arc"),
            "pub struct FixtureOnly {}\n",
        )
        .unwrap();

        let workspace = load_workspace(&ManifestRequest::discover_from(&fixture.0)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        let output = check_workspace_c1(&workspace, &graph, &[]).unwrap();
        let app = output
            .inventory
            .packages
            .iter()
            .find(|package| package.provenance.scoped_name == "example/app")
            .unwrap();
        let test_support = output
            .inventory
            .packages
            .iter()
            .find(|package| package.provenance.scoped_name == "example/test-support")
            .unwrap();
        assert_eq!(
            app.provenance.dependencies,
            [PackageDependencySkeleton {
                alias: "test_support".to_owned(),
                package: test_support.package,
                requirement: "=0.1.0".to_owned(),
                kind: DependencyKind::Development,
            }]
        );
        drop(output);

        let invalid_source =
            "use test_support::FixtureOnly;\npub fn impossible(value: FixtureOnly) {}\n";
        fs::write(fixture.0.join("src/lib.arc"), invalid_source).unwrap();
        let workspace = load_workspace(&ManifestRequest::discover_from(&fixture.0)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        let error = check_workspace_c1(&workspace, &graph, &[]).unwrap_err();
        assert_eq!(error.kind, FrontendErrorCode::Name);
        assert_eq!(error.diagnostic.code, "NAME002");
        assert_eq!(
            error.diagnostic.message,
            "unresolved import in C1 workspace name resolution"
        );
        assert_eq!(error.diagnostic.primary.message, error.diagnostic.message);
        assert_eq!(
            error.diagnostic.primary.span,
            Some(Span {
                file: FileId(0),
                start: SourcePosition {
                    byte: 4,
                    line: 1,
                    column: 5,
                },
                end: SourcePosition {
                    byte: 29,
                    line: 1,
                    column: 30,
                },
            })
        );
        assert!(error.diagnostic.secondary.is_empty());
        assert!(error.diagnostic.notes.is_empty());
    }

    #[test]
    fn owned_method_generics_use_outer_binder_depth_one() {
        let output = corpus("language-game");
        let methods = output
            .hir
            .packages
            .iter()
            .flat_map(|package| &package.targets)
            .flat_map(|target| &target.items)
            .filter(|item| item.owner.is_some() && item.name.as_deref() == Some("combine"))
            .collect::<Vec<_>>();
        assert_eq!(methods.len(), 4);
        assert!(methods.iter().all(|method| {
            method.symbolic_shape.types.iter().any(|ty| {
                matches!(
                    ty,
                    ResolvedSymbolicType::Resolved(ty)
                        if matches!(ty.as_ref(), SymbolicType::BoundType { depth: 1, .. })
                )
            })
        }));
    }

    #[test]
    fn integer_const_generic_actuals_use_the_resolved_formal_kind_everywhere() {
        let output = inline_library(concat!(
            "pub struct Tiny<const N: u8> { value: u8 }\n",
            "pub struct Boxed<T> { value: T }\n",
            "pub type Direct = Tiny<const 7>;\n",
            "pub type Nested = Boxed<Tiny<const 9>>;\n",
            "pub fn carry<const N: u8>(value: Tiny<const N>) -> Tiny<const N> { value }\n",
            "pub struct Owner<const N: u8> { value: u8 }\n",
            "impl<const N: u8> Owner<const N> {\n",
            "    pub fn owned(&self, value: Tiny<const N>) -> Tiny<const N> { value }\n",
            "}\n",
        ));
        let target = &output.hir.packages[0].targets[0];
        let item = |name: &str| {
            target
                .items
                .iter()
                .find(|item| item.name.as_deref() == Some(name))
                .unwrap()
        };
        fn resolved_type(item: &ResolvedSymbolicItem, index: usize) -> &SymbolicType {
            let ResolvedSymbolicType::Resolved(ty) = &item.symbolic_shape.types[index] else {
                panic!("{index} on {:?} must be resolved", item.name);
            };
            ty.as_ref()
        }
        fn resolved_nominal<'a>(item: &'a ResolvedSymbolicItem, name: &str) -> &'a SymbolicType {
            item.symbolic_shape
                .types
                .iter()
                .find_map(|ty| match ty {
                    ResolvedSymbolicType::Resolved(ty)
                        if matches!(
                            ty.as_ref(),
                            SymbolicType::NominalPath { declaration, .. }
                                if declaration.name == name
                        ) =>
                    {
                        Some(ty.as_ref())
                    }
                    ResolvedSymbolicType::Resolved(_) | ResolvedSymbolicType::Pending { .. } => {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("missing resolved nominal {name}"))
        }
        let tiny_argument = |ty: &SymbolicType| {
            let SymbolicType::NominalPath {
                declaration,
                arguments,
            } = ty
            else {
                panic!("Tiny must lower to a nominal path");
            };
            assert_eq!(declaration.name, "Tiny");
            let [GenericArgumentShape::IntegerConst(value)] = arguments.as_slice() else {
                panic!("Tiny must retain one integer-const argument");
            };
            value.clone()
        };

        assert_eq!(
            tiny_argument(resolved_type(item("Direct"), 0)),
            SymbolicConstExpression {
                integer_type: IntegerType::U8,
                node: SymbolicConstNode::IntegerLiteral(vec![7]),
            }
        );
        let SymbolicType::NominalPath { arguments, .. } = resolved_type(item("Nested"), 0) else {
            panic!("Nested must lower to a nominal path");
        };
        let [GenericArgumentShape::Type(nested_tiny)] = arguments.as_slice() else {
            panic!("Boxed must retain its nested Tiny type argument");
        };
        assert_eq!(
            tiny_argument(nested_tiny),
            SymbolicConstExpression {
                integer_type: IntegerType::U8,
                node: SymbolicConstNode::IntegerLiteral(vec![9]),
            }
        );
        assert_eq!(
            tiny_argument(resolved_type(item("carry"), 0)),
            SymbolicConstExpression {
                integer_type: IntegerType::U8,
                node: SymbolicConstNode::Bound { depth: 0, index: 0 },
            }
        );
        assert_eq!(
            tiny_argument(resolved_nominal(item("owned"), "Tiny")),
            SymbolicConstExpression {
                integer_type: IntegerType::U8,
                node: SymbolicConstNode::Bound { depth: 1, index: 0 },
            }
        );

        for (name, expected_nodes) in [
            ("Direct", vec![SymbolicConstNode::IntegerLiteral(vec![7])]),
            ("Nested", vec![SymbolicConstNode::IntegerLiteral(vec![9])]),
            (
                "carry",
                vec![
                    SymbolicConstNode::Bound { depth: 0, index: 0 },
                    SymbolicConstNode::Bound { depth: 0, index: 0 },
                ],
            ),
            (
                "owned",
                vec![
                    SymbolicConstNode::Bound { depth: 1, index: 0 },
                    SymbolicConstNode::Bound { depth: 1, index: 0 },
                ],
            ),
        ] {
            let actual = item(name)
                .symbolic_shape
                .consts
                .iter()
                .map(|value| {
                    let ResolvedSymbolicConst::Resolved(value) = value else {
                        panic!("{name} const row must resolve");
                    };
                    assert_eq!(value.integer_type, IntegerType::U8);
                    value.node.clone()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                actual, expected_nodes,
                "{name} flattened const rows changed"
            );
        }
    }

    #[test]
    fn wrong_generic_argument_kind_stays_an_exact_c1_pending_shape() {
        let output = inline_library(concat!(
            "pub struct Tiny<const N: u8> { value: u8 }\n",
            "pub type Wrong = Tiny<i32>;\n",
        ));
        let wrong = output.hir.packages[0].targets[0]
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("Wrong"))
            .unwrap();
        assert!(
            matches!(
                &wrong.symbolic_shape.types[0],
                ResolvedSymbolicType::Pending {
                    reason: UnresolvedPathKind::GenericFormationPendingC2,
                    canonical,
                    ..
                } if canonical == "<i32>::Tiny"
            ),
            "{:#?}",
            wrong.symbolic_shape.types[0]
        );
        let path_use = wrong
            .path_uses
            .iter()
            .find(|path_use| !path_use.generic_arguments.is_empty())
            .unwrap();
        assert!(matches!(
            &path_use.generic_arguments[0],
            HirGenericArgumentUse {
                formal_kind: Some(GenericParameterKind::IntegerConst(IntegerType::U8)),
                value: ResolvedGenericArgument::Type(ResolvedSymbolicType::Pending {
                    reason: UnresolvedPathKind::GenericFormationPendingC2,
                    ..
                }),
                ..
            }
        ));
    }

    #[test]
    fn ordered_generic_actuals_retain_lifetimes_formal_kinds_and_outer_depth() {
        let output = inline_library(concat!(
            "pub fn sink<'x, U, const M: u8>(value: &'x U) -> &'x U { value }\n",
            "pub struct Wrapper<'a, T, const N: u8> { value: &'a T }\n",
            "impl<'a, T, const N: u8> Wrapper<'a, T, const N> {\n",
            "    pub fn bound(&self) -> &'a T {\n",
            "        sink::<'a, T, const N>(self.value)\n",
            "    }\n",
            "    pub fn static_lifetime(&self) -> &'a T {\n",
            "        sink::<'static, T, const N>(self.value)\n",
            "    }\n",
            "}\n",
        ));
        let target = &output.hir.packages[0].targets[0];
        let arguments = |item_name: &str| {
            target
                .items
                .iter()
                .find(|item| item.name.as_deref() == Some(item_name))
                .unwrap()
                .postfix_generic_argument_uses
                .iter()
                .find(|generic_use| !generic_use.arguments.is_empty())
                .unwrap()
                .arguments
                .clone()
        };
        let bound = arguments("bound");
        assert_eq!(bound.len(), 3);
        assert_eq!(
            bound
                .iter()
                .map(|argument| argument.formal_kind.clone().unwrap())
                .collect::<Vec<_>>(),
            [
                GenericParameterKind::Lifetime,
                GenericParameterKind::Type,
                GenericParameterKind::IntegerConst(IntegerType::U8),
            ]
        );
        assert!(matches!(
            bound[0].value,
            ResolvedGenericArgument::Lifetime(ResolvedSymbolicLifetime::Resolved(
                SymbolicLifetime::Bound { depth: 1, index: 0 }
            ))
        ));
        assert!(matches!(
            bound[1].value,
            ResolvedGenericArgument::Type(ResolvedSymbolicType::Resolved(ref ty))
                if **ty == SymbolicType::BoundType { depth: 1, index: 1 }
        ));
        assert!(matches!(
            bound[2].value,
            ResolvedGenericArgument::IntegerConst(ResolvedSymbolicConst::Resolved(
                SymbolicConstExpression {
                    integer_type: IntegerType::U8,
                    node: SymbolicConstNode::Bound { depth: 1, index: 2 },
                }
            ))
        ));
        let static_arguments = arguments("static_lifetime");
        assert!(matches!(
            static_arguments[0].value,
            ResolvedGenericArgument::Lifetime(ResolvedSymbolicLifetime::Resolved(
                SymbolicLifetime::Static
            ))
        ));

        let original_dump = dump_hir_c1(&output.hir).unwrap();
        let mut mutated = output.hir.clone();
        let argument = mutated
            .packages
            .iter_mut()
            .flat_map(|package| &mut package.targets)
            .flat_map(|target| &mut target.items)
            .find(|item| item.name.as_deref() == Some("bound"))
            .unwrap()
            .postfix_generic_argument_uses
            .iter_mut()
            .find(|generic_use| !generic_use.arguments.is_empty())
            .unwrap()
            .arguments
            .first_mut()
            .unwrap();
        argument.value = ResolvedGenericArgument::Lifetime(ResolvedSymbolicLifetime::Resolved(
            SymbolicLifetime::Static,
        ));
        assert_ne!(dump_hir_c1(&mutated).unwrap(), original_dump);

        for name in ["language-game", "language-environment"] {
            let corpus = corpus(name);
            let call = corpus
                .hir
                .packages
                .iter()
                .flat_map(|package| &package.targets)
                .flat_map(|target| &target.items)
                .find(|item| item.name.as_deref() == Some("explicit_lifetime_call"))
                .unwrap();
            assert!(call
                .postfix_generic_argument_uses
                .iter()
                .flat_map(|generic_use| &generic_use.arguments)
                .any(|argument| {
                    matches!(
                        argument,
                        HirGenericArgumentUse {
                            formal_kind: Some(GenericParameterKind::Lifetime),
                            value: ResolvedGenericArgument::Lifetime(
                                ResolvedSymbolicLifetime::Resolved(SymbolicLifetime::Bound { .. })
                            ),
                            ..
                        }
                    )
                }));
        }
    }

    #[test]
    fn declaration_elision_uses_hidden_binders_and_receiver_output_association() {
        let output = inline_library(concat!(
            "pub fn borrow(value: &i32) -> &i32 { value }\n",
            "pub struct Wrapper { value: i32 }\n",
            "impl Wrapper {\n",
            "    pub fn elided(&self) -> &i32 { &self.value }\n",
            "    pub fn named<'a>(&'a self) -> &'a i32 { &self.value }\n",
            "    pub fn static_receiver(&'static self) -> &'static i32 { &self.value }\n",
            "    pub fn mutable(&mut self) -> &mut i32 { &mut self.value }\n",
            "}\n",
            "pub fn body_local(value: *const i32) { let _ = value as &i32; }\n",
        ));
        let target = &output.hir.packages[0].targets[0];
        let item = |name: &str| {
            target
                .items
                .iter()
                .find(|item| item.name.as_deref() == Some(name))
                .unwrap()
        };
        let reference_lifetimes = |name: &str| {
            item(name)
                .symbolic_shape
                .types
                .iter()
                .filter_map(|ty| match ty {
                    ResolvedSymbolicType::Resolved(ty) => match ty.as_ref() {
                        SymbolicType::Reference { lifetime, .. } => Some(lifetime.clone()),
                        _ => None,
                    },
                    ResolvedSymbolicType::Pending { .. } => None,
                })
                .collect::<Vec<_>>()
        };

        let borrow = item("borrow");
        assert_eq!(borrow.symbolic_shape.hidden_lifetime_binders.len(), 1);
        assert_eq!(
            borrow.symbolic_shape.hidden_lifetime_binders[0].source,
            HiddenLifetimeBinderSource::Input
        );
        assert_eq!(borrow.symbolic_shape.hidden_lifetime_binders[0].index, 0);
        assert_eq!(
            reference_lifetimes("borrow"),
            [
                SymbolicLifetime::Bound { depth: 0, index: 0 },
                SymbolicLifetime::Bound { depth: 0, index: 0 },
            ]
        );

        let elided = item("elided");
        assert_eq!(elided.symbolic_shape.hidden_lifetime_binders.len(), 1);
        assert_eq!(
            elided.symbolic_shape.hidden_lifetime_binders[0].source,
            HiddenLifetimeBinderSource::Receiver
        );
        assert!(reference_lifetimes("elided")
            .iter()
            .all(|lifetime| *lifetime == SymbolicLifetime::Bound { depth: 0, index: 0 }));
        assert!(item("named")
            .symbolic_shape
            .hidden_lifetime_binders
            .is_empty());
        assert!(reference_lifetimes("named")
            .iter()
            .all(|lifetime| *lifetime == SymbolicLifetime::Bound { depth: 0, index: 0 }));
        assert!(item("static_receiver")
            .symbolic_shape
            .hidden_lifetime_binders
            .is_empty());
        assert!(reference_lifetimes("static_receiver")
            .iter()
            .all(|lifetime| *lifetime == SymbolicLifetime::Static));
        assert!(matches!(
            item("mutable").symbolic_shape.types.first(),
            Some(ResolvedSymbolicType::Resolved(ty))
                if matches!(ty.as_ref(), SymbolicType::Reference { mutability: Mutability::Mutable, .. })
        ));

        let definition_shape = |name: &str| {
            output.inventory.packages[0]
                .definitions
                .iter()
                .find(|definition| definition.key.name == name)
                .unwrap()
                .symbolic_shape
                .clone()
        };
        assert_eq!(
            definition_shape("elided"),
            definition_shape("named"),
            "alpha-normalized hidden and explicit lifetime binders share one shape"
        );
        assert_ne!(
            definition_shape("named"),
            definition_shape("static_receiver")
        );
        assert_ne!(definition_shape("elided"), definition_shape("mutable"));

        let body_local = item("body_local");
        assert!(body_local
            .body_symbolic_shape
            .types
            .iter()
            .any(|ty| matches!(
                ty,
                ResolvedSymbolicType::Resolved(ty)
                    if symbolic_type_has_erased_local(ty)
            )));
        for declaration in target.items.iter() {
            assert_declaration_shape_has_no_erased_local(declaration, &declaration.symbolic_shape)
                .unwrap();
        }

        for source in [
            "pub fn ambiguous(left: &i32, right: &i32) -> &i32 { left }\n",
            "pub fn missing() -> &i32 { loop {} }\n",
            "pub struct Invalid { value: &i32 }\n",
        ] {
            let error = inline_library_result(source).unwrap_err();
            assert_eq!(error.diagnostic.code, "TYPE001", "{source}");
        }
    }

    #[test]
    fn lowercase_imported_const_and_unit_variant_paths_never_become_bindings() {
        let output = inline_library_with_files(
            concat!(
                "mod values;\n",
                "mod flags;\n",
                "pub use self::values::lower_const;\n",
                "pub use self::flags::Flag;\n",
                "pub use self::flags::Flag::lower;\n",
                "pub fn classify(value: i32, choice: Flag) -> i32 {\n",
                "    let from_const = match value {\n",
                "        lower_const => 1i32,\n",
                "        captured => captured,\n",
                "    };\n",
                "    let from_variant = match choice {\n",
                "        lower => 2i32,\n",
                "    };\n",
                "    from_const + from_variant\n",
                "}\n",
            ),
            &[
                ("src/values.arc", "pub const lower_const: i32 = 7i32;\n"),
                ("src/flags.arc", "pub enum Flag { lower, upper(i32), }\n"),
            ],
        )
        .unwrap();
        let target = &output.hir.packages[0].targets[0];
        let item = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("classify"))
            .unwrap();
        assert!(item
            .locals
            .iter()
            .all(|local| local.name != "lower" && local.name != "lower_const"));
        assert!(item.locals.iter().any(|local| local.name == "captured"));

        let path_use = |canonical: &str| {
            item.path_uses
                .iter()
                .find(|path_use| canonical_path(&path_use.path) == canonical)
                .unwrap()
        };
        let lower_const = path_use("lower_const");
        assert_eq!(lower_const.namespace, Some(Namespace::Value));
        assert_eq!(lower_const.lexical_local, None);
        let lower_const_resolution = target
            .path_resolutions
            .iter()
            .find(|resolution| resolution.span == lower_const.path.span)
            .unwrap();
        let [Res::Item(HirItemRes::Definition(lower_item))] =
            lower_const_resolution.resolutions.as_slice()
        else {
            panic!("imported lowercase const must resolve to its item");
        };
        assert_eq!(
            target
                .items
                .iter()
                .find(|item| item.id == *lower_item)
                .unwrap()
                .kind,
            DeclarationKind::Const
        );

        let variant = path_use("lower");
        assert_eq!(variant.namespace, Some(Namespace::Value));
        assert_eq!(variant.lexical_local, None);
        let variant_resolution = target
            .path_resolutions
            .iter()
            .find(|resolution| resolution.span == variant.path.span)
            .unwrap();
        let [Res::Item(HirItemRes::EnumVariant { owner, ordinal: 0 })] =
            variant_resolution.resolutions.as_slice()
        else {
            panic!("bare imported lowercase unit variant must retain its exact member identity");
        };
        assert_eq!(
            target
                .items
                .iter()
                .find(|item| item.id == *owner)
                .unwrap()
                .name
                .as_deref(),
            Some("Flag")
        );
        assert_eq!(variant_resolution.unresolved, None);

        let inventory_binding = output.inventory.packages[0]
            .modules
            .iter()
            .find(|module| module.module.path.is_empty())
            .unwrap()
            .bindings
            .iter()
            .find(|binding| binding.name == "lower" && binding.namespace == Namespace::Value)
            .unwrap();
        let SemanticBindingTarget::EnumVariant {
            owner: inventory_owner,
            ordinal: 0,
        } = &inventory_binding.target
        else {
            panic!("inventory re-export must preserve the exact enum member target");
        };
        assert_eq!(inventory_owner.kind, DeclarationKind::Enum);
        assert_eq!(inventory_owner.name, "Flag");
        let SemanticBindingOrigin::ReExport {
            source,
            target: origin_target,
        } = &inventory_binding.origin
        else {
            panic!("public imported variant must retain its re-export origin");
        };
        assert_eq!(source.module.path, ["flags"]);
        assert_eq!(source.segments, ["Flag", "lower"]);
        assert_eq!(source.namespace, Namespace::Value);
        assert_eq!(origin_target.as_ref(), &inventory_binding.target);
    }

    #[test]
    fn public_nominal_constructor_reexport_retains_its_source_path() {
        let output = inline_library_with_files(
            concat!(
                "mod values;\n",
                "pub use self::values::Wrapper;\n",
                "pub fn make() -> Wrapper { Wrapper { value: 1i32 } }\n",
            ),
            &[("src/values.arc", "pub struct Wrapper { pub value: i32 }\n")],
        )
        .unwrap();
        let root = output.inventory.packages[0]
            .modules
            .iter()
            .find(|module| module.module.path.is_empty())
            .unwrap();
        let constructor = root
            .bindings
            .iter()
            .find(|binding| {
                binding.name == "Wrapper"
                    && binding.namespace == Namespace::Value
                    && matches!(binding.target, SemanticBindingTarget::NominalConstructor(_))
            })
            .unwrap();
        let SemanticBindingOrigin::ReExport { source, target } = &constructor.origin else {
            panic!("constructor import must retain a re-export origin");
        };
        assert_eq!(source.module.path, ["values"]);
        assert_eq!(source.segments, ["Wrapper"]);
        assert_eq!(source.namespace, Namespace::Value);
        assert_eq!(target.as_ref(), &constructor.target);
    }

    #[test]
    fn or_pattern_alternatives_share_one_binding_identity() {
        let output = inline_library(concat!(
            "pub fn choose(value: i32) -> i32 {\n",
            "    match (value, value) {\n",
            "        (shared | shared, _) if shared > 0i32 => shared,\n",
            "        _ => 0i32,\n",
            "    }\n",
            "}\n",
        ));
        let target = &output.hir.packages[0].targets[0];
        let item = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("choose"))
            .unwrap();
        let shared = item
            .locals
            .iter()
            .filter(|local| local.name == "shared")
            .collect::<Vec<_>>();
        assert_eq!(shared.len(), 1);
        let uses = target
            .path_resolutions
            .iter()
            .filter(|resolution| resolution.resolutions == [Res::Local(shared[0].id)])
            .count();
        assert_eq!(uses, 2, "guard and arm value must share the same LocalId");
    }

    #[test]
    fn inconsistent_or_pattern_binding_sets_are_pattern001_deterministically() {
        let error = inline_library_result(concat!(
            "pub fn choose(value: i32) -> i32 {\n",
            "    match (value, value) {\n",
            "        (left | right, _) => 0i32,\n",
            "        _ => 1i32,\n",
            "    }\n",
            "}\n",
        ))
        .unwrap_err();
        assert_eq!(error.kind, FrontendErrorCode::Name);
        assert_eq!(error.diagnostic.code, "PATTERN001");
        assert_eq!(
            error.diagnostic.message,
            "or-pattern alternative binds {\"right\"}, expected {\"left\"}"
        );
        assert_eq!(error.diagnostic.secondary.len(), 1);
        assert_eq!(
            error.diagnostic.secondary[0].message,
            "first alternative establishes the binding set"
        );
    }

    #[test]
    fn nominal_constructors_and_variants_have_exact_value_identities() {
        let output = inline_library(concat!(
            "pub struct Wrapper { pub value: i32 }\n",
            "pub enum Choice { None, One(i32), Pair { left: i32, right: i32 } }\n",
            "pub fn construct(choice: Choice) -> Wrapper {\n",
            "    let _ = match choice {\n",
            "        Choice::None => 0i32,\n",
            "        Choice::One(value) => value,\n",
            "        Choice::Pair { left: left, right: right } => left + right,\n",
            "    };\n",
            "    Wrapper { value: 1i32 }\n",
            "}\n",
        ));
        let target = &output.hir.packages[0].targets[0];
        let root = &target.modules[0];
        let wrapper = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("Wrapper"))
            .unwrap();
        let choice = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("Choice"))
            .unwrap();
        assert!(root.bindings.iter().any(|binding| {
            binding.name == "Wrapper"
                && binding.namespace == Namespace::Type
                && binding.target == HirBindingTarget::Item(HirItemRes::Definition(wrapper.id))
        }));
        assert!(root.bindings.iter().any(|binding| {
            binding.name == "Wrapper"
                && binding.namespace == Namespace::Value
                && binding.target
                    == HirBindingTarget::Item(HirItemRes::NominalConstructor { owner: wrapper.id })
        }));
        assert!(root.bindings.iter().any(|binding| {
            binding.name == "Choice"
                && binding.namespace == Namespace::Type
                && binding.target == HirBindingTarget::Item(HirItemRes::Definition(choice.id))
        }));
        assert!(!root
            .bindings
            .iter()
            .any(|binding| binding.name == "Choice" && binding.namespace == Namespace::Value));

        let construct = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("construct"))
            .unwrap();
        let resolution_for = |canonical: &str, namespace: Namespace| {
            let path_use = construct
                .path_uses
                .iter()
                .find(|path_use| {
                    canonical_path(&path_use.path) == canonical
                        && path_use.namespace == Some(namespace)
                })
                .unwrap();
            target
                .path_resolutions
                .iter()
                .find(|resolution| resolution.span == path_use.path.span)
                .unwrap()
        };
        assert_eq!(
            resolution_for("Wrapper", Namespace::Value).resolutions,
            [Res::Item(HirItemRes::NominalConstructor {
                owner: wrapper.id,
            })]
        );
        for (name, ordinal) in [("Choice::None", 0), ("Choice::One", 1), ("Choice::Pair", 2)] {
            assert_eq!(
                resolution_for(name, Namespace::Value).resolutions,
                [Res::Item(HirItemRes::EnumVariant {
                    owner: choice.id,
                    ordinal,
                })],
                "{name} lost its exact variant ordinal"
            );
        }

        let wrapper_bindings = output.inventory.packages[0]
            .modules
            .iter()
            .find(|module| module.module.path.is_empty())
            .unwrap()
            .bindings
            .iter()
            .filter(|binding| binding.name == "Wrapper")
            .collect::<Vec<_>>();
        assert_eq!(wrapper_bindings.len(), 2);
        assert!(wrapper_bindings
            .iter()
            .any(|binding| matches!(binding.target, SemanticBindingTarget::Definition(_))));
        assert!(wrapper_bindings
            .iter()
            .any(|binding| matches!(binding.target, SemanticBindingTarget::NominalConstructor(_))));
    }

    #[test]
    fn mandatory_system_parameters_and_receivers_resolve_to_checked_locals() {
        let game = corpus("language-game");
        let environment = corpus("language-environment");
        for (output, expectations) in [
            (
                &game,
                &[
                    ("Advance", &["clock", "positions", "cmd", "udp"][..]),
                    ("GenericSystem", &["board", "positions", "cmd", "stdio"][..]),
                    ("EmptyQuerySystem", &["empty"][..]),
                ][..],
            ),
            (
                &environment,
                &[
                    ("ResetSystem", &["episode", "agents", "cmd"][..]),
                    ("StepSystem", &["episode", "agents"][..]),
                    ("SelfPlaySystem", &["episode", "agents"][..]),
                ][..],
            ),
        ] {
            for (system_name, expected_names) in expectations {
                let (target, item) = output
                    .hir
                    .packages
                    .iter()
                    .flat_map(|package| &package.targets)
                    .find_map(|target| {
                        target
                            .items
                            .iter()
                            .find(|item| item.name.as_deref() == Some(*system_name))
                            .map(|item| (target, item))
                    })
                    .unwrap_or_else(|| panic!("missing system {system_name}"));
                assert_eq!(
                    item.locals
                        .iter()
                        .take(expected_names.len())
                        .map(|local| local.name.as_str())
                        .collect::<Vec<_>>(),
                    *expected_names,
                    "{system_name} parameter LocalIds changed source order"
                );
                for (ordinal, expected_name) in expected_names.iter().enumerate() {
                    let local = &item.locals[ordinal];
                    assert_eq!(local.id.owner, item.id);
                    assert_eq!(local.id.ordinal, u64::try_from(ordinal).unwrap());
                    assert_eq!(&local.name, expected_name);
                    for path_use in item
                        .path_uses
                        .iter()
                        .filter(|path_use| path_use.lexical_local == Some(local.id))
                    {
                        let resolution = target
                            .path_resolutions
                            .iter()
                            .find(|resolution| resolution.span == path_use.path.span)
                            .unwrap();
                        assert_eq!(resolution.resolutions, [Res::Local(local.id)]);
                        assert_eq!(resolution.unresolved, None);
                    }
                }
            }
        }

        let mut self_use_count = 0;
        for output in [&game, &environment] {
            for target in output
                .hir
                .packages
                .iter()
                .flat_map(|package| &package.targets)
            {
                for item in &target.items {
                    for self_use in &item.self_uses {
                        self_use_count += 1;
                        let receiver = item
                            .locals
                            .iter()
                            .find(|local| local.id == self_use.receiver)
                            .expect("every self expression names its retained receiver local");
                        assert_eq!(receiver.name, "self");
                        let resolution = target
                            .path_resolutions
                            .iter()
                            .find(|resolution| resolution.span == self_use.span)
                            .unwrap();
                        assert_eq!(resolution.resolutions, [Res::Local(receiver.id)]);
                        assert_eq!(resolution.unresolved, None);
                    }
                    if matches!(
                        &item.source,
                        HirItemSource::ImplMethod(method)
                            if method
                                .signature
                                .parameters
                                .iter()
                                .all(|parameter| !matches!(parameter, AstMethodParameter::Receiver(_)))
                    ) {
                        assert!(item.self_uses.is_empty());
                        assert!(item.locals.iter().all(|local| local.name != "self"));
                    }
                }
            }
        }
        assert!(
            self_use_count >= 8,
            "mandatory receiver coverage unexpectedly shrank"
        );

        let error = inline_library_result(concat!(
            "pub struct Wrapper {}\n",
            "impl Wrapper {\n",
            "    pub fn invalid() { let _ = self; }\n",
            "}\n",
        ))
        .unwrap_err();
        assert_eq!(error.kind, FrontendErrorCode::Name);
        assert_eq!(error.diagnostic.code, "NAME001");
        assert_eq!(
            error.diagnostic.message,
            "lowercase `self` is only available in a method with a receiver"
        );
    }

    #[test]
    fn system_capability_access_tag_distinguishes_shared_and_mutable_references() {
        let output = inline_library(concat!(
            "pub struct Fake;\n",
            "pub system Shared(capability: &Udp) requires {} throws {} {}\n",
            "pub system Mutable(capability: &mut Udp) requires {} throws {} {}\n",
            "pub system Overlap(capability: &Udp) requires {Udp} throws {} {}\n",
            "pub system PendingFake(capability: &Fake) requires {} throws {} {}\n",
        ));
        let shape = |name: &str| {
            output.inventory.packages[0]
                .definitions
                .iter()
                .find(|definition| definition.key.name == name)
                .unwrap()
                .symbolic_shape
                .clone()
        };
        let shared_shape = shape("Shared");
        let mutable_shape = shape("Mutable");
        let SymbolicDeclarationPayloadSkeleton::System {
            accesses: shared_accesses,
            implied_requires: shared_implied,
            effects: shared_effects,
            ..
        } = &shared_shape.payload
        else {
            panic!("system definition retains a typed system payload");
        };
        let SymbolicDeclarationPayloadSkeleton::System {
            accesses: mutable_accesses,
            implied_requires: mutable_implied,
            ..
        } = &mutable_shape.payload
        else {
            panic!("system definition retains a typed system payload");
        };
        let shared = shared_accesses[0].clone();
        let mutable = mutable_accesses[0].clone();
        assert!(matches!(
            shared,
            SymbolicSystemAccessShapeSkeleton::CapabilityShared(_)
        ));
        assert!(matches!(
            mutable,
            SymbolicSystemAccessShapeSkeleton::CapabilityMutable(_)
        ));
        assert_ne!(shared, mutable);
        assert!(shared_effects.requires.is_empty());
        assert_eq!(shared_implied.len(), 1);
        assert_eq!(mutable_implied.len(), 1);
        assert_eq!(shared_implied[0].parameter_ordinal, 0);
        assert_eq!(
            shared_implied[0].access,
            SymbolicCapabilityAccessMode::Shared
        );
        assert_eq!(
            mutable_implied[0].access,
            SymbolicCapabilityAccessMode::Mutable
        );
        assert_eq!(shared_implied[0].referent, mutable_implied[0].referent);
        assert_eq!(
            shared_implied[0].readiness,
            SymbolicShapeReadiness::PendingC4
        );
        assert!(try_canonicalize_declaration_shape(&shared_shape)
            .unwrap()
            .is_none());
        assert_ne!(
            encode_symbolic_declaration_shape_skeleton_c1(&shared_shape).unwrap(),
            encode_symbolic_declaration_shape_skeleton_c1(&mutable_shape).unwrap()
        );

        let overlap = shape("Overlap");
        let SymbolicDeclarationPayloadSkeleton::System {
            implied_requires,
            effects,
            ..
        } = &overlap.payload
        else {
            unreachable!();
        };
        assert_eq!(implied_requires.len(), 1);
        assert_eq!(effects.requires.len(), 1);

        let pending_fake = shape("PendingFake");
        let SymbolicDeclarationPayloadSkeleton::System {
            implied_requires, ..
        } = &pending_fake.payload
        else {
            unreachable!();
        };
        assert_eq!(implied_requires.len(), 1);
        assert_eq!(
            implied_requires[0].readiness,
            SymbolicShapeReadiness::PendingC4
        );
        assert!(try_canonicalize_declaration_shape(&pending_fake)
            .unwrap()
            .is_none());
    }

    #[test]
    fn c1_retains_duplicate_effect_atoms_in_source_order_pending_c4() {
        let output = inline_library(concat!(
            "pub fn duplicate_effects()\n",
            "    requires {Stdio, Udp, Stdio}\n",
            "    throws {i32, u8, i32} {}\n",
        ));
        let definition = output.inventory.packages[0]
            .definitions
            .iter()
            .find(|definition| definition.key.name == "duplicate_effects")
            .expect("C1 accepts duplicate source effects");
        let SymbolicDeclarationPayloadSkeleton::Callable(callable) =
            &definition.symbolic_shape.payload
        else {
            panic!("function retains a callable symbolic shape");
        };

        assert_eq!(callable.effects.requires.len(), 3);
        assert_eq!(callable.effects.throws.len(), 3);
        assert!(callable
            .effects
            .requires
            .iter()
            .chain(&callable.effects.throws)
            .all(|effect| effect.readiness() == SymbolicShapeReadiness::PendingC4));

        let requires_names = callable
            .effects
            .requires
            .iter()
            .map(|effect| match effect {
                SymbolicEffectShapeSkeleton::Resolved {
                    value: SymbolicType::NominalPath { declaration, .. },
                    ..
                } => declaration.name.as_str(),
                effect => panic!("requires atom is not a resolved nominal path: {effect:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(requires_names, ["Stdio", "Udp", "Stdio"]);

        let throws_types = callable
            .effects
            .throws
            .iter()
            .map(|effect| match effect {
                SymbolicEffectShapeSkeleton::Resolved { value, .. } => value,
                effect => panic!("throws atom is not a resolved type: {effect:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            throws_types,
            [&SymbolicType::I32, &SymbolicType::U8, &SymbolicType::I32]
        );
        assert_eq!(
            declaration_shape_readiness(&definition.symbolic_shape).unwrap(),
            SymbolicShapeReadiness::PendingC4
        );
        assert!(
            try_canonicalize_declaration_shape(&definition.symbolic_shape)
                .unwrap()
                .is_none(),
            "C1 must not forge a canonical effect-set identity"
        );

        let source_order =
            encode_symbolic_declaration_shape_skeleton_c1(&definition.symbolic_shape).unwrap();
        let mut reordered = definition.symbolic_shape.clone();
        let SymbolicDeclarationPayloadSkeleton::Callable(callable) = &mut reordered.payload else {
            unreachable!();
        };
        callable.effects.requires.swap(0, 1);
        assert_ne!(
            source_order,
            encode_symbolic_declaration_shape_skeleton_c1(&reordered).unwrap(),
            "the C1 debug projection must retain source atom order"
        );
    }

    #[test]
    fn c1_retains_nested_function_pointer_effects_pending_c4() {
        let output = inline_library(concat!(
            "pub type Callback = fn(usize)\n",
            "    requires {Udp, Stdio, Udp}\n",
            "    throws {i32, u8, i32} -> ();\n",
            "pub type Task = JoinHandle<i32, u8, i32>;\n",
        ));
        let definition = output.inventory.packages[0]
            .definitions
            .iter()
            .find(|definition| definition.key.name == "Callback")
            .expect("C1 retains the function-pointer alias");
        let SymbolicDeclarationPayloadSkeleton::Alias { target } =
            &definition.symbolic_shape.payload
        else {
            panic!("function-pointer alias retains an alias shape");
        };
        let SymbolicTypeShapeSkeleton::Resolved {
            value:
                SymbolicType::FunctionPointer {
                    requires, throws, ..
                },
            readiness,
        } = target
        else {
            panic!("alias target is a resolved function-pointer skeleton");
        };
        assert_eq!(*readiness, SymbolicShapeReadiness::PendingC4);
        assert_eq!(requires.readiness(), SymbolicShapeReadiness::PendingC4);
        assert_eq!(throws.readiness(), SymbolicShapeReadiness::PendingC4);
        assert_eq!(
            requires
                .members()
                .iter()
                .map(|ty| match ty {
                    SymbolicType::NominalPath { declaration, .. } => declaration.name.as_str(),
                    ty => panic!("requires member is not a nominal capability: {ty:?}"),
                })
                .collect::<Vec<_>>(),
            ["Udp", "Stdio", "Udp"]
        );
        assert_eq!(
            throws.members(),
            [SymbolicType::I32, SymbolicType::U8, SymbolicType::I32]
        );
        assert_eq!(
            declaration_shape_readiness(&definition.symbolic_shape).unwrap(),
            SymbolicShapeReadiness::PendingC4
        );
        assert!(
            try_canonicalize_declaration_shape(&definition.symbolic_shape)
                .unwrap()
                .is_none()
        );
        assert!(encode_inventory_c1(&output.inventory).is_ok());

        let task = output.inventory.packages[0]
            .definitions
            .iter()
            .find(|definition| definition.key.name == "Task")
            .expect("C1 retains the JoinHandle alias");
        let SymbolicDeclarationPayloadSkeleton::Alias {
            target:
                SymbolicTypeShapeSkeleton::Resolved {
                    value: SymbolicType::JoinHandle { throws, .. },
                    readiness,
                },
        } = &task.symbolic_shape.payload
        else {
            panic!("JoinHandle alias retains the sealed tag-30 type shape");
        };
        assert_eq!(*readiness, SymbolicShapeReadiness::PendingC4);
        assert_eq!(throws.members(), [SymbolicType::U8, SymbolicType::I32]);
        assert_eq!(throws.readiness(), SymbolicShapeReadiness::PendingC4);

        let source_order =
            encode_symbolic_declaration_shape_skeleton_c1(&definition.symbolic_shape).unwrap();
        let mut reordered = definition.symbolic_shape.clone();
        let SymbolicDeclarationPayloadSkeleton::Alias { target } = &mut reordered.payload else {
            unreachable!();
        };
        let SymbolicTypeShapeSkeleton::Resolved {
            value: SymbolicType::FunctionPointer { requires, .. },
            ..
        } = target
        else {
            unreachable!();
        };
        let mut members = requires.members().to_vec();
        members.swap(0, 1);
        *requires = crate::SymbolicTypeEffectSet::pending_c4(members);
        assert_ne!(
            source_order,
            encode_symbolic_declaration_shape_skeleton_c1(&reordered).unwrap()
        );
    }

    #[test]
    fn mandatory_corpora_have_no_arbitrary_unknown_names() {
        for name in ["language-game", "language-environment"] {
            let output = corpus(name);
            let unknown = output
                .hir
                .packages
                .iter()
                .flat_map(|package| &package.targets)
                .flat_map(|target| &target.path_resolutions)
                .filter(|resolution| resolution.unresolved == Some(UnresolvedPathKind::UnknownName))
                .collect::<Vec<_>>();
            assert!(
                unknown.is_empty(),
                "{name} retained UnknownName rows: {unknown:#?}"
            );
        }
    }

    #[test]
    fn declaration_and_lexical_names_are_unique_in_their_frozen_scopes() {
        let generic_cases = [
            "pub struct Pair<T, const T: usize> { value: T }\n",
            "pub struct Pair<'a, a> { value: &'a a }\n",
            "pub struct Wrapper<T> { value: T }\nimpl<T, T> Wrapper<T> {}\n",
            "pub trait Marker { fn duplicate<T, T>(); }\n",
            "pub system Duplicate<T, T>() requires {} throws {} {}\n",
            concat!(
                "pub gen fn Duplicate<T, T>()\n",
                "    resume () yields () requires {} throws {} { () }\n",
            ),
            "pub trait Duplicate<T, T> {}\n",
        ];
        for source in generic_cases {
            let error = inline_library_result(source).unwrap_err();
            assert_eq!(error.kind, FrontendErrorCode::Name, "{source}");
            assert_eq!(error.diagnostic.code, "NAME001", "{source}");
            assert!(error.diagnostic.message.contains("generic parameter"));
            assert_eq!(error.diagnostic.secondary.len(), 1);
            assert_eq!(error.diagnostic.secondary[0].message, "first declared here");
        }

        let owner_shadow_cases = [
            concat!(
                "pub struct Wrapper<T> { value: T }\n",
                "impl<T> Wrapper<T> { fn duplicate<T>(&self) {} }\n",
            ),
            "pub trait Duplicate<'a> { fn method<a>(&self); }\n",
            concat!(
                "pub struct Wrapper<const N: usize> { value: [u8; N] }\n",
                "impl<const N: usize> Wrapper<const N> {\n",
                "    fn duplicate<const N: usize>(&self) {}\n",
                "}\n",
            ),
        ];
        for source in owner_shadow_cases {
            let error = inline_library_result(source).unwrap_err();
            assert_eq!(error.kind, FrontendErrorCode::Name, "{source}");
            assert_eq!(error.diagnostic.code, "NAME001", "{source}");
            assert!(error.diagnostic.message.contains("active owner parameter"));
            assert_eq!(error.diagnostic.secondary.len(), 1);
            assert_eq!(error.diagnostic.secondary[0].message, "first declared here");
        }
        inline_library("pub trait Siblings { fn first<T>(); fn second<T>(); }\n");

        let declaration_cases = [
            "pub struct Duplicate { same: i32, same: i32 }\n",
            "pub enum Duplicate { same, same(i32) }\n",
            "pub enum Duplicate { Only { same: i32, same: i32 } }\n",
            "pub trait Duplicate { fn same(); fn same(); }\n",
            concat!(
                "pub struct Duplicate {}\n",
                "impl Duplicate { fn same() {} fn same() {} }\n",
            ),
        ];
        for source in declaration_cases {
            let error = inline_library_result(source).unwrap_err();
            assert_eq!(error.kind, FrontendErrorCode::Name, "{source}");
            assert_eq!(error.diagnostic.code, "NAME001", "{source}");
            assert_eq!(error.diagnostic.secondary.len(), 1);
            assert_eq!(error.diagnostic.secondary[0].message, "first declared here");
        }

        let lexical_cases = [
            "pub fn duplicate(same: i32, same: i32) {}\n",
            "pub fn duplicate(pair: (i32, i32)) { let (same, same) = pair; }\n",
            "pub fn duplicate(same: i32) { { let same = 1i32; } }\n",
            "pub fn duplicate(same: i32) { let callback = |same| same; }\n",
            concat!(
                "pub fn duplicate(pair: (i32, i32)) -> i32 {\n",
                "    match pair { (same, same) | (same, same) => same }\n",
                "}\n",
            ),
        ];
        for source in lexical_cases {
            let error = inline_library_result(source).unwrap_err();
            assert_eq!(error.kind, FrontendErrorCode::Name, "{source}");
            assert_eq!(error.diagnostic.code, "NAME001", "{source}");
            assert_eq!(error.diagnostic.secondary.len(), 1);
            assert_eq!(error.diagnostic.secondary[0].message, "first bound here");
        }
    }

    #[test]
    fn root_only_visibility_paths_resolve_to_exact_modules() {
        let output = inline_library_with_files(
            concat!("mod inner;\n", "pub(in package) struct PackageVisible;\n",),
            &[(
                "src/inner.arc",
                concat!(
                    "pub(in self) struct SelfVisible;\n",
                    "pub(in super) struct ParentVisible;\n",
                ),
            )],
        )
        .unwrap();
        let target = &output.hir.packages[0].targets[0];
        let root = target
            .modules
            .iter()
            .find(|module| module.parent.is_none())
            .unwrap();
        let inner = target
            .modules
            .iter()
            .find(|module| {
                module
                    .name
                    .as_ref()
                    .is_some_and(|name| name.as_str() == "inner")
            })
            .unwrap();
        for (module, expected) in [(root, root.id), (inner, inner.id), (inner, root.id)] {
            let path = module
                .ast
                .items
                .iter()
                .filter_map(|item| match item {
                    AstItem::Declaration(declaration) => match &declaration.visibility.kind {
                        AstVisibilityKind::In(path) => Some(path),
                        AstVisibilityKind::Private
                        | AstVisibilityKind::Public
                        | AstVisibilityKind::Package
                        | AstVisibilityKind::Super => None,
                    },
                    AstItem::Module(_) | AstItem::Import(_) | AstItem::Impl(_) => None,
                })
                .find(|path| match expected == module.id {
                    true => matches!(
                        path.root,
                        crate::ast::AstPathRoot::Package | crate::ast::AstPathRoot::SelfValue
                    ),
                    false => matches!(path.root, crate::ast::AstPathRoot::Super(1)),
                })
                .unwrap();
            let resolution = target
                .path_resolutions
                .iter()
                .find(|resolution| resolution.span == path.span)
                .unwrap();
            assert_eq!(resolution.resolutions, [Res::Module(expected)]);
            assert_eq!(resolution.unresolved, None);
        }

        let error = inline_library_result("pub(in super) struct Invalid;\n").unwrap_err();
        assert_eq!(error.kind, FrontendErrorCode::Visibility);
        assert_eq!(error.diagnostic.code, "VISIBILITY001");
    }

    #[test]
    fn import_visibility_compares_module_audiences_in_their_declaring_contexts() {
        let root_source = "mod child;\nconst K: i32 = 1i32;\n";
        let output = inline_library_with_files(
            root_source,
            &[("src/child.arc", "pub(super) use super::K;\n")],
        )
        .unwrap();
        let target = &output.hir.packages[0].targets[0];
        let root = target
            .modules
            .iter()
            .find(|module| module.parent.is_none())
            .unwrap();
        let child = target
            .modules
            .iter()
            .find(|module| module.path.iter().map(Symbol::as_str).eq(["child"]))
            .unwrap();
        let source = root
            .bindings
            .iter()
            .find(|binding| binding.name == "K" && binding.namespace == Namespace::Value)
            .unwrap();
        let reexport = child
            .bindings
            .iter()
            .find(|binding| binding.name == "K" && binding.namespace == Namespace::Value)
            .unwrap();
        assert_eq!(
            reexport.declared_visibility,
            Visibility::AncestorModule { path: Vec::new() }
        );
        assert_eq!(reexport.target, source.target);
        let HirBindingOrigin::ReExport {
            source_module,
            source_segments,
            target: origin_target,
        } = &reexport.origin
        else {
            panic!("module-private source must retain an exact re-export origin");
        };
        assert_eq!(*source_module, root.id);
        assert_eq!(
            source_segments
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["K"]
        );
        assert_eq!(origin_target, &reexport.target);

        let widening = "pub use super::K;\n";
        let error =
            inline_library_with_files(root_source, &[("src/child.arc", widening)]).unwrap_err();
        assert_eq!(error.kind, FrontendErrorCode::Visibility);
        assert_eq!(error.diagnostic.code, "VISIBILITY004");
        assert_eq!(
            error.diagnostic.message,
            "an import visibility cannot widen the declaration it exposes"
        );
        assert_eq!(error.diagnostic.primary.message, error.diagnostic.message);
        assert_eq!(
            error.diagnostic.primary.span,
            Some(Span {
                file: FileId(1),
                start: SourcePosition {
                    byte: 4,
                    line: 1,
                    column: 5,
                },
                end: SourcePosition {
                    byte: u64::try_from(widening.trim_end().len()).unwrap(),
                    line: 1,
                    column: u64::try_from(widening.trim_end().len() + 1).unwrap(),
                },
            })
        );
        assert!(error.diagnostic.secondary.is_empty());
        assert!(error.diagnostic.notes.is_empty());
    }

    #[test]
    fn canonical_hir_dump_retains_local_declarations_and_self_use_mappings() {
        let output = inline_library(concat!(
            "pub struct Wrapper { value: i32 }\n",
            "impl Wrapper {\n",
            "    pub fn read(&self, unused: i32) -> i32 { self.value }\n",
            "}\n",
        ));
        let original = dump_hir_c1(&output.hir).unwrap();
        assert!(original.contains("(local (owner "));
        assert!(original.contains("(name \"self\")"));
        assert!(original.contains("(name \"unused\")"));
        assert!(original.contains("(self-use (span "));

        let mut without_locals = output.hir.clone();
        let item = without_locals
            .packages
            .iter_mut()
            .flat_map(|package| &mut package.targets)
            .flat_map(|target| &mut target.items)
            .find(|item| item.name.as_deref() == Some("read"))
            .unwrap();
        item.locals.clear();
        assert_ne!(dump_hir_c1(&without_locals).unwrap(), original);

        let mut without_self_uses = output.hir.clone();
        let item = without_self_uses
            .packages
            .iter_mut()
            .flat_map(|package| &mut package.targets)
            .flat_map(|target| &mut target.items)
            .find(|item| item.name.as_deref() == Some("read"))
            .unwrap();
        item.self_uses.clear();
        assert_ne!(dump_hir_c1(&without_self_uses).unwrap(), original);

        let mut remapped_self = output.hir.clone();
        let item = remapped_self
            .packages
            .iter_mut()
            .flat_map(|package| &mut package.targets)
            .flat_map(|target| &mut target.items)
            .find(|item| item.name.as_deref() == Some("read"))
            .unwrap();
        item.self_uses[0].receiver.ordinal += 1;
        assert_ne!(dump_hir_c1(&remapped_self).unwrap(), original);
    }

    #[test]
    fn body_only_type_changes_do_not_change_declaration_shapes() {
        let first = inline_library(concat!(
            "pub fn stable(value: i32) -> i32 {\n",
            "    let local = value as i32;\n",
            "    local\n",
            "}\n",
            "pub struct Wrapper { value: i32 }\n",
            "impl Wrapper {\n",
            "    pub fn stable_method(&self, value: i32) -> i32 {\n",
            "        let local = value as i32;\n",
            "        local\n",
            "    }\n",
            "}\n",
        ));
        let second = inline_library(concat!(
            "pub fn stable(value: i32) -> i32 {\n",
            "    let local = value as u32;\n",
            "    local\n",
            "}\n",
            "pub struct Wrapper { value: i32 }\n",
            "impl Wrapper {\n",
            "    pub fn stable_method(&self, value: i32) -> i32 {\n",
            "        let local = value as u32;\n",
            "        local\n",
            "    }\n",
            "}\n",
        ));
        let item_shapes = |output: &FrontendOutput, name: &str| {
            let item = output
                .hir
                .packages
                .iter()
                .flat_map(|package| &package.targets)
                .flat_map(|target| &target.items)
                .find(|item| item.name.as_deref() == Some(name))
                .unwrap();
            (
                item.symbolic_shape.clone(),
                item.body_symbolic_shape.clone(),
            )
        };
        let definition_shape = |output: &FrontendOutput, name: &str| {
            output
                .inventory
                .packages
                .iter()
                .flat_map(|package| &package.definitions)
                .find(|definition| definition.key.name == name)
                .unwrap()
                .symbolic_shape
                .clone()
        };

        for name in ["stable", "stable_method"] {
            assert_eq!(item_shapes(&first, name).0, item_shapes(&second, name).0);
            assert_eq!(
                definition_shape(&first, name),
                definition_shape(&second, name)
            );
            assert_ne!(item_shapes(&first, name).1, item_shapes(&second, name).1);
        }
        let impl_shape = |output: &FrontendOutput| {
            output.inventory.packages[0]
                .definitions
                .iter()
                .find(|definition| definition.key.kind == DeclarationKind::Impl)
                .unwrap()
                .symbolic_shape
                .clone()
        };
        assert_eq!(impl_shape(&first), impl_shape(&second));
        assert_eq!(
            first.inventory.packages[0]
                .bodies
                .iter()
                .map(|body| (body.key.body_kind, body.key.ordinal))
                .collect::<Vec<_>>(),
            second.inventory.packages[0]
                .bodies
                .iter()
                .map(|body| (body.key.body_kind, body.key.ordinal))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn impl_owner_heads_ignore_sibling_methods_and_collide_only_on_the_same_head() {
        let inherent_first = inline_library(concat!(
            "pub struct Wrapper<T> { value: T }\n",
            "impl<T> Wrapper<T> {\n",
            "    pub fn same(&self) { }\n",
            "    pub fn alpha(&self) { }\n",
            "}\n",
        ));
        let inherent_second = inline_library(concat!(
            "pub struct Wrapper<T> { value: T }\n",
            "impl<T> Wrapper<T> {\n",
            "    pub fn same(&self) { }\n",
            "    pub fn omega(&self) { }\n",
            "}\n",
        ));
        let trait_first = inline_library(concat!(
            "pub trait Marker { }\n",
            "pub struct Wrapper<T> { value: T }\n",
            "impl<T> Marker for Wrapper<T> {\n",
            "    pub fn same(&self) { }\n",
            "    pub fn alpha(&self) { }\n",
            "}\n",
        ));
        let trait_second = inline_library(concat!(
            "pub trait Marker { }\n",
            "pub struct Wrapper<T> { value: T }\n",
            "impl<T> Marker for Wrapper<T> {\n",
            "    pub fn same(&self) { }\n",
            "    pub fn omega(&self) { }\n",
            "}\n",
        ));
        let owner = |output: &FrontendOutput| {
            output.inventory.packages[0]
                .definitions
                .iter()
                .find(|definition| definition.key.name == "same")
                .unwrap()
                .key
                .owner_path
                .clone()
        };
        let impl_shape = |output: &FrontendOutput| {
            output.inventory.packages[0]
                .definitions
                .iter()
                .find(|definition| definition.key.kind == DeclarationKind::Impl)
                .unwrap()
                .symbolic_shape
                .clone()
        };

        assert_eq!(owner(&inherent_first), owner(&inherent_second));
        assert_ne!(impl_shape(&inherent_first), impl_shape(&inherent_second));
        assert_eq!(owner(&trait_first), owner(&trait_second));
        assert_ne!(impl_shape(&trait_first), impl_shape(&trait_second));

        let repeated = inline_library(concat!(
            "pub struct Wrapper<T> { value: T }\n",
            "impl<T> Wrapper<T> { pub fn same(&self) { } }\n",
            "impl<T> Wrapper<T> { pub fn same(&self) { } }\n",
        ));
        let repeated_owners = repeated.inventory.packages[0]
            .definitions
            .iter()
            .filter(|definition| definition.key.name == "same")
            .map(|definition| definition.key.owner_path.clone())
            .collect::<Vec<_>>();
        assert_eq!(repeated_owners.len(), 2);
        assert_eq!(repeated_owners[0], repeated_owners[1]);
    }

    #[test]
    fn conditional_inherent_impl_methods_have_distinct_complete_owner_keys() {
        let output = inline_library(concat!(
            "pub trait Left { }\n",
            "pub trait Right { }\n",
            "pub struct Wrapper<T> { value: T }\n",
            "impl<T> Wrapper<T> where T: Left { pub fn same(&self) { } }\n",
            "impl<T> Wrapper<T> where T: Right { pub fn same(&self) { } }\n",
        ));
        let mut keys = output.inventory.packages[0]
            .definitions
            .iter()
            .filter(|definition| definition.key.name == "same")
            .map(|definition| definition.key.owner_path.clone())
            .collect::<Vec<_>>();
        keys.sort();
        assert_eq!(keys.len(), 2);
        assert!(!matches!(
            keys[0],
            SymbolicDefinitionOwnerSkeleton::TopLevel
        ));
        assert_ne!(keys[0], keys[1]);
        let mut digest = Sha256::new();
        for key in &keys {
            let key = encode_symbolic_definition_owner_skeleton_c1(key).unwrap();
            digest.update(u64::try_from(key.len()).unwrap().to_le_bytes());
            digest.update(key);
        }
        let digest = uppercase_hex(&digest.finalize());
        assert_eq!(
            digest,
            "6AC32797C5E3BAE4A8DBCD85AF2001B1EA94F327144F7EE206B5B575FE935801"
        );
    }

    #[test]
    fn user_associated_paths_retain_typed_owner_spans_and_candidates() {
        let output = inline_library(concat!(
            "pub trait Factory { fn make() -> i32; }\n",
            "pub struct Wrapper;\n",
            "impl Wrapper {\n",
            "    pub fn make() -> i32 { 1i32 }\n",
            "    pub fn via_self() -> i32 { Self::make() }\n",
            "}\n",
            "pub fn generic<T: Factory>() -> i32 { T::make() }\n",
            "pub fn call() -> i32 { Wrapper::make() + Factory::make() }\n",
        ));
        let target = &output.hir.packages[0].targets[0];
        let item = |name: &str| {
            target
                .items
                .iter()
                .find(|item| item.name.as_deref() == Some(name))
                .unwrap()
        };
        let resolution = |owner_name: &str, path_name: &str| {
            let owner = item(owner_name);
            let path_use = owner
                .path_uses
                .iter()
                .find(|path_use| canonical_path(&path_use.path) == path_name)
                .unwrap();
            target
                .path_resolutions
                .iter()
                .find(|resolution| resolution.span == path_use.path.span)
                .unwrap()
        };

        let wrapper = item("Wrapper");
        let wrapper_make = target
            .items
            .iter()
            .find(|candidate| {
                candidate.name.as_deref() == Some("make")
                    && matches!(candidate.source, HirItemSource::ImplMethod(_))
            })
            .unwrap();
        let factory = item("Factory");
        let trait_method = target
            .items
            .iter()
            .find(|candidate| {
                candidate.owner == Some(factory.id) && candidate.name.as_deref() == Some("make")
            })
            .unwrap();
        let associated = resolution("call", "Wrapper::make")
            .associated
            .as_ref()
            .unwrap();
        assert_eq!(associated.owner, AssociatedPathOwner::Nominal(wrapper.id));
        assert_eq!(associated.member, "make");
        assert_eq!(
            associated.path_span,
            resolution("call", "Wrapper::make").span
        );
        let mut expected = vec![
            AssociatedPathCandidate::Item(HirItemRes::Definition(wrapper_make.id)),
            AssociatedPathCandidate::Item(HirItemRes::Definition(trait_method.id)),
        ];
        expected.sort();
        assert_eq!(associated.candidates, expected);
        assert_eq!(
            resolution("call", "Wrapper::make").unresolved,
            Some(UnresolvedPathKind::AssociatedItemPendingC2)
        );

        let associated = resolution("call", "Factory::make")
            .associated
            .as_ref()
            .unwrap();
        assert_eq!(associated.owner, AssociatedPathOwner::Nominal(factory.id));
        assert_eq!(
            associated.candidates,
            [AssociatedPathCandidate::Item(HirItemRes::Definition(
                trait_method.id
            ))]
        );

        let generic = item("generic");
        let associated = resolution("generic", "T::make")
            .associated
            .as_ref()
            .unwrap();
        assert_eq!(
            associated.owner,
            AssociatedPathOwner::Generic(GenericParameterId {
                owner: generic.id,
                index: 0,
            })
        );
        assert_eq!(
            associated.candidates,
            [AssociatedPathCandidate::Item(HirItemRes::Definition(
                trait_method.id
            ))]
        );

        let via_self = item("via_self");
        let associated = resolution("via_self", "Self::make")
            .associated
            .as_ref()
            .unwrap();
        assert_eq!(
            associated.owner,
            AssociatedPathOwner::ContextualSelf {
                context: via_self.id,
            }
        );
        assert_eq!(
            associated.candidates,
            [AssociatedPathCandidate::Item(HirItemRes::Definition(
                wrapper_make.id
            ))]
        );
        assert_eq!(
            associated.member_span,
            via_self.path_uses[0].path.segments[0].span
        );

        let original = dump_hir_c1(&output.hir).unwrap();
        assert!(original.contains("(associated (owner"));
        let mut mutated = output.hir.clone();
        mutated.packages[0].targets[0]
            .path_resolutions
            .iter_mut()
            .find(|resolution| resolution.associated.is_some())
            .unwrap()
            .associated
            .as_mut()
            .unwrap()
            .candidates
            .clear();
        assert_ne!(dump_hir_c1(&mutated).unwrap(), original);
    }

    #[test]
    fn contextual_self_uses_the_complete_identical_impl_head_and_direct_enum_variant() {
        let output = inline_library(concat!(
            "pub struct Wrapper;\n",
            "impl Wrapper { pub fn first() -> i32 { Self::second() } }\n",
            "impl Wrapper { pub fn second() -> i32 { 2i32 } }\n",
            "pub enum Choice { First, Second(i32) }\n",
            "impl Choice {\n",
            "    pub fn first() -> Choice { Self::First }\n",
            "    pub fn second() -> Choice { Self::Second(2i32) }\n",
            "    pub fn classify(value: Choice) -> i32 {\n",
            "        match value {\n",
            "            Self::First => 1i32,\n",
            "            Self::Second(inner) => inner,\n",
            "        }\n",
            "    }\n",
            "}\n",
        ));
        let target = &output.hir.packages[0].targets[0];
        let methods = |name: &str| {
            target
                .items
                .iter()
                .filter(|item| {
                    item.name.as_deref() == Some(name)
                        && matches!(item.source, HirItemSource::ImplMethod(_))
                })
                .collect::<Vec<_>>()
        };
        let wrapper_first = methods("first")
            .into_iter()
            .find(|item| {
                item.path_uses
                    .iter()
                    .any(|path| canonical_path(&path.path) == "Self::second")
            })
            .unwrap();
        let wrapper_second = methods("second").into_iter().next().unwrap();
        let split_path = wrapper_first
            .path_uses
            .iter()
            .find(|path| canonical_path(&path.path) == "Self::second")
            .unwrap();
        let split_resolution = target
            .path_resolutions
            .iter()
            .find(|resolution| resolution.span == split_path.path.span)
            .unwrap();
        assert_eq!(
            split_resolution.unresolved,
            Some(UnresolvedPathKind::AssociatedItemPendingC2)
        );
        let split_associated = split_resolution.associated.as_ref().unwrap();
        assert_eq!(
            split_associated.owner,
            AssociatedPathOwner::ContextualSelf {
                context: wrapper_first.id,
            }
        );
        assert_eq!(
            split_associated.candidates,
            [AssociatedPathCandidate::Item(HirItemRes::Definition(
                wrapper_second.id
            ))]
        );

        let choice = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("Choice"))
            .unwrap();
        let variant_paths = target
            .items
            .iter()
            .flat_map(|item| &item.path_uses)
            .filter_map(|path| match canonical_path(&path.path).as_str() {
                "Self::First" => Some((path, 0)),
                "Self::Second" => Some((path, 1)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            variant_paths
                .iter()
                .map(|(_, ordinal)| *ordinal)
                .collect::<Vec<_>>(),
            [0, 1, 0, 1]
        );
        for (variant_path, ordinal) in variant_paths {
            let variant_resolution = target
                .path_resolutions
                .iter()
                .find(|resolution| resolution.span == variant_path.path.span)
                .unwrap();
            assert_eq!(
                variant_resolution.resolutions,
                [Res::Item(HirItemRes::EnumVariant {
                    owner: choice.id,
                    ordinal,
                })]
            );
            assert_eq!(variant_resolution.unresolved, None);
            assert_eq!(variant_resolution.associated, None);
        }

        let blanket = inline_library(concat!(
            "pub trait Maker { fn make() -> i32; fn call() -> i32; }\n",
            "impl<T> Maker for T {\n",
            "    pub fn make() -> i32 { 1i32 }\n",
            "    pub fn call() -> i32 { Self::make() }\n",
            "}\n",
        ));
        let target = &blanket.hir.packages[0].targets[0];
        let call = target
            .items
            .iter()
            .find(|item| {
                item.name.as_deref() == Some("call")
                    && matches!(item.source, HirItemSource::ImplMethod(_))
            })
            .unwrap();
        let path = call
            .path_uses
            .iter()
            .find(|path| canonical_path(&path.path) == "Self::make")
            .unwrap();
        let resolution = target
            .path_resolutions
            .iter()
            .find(|resolution| resolution.span == path.path.span)
            .unwrap();
        assert_eq!(
            resolution.unresolved,
            Some(UnresolvedPathKind::AssociatedItemPendingC2)
        );
        assert_eq!(resolution.associated.as_ref().unwrap().candidates.len(), 1);
    }

    #[test]
    fn contextual_self_uses_canonical_impl_heads_and_retains_duplicate_candidates_for_c2() {
        let output = inline_library(concat!(
            "pub trait Left { }\n",
            "pub trait Right { }\n",
            "pub struct Wrapper<T> { value: T }\n",
            "impl<T> Wrapper<T> where T: Left, T: Right {\n",
            "    pub fn call() -> i32 { Self::make() }\n",
            "    pub fn make() -> i32 { 1i32 }\n",
            "}\n",
            "impl<T> Wrapper<T> where T: Right, T: Left {\n",
            "    pub fn make() -> i32 { 2i32 }\n",
            "}\n",
        ));
        let target = &output.hir.packages[0].targets[0];
        let call = target
            .items
            .iter()
            .find(|item| {
                item.name.as_deref() == Some("call")
                    && matches!(item.source, HirItemSource::ImplMethod(_))
            })
            .unwrap();
        let path = call
            .path_uses
            .iter()
            .find(|path| canonical_path(&path.path) == "Self::make")
            .unwrap();
        let resolution = target
            .path_resolutions
            .iter()
            .find(|resolution| resolution.span == path.path.span)
            .unwrap();
        assert_eq!(
            resolution.unresolved,
            Some(UnresolvedPathKind::AssociatedItemPendingC2)
        );
        let associated = resolution.associated.as_ref().unwrap();
        assert_eq!(
            associated.owner,
            AssociatedPathOwner::ContextualSelf { context: call.id }
        );

        let mut expected = target
            .items
            .iter()
            .filter(|item| {
                item.name.as_deref() == Some("make")
                    && matches!(item.source, HirItemSource::ImplMethod(_))
            })
            .map(|item| AssociatedPathCandidate::Item(HirItemRes::Definition(item.id)))
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(expected.len(), 2);
        assert_eq!(associated.candidates, expected);
        assert!(associated
            .candidates
            .windows(2)
            .all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn conditional_impl_candidates_are_all_retained_for_c2_selection() {
        let output = inline_library(concat!(
            "pub trait Left { }\n",
            "pub trait Right { }\n",
            "pub struct Wrapper<T> { value: T }\n",
            "impl<T> Wrapper<T> where T: Left { pub fn choose() -> i32 { 1i32 } }\n",
            "impl<T> Wrapper<T> where T: Right { pub fn choose() -> i32 { 2i32 } }\n",
            "pub fn call() -> i32 { Wrapper::choose() }\n",
        ));
        let target = &output.hir.packages[0].targets[0];
        let call = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("call"))
            .unwrap();
        let path = call
            .path_uses
            .iter()
            .find(|path| canonical_path(&path.path) == "Wrapper::choose")
            .unwrap();
        let associated = target
            .path_resolutions
            .iter()
            .find(|resolution| resolution.span == path.path.span)
            .unwrap()
            .associated
            .as_ref()
            .unwrap();
        assert_eq!(associated.candidates.len(), 2);
        assert!(associated
            .candidates
            .windows(2)
            .all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn virtual_trait_candidates_are_retained_for_nominal_and_generic_owners() {
        let output = inline_library(concat!(
            "pub struct Wrapper;\n",
            "pub fn concrete(value: Wrapper) -> Wrapper { Wrapper::from(value) }\n",
            "pub fn generic<T: From<T,T>>(value: T) -> T { T::from(value) }\n",
        ));
        let target = &output.hir.packages[0].targets[0];
        let wrapper = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("Wrapper"))
            .unwrap();
        let generic = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("generic"))
            .unwrap();
        let from_owner = output
            .inventory
            .embedded_core
            .lookup_prelude_definition("From", VirtualNamespace::Type)
            .unwrap();
        let from_method = output
            .inventory
            .embedded_core
            .lookup_method(Some(from_owner), "from")
            .unwrap();
        let expected = [AssociatedPathCandidate::Builtin(BuiltinRes {
            target: BuiltinResTarget::Method(from_method),
        })];
        let associated = |owner_name: &str, canonical: &str| {
            let owner = target
                .items
                .iter()
                .find(|item| item.name.as_deref() == Some(owner_name))
                .unwrap();
            let path = owner
                .path_uses
                .iter()
                .find(|path| canonical_path(&path.path) == canonical)
                .unwrap();
            target
                .path_resolutions
                .iter()
                .find(|resolution| resolution.span == path.path.span)
                .unwrap()
                .associated
                .as_ref()
                .unwrap()
        };
        let concrete = associated("concrete", "Wrapper::from");
        assert_eq!(concrete.owner, AssociatedPathOwner::Nominal(wrapper.id));
        assert_eq!(concrete.candidates, expected);
        let bounded = associated("generic", "T::from");
        assert_eq!(
            bounded.owner,
            AssociatedPathOwner::Generic(GenericParameterId {
                owner: generic.id,
                index: 0,
            })
        );
        assert_eq!(bounded.candidates, expected);
    }

    #[test]
    fn user_type_binding_shadows_virtual_trait_candidate_partition() {
        let output = inline_library(concat!(
            "pub trait From { fn from() -> i32; }\n",
            "pub struct Wrapper;\n",
            "pub fn call() -> i32 { Wrapper::from() }\n",
        ));
        let target = &output.hir.packages[0].targets[0];
        let trait_owner = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("From"))
            .unwrap();
        let method = target
            .items
            .iter()
            .find(|item| item.owner == Some(trait_owner.id))
            .unwrap();
        let call = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("call"))
            .unwrap();
        let path = call
            .path_uses
            .iter()
            .find(|path| canonical_path(&path.path) == "Wrapper::from")
            .unwrap();
        let associated = target
            .path_resolutions
            .iter()
            .find(|resolution| resolution.span == path.path.span)
            .unwrap()
            .associated
            .as_ref()
            .unwrap();
        assert_eq!(
            associated.candidates,
            [AssociatedPathCandidate::Item(HirItemRes::Definition(
                method.id
            ))]
        );
    }

    #[test]
    fn embedded_qualified_members_and_record_constructors_resolve_exactly() {
        let output = inline_library(concat!(
            "pub fn builtins(value: i32) {\n",
            "    let _ = Option::Some(value);\n",
            "    let _ = Result::Err(value);\n",
            "    let _ = String::new();\n",
            "    let _ = OpenOptions {};\n",
            "}\n",
        ));
        let target = &output.hir.packages[0].targets[0];
        let item = target
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("builtins"))
            .unwrap();
        let resolved = |canonical: &str| {
            let path = item
                .path_uses
                .iter()
                .find(|path| canonical_path(&path.path) == canonical)
                .unwrap();
            target
                .path_resolutions
                .iter()
                .find(|resolution| resolution.span == path.path.span)
                .unwrap()
        };
        assert!(matches!(
            resolved("Option::Some").resolutions.as_slice(),
            [Res::Builtin(BuiltinRes {
                target: BuiltinResTarget::EnumVariant(_),
            })]
        ));
        assert!(matches!(
            resolved("Result::Err").resolutions.as_slice(),
            [Res::Builtin(BuiltinRes {
                target: BuiltinResTarget::EnumVariant(_),
            })]
        ));
        assert!(matches!(
            resolved("String::new").resolutions.as_slice(),
            [Res::Builtin(BuiltinRes {
                target: BuiltinResTarget::Method(_),
            })]
        ));
        assert!(matches!(
            resolved("OpenOptions").resolutions.as_slice(),
            [Res::Builtin(BuiltinRes {
                target: BuiltinResTarget::RecordConstructor(_),
            })]
        ));
        assert!(target
            .path_resolutions
            .iter()
            .all(|resolution| { resolution.unresolved != Some(UnresolvedPathKind::UnknownName) }));
    }

    #[test]
    fn associated_partition_zero_and_multiple_are_name002() {
        let missing = inline_library_result(concat!(
            "pub struct Wrapper;\n",
            "pub fn call() { Wrapper::missing(); }\n",
        ))
        .unwrap_err();
        assert_eq!(missing.kind, FrontendErrorCode::Name);
        assert_eq!(missing.diagnostic.code, "NAME002");

        let hidden = inline_library_with_files(
            concat!(
                "mod hidden;\n",
                "pub use self::hidden::Wrapper;\n",
                "pub fn call() { Wrapper::secret(); }\n",
            ),
            &[(
                "src/hidden.arc",
                "pub struct Wrapper;\nimpl Wrapper { fn secret() { } }\n",
            )],
        )
        .unwrap_err();
        assert_eq!(hidden.kind, FrontendErrorCode::Name);
        assert_eq!(hidden.diagnostic.code, "NAME002");

        let multiple = inline_library_with_files(
            concat!(
                "pub mod Ambiguous;\n",
                "pub enum Ambiguous { member, }\n",
                "pub fn call() { Ambiguous::member; }\n",
            ),
            &[("src/Ambiguous.arc", "pub const member: i32 = 1i32;\n")],
        )
        .unwrap_err();
        assert_eq!(multiple.kind, FrontendErrorCode::Name);
        assert_eq!(multiple.diagnostic.code, "NAME002");

        for declaration in [
            concat!(
                "pub mod Ambiguous;\n",
                "pub struct Ambiguous;\n",
                "impl Ambiguous { pub fn member() { } }\n",
                "pub fn call() { Ambiguous::member(); }\n",
            ),
            concat!(
                "pub mod Ambiguous;\n",
                "pub trait Ambiguous { fn member(); }\n",
                "pub fn call() { Ambiguous::member(); }\n",
            ),
        ] {
            let expected_start = u64::try_from(declaration.rfind("member").unwrap()).unwrap();
            let multiple = inline_library_with_files(
                declaration,
                &[("src/Ambiguous.arc", "pub fn member() { }\n")],
            )
            .unwrap_err();
            assert_eq!(multiple.kind, FrontendErrorCode::Name);
            assert_eq!(multiple.diagnostic.code, "NAME002");
            let span = multiple.diagnostic.primary.span.unwrap();
            assert_eq!(span.file, FileId(0));
            assert_eq!(span.start.byte, expected_start);
            assert_eq!(span.end.byte, expected_start + 6);
        }
    }

    #[test]
    fn terminal_unknown_names_and_contextless_self_fail_closed_as_name002() {
        for source in [
            "pub fn call() { missing(); }\n",
            "pub fn call() { missing::value; }\n",
            "pub struct Wrapper { value: Missing }\n",
            "pub fn borrow<'a>(value: &'missing i32) { }\n",
            "pub fn effect() requires { Missing } throws {} { }\n",
            "pub fn effect() requires {} throws { Missing } { }\n",
            "pub fn contextless(value: Self) { }\n",
        ] {
            let error = inline_library_result(source).unwrap_err();
            assert_eq!(error.kind, FrontendErrorCode::Name, "{source}");
            assert_eq!(error.diagnostic.code, "NAME002", "{source}");
            assert!(error.diagnostic.primary.span.is_some(), "{source}");
        }
    }

    #[test]
    fn contextual_self_template_preserves_nested_structure_and_binder_depths() {
        let output = inline_library(concat!(
            "pub struct Envelope<T> { value: T }\n",
            "pub trait Nested<T> {\n",
            "    fn project<'a>(&'a self, value: T) -> Envelope<(Self, &'a Self, T)>;\n",
            "}\n",
        ));
        let method = output.hir.packages[0].targets[0]
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("project"))
            .unwrap();
        let SymbolicDeclarationPayloadSkeleton::Callable(callable) =
            &method.definition_shape.payload
        else {
            panic!("trait method must retain a callable definition shape");
        };
        let SymbolicTypeShapeSkeleton::Pending(pending) = &callable.result else {
            panic!("C1 must not resolve trait-method contextual Self");
        };
        assert_eq!(pending.kind, PendingShapeKind::ContextualSelf);

        let template = method
            .symbolic_shape
            .contextual_self_type_template(pending)
            .unwrap();
        assert_eq!(template.hole_count(), 2);
        let HirItemSource::TraitMethod(source_method) = &method.source else {
            panic!("project must retain its trait-method source");
        };
        assert_eq!(
            template.root_span(),
            symbolic_source_span(source_method.signature.result.as_ref().unwrap().span)
        );

        let instantiated = template
            .instantiate_contextual_self(&SymbolicType::I32)
            .unwrap();
        let SymbolicType::NominalPath {
            declaration,
            arguments,
        } = instantiated
        else {
            panic!("Envelope result must retain its nominal head");
        };
        assert_eq!(declaration.name, "Envelope");
        let [GenericArgumentShape::Type(SymbolicType::Tuple(elements))] = arguments.as_slice()
        else {
            panic!("Envelope must retain one tuple type argument");
        };
        assert_eq!(
            elements,
            &[
                SymbolicType::I32,
                SymbolicType::Reference {
                    mutability: Mutability::Shared,
                    lifetime: SymbolicLifetime::Bound { depth: 0, index: 0 },
                    pointee: Box::new(SymbolicType::I32),
                },
                SymbolicType::BoundType { depth: 1, index: 0 },
            ]
        );
        assert!(!symbolic_type_contains_c2_self_marker(
            &SymbolicType::NominalPath {
                declaration,
                arguments,
            }
        ));

        let encoded = encode_alpha_symbolic_shape(&method.symbolic_shape).unwrap();
        let mut without_noncanonical_sidecar = method.symbolic_shape.clone();
        without_noncanonical_sidecar
            .c2_contextual_self_templates
            .clear();
        assert_eq!(
            encode_alpha_symbolic_shape(&without_noncanonical_sidecar).unwrap(),
            encoded,
            "the C2 template sidecar must not enter C1 golden bytes"
        );
    }

    #[test]
    fn contextual_self_template_lookup_and_instantiation_fail_closed() {
        let pending = SymbolicPendingShape {
            readiness: SymbolicShapeReadiness::PendingC2,
            source_span: SymbolicSourceSpan {
                file: 0,
                start_byte: 1,
                end_byte: 5,
                start_line: 1,
                start_column: 2,
                end_line: 1,
                end_column: 6,
            },
            kind: PendingShapeKind::ContextualSelf,
            debug_spelling: "&Self".to_owned(),
        };
        assert_eq!(
            ResolvedSymbolicShape::default().contextual_self_type_template(&pending),
            Err(C2TypeTemplateLookupError::Missing)
        );

        let mut wrong_domain = pending.clone();
        wrong_domain.kind = PendingShapeKind::GenericFormation;
        assert_eq!(
            ResolvedSymbolicShape::default().contextual_self_type_template(&wrong_domain),
            Err(C2TypeTemplateLookupError::NotContextualSelf)
        );

        let output = inline_library("pub trait Borrow { fn borrow(&self) -> &Self; }\n");
        let method = output.hir.packages[0].targets[0]
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("borrow"))
            .unwrap();
        let SymbolicDeclarationPayloadSkeleton::Callable(callable) =
            &method.definition_shape.payload
        else {
            unreachable!();
        };
        let SymbolicTypeShapeSkeleton::Pending(pending) = &callable.result else {
            unreachable!();
        };
        let template = method
            .symbolic_shape
            .contextual_self_type_template(pending)
            .unwrap();
        assert_eq!(
            template.instantiate_contextual_self(&c2_contextual_self_marker()),
            Err(C2TypeTemplateInstantiationError::ReservedTemplateCoordinate)
        );
    }

    #[test]
    fn trait_impl_header_self_stays_pending_but_retains_its_nominal_template() {
        let output = inline_library(concat!(
            "pub trait Pair<T> { }\n",
            "pub struct Wrapper;\n",
            "impl Pair<(Self, Self)> for Wrapper { }\n",
        ));
        let implementation = output.hir.packages[0].targets[0]
            .items
            .iter()
            .find(|item| item.kind == DeclarationKind::Impl)
            .unwrap();
        let SymbolicDefinitionOwnerSkeleton::TraitImpl {
            trait_ref, target, ..
        } = &implementation.owner_shape
        else {
            panic!("implementation must retain its trait owner header");
        };
        let SymbolicTypeShapeSkeleton::Pending(pending) = trait_ref else {
            panic!("C1 must not resolve contextual Self in a trait-impl header");
        };
        let SymbolicTypeShapeSkeleton::Resolved { value: target, .. } = target else {
            panic!("implementation target must already be C1-resolved");
        };
        let template = implementation
            .symbolic_shape
            .contextual_self_type_template(pending)
            .unwrap();
        assert_eq!(template.hole_count(), 2);
        let instantiated = template.instantiate_contextual_self(target).unwrap();
        let SymbolicType::NominalPath {
            declaration,
            arguments,
        } = instantiated
        else {
            panic!("trait header must retain its nominal trait head");
        };
        assert_eq!(declaration.name, "Pair");
        let [GenericArgumentShape::Type(SymbolicType::Tuple(elements))] = arguments.as_slice()
        else {
            panic!("Pair must retain its tuple type argument");
        };
        assert_eq!(elements, &[target.clone(), target.clone()]);
    }
}
