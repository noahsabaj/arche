//! Unverified semantic-inventory skeletons for M27-C1.
//!
//! C1 proves deterministic completeness and symbolic identity inputs. It does
//! not construct `VerifiedSemanticInventory`, stable definition/type/interface
//! identities, effective visibility, trait selections, or typed-MIR evidence.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use arche_foundation::identity::PackageId;
use arche_package::PackageNodeId;

use crate::embedded_core::VerifiedEmbeddedCoreAuthority;
use crate::source::{FileId, Span};

use super::shape::{
    encode_symbolic_declaration_shape_skeleton_c1, encode_symbolic_definition_owner_skeleton_c1,
    encode_target_root, DeclarationKind, ShapeEncodingError, SymbolicDeclarationShapeSkeleton,
    SymbolicDefinitionOwnerSkeleton, TargetRoot,
};
use super::{HirBodyId, HirItemId, HirModuleId, TargetId};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DependencyKind {
    Normal,
    Development,
}

impl DependencyKind {
    const fn tag(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Development => 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackageSourceSkeleton {
    Workspace {
        path: String,
        source_digest: [u8; 32],
    },
    Registry {
        archive_digest: [u8; 32],
        source_digest: [u8; 32],
        provenance_record_digest: [u8; 32],
        inclusion_record_digest: [u8; 32],
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageDependencySkeleton {
    pub alias: String,
    pub package: PackageId,
    pub requirement: String,
    pub kind: DependencyKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageProvenanceSkeleton {
    pub registry_origin: String,
    pub scoped_name: String,
    pub version: String,
    pub source: PackageSourceSkeleton,
    pub dependencies: Vec<PackageDependencySkeleton>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CtfeBudgetsSkeleton {
    pub step_limit: u64,
    pub depth_limit: u64,
    pub heap_limit: u64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ModuleRef {
    pub package: PackageId,
    pub target: TargetRoot,
    pub path: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Visibility {
    DeclaringModule,
    AncestorModule { path: Vec<String> },
    Package,
    Public,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Namespace {
    Module,
    Type,
    Value,
}

impl Namespace {
    const fn tag(self) -> u8 {
        match self {
            Self::Module => 1,
            Self::Type => 2,
            Self::Value => 3,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticDefinitionKey {
    pub module: ModuleRef,
    /// Typed C1 owner authority. C4 produces raw owner-entry bytes only after
    /// this skeleton and every referenced parent shape are pre-result ready.
    pub owner_path: SymbolicDefinitionOwnerSkeleton,
    pub kind: DeclarationKind,
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MemberVisibilityPath {
    Field {
        ordinal: u64,
    },
    Variant {
        ordinal: u64,
    },
    VariantField {
        variant_ordinal: u64,
        field_ordinal: u64,
    },
    Method {
        ordinal: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticMemberVisibility {
    pub path: MemberVisibilityPath,
    pub declared_visibility: Visibility,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticDefinitionInventorySkeleton {
    pub hir_item: HirItemId,
    pub key: SemanticDefinitionKey,
    pub declared_visibility: Visibility,
    pub member_visibilities: Vec<SemanticMemberVisibility>,
    /// Complete interleaved C1 declaration-shape authority. Stable pre-result
    /// bytes are a later projection and do not replace pending typed leaves.
    pub symbolic_shape: SymbolicDeclarationShapeSkeleton,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticBindingTarget {
    Module(ModuleRef),
    Definition(SemanticDefinitionKey),
    NominalConstructor(SemanticDefinitionKey),
    EnumVariant {
        owner: SemanticDefinitionKey,
        ordinal: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticBindingPath {
    pub module: ModuleRef,
    /// Canonical nonempty path relative to `module`. Nominal members retain
    /// both the owner and member segments.
    pub segments: Vec<String>,
    pub namespace: Namespace,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticBindingOrigin {
    Declaration,
    ReExport {
        source: SemanticBindingPath,
        target: Box<SemanticBindingTarget>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticBindingInventorySkeleton {
    pub name: String,
    pub namespace: Namespace,
    pub target: SemanticBindingTarget,
    pub declared_visibility: Visibility,
    pub origin: SemanticBindingOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticModuleInventorySkeleton {
    pub hir_module: HirModuleId,
    pub module: ModuleRef,
    pub file: FileId,
    pub declared_visibility: Visibility,
    pub bindings: Vec<SemanticBindingInventorySkeleton>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ManifestCapability {
    Args,
    Atomics,
    Environment,
    Files,
    MonotonicClock,
    Stdio,
    Subprocess,
    Synchronization,
    Tcp,
    Threads,
    Udp,
    WallClock,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticTargetContractSkeleton {
    Library,
    Binary {
        root_world: Box<SemanticDefinitionKey>,
        main: Box<SemanticDefinitionKey>,
        capabilities: Vec<ManifestCapability>,
    },
    Environment {
        root_world: Box<SemanticDefinitionKey>,
        profile: String,
        reset: Box<SemanticDefinitionKey>,
        step: Box<SemanticDefinitionKey>,
        self_play: Box<SemanticDefinitionKey>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticTargetInventorySkeleton {
    pub manifest_ordinal: u64,
    pub target_id: TargetId,
    pub target: TargetRoot,
    pub root_module: HirModuleId,
    pub contract: SemanticTargetContractSkeleton,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SemanticBodyKind {
    Declaration,
    Closure,
    Generator,
    WorldInitializer,
    ArrayLength,
    RepeatCount,
    IntegerGenericArgument,
}

impl SemanticBodyKind {
    const fn tag(self) -> u8 {
        match self {
            Self::Declaration => 1,
            Self::Closure => 2,
            Self::Generator => 3,
            Self::WorldInitializer => 4,
            Self::ArrayLength => 5,
            Self::RepeatCount => 6,
            Self::IntegerGenericArgument => 7,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticBodyKey {
    pub package: PackageId,
    pub target: TargetRoot,
    pub modules: Vec<String>,
    pub declaration_kind: DeclarationKind,
    pub declaration_name: Option<String>,
    pub declaration_span: Span,
    pub body_kind: SemanticBodyKind,
    pub ordinal: u64,
    pub body_span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticBodyInventorySkeleton {
    pub hir_body: HirBodyId,
    pub key: SemanticBodyKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticPackageInventorySkeleton {
    pub package_node: PackageNodeId,
    pub package: PackageId,
    pub provenance: PackageProvenanceSkeleton,
    pub ctfe_budgets: CtfeBudgetsSkeleton,
    pub targets: Vec<SemanticTargetInventorySkeleton>,
    pub modules: Vec<SemanticModuleInventorySkeleton>,
    pub definitions: Vec<SemanticDefinitionInventorySkeleton>,
    pub bodies: Vec<SemanticBodyInventorySkeleton>,
}

/// Complete C1 inventory shape parameterized by the compiler-private embedded
/// Core authority. The generic parameter lets C1 retain an opaque branded Arc
/// without defining or forging the later authority in the frontend crate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticInventorySkeleton<EmbeddedCore> {
    pub embedded_core: EmbeddedCore,
    pub source_trees: Vec<(PackageId, [u8; 32])>,
    pub workspace_roots: Vec<PackageId>,
    pub packages: Vec<SemanticPackageInventorySkeleton>,
}

/// Field-complete, host-path-free C1 inventory bytes. This is a deterministic
/// golden/debug projection of the unverified skeleton, not a stable identity
/// preimage and not a `VerifiedSemanticInventory` brand.
pub fn encode_inventory_c1(
    inventory: &SemanticInventorySkeleton<Arc<VerifiedEmbeddedCoreAuthority>>,
) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = b"ARCHE-C1-INVENTORY\0".to_vec();
    output.extend_from_slice(&1_u32.to_le_bytes());
    inventory_blob(
        &mut output,
        inventory.embedded_core.projection().canonical_bytes(),
        "embedded-Core projection",
    )?;

    inventory_count(&mut output, inventory.source_trees.len(), "source trees")?;
    for (package, digest) in &inventory.source_trees {
        output.extend_from_slice(package.as_bytes());
        output.extend_from_slice(digest);
    }
    inventory_count(
        &mut output,
        inventory.workspace_roots.len(),
        "workspace roots",
    )?;
    for package in &inventory.workspace_roots {
        output.extend_from_slice(package.as_bytes());
    }
    inventory_count(&mut output, inventory.packages.len(), "packages")?;
    for package in &inventory.packages {
        encode_inventory_package(&mut output, package)?;
    }
    Ok(output)
}

fn encode_inventory_package(
    output: &mut Vec<u8>,
    package: &SemanticPackageInventorySkeleton,
) -> Result<(), ShapeEncodingError> {
    output.extend_from_slice(&package.package_node.get().to_le_bytes());
    output.extend_from_slice(package.package.as_bytes());
    inventory_string(
        output,
        &package.provenance.registry_origin,
        "package registry origin",
    )?;
    inventory_string(
        output,
        &package.provenance.scoped_name,
        "package scoped name",
    )?;
    inventory_string(output, &package.provenance.version, "package version")?;
    match &package.provenance.source {
        PackageSourceSkeleton::Workspace {
            path,
            source_digest,
        } => {
            output.push(1);
            inventory_string(output, path, "workspace package path")?;
            output.extend_from_slice(source_digest);
        }
        PackageSourceSkeleton::Registry {
            archive_digest,
            source_digest,
            provenance_record_digest,
            inclusion_record_digest,
        } => {
            output.push(2);
            output.extend_from_slice(archive_digest);
            output.extend_from_slice(source_digest);
            output.extend_from_slice(provenance_record_digest);
            output.extend_from_slice(inclusion_record_digest);
        }
    }
    inventory_count(
        output,
        package.provenance.dependencies.len(),
        "package dependencies",
    )?;
    for dependency in &package.provenance.dependencies {
        inventory_string(output, &dependency.alias, "dependency alias")?;
        output.extend_from_slice(dependency.package.as_bytes());
        inventory_string(output, &dependency.requirement, "dependency requirement")?;
        output.push(dependency.kind.tag());
    }
    output.extend_from_slice(&package.ctfe_budgets.step_limit.to_le_bytes());
    output.extend_from_slice(&package.ctfe_budgets.depth_limit.to_le_bytes());
    output.extend_from_slice(&package.ctfe_budgets.heap_limit.to_le_bytes());

    inventory_count(output, package.targets.len(), "package targets")?;
    for target in &package.targets {
        output.extend_from_slice(&target.manifest_ordinal.to_le_bytes());
        output.extend_from_slice(&target.target_id.0.to_le_bytes());
        encode_target_root(&target.target, output)?;
        encode_hir_module_id(output, target.root_module);
        encode_target_contract(output, &target.contract)?;
    }
    inventory_count(output, package.modules.len(), "package modules")?;
    for module in &package.modules {
        encode_hir_module_id(output, module.hir_module);
        encode_module_ref(output, &module.module)?;
        output.extend_from_slice(&module.file.0.to_le_bytes());
        encode_visibility(output, &module.declared_visibility)?;
        inventory_count(output, module.bindings.len(), "module bindings")?;
        for binding in &module.bindings {
            encode_binding(output, binding)?;
        }
    }
    inventory_count(output, package.definitions.len(), "package definitions")?;
    for definition in &package.definitions {
        output.extend_from_slice(&definition.hir_item.0.to_le_bytes());
        encode_definition_key(output, &definition.key)?;
        encode_visibility(output, &definition.declared_visibility)?;
        inventory_count(
            output,
            definition.member_visibilities.len(),
            "member visibilities",
        )?;
        for member in &definition.member_visibilities {
            encode_member_visibility(output, member)?;
        }
        inventory_blob(
            output,
            &encode_symbolic_declaration_shape_skeleton_c1(&definition.symbolic_shape)?,
            "definition symbolic shape skeleton",
        )?;
    }
    inventory_count(output, package.bodies.len(), "package bodies")?;
    for body in &package.bodies {
        output.extend_from_slice(&body.hir_body.0.to_le_bytes());
        encode_body_key(output, &body.key)?;
    }
    Ok(())
}

fn encode_target_contract(
    output: &mut Vec<u8>,
    contract: &SemanticTargetContractSkeleton,
) -> Result<(), ShapeEncodingError> {
    match contract {
        SemanticTargetContractSkeleton::Library => output.push(1),
        SemanticTargetContractSkeleton::Binary {
            root_world,
            main,
            capabilities,
        } => {
            output.push(2);
            encode_definition_key(output, root_world)?;
            encode_definition_key(output, main)?;
            inventory_count(output, capabilities.len(), "manifest capabilities")?;
            for capability in capabilities {
                output.push(manifest_capability_tag(*capability));
            }
        }
        SemanticTargetContractSkeleton::Environment {
            root_world,
            profile,
            reset,
            step,
            self_play,
        } => {
            output.push(3);
            encode_definition_key(output, root_world)?;
            inventory_string(output, profile, "environment profile")?;
            encode_definition_key(output, reset)?;
            encode_definition_key(output, step)?;
            encode_definition_key(output, self_play)?;
        }
    }
    Ok(())
}

fn manifest_capability_tag(capability: ManifestCapability) -> u8 {
    match capability {
        ManifestCapability::Args => 1,
        ManifestCapability::Atomics => 2,
        ManifestCapability::Environment => 3,
        ManifestCapability::Files => 4,
        ManifestCapability::MonotonicClock => 5,
        ManifestCapability::Stdio => 6,
        ManifestCapability::Subprocess => 7,
        ManifestCapability::Synchronization => 8,
        ManifestCapability::Tcp => 9,
        ManifestCapability::Threads => 10,
        ManifestCapability::Udp => 11,
        ManifestCapability::WallClock => 12,
    }
}

fn encode_binding(
    output: &mut Vec<u8>,
    binding: &SemanticBindingInventorySkeleton,
) -> Result<(), ShapeEncodingError> {
    inventory_string(output, &binding.name, "binding name")?;
    output.push(binding.namespace.tag());
    encode_binding_target(output, &binding.target)?;
    encode_visibility(output, &binding.declared_visibility)?;
    match &binding.origin {
        SemanticBindingOrigin::Declaration => output.push(1),
        SemanticBindingOrigin::ReExport { source, target } => {
            output.push(2);
            encode_binding_path(output, source)?;
            encode_binding_target(output, target)?;
        }
    }
    Ok(())
}

fn encode_binding_path(
    output: &mut Vec<u8>,
    path: &SemanticBindingPath,
) -> Result<(), ShapeEncodingError> {
    if path.segments.is_empty() {
        return Err(ShapeEncodingError::InvalidDeclarationShape(
            "re-export binding source path must contain at least one segment",
        ));
    }
    encode_module_ref(output, &path.module)?;
    inventory_strings(output, &path.segments, "binding path segment")?;
    output.push(path.namespace.tag());
    Ok(())
}

fn encode_binding_target(
    output: &mut Vec<u8>,
    target: &SemanticBindingTarget,
) -> Result<(), ShapeEncodingError> {
    match target {
        SemanticBindingTarget::Module(module) => {
            output.push(1);
            encode_module_ref(output, module)?;
        }
        SemanticBindingTarget::Definition(definition) => {
            output.push(2);
            encode_definition_key(output, definition)?;
        }
        SemanticBindingTarget::NominalConstructor(owner) => {
            output.push(3);
            encode_definition_key(output, owner)?;
        }
        SemanticBindingTarget::EnumVariant { owner, ordinal } => {
            output.push(4);
            encode_definition_key(output, owner)?;
            output.extend_from_slice(&ordinal.to_le_bytes());
        }
    }
    Ok(())
}

fn encode_definition_key(
    output: &mut Vec<u8>,
    key: &SemanticDefinitionKey,
) -> Result<(), ShapeEncodingError> {
    encode_module_ref(output, &key.module)?;
    inventory_blob(
        output,
        &encode_symbolic_definition_owner_skeleton_c1(&key.owner_path)?,
        "definition owner skeleton",
    )?;
    output.push(key.kind.tag());
    inventory_string(output, &key.name, "definition name")?;
    encode_span(output, key.span);
    Ok(())
}

fn encode_member_visibility(
    output: &mut Vec<u8>,
    member: &SemanticMemberVisibility,
) -> Result<(), ShapeEncodingError> {
    match member.path {
        MemberVisibilityPath::Field { ordinal } => {
            output.push(1);
            output.extend_from_slice(&ordinal.to_le_bytes());
        }
        MemberVisibilityPath::Variant { ordinal } => {
            output.push(2);
            output.extend_from_slice(&ordinal.to_le_bytes());
        }
        MemberVisibilityPath::VariantField {
            variant_ordinal,
            field_ordinal,
        } => {
            output.push(3);
            output.extend_from_slice(&variant_ordinal.to_le_bytes());
            output.extend_from_slice(&field_ordinal.to_le_bytes());
        }
        MemberVisibilityPath::Method { ordinal } => {
            output.push(4);
            output.extend_from_slice(&ordinal.to_le_bytes());
        }
    }
    encode_visibility(output, &member.declared_visibility)
}

fn encode_body_key(output: &mut Vec<u8>, key: &SemanticBodyKey) -> Result<(), ShapeEncodingError> {
    output.extend_from_slice(key.package.as_bytes());
    encode_target_root(&key.target, output)?;
    inventory_strings(output, &key.modules, "body module path")?;
    output.push(key.declaration_kind.tag());
    match &key.declaration_name {
        Some(name) => {
            output.push(1);
            inventory_string(output, name, "body declaration name")?;
        }
        None => output.push(0),
    }
    encode_span(output, key.declaration_span);
    output.push(key.body_kind.tag());
    output.extend_from_slice(&key.ordinal.to_le_bytes());
    encode_span(output, key.body_span);
    Ok(())
}

fn encode_module_ref(output: &mut Vec<u8>, module: &ModuleRef) -> Result<(), ShapeEncodingError> {
    output.extend_from_slice(module.package.as_bytes());
    encode_target_root(&module.target, output)?;
    inventory_strings(output, &module.path, "module path")
}

fn encode_hir_module_id(output: &mut Vec<u8>, module: HirModuleId) {
    output.extend_from_slice(&module.package().get().to_le_bytes());
    output.extend_from_slice(&module.target().0.to_le_bytes());
    output.extend_from_slice(&module.local().to_le_bytes());
}

fn encode_visibility(
    output: &mut Vec<u8>,
    visibility: &Visibility,
) -> Result<(), ShapeEncodingError> {
    match visibility {
        Visibility::DeclaringModule => output.push(1),
        Visibility::AncestorModule { path } => {
            output.push(2);
            inventory_strings(output, path, "visibility ancestor path")?;
        }
        Visibility::Package => output.push(3),
        Visibility::Public => output.push(4),
    }
    Ok(())
}

fn encode_span(output: &mut Vec<u8>, span: Span) {
    output.extend_from_slice(&span.file.0.to_le_bytes());
    output.extend_from_slice(&span.start.byte.to_le_bytes());
    output.extend_from_slice(&span.end.byte.to_le_bytes());
    output.extend_from_slice(&span.start.line.to_le_bytes());
    output.extend_from_slice(&span.start.column.to_le_bytes());
    output.extend_from_slice(&span.end.line.to_le_bytes());
    output.extend_from_slice(&span.end.column.to_le_bytes());
}

fn inventory_strings(
    output: &mut Vec<u8>,
    values: &[String],
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    inventory_count(output, values.len(), label)?;
    for value in values {
        inventory_string(output, value, label)?;
    }
    Ok(())
}

fn inventory_string(
    output: &mut Vec<u8>,
    value: &str,
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    inventory_blob(output, value.as_bytes(), label)
}

fn inventory_blob(
    output: &mut Vec<u8>,
    value: &[u8],
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    inventory_count(output, value.len(), label)?;
    output.extend_from_slice(value);
    Ok(())
}

fn inventory_count(
    output: &mut Vec<u8>,
    count: usize,
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    let count = u64::try_from(count).map_err(|_| ShapeEncodingError::LengthOverflow(label))?;
    output.extend_from_slice(&count.to_le_bytes());
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InventoryError {
    Shape(ShapeEncodingError),
    DuplicatePackage,
    DuplicateSourceTree,
    DuplicateWorkspaceRoot,
    SourceTreePackageMismatch,
    WorkspaceRootPackageMismatch,
    PackageNodeMismatch,
    TargetOrder,
    DuplicateDependency,
    ModulePackageMismatch,
    DuplicateModule,
    MissingRootModule,
    DuplicateBinding,
    DefinitionPackageMismatch,
    DuplicateDefinition,
    DuplicateMemberVisibility,
    BodyPackageMismatch,
    DuplicateBody,
    DuplicateSessionId,
    NonDenseSessionId,
}

impl From<ShapeEncodingError> for InventoryError {
    fn from(error: ShapeEncodingError) -> Self {
        Self::Shape(error)
    }
}

impl fmt::Display for InventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Shape(error) => return error.fmt(formatter),
            Self::DuplicatePackage => "semantic inventory repeats a package",
            Self::DuplicateSourceTree => "semantic inventory repeats a source-tree row",
            Self::DuplicateWorkspaceRoot => "semantic inventory repeats a workspace root",
            Self::SourceTreePackageMismatch => {
                "semantic inventory source trees do not exactly cover packages"
            }
            Self::WorkspaceRootPackageMismatch => {
                "semantic inventory workspace root is not an ordinary package"
            }
            Self::PackageNodeMismatch => {
                "semantic inventory package-node IDs are not canonical and dense"
            }
            Self::TargetOrder => {
                "semantic target IDs/manifest ordinals are not zero-based and dense"
            }
            Self::DuplicateDependency => "semantic inventory repeats a dependency row",
            Self::ModulePackageMismatch => "semantic module belongs to the wrong package",
            Self::DuplicateModule => "semantic inventory repeats a module",
            Self::MissingRootModule => {
                "semantic target does not have exactly one public empty-path root module"
            }
            Self::DuplicateBinding => "semantic module repeats a binding",
            Self::DefinitionPackageMismatch => "semantic definition belongs to the wrong package",
            Self::DuplicateDefinition => "semantic inventory repeats a definition key",
            Self::DuplicateMemberVisibility => {
                "semantic definition repeats a member-visibility path"
            }
            Self::BodyPackageMismatch => "semantic body belongs to the wrong package",
            Self::DuplicateBody => "semantic inventory repeats a body key",
            Self::DuplicateSessionId => {
                "semantic inventory repeats a globally unique HIR session ID"
            }
            Self::NonDenseSessionId => {
                "semantic inventory HIR session IDs are not zero-based and dense"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for InventoryError {}

pub struct SemanticInventoryBuilder<EmbeddedCore> {
    embedded_core: EmbeddedCore,
    source_trees: Vec<(PackageId, [u8; 32])>,
    workspace_roots: Vec<PackageId>,
    packages: Vec<SemanticPackageInventorySkeleton>,
}

impl<EmbeddedCore> SemanticInventoryBuilder<EmbeddedCore> {
    pub fn new(embedded_core: EmbeddedCore) -> Self {
        Self {
            embedded_core,
            source_trees: Vec::new(),
            workspace_roots: Vec::new(),
            packages: Vec::new(),
        }
    }

    pub fn push_source_tree(&mut self, package: PackageId, digest: [u8; 32]) {
        self.source_trees.push((package, digest));
    }

    pub fn push_workspace_root(&mut self, package: PackageId) {
        self.workspace_roots.push(package);
    }

    pub fn push_package(&mut self, package: SemanticPackageInventorySkeleton) {
        self.packages.push(package);
    }

    pub fn finish(mut self) -> Result<SemanticInventorySkeleton<EmbeddedCore>, InventoryError> {
        self.packages
            .sort_by_key(|package| *package.package.as_bytes());
        if self
            .packages
            .windows(2)
            .any(|pair| pair[0].package == pair[1].package)
        {
            return Err(InventoryError::DuplicatePackage);
        }
        validate_package_nodes(&self.packages)?;
        for package in &mut self.packages {
            canonicalize_package(package)?;
        }
        validate_global_session_ids(&self.packages)?;

        self.source_trees.sort_by_key(|row| *row.0.as_bytes());
        if self
            .source_trees
            .windows(2)
            .any(|pair| pair[0].0 == pair[1].0)
        {
            return Err(InventoryError::DuplicateSourceTree);
        }
        if self.source_trees.len() != self.packages.len()
            || self
                .source_trees
                .iter()
                .zip(&self.packages)
                .any(|(source, package)| source.0 != package.package)
        {
            return Err(InventoryError::SourceTreePackageMismatch);
        }

        self.workspace_roots
            .sort_by_key(|package| *package.as_bytes());
        if self
            .workspace_roots
            .windows(2)
            .any(|pair| pair[0] == pair[1])
        {
            return Err(InventoryError::DuplicateWorkspaceRoot);
        }
        let ordinary_packages = self
            .packages
            .iter()
            .map(|package| package.package)
            .collect::<BTreeSet<_>>();
        if self
            .workspace_roots
            .iter()
            .any(|package| !ordinary_packages.contains(package))
        {
            return Err(InventoryError::WorkspaceRootPackageMismatch);
        }

        Ok(SemanticInventorySkeleton {
            embedded_core: self.embedded_core,
            source_trees: self.source_trees,
            workspace_roots: self.workspace_roots,
            packages: self.packages,
        })
    }
}

fn validate_package_nodes(
    packages: &[SemanticPackageInventorySkeleton],
) -> Result<(), InventoryError> {
    let mut nodes = packages
        .iter()
        .map(|package| package.package_node)
        .collect::<Vec<_>>();
    nodes.sort();
    for (expected, actual) in nodes.into_iter().enumerate() {
        let expected = u64::try_from(expected).map_err(|_| InventoryError::PackageNodeMismatch)?;
        if actual != PackageNodeId::new(expected) {
            return Err(InventoryError::PackageNodeMismatch);
        }
    }
    Ok(())
}

fn canonicalize_package(
    package: &mut SemanticPackageInventorySkeleton,
) -> Result<(), InventoryError> {
    package.provenance.dependencies.sort_by(|left, right| {
        left.alias
            .as_bytes()
            .cmp(right.alias.as_bytes())
            .then_with(|| left.package.as_bytes().cmp(right.package.as_bytes()))
            .then_with(|| {
                left.requirement
                    .as_bytes()
                    .cmp(right.requirement.as_bytes())
            })
            .then_with(|| left.kind.tag().cmp(&right.kind.tag()))
    });
    if package
        .provenance
        .dependencies
        .windows(2)
        .any(|pair| pair[0].alias == pair[1].alias)
    {
        return Err(InventoryError::DuplicateDependency);
    }

    package
        .targets
        .sort_by_key(|target| target.manifest_ordinal);
    for (expected, target) in package.targets.iter().enumerate() {
        let expected = u64::try_from(expected).map_err(|_| InventoryError::TargetOrder)?;
        if target.manifest_ordinal != expected || target.target_id != TargetId(expected) {
            return Err(InventoryError::TargetOrder);
        }
        if target.root_module.package() != package.package_node
            || target.root_module.target() != target.target_id
            || target.root_module.local() != 0
        {
            return Err(InventoryError::TargetOrder);
        }
    }

    for module in &mut package.modules {
        if module.module.package != package.package
            || module.hir_module.package() != package.package_node
            || package.targets.iter().all(|target| {
                target.target_id != module.hir_module.target()
                    || target.target != module.module.target
            })
        {
            return Err(InventoryError::ModulePackageMismatch);
        }
        canonical_sort(&mut module.bindings, binding_bytes)?;
        if module
            .bindings
            .windows(2)
            .any(|pair| pair[0].name == pair[1].name && pair[0].namespace == pair[1].namespace)
        {
            return Err(InventoryError::DuplicateBinding);
        }
    }
    canonical_sort(&mut package.modules, |module| module_bytes(&module.module))?;
    if package
        .modules
        .windows(2)
        .any(|pair| pair[0].module == pair[1].module)
    {
        return Err(InventoryError::DuplicateModule);
    }
    for target in &package.targets {
        let roots = package
            .modules
            .iter()
            .filter(|module| {
                module.hir_module == target.root_module
                    && module.module.target == target.target
                    && module.module.path.is_empty()
                    && module.declared_visibility == Visibility::Public
            })
            .count();
        if roots != 1 {
            return Err(InventoryError::MissingRootModule);
        }
        let mut locals = package
            .modules
            .iter()
            .filter(|module| module.hir_module.target() == target.target_id)
            .map(|module| module.hir_module.local())
            .collect::<Vec<_>>();
        locals.sort_unstable();
        for (expected, actual) in locals.into_iter().enumerate() {
            let expected =
                u64::try_from(expected).map_err(|_| InventoryError::NonDenseSessionId)?;
            if actual != expected {
                return Err(InventoryError::NonDenseSessionId);
            }
        }
    }

    for definition in &mut package.definitions {
        if definition.key.module.package != package.package {
            return Err(InventoryError::DefinitionPackageMismatch);
        }
        definition
            .member_visibilities
            .sort_by_key(|member| member.path.clone());
        if definition
            .member_visibilities
            .windows(2)
            .any(|pair| pair[0].path == pair[1].path)
        {
            return Err(InventoryError::DuplicateMemberVisibility);
        }
    }
    canonical_sort(&mut package.definitions, |definition| {
        definition_key_bytes(&definition.key)
    })?;
    if package
        .definitions
        .windows(2)
        .any(|pair| pair[0].key == pair[1].key)
    {
        return Err(InventoryError::DuplicateDefinition);
    }

    if package
        .bodies
        .iter()
        .any(|body| body.key.package != package.package)
    {
        return Err(InventoryError::BodyPackageMismatch);
    }
    canonical_sort(&mut package.bodies, |body| body_key_bytes(&body.key))?;
    if package
        .bodies
        .windows(2)
        .any(|pair| pair[0].key == pair[1].key)
    {
        return Err(InventoryError::DuplicateBody);
    }
    Ok(())
}

fn validate_global_session_ids(
    packages: &[SemanticPackageInventorySkeleton],
) -> Result<(), InventoryError> {
    let mut modules = BTreeSet::new();
    let mut items = BTreeSet::new();
    let mut bodies = BTreeSet::new();
    for package in packages {
        if package
            .modules
            .iter()
            .any(|module| !modules.insert(module.hir_module))
            || package
                .definitions
                .iter()
                .any(|definition| !items.insert(definition.hir_item))
            || package
                .bodies
                .iter()
                .any(|body| !bodies.insert(body.hir_body))
        {
            return Err(InventoryError::DuplicateSessionId);
        }
    }
    for (expected, actual) in items.into_iter().enumerate() {
        let expected = u64::try_from(expected).map_err(|_| InventoryError::NonDenseSessionId)?;
        if actual.0 != expected {
            return Err(InventoryError::NonDenseSessionId);
        }
    }
    for (expected, actual) in bodies.into_iter().enumerate() {
        let expected = u64::try_from(expected).map_err(|_| InventoryError::NonDenseSessionId)?;
        if actual.0 != expected {
            return Err(InventoryError::NonDenseSessionId);
        }
    }
    Ok(())
}

fn canonical_sort<T>(
    values: &mut Vec<T>,
    mut encode: impl FnMut(&T) -> Result<Vec<u8>, ShapeEncodingError>,
) -> Result<(), InventoryError> {
    let mut keyed = std::mem::take(values)
        .into_iter()
        .map(|value| encode(&value).map(|key| (key, value)))
        .collect::<Result<Vec<_>, _>>()?;
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    *values = keyed.into_iter().map(|(_, value)| value).collect();
    Ok(())
}

fn module_bytes(module: &ModuleRef) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = module.package.as_bytes().to_vec();
    encode_target_root(&module.target, &mut output)?;
    strings(&mut output, &module.path, "module path")?;
    Ok(output)
}

fn definition_key_bytes(key: &SemanticDefinitionKey) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = module_bytes(&key.module)?;
    bytes(
        &mut output,
        &encode_symbolic_definition_owner_skeleton_c1(&key.owner_path)?,
        "definition owner skeleton",
    )?;
    output.push(key.kind.tag());
    string(&mut output, &key.name, "definition name")?;
    span(&mut output, key.span);
    Ok(output)
}

fn body_key_bytes(key: &SemanticBodyKey) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = key.package.as_bytes().to_vec();
    encode_target_root(&key.target, &mut output)?;
    strings(&mut output, &key.modules, "body module path")?;
    output.push(key.declaration_kind.tag());
    option_string(
        &mut output,
        key.declaration_name.as_deref(),
        "body declaration name",
    )?;
    span(&mut output, key.declaration_span);
    output.push(key.body_kind.tag());
    output.extend_from_slice(&key.ordinal.to_le_bytes());
    span(&mut output, key.body_span);
    Ok(output)
}

fn binding_bytes(
    binding: &SemanticBindingInventorySkeleton,
) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = Vec::new();
    string(&mut output, &binding.name, "binding name")?;
    output.push(binding.namespace.tag());
    encode_binding_target(&mut output, &binding.target)?;
    Ok(output)
}

fn span(output: &mut Vec<u8>, value: Span) {
    for field in [
        value.file.0,
        value.start.byte,
        value.end.byte,
        value.start.line,
        value.start.column,
        value.end.line,
        value.end.column,
    ] {
        output.extend_from_slice(&field.to_le_bytes());
    }
}

fn option_string(
    output: &mut Vec<u8>,
    value: Option<&str>,
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    match value {
        None => output.push(0),
        Some(value) => {
            output.push(1);
            string(output, value, label)?;
        }
    }
    Ok(())
}

fn strings(
    output: &mut Vec<u8>,
    values: &[String],
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    count(output, values.len(), label)?;
    for value in values {
        string(output, value, label)?;
    }
    Ok(())
}

fn string(
    output: &mut Vec<u8>,
    value: &str,
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    bytes(output, value.as_bytes(), label)
}

fn bytes(
    output: &mut Vec<u8>,
    value: &[u8],
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    count(output, value.len(), label)?;
    output.extend_from_slice(value);
    Ok(())
}

fn count(
    output: &mut Vec<u8>,
    value: usize,
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    let value = u64::try_from(value).map_err(|_| ShapeEncodingError::LengthOverflow(label))?;
    output.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use arche_package::{load_workspace, resolve, ManifestRequest, RegistrySnapshot};
    use sha2::{Digest as _, Sha256};

    use super::*;
    use crate::hir::check_workspace_c1;
    use crate::source::SourcePosition;

    fn package(value: u8) -> PackageId {
        PackageId::from_bytes([value; 16])
    }

    fn span_at(file: u64, byte: u64) -> Span {
        Span {
            file: FileId(file),
            start: SourcePosition {
                byte,
                line: 1,
                column: byte + 1,
            },
            end: SourcePosition {
                byte: byte + 1,
                line: 1,
                column: byte + 2,
            },
        }
    }

    fn module(package: PackageId) -> ModuleRef {
        ModuleRef {
            package,
            target: TargetRoot::Library,
            path: Vec::new(),
        }
    }

    fn root_module(
        node: u64,
        stable: PackageId,
        target_id: TargetId,
        target: TargetRoot,
    ) -> SemanticModuleInventorySkeleton {
        SemanticModuleInventorySkeleton {
            hir_module: HirModuleId::new(PackageNodeId::new(node), target_id, 0),
            module: ModuleRef {
                package: stable,
                target,
                path: Vec::new(),
            },
            file: FileId(node),
            declared_visibility: Visibility::Public,
            bindings: Vec::new(),
        }
    }

    fn definition(
        package: PackageId,
        name: &str,
        item: u64,
    ) -> SemanticDefinitionInventorySkeleton {
        SemanticDefinitionInventorySkeleton {
            hir_item: HirItemId(item),
            key: SemanticDefinitionKey {
                module: module(package),
                owner_path: SymbolicDefinitionOwnerSkeleton::TopLevel,
                kind: DeclarationKind::Struct,
                name: name.to_owned(),
                span: span_at(0, item),
            },
            declared_visibility: Visibility::DeclaringModule,
            member_visibilities: Vec::new(),
            symbolic_shape: SymbolicDeclarationShapeSkeleton::default(),
        }
    }

    fn empty_package(node: u64, stable: PackageId, name: &str) -> SemanticPackageInventorySkeleton {
        SemanticPackageInventorySkeleton {
            package_node: PackageNodeId::new(node),
            package: stable,
            provenance: PackageProvenanceSkeleton {
                registry_origin: "registry+https://packages.arche-lang.org".to_owned(),
                scoped_name: name.to_owned(),
                version: "0.1.0".to_owned(),
                source: PackageSourceSkeleton::Workspace {
                    path: ".".to_owned(),
                    source_digest: [stable.as_bytes()[0]; 32],
                },
                dependencies: Vec::new(),
            },
            ctfe_budgets: CtfeBudgetsSkeleton {
                step_limit: 10_000_000,
                depth_limit: 1_024,
                heap_limit: 64 * 1024 * 1024,
            },
            targets: Vec::new(),
            modules: Vec::new(),
            definitions: Vec::new(),
            bodies: Vec::new(),
        }
    }

    #[test]
    fn builder_sorts_raw_package_ids_but_preserves_dense_package_nodes() {
        let low = package(1);
        let high = package(200);
        let mut builder = SemanticInventoryBuilder::new(());
        builder.push_package(empty_package(0, high, "example/high"));
        builder.push_package(empty_package(1, low, "example/low"));
        builder.push_source_tree(high, [2; 32]);
        builder.push_source_tree(low, [1; 32]);
        builder.push_workspace_root(high);
        builder.push_workspace_root(low);
        let inventory = builder.finish().unwrap();
        assert_eq!(inventory.packages[0].package, low);
        assert_eq!(inventory.packages[1].package, high);
        assert_eq!(inventory.packages[0].package_node, PackageNodeId::new(1));
        assert_eq!(inventory.packages[1].package_node, PackageNodeId::new(0));
    }

    #[test]
    fn private_items_and_empty_targets_are_not_projected_away() {
        let stable = package(7);
        let mut row = empty_package(0, stable, "example/private");
        row.targets.push(SemanticTargetInventorySkeleton {
            manifest_ordinal: 0,
            target_id: TargetId(0),
            target: TargetRoot::Library,
            root_module: HirModuleId::new(PackageNodeId::new(0), TargetId(0), 0),
            contract: SemanticTargetContractSkeleton::Library,
        });
        row.modules
            .push(root_module(0, stable, TargetId(0), TargetRoot::Library));
        row.definitions.push(definition(stable, "Unused", 0));
        let mut builder = SemanticInventoryBuilder::new(());
        builder.push_package(row);
        builder.push_source_tree(stable, [7; 32]);
        builder.push_workspace_root(stable);
        let inventory = builder.finish().unwrap();
        assert_eq!(inventory.packages[0].targets.len(), 1);
        assert_eq!(inventory.packages[0].definitions[0].key.name, "Unused");
    }

    #[test]
    fn development_dependencies_remain_inventory_visible() {
        let stable = package(9);
        let dependency = package(10);
        let mut row = empty_package(0, stable, "example/app");
        row.provenance.dependencies = vec![
            PackageDependencySkeleton {
                alias: "runtime".to_owned(),
                package: dependency,
                requirement: "^1.0".to_owned(),
                kind: DependencyKind::Normal,
            },
            PackageDependencySkeleton {
                alias: "tests".to_owned(),
                package: dependency,
                requirement: "^1.0".to_owned(),
                kind: DependencyKind::Development,
            },
        ];
        let mut dependency_row = empty_package(1, dependency, "example/dependency");
        dependency_row.provenance.source = PackageSourceSkeleton::Registry {
            archive_digest: [1; 32],
            source_digest: [2; 32],
            provenance_record_digest: [3; 32],
            inclusion_record_digest: [4; 32],
        };
        let mut builder = SemanticInventoryBuilder::new(());
        builder.push_package(row);
        builder.push_package(dependency_row);
        builder.push_source_tree(stable, [9; 32]);
        builder.push_source_tree(dependency, [10; 32]);
        builder.push_workspace_root(stable);
        let inventory = builder.finish().unwrap();
        let app = inventory
            .packages
            .iter()
            .find(|package| package.package == stable)
            .unwrap();
        assert_eq!(app.provenance.dependencies.len(), 2);
        assert!(app
            .provenance
            .dependencies
            .iter()
            .any(|row| row.kind == DependencyKind::Development));
    }

    #[test]
    fn module_and_definition_binding_payloads_are_distinct() {
        let stable = package(11);
        let module_target = SemanticBindingTarget::Module(ModuleRef {
            package: stable,
            target: TargetRoot::Library,
            path: vec!["api".to_owned()],
        });
        let definition_target = SemanticBindingTarget::Definition(definition(stable, "api", 0).key);
        let module_binding = SemanticBindingInventorySkeleton {
            name: "api".to_owned(),
            namespace: Namespace::Module,
            target: module_target,
            declared_visibility: Visibility::Public,
            origin: SemanticBindingOrigin::Declaration,
        };
        let definition_binding = SemanticBindingInventorySkeleton {
            name: "api".to_owned(),
            namespace: Namespace::Type,
            target: definition_target,
            declared_visibility: Visibility::Public,
            origin: SemanticBindingOrigin::Declaration,
        };
        let module_bytes = binding_bytes(&module_binding).unwrap();
        let definition_bytes = binding_bytes(&definition_binding).unwrap();
        assert_ne!(module_bytes, definition_bytes);
        assert_eq!(module_bytes[8 + 3], Namespace::Module.tag());
        assert_eq!(definition_bytes[8 + 3], Namespace::Type.tag());
    }

    #[test]
    fn binding_target_tags_are_the_single_serialization_and_sort_authority() {
        let stable = package(12);
        let owner = definition(stable, "Owner", 0).key;
        let targets = vec![
            SemanticBindingTarget::EnumVariant {
                owner: owner.clone(),
                ordinal: 7,
            },
            SemanticBindingTarget::NominalConstructor(owner.clone()),
            SemanticBindingTarget::Definition(owner),
            SemanticBindingTarget::Module(ModuleRef {
                package: stable,
                target: TargetRoot::Library,
                path: vec!["api".to_owned()],
            }),
        ];
        let mut bindings = targets
            .into_iter()
            .map(|target| SemanticBindingInventorySkeleton {
                name: "same".to_owned(),
                namespace: Namespace::Value,
                target,
                declared_visibility: Visibility::Public,
                origin: SemanticBindingOrigin::Declaration,
            })
            .collect::<Vec<_>>();
        bindings.sort_by_key(|binding| binding_bytes(binding).unwrap());

        let mut tags = Vec::new();
        for binding in &bindings {
            let mut target = Vec::new();
            encode_binding_target(&mut target, &binding.target).unwrap();
            let sort_key = binding_bytes(binding).unwrap();
            let target_offset = 8 + binding.name.len() + 1;
            assert_eq!(&sort_key[target_offset..], target.as_slice());

            let mut serialized = Vec::new();
            encode_binding(&mut serialized, binding).unwrap();
            assert_eq!(
                &serialized[target_offset..target_offset + target.len()],
                target.as_slice()
            );
            tags.push(target[0]);
        }
        assert_eq!(tags, [1, 2, 3, 4]);
    }

    #[test]
    fn binding_source_path_is_nonempty_and_owner_segments_are_canonical_bytes() {
        let stable = package(13);
        let path = SemanticBindingPath {
            module: ModuleRef {
                package: stable,
                target: TargetRoot::Library,
                path: vec!["values".to_owned()],
            },
            segments: vec!["Flag".to_owned(), "lower".to_owned()],
            namespace: Namespace::Value,
        };
        let mut expected = Vec::new();
        encode_module_ref(&mut expected, &path.module).unwrap();
        inventory_strings(&mut expected, &path.segments, "binding path segment").unwrap();
        expected.push(Namespace::Value.tag());
        let mut actual = Vec::new();
        encode_binding_path(&mut actual, &path).unwrap();
        assert_eq!(actual, expected);

        let mut changed = path.clone();
        changed.segments[0] = "OtherFlag".to_owned();
        let mut changed_bytes = Vec::new();
        encode_binding_path(&mut changed_bytes, &changed).unwrap();
        assert_ne!(actual, changed_bytes);

        let mut removed_owner = path.clone();
        removed_owner.segments.remove(0);
        let mut removed_bytes = Vec::new();
        encode_binding_path(&mut removed_bytes, &removed_owner).unwrap();
        assert_ne!(actual, removed_bytes);

        let mut empty = path;
        empty.segments.clear();
        assert_eq!(
            encode_binding_path(&mut Vec::new(), &empty),
            Err(ShapeEncodingError::InvalidDeclarationShape(
                "re-export binding source path must contain at least one segment"
            ))
        );
    }

    #[test]
    fn target_ids_restart_per_package_and_must_match_manifest_ordinals() {
        let mut builder = SemanticInventoryBuilder::new(());
        for (node, stable) in [(0, package(20)), (1, package(21))] {
            let mut row = empty_package(node, stable, "example/target");
            row.targets.push(SemanticTargetInventorySkeleton {
                manifest_ordinal: 0,
                target_id: TargetId(0),
                target: TargetRoot::Library,
                root_module: HirModuleId::new(PackageNodeId::new(node), TargetId(0), 0),
                contract: SemanticTargetContractSkeleton::Library,
            });
            row.modules
                .push(root_module(node, stable, TargetId(0), TargetRoot::Library));
            builder.push_package(row);
            builder.push_source_tree(stable, [node as u8; 32]);
            builder.push_workspace_root(stable);
        }
        let inventory = builder.finish().unwrap();
        assert_eq!(inventory.packages.len(), 2);
        assert!(inventory
            .packages
            .iter()
            .all(|package| package.targets[0].target_id == TargetId(0)));
    }

    #[test]
    fn dependency_alias_is_unique_even_when_other_fields_differ() {
        let app = package(30);
        let first = package(31);
        let second = package(32);
        let mut app_row = empty_package(0, app, "example/app");
        app_row.provenance.dependencies = vec![
            PackageDependencySkeleton {
                alias: "shared".to_owned(),
                package: first,
                requirement: "^1".to_owned(),
                kind: DependencyKind::Normal,
            },
            PackageDependencySkeleton {
                alias: "shared".to_owned(),
                package: second,
                requirement: "^2".to_owned(),
                kind: DependencyKind::Development,
            },
        ];
        let mut builder = SemanticInventoryBuilder::new(());
        builder.push_package(app_row);
        builder.push_package(empty_package(1, first, "example/first"));
        builder.push_package(empty_package(2, second, "example/second"));
        for (stable, digest) in [(app, 30), (first, 31), (second, 32)] {
            builder.push_source_tree(stable, [digest; 32]);
        }
        builder.push_workspace_root(app);
        assert_eq!(builder.finish(), Err(InventoryError::DuplicateDependency));
    }

    #[test]
    fn item_and_module_session_ids_must_be_dense() {
        let stable = package(40);
        let mut item_gap = empty_package(0, stable, "example/gap");
        item_gap.definitions.push(definition(stable, "Gap", 1));
        let mut builder = SemanticInventoryBuilder::new(());
        builder.push_package(item_gap);
        builder.push_source_tree(stable, [40; 32]);
        builder.push_workspace_root(stable);
        assert_eq!(builder.finish(), Err(InventoryError::NonDenseSessionId));

        let mut module_gap = empty_package(0, stable, "example/gap");
        module_gap.targets.push(SemanticTargetInventorySkeleton {
            manifest_ordinal: 0,
            target_id: TargetId(0),
            target: TargetRoot::Library,
            root_module: HirModuleId::new(PackageNodeId::new(0), TargetId(0), 0),
            contract: SemanticTargetContractSkeleton::Library,
        });
        module_gap
            .modules
            .push(root_module(0, stable, TargetId(0), TargetRoot::Library));
        module_gap.modules.push(SemanticModuleInventorySkeleton {
            hir_module: HirModuleId::new(PackageNodeId::new(0), TargetId(0), 2),
            module: ModuleRef {
                package: stable,
                target: TargetRoot::Library,
                path: vec!["gap".to_owned()],
            },
            file: FileId(1),
            declared_visibility: Visibility::DeclaringModule,
            bindings: Vec::new(),
        });
        let mut builder = SemanticInventoryBuilder::new(());
        builder.push_package(module_gap);
        builder.push_source_tree(stable, [40; 32]);
        builder.push_workspace_root(stable);
        assert_eq!(builder.finish(), Err(InventoryError::NonDenseSessionId));
    }

    #[test]
    fn mandatory_corpora_inventory_bytes_are_complete_and_exact() {
        let mut combined = b"ARCHE-C1-INVENTORY-GOLDEN\0".to_vec();
        let mut lengths = Vec::new();
        for name in ["language-environment", "language-game"] {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../../tests/m27c1")
                .join(name);
            let workspace = load_workspace(&ManifestRequest::discover_from(&root)).unwrap();
            let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
            let output = check_workspace_c1(&workspace, &graph, &[]).unwrap();
            let bytes = encode_inventory_c1(output.inventory()).unwrap();
            assert!(bytes.starts_with(b"ARCHE-C1-INVENTORY\0\x01\0\0\0"));
            lengths.push(bytes.len());
            combined.extend_from_slice(&u64::try_from(bytes.len()).unwrap().to_le_bytes());
            combined.extend_from_slice(&bytes);
        }
        let digest: [u8; 32] = Sha256::digest(&combined).into();
        assert_eq!(lengths, vec![136_616, 175_057]);
        assert_eq!(
            digest,
            [
                0x63, 0xd3, 0x3b, 0x68, 0x15, 0xa9, 0xe1, 0xe6, 0xe7, 0x31, 0x9f, 0x53, 0xbf, 0x87,
                0xe3, 0xf2, 0x6a, 0x4d, 0x1a, 0xcb, 0xf0, 0xd4, 0x7f, 0xd2, 0xd7, 0xae, 0x81, 0x09,
                0xa2, 0x18, 0xf0, 0x88,
            ]
        );
    }
}
