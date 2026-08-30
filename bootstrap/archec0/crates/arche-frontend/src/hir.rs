use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::path::PathBuf;

use arche_package::{PackageNodeId, SourceTreeEntry};

use crate::source::{FileId, Span};
use crate::symbol::Symbol;

// These C1 primitives remain nested behind the existing HIR module so the
// compatible M27-B entry points keep their original module layout. The crate
// root re-exports the complete C1 data API; none of these unverified rows is a
// stable-identity or semantic-verification authority.
#[path = "arena.rs"]
mod arena;
#[path = "inventory.rs"]
mod inventory;
#[path = "shape.rs"]
mod shape;
#[path = "workspace_frontend.rs"]
mod workspace_frontend;

pub use self::inventory::{
    encode_inventory_c1, encode_semantic_definition_key_session, CtfeBudgetsSkeleton,
    DependencyKind, ManifestCapability, MemberVisibilityPath, ModuleRef, Namespace,
    PackageDependencySkeleton, PackageProvenanceSkeleton, PackageSourceSkeleton,
    SemanticBindingInventorySkeleton, SemanticBindingOrigin, SemanticBindingPath,
    SemanticBindingTarget, SemanticBodyInventorySkeleton, SemanticBodyKey, SemanticBodyKind,
    SemanticDefinitionInventorySkeleton, SemanticDefinitionKey, SemanticInventorySkeleton,
    SemanticMemberVisibility, SemanticModuleInventorySkeleton, SemanticPackageInventorySkeleton,
    SemanticTargetContractSkeleton, SemanticTargetInventorySkeleton, Visibility,
};
pub use self::shape::{
    declaration_shape_readiness, encode_declaration_shape_preimage,
    encode_definition_identity_preimage, encode_definition_owner_entry,
    encode_final_declaration_shape_identity, encode_final_definition_owner_identity,
    encode_generic_arguments, encode_generic_parameters, encode_method_entry,
    encode_symbolic_const, encode_symbolic_effect, encode_symbolic_effect_set,
    encode_symbolic_predicate, encode_symbolic_predicate_set, encode_symbolic_type,
    mint_definition_id, owner_shape_readiness, try_canonicalize_declaration_shape,
    try_canonicalize_definition_owner, CallTrait, CanonicalDeclarationShape,
    CanonicalDefinitionOwner, CaptureMode, DeclarationKind, EffectKind, GeneratorTarget,
    GenericArgumentShape, GenericParameterKind, GenericParameterShape, IntegerType, Mutability,
    PendingShapeKind, SemanticDeclarationPath, ShapeEncodingError, SymbolicCallableKind,
    SymbolicCallableParameterMode, SymbolicCallableParameterSkeleton,
    SymbolicCallableShapeSkeleton, SymbolicCapabilityAccessMode, SymbolicCapture,
    SymbolicConstExpression, SymbolicConstNode, SymbolicDeclarationPayloadSkeleton,
    SymbolicDeclarationShapeSkeleton, SymbolicDefinitionOwnerSkeleton, SymbolicEffectAtom,
    SymbolicEffectSetsSkeleton, SymbolicEffectShapeSkeleton, SymbolicFieldShapeSkeleton,
    SymbolicImpliedCapabilityRequirementSkeleton, SymbolicLifetime, SymbolicMethodShapeSkeleton,
    SymbolicPendingShape, SymbolicPredicate, SymbolicPredicateShapeSkeleton, SymbolicQueryTermKind,
    SymbolicQueryTermShapeSkeleton, SymbolicRecordForm, SymbolicRecordShapeSkeleton,
    SymbolicShapeReadiness, SymbolicSourceSpan, SymbolicSystemAccessShapeSkeleton, SymbolicType,
    SymbolicTypeEffectSet, SymbolicTypeShapeSkeleton, SymbolicVariantShapeSkeleton, TargetRoot,
};
pub use self::workspace_frontend::{
    check_workspace_c1, dump_hir_c1, AssociatedPathCandidate, AssociatedPathOwner,
    AssociatedPathResolution, BuiltinRes, BuiltinResTarget, C2ContextualSelfTypeTemplate,
    C2TypeTemplateBlocker, C2TypeTemplateInstantiationError, C2TypeTemplateLookupError,
    FrontendOutput, GenericParameterId, HiddenLifetimeBinder, HiddenLifetimeBinderSource,
    HirBinding, HirBindingOrigin, HirBindingTarget, HirBodySource, HirGenericArgumentUse,
    HirGenericArgumentsUse, HirItemRes, HirItemSource, HirLocalBinding, HirPathUse, HirSelfUse,
    LocalId, MaterializedRegistryPackage, PathResolution, Res, ResolvedGenericArgument,
    ResolvedSymbolicBody, ResolvedSymbolicConst, ResolvedSymbolicEffect, ResolvedSymbolicItem,
    ResolvedSymbolicLifetime, ResolvedSymbolicModule, ResolvedSymbolicPackageHir,
    ResolvedSymbolicShape, ResolvedSymbolicTargetHir, ResolvedSymbolicType,
    ResolvedSymbolicWorkspaceHir, ResolvedTargetContract, UnresolvedPathKind,
    WorkspaceInventorySkeleton,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TargetId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ModuleId(pub u64);

/// Exact package/target-qualified module session identity required by M27-C1.
/// `ModuleId` remains temporarily available for the compatible M27-B target
/// API while module loading is migrated to the workspace-wide arena.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HirModuleId {
    package: PackageNodeId,
    target: TargetId,
    local: u64,
}

impl HirModuleId {
    pub const fn new(package: PackageNodeId, target: TargetId, local: u64) -> Self {
        Self {
            package,
            target,
            local,
        }
    }

    pub const fn package(self) -> PackageNodeId {
        self.package
    }

    pub const fn target(self) -> TargetId {
        self.target
    }

    pub const fn local(self) -> u64 {
        self.local
    }
}

impl fmt::Display for HirModuleId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "p{}t{}m{}",
            self.package.get(),
            self.target.0,
            self.local
        )
    }
}

/// Globally unique item arena ID within one `FrontendOutput`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HirItemId(pub u64);

/// Globally unique body arena ID within one `FrontendOutput`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HirBodyId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HirDefinitionId {
    package: PackageNodeId,
    target: TargetId,
    local: u64,
}

impl HirDefinitionId {
    pub const fn new(package: PackageNodeId, target: TargetId, local: u64) -> Self {
        Self {
            package,
            target,
            local,
        }
    }

    pub const fn package(self) -> PackageNodeId {
        self.package
    }

    pub const fn target(self) -> TargetId {
        self.target
    }

    pub const fn local(self) -> u64 {
        self.local
    }
}

impl fmt::Display for HirDefinitionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "p{}t{}d{}",
            self.package.get(),
            self.target.0,
            self.local
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetKind {
    Library,
    Binary,
    Environment,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentSchedulePaths {
    pub reset: String,
    pub step: String,
    pub self_play: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckTargetRequest {
    pub package_root: PathBuf,
    pub package: PackageNodeId,
    pub target_id: TargetId,
    pub target_name: String,
    pub kind: TargetKind,
    /// Absolute or package-root-relative source root.
    pub source_root: PathBuf,
    /// A package-relative item path such as `package::server::Game`.
    pub root_world: Option<String>,
    pub environment_schedules: Option<EnvironmentSchedulePaths>,
    /// Source-visible aliases. M27-B validates their namespace reservation;
    /// dependency export loading is supplied by the package graph adapter.
    pub dependency_aliases: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HirNamespace {
    Module,
    Type,
    Value,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HirDefinitionKind {
    Module(ModuleId),
    World,
    Component,
    Resource,
    Tag,
    System,
    Schedule,
    Function,
    Struct,
    Enum,
    Trait,
    TypeAlias,
    Const,
    Static,
}

impl HirDefinitionKind {
    pub fn namespace(&self) -> HirNamespace {
        match self {
            Self::Module(_) => HirNamespace::Module,
            Self::World
            | Self::Component
            | Self::Resource
            | Self::Tag
            | Self::Struct
            | Self::Enum
            | Self::Trait
            | Self::TypeAlias => HirNamespace::Type,
            Self::System | Self::Schedule | Self::Function | Self::Const | Self::Static => {
                HirNamespace::Value
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HirVisibility {
    Module(ModuleId),
    Package,
    Public,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirDefinition {
    pub id: HirDefinitionId,
    pub module: ModuleId,
    pub name: Symbol,
    pub kind: HirDefinitionKind,
    pub visibility: HirVisibility,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirScopeEntry {
    pub name: Symbol,
    pub definition: HirDefinitionId,
    pub kind: HirDefinitionKind,
    pub namespace: HirNamespace,
    pub visibility: HirVisibility,
    /// Whether the ultimate declaration may be exported outside its package.
    /// This stays false through visibility-widening imports.
    pub exportable: bool,
    pub span: Span,
    pub imported: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirModule {
    pub id: ModuleId,
    pub parent: Option<ModuleId>,
    pub name: Option<Symbol>,
    pub path: Vec<Symbol>,
    pub file: FileId,
    pub scopes: BTreeMap<HirNamespace, Vec<HirScopeEntry>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkedTarget {
    pub package: PackageNodeId,
    pub id: TargetId,
    pub name: String,
    pub kind: TargetKind,
    pub root_module: ModuleId,
    pub root_world: Option<HirDefinitionId>,
    pub main: Option<HirDefinitionId>,
    pub reset_schedule: Option<HirDefinitionId>,
    pub step_schedule: Option<HirDefinitionId>,
    pub self_play_schedule: Option<HirDefinitionId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PackageExportEntry {
    pub(crate) path: Vec<Symbol>,
    pub(crate) definition: HirDefinitionId,
    pub(crate) kind: HirDefinitionKind,
    pub(crate) namespace: HirNamespace,
    pub(crate) externally_visible: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PackageExportSurface {
    pub(crate) package: PackageNodeId,
    pub(crate) entries: Vec<PackageExportEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DependencyExport {
    pub(crate) alias: String,
    pub(crate) surface: Option<PackageExportSurface>,
}

/// A structurally complete package/module HIR. Its fields and constructor are
/// private so M27-C cannot accidentally consume an unresolved tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedTargetHir {
    target: LinkedTarget,
    modules: Vec<HirModule>,
    definitions: Vec<HirDefinition>,
    source_entries: Vec<SourceTreeEntry>,
}

/// All source targets checked in one resolved package graph. Target and
/// definition identifiers are assigned before topological checking, so this
/// order is stable even when dependency order differs from package-ID order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedWorkspaceHir {
    targets: Vec<ResolvedTargetHir>,
}

impl ResolvedWorkspaceHir {
    pub(crate) fn new(mut targets: Vec<ResolvedTargetHir>) -> Self {
        targets.sort_by_key(|target| (target.target.package, target.target.id));
        Self { targets }
    }

    pub fn targets(&self) -> &[ResolvedTargetHir] {
        &self.targets
    }

    pub fn definition(&self, id: HirDefinitionId) -> Option<&HirDefinition> {
        let target = self
            .targets
            .iter()
            .find(|target| target.target.package == id.package && target.target.id == id.target)?;
        let definition = usize::try_from(id.local)
            .ok()
            .and_then(|index| target.definitions.get(index))?;
        (definition.id == id).then_some(definition)
    }
}

impl ResolvedTargetHir {
    pub(crate) fn new(
        target: LinkedTarget,
        modules: Vec<HirModule>,
        definitions: Vec<HirDefinition>,
        source_entries: Vec<SourceTreeEntry>,
    ) -> Self {
        Self {
            target,
            modules,
            definitions,
            source_entries,
        }
    }

    pub fn target(&self) -> &LinkedTarget {
        &self.target
    }

    pub fn modules(&self) -> &[HirModule] {
        &self.modules
    }

    pub fn definitions(&self) -> &[HirDefinition] {
        &self.definitions
    }

    /// Returns package-relative entries derived from the immutable snapshots
    /// used by this check. Entries are sorted by portable path.
    pub fn source_entries(&self) -> &[SourceTreeEntry] {
        &self.source_entries
    }
}

/// Deterministic textual form used for M27-B goldens and debugging. It omits
/// host-dependent absolute package prefixes and prints logical module paths.
pub fn dump_resolved_target(hir: &ResolvedTargetHir) -> String {
    let mut output = String::new();
    let target = hir.target();
    let _ = writeln!(
        output,
        "target p{}t{} {} {:?} root=module{}",
        target.package.get(),
        target.id.0,
        target.name,
        target.kind,
        target.root_module.0
    );
    for module in hir.modules() {
        let logical = if module.path.is_empty() {
            "package".to_owned()
        } else {
            format!(
                "package::{}",
                module
                    .path
                    .iter()
                    .map(Symbol::as_str)
                    .collect::<Vec<_>>()
                    .join("::")
            )
        };
        let _ = writeln!(output, "module {} {logical}", module.id.0);
        for namespace in [
            HirNamespace::Module,
            HirNamespace::Type,
            HirNamespace::Value,
        ] {
            if let Some(entries) = module.scopes.get(&namespace) {
                for entry in entries {
                    let _ = writeln!(
                        output,
                        "  {:?} {} -> {}{}",
                        namespace,
                        entry.name,
                        entry.definition,
                        if entry.imported { " imported" } else { "" }
                    );
                }
            }
        }
    }
    for definition in hir.definitions() {
        let _ = writeln!(
            output,
            "{} module{} {:?} {} {:?}",
            definition.id,
            definition.module.0,
            definition.kind,
            definition.name,
            definition.visibility
        );
    }
    for (label, value) in [
        ("world", target.root_world),
        ("main", target.main),
        ("reset", target.reset_schedule),
        ("step", target.step_schedule),
        ("self_play", target.self_play_schedule),
    ] {
        if let Some(definition) = value {
            let _ = writeln!(output, "link {label}={definition}");
        }
    }
    output
}

impl fmt::Display for ResolvedTargetHir {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&dump_resolved_target(self))
    }
}

#[cfg(test)]
mod c1_contract_tests {
    use super::*;

    fn empty_target(package: u64, target: u64) -> ResolvedTargetHir {
        ResolvedTargetHir::new(
            LinkedTarget {
                package: PackageNodeId::new(package),
                id: TargetId(target),
                name: format!("target-{package}-{target}"),
                kind: TargetKind::Library,
                root_module: ModuleId(0),
                root_world: None,
                main: None,
                reset_schedule: None,
                step_schedule: None,
                self_play_schedule: None,
            },
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    }

    #[test]
    fn module_session_ids_include_the_package_and_per_package_target() {
        let first = HirModuleId::new(PackageNodeId::new(0), TargetId(0), 3);
        let second = HirModuleId::new(PackageNodeId::new(1), TargetId(0), 3);
        assert_ne!(first, second);
        assert_eq!(first.to_string(), "p0t0m3");
        assert_eq!(second.to_string(), "p1t0m3");
    }

    #[test]
    fn workspace_order_uses_package_then_per_package_target_id() {
        let workspace = ResolvedWorkspaceHir::new(vec![
            empty_target(1, 0),
            empty_target(0, 1),
            empty_target(0, 0),
        ]);
        let keys = workspace
            .targets()
            .iter()
            .map(|target| (target.target().package.get(), target.target().id.0))
            .collect::<Vec<_>>();
        assert_eq!(keys, [(0, 0), (0, 1), (1, 0)]);
    }
}
