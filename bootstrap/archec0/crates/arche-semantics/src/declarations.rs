//! Deterministic C2 declaration rows and the ordinary-impl search boundary.
//!
//! This module retains C1's symbolic definition keys only as session traversal
//! material. It does not mint stable definition identity, and candidate order
//! is never a selection rule.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use arche_foundation::identity::PackageId;
use arche_frontend::{
    encode_semantic_definition_key_session, DeclarationKind, DependencyKind, HirItemId,
    ResolvedSymbolicWorkspaceHir, SemanticDefinitionInventorySkeleton, ShapeEncodingError,
    SymbolicDefinitionOwnerSkeleton, TargetId, TargetRoot, WorkspaceInventorySkeleton,
};

use crate::model::{C2Handoff, SessionItemIndex};

/// Opaque handle owned by one declaration table.
///
/// Its offset is useful for session-local diagnostics only. It is not a stable
/// definition identity and cannot be resolved by a different table.
#[derive(Clone, Debug)]
pub struct DeclarationHandle {
    owner: Arc<DeclarationOwner>,
    offset: u64,
}

impl DeclarationHandle {
    /// Returns the dense, session-local traversal offset.
    pub const fn offset(&self) -> u64 {
        self.offset
    }
}

/// Read-only declaration row tied to the table that owns it.
#[derive(Clone, Copy, Debug)]
pub struct DeclarationView<'a> {
    row: &'a DeclarationRow,
}

impl<'a> DeclarationView<'a> {
    /// Returns the underlying owner-branded C1 item handle.
    pub const fn session_item(self) -> &'a SessionItemIndex {
        &self.row.session_item
    }

    pub const fn package(self) -> PackageId {
        self.row.metadata.package
    }

    pub const fn target(self) -> TargetId {
        self.row.metadata.target
    }

    pub const fn target_root(self) -> &'a TargetRoot {
        &self.row.metadata.target_root
    }

    pub fn module_path(self) -> &'a [String] {
        &self.row.metadata.module_path
    }

    pub const fn kind(self) -> DeclarationKind {
        self.row.metadata.kind
    }

    pub fn name(self) -> &'a str {
        &self.row.metadata.name
    }

    /// Returns the complete symbolic key bytes used only for deterministic
    /// traversal in this session. These bytes are not a stable identity.
    pub fn session_traversal_bytes(self) -> &'a [u8] {
        &self.row.metadata.key_bytes
    }
}

/// Complete, one-to-one C1 HIR/inventory declaration table for one C2 handoff.
#[derive(Debug)]
pub struct DeclarationTable {
    owner: Arc<DeclarationOwner>,
    rows: Vec<DeclarationRow>,
    by_item: BTreeMap<HirItemId, u64>,
    packages: BTreeMap<PackageId, PackageScope>,
}

impl DeclarationTable {
    /// Validates the HIR/inventory relation and constructs canonical rows.
    pub fn build(handoff: &C2Handoff) -> Result<Self, DeclarationTableError> {
        let planned = plan_declarations(handoff.frontend().hir(), handoff.frontend().inventory())?;
        if handoff.indexes().item_count() != planned.rows.len() {
            return Err(DeclarationTableError::SessionItemCountMismatch {
                expected: planned.rows.len(),
                actual: handoff.indexes().item_count(),
            });
        }

        let owner = Arc::new(DeclarationOwner);
        let mut rows = Vec::with_capacity(planned.rows.len());
        let mut by_item = BTreeMap::new();
        for row in planned.rows {
            let session_item = handoff
                .indexes()
                .item_index(row.item)
                .ok_or(DeclarationTableError::MissingSessionItem(row.item))?;
            if handoff.indexes().item(&session_item).map(|view| view.id()) != Some(row.item) {
                return Err(DeclarationTableError::SessionItemRoundTrip(row.item));
            }
            let offset = u64::try_from(rows.len())
                .map_err(|_| DeclarationTableError::DeclarationIndexExhausted)?;
            by_item.insert(row.item, offset);
            rows.push(DeclarationRow {
                session_item,
                metadata: row.metadata,
            });
        }

        Ok(Self {
            owner,
            rows,
            by_item,
            packages: planned.packages,
        })
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Iterates every declaration by complete symbolic key bytes.
    pub fn declarations(&self) -> impl ExactSizeIterator<Item = DeclarationView<'_>> + '_ {
        self.rows.iter().map(|row| DeclarationView { row })
    }

    /// Returns the table-owned handle for a session-local HIR item.
    pub fn handle_for_session_item(&self, item: HirItemId) -> Option<DeclarationHandle> {
        self.by_item.get(&item).map(|&offset| DeclarationHandle {
            owner: Arc::clone(&self.owner),
            offset,
        })
    }

    /// Resolves a handle only when it belongs to this table.
    pub fn declaration(&self, handle: &DeclarationHandle) -> Option<DeclarationView<'_>> {
        if !Arc::ptr_eq(&self.owner, &handle.owner) {
            return None;
        }
        usize::try_from(handle.offset)
            .ok()
            .and_then(|offset| self.rows.get(offset))
            .map(|row| DeclarationView { row })
    }

    /// Computes the exact ordinary-impl candidate set for one checked target.
    ///
    /// Traversal is byte-sorted for reproducibility. A checker must treat the
    /// returned rows as a set and prove a unique semantic match; position must
    /// never break ambiguity or specialization ties.
    pub fn ordinary_impl_candidates(
        &self,
        package: PackageId,
        target: TargetId,
    ) -> Result<OrdinaryImplCandidateUniverse<'_>, CandidateUniverseError> {
        let offsets = select_candidate_offsets(&self.rows, &self.packages, package, target)?;
        Ok(OrdinaryImplCandidateUniverse {
            table: self,
            offsets: offsets.into_boxed_slice(),
        })
    }
}

/// Exact ordinary-impl search universe borrowed from a declaration table.
#[derive(Debug)]
pub struct OrdinaryImplCandidateUniverse<'a> {
    table: &'a DeclarationTable,
    offsets: Box<[u64]>,
}

impl OrdinaryImplCandidateUniverse<'_> {
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    pub fn candidates(&self) -> impl ExactSizeIterator<Item = DeclarationView<'_>> + '_ {
        self.offsets.iter().map(|&offset| {
            let offset = usize::try_from(offset).expect("validated declaration offset fits usize");
            let row = &self.table.rows[offset];
            DeclarationView { row }
        })
    }

    pub fn handles(&self) -> impl ExactSizeIterator<Item = DeclarationHandle> + '_ {
        self.offsets.iter().map(|&offset| DeclarationHandle {
            owner: Arc::clone(&self.table.owner),
            offset,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeclarationTableError {
    DuplicateHirPackage(PackageId),
    DuplicateInventoryPackage(PackageId),
    MissingInventoryPackage(PackageId),
    InventoryPackageWithoutHir(PackageId),
    DuplicateHirTarget {
        package: PackageId,
        target: TargetId,
    },
    DuplicateInventoryTarget {
        package: PackageId,
        target: TargetId,
    },
    MissingInventoryTarget {
        package: PackageId,
        target: TargetId,
    },
    InventoryTargetWithoutHir {
        package: PackageId,
        target: TargetId,
    },
    TargetRootMismatch {
        package: PackageId,
        target: TargetId,
    },
    DuplicateHirModule,
    ItemWithoutModule(HirItemId),
    DuplicateHirItem(HirItemId),
    DuplicateInventoryItem(HirItemId),
    MissingInventoryDefinition(HirItemId),
    InventoryDefinitionWithoutHir(HirItemId),
    DefinitionPackageMismatch(HirItemId),
    DefinitionTargetMismatch(HirItemId),
    DefinitionModuleMismatch(HirItemId),
    DefinitionKindMismatch(HirItemId),
    DefinitionNameMismatch(HirItemId),
    DefinitionSpanMismatch(HirItemId),
    DefinitionOwnerMismatch(HirItemId),
    DefinitionVisibilityMismatch(HirItemId),
    DefinitionMemberVisibilityMismatch(HirItemId),
    DuplicateSemanticKey {
        first: HirItemId,
        second: HirItemId,
    },
    KeyEncoding(ShapeEncodingError),
    SessionItemCountMismatch {
        expected: usize,
        actual: usize,
    },
    MissingSessionItem(HirItemId),
    SessionItemRoundTrip(HirItemId),
    DeclarationIndexExhausted,
}

impl fmt::Display for DeclarationTableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid C2 declaration authority: {self:?}")
    }
}

impl std::error::Error for DeclarationTableError {}

impl From<ShapeEncodingError> for DeclarationTableError {
    fn from(error: ShapeEncodingError) -> Self {
        Self::KeyEncoding(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateUniverseError {
    UnknownPackage(PackageId),
    UnknownTarget {
        package: PackageId,
        target: TargetId,
    },
    MissingNormalDependency {
        package: PackageId,
        dependency: PackageId,
    },
    CandidateIndexExhausted,
}

impl fmt::Display for CandidateUniverseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot construct ordinary impl universe: {self:?}"
        )
    }
}

impl std::error::Error for CandidateUniverseError {}

#[derive(Debug)]
struct DeclarationOwner;

#[derive(Debug)]
struct DeclarationRow {
    session_item: SessionItemIndex,
    metadata: DeclarationMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DeclarationMetadata {
    package: PackageId,
    target: TargetId,
    target_root: TargetRoot,
    module_path: Vec<String>,
    kind: DeclarationKind,
    name: String,
    key_bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScopedDependency {
    package: PackageId,
    kind: DependencyKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PackageScope {
    targets: BTreeMap<TargetId, TargetRoot>,
    dependencies: Vec<ScopedDependency>,
}

#[derive(Debug)]
struct PlannedDeclaration {
    item: HirItemId,
    metadata: DeclarationMetadata,
}

#[derive(Debug)]
struct DeclarationPlan {
    rows: Vec<PlannedDeclaration>,
    packages: BTreeMap<PackageId, PackageScope>,
}

#[derive(Clone)]
struct HirItemAuthority {
    package: PackageId,
    target: TargetId,
    target_root: TargetRoot,
    module_path: Vec<String>,
    kind: DeclarationKind,
    name: String,
    span: arche_frontend::Span,
    top_level: bool,
    declared_visibility: arche_frontend::Visibility,
    member_visibilities: Vec<arche_frontend::SemanticMemberVisibility>,
}

fn plan_declarations(
    hir: &ResolvedSymbolicWorkspaceHir,
    inventory: &WorkspaceInventorySkeleton,
) -> Result<DeclarationPlan, DeclarationTableError> {
    let mut hir_packages = BTreeMap::new();
    let mut hir_items = BTreeMap::new();
    for package in &hir.packages {
        if hir_packages.contains_key(&package.package) {
            return Err(DeclarationTableError::DuplicateHirPackage(package.package));
        }
        let mut targets = BTreeMap::new();
        let mut target_roots = BTreeSet::new();
        for target in &package.targets {
            if targets.insert(target.id, target.target.clone()).is_some()
                || !target_roots.insert(target.target.clone())
            {
                return Err(DeclarationTableError::DuplicateHirTarget {
                    package: package.package,
                    target: target.id,
                });
            }
            let mut modules = BTreeMap::new();
            for module in &target.modules {
                let path = module
                    .path
                    .iter()
                    .map(|segment| segment.as_str().to_owned())
                    .collect::<Vec<_>>();
                if modules.insert(module.id, path).is_some() {
                    return Err(DeclarationTableError::DuplicateHirModule);
                }
            }
            for item in &target.items {
                let module_path = modules
                    .get(&item.module)
                    .ok_or(DeclarationTableError::ItemWithoutModule(item.id))?;
                let authority = HirItemAuthority {
                    package: package.package,
                    target: target.id,
                    target_root: target.target.clone(),
                    module_path: module_path.clone(),
                    kind: item.kind,
                    name: item.name.clone().unwrap_or_default(),
                    span: item.span,
                    top_level: item.owner.is_none(),
                    declared_visibility: item.declared_visibility.clone(),
                    member_visibilities: item.member_visibilities.clone(),
                };
                if hir_items.insert(item.id, authority).is_some() {
                    return Err(DeclarationTableError::DuplicateHirItem(item.id));
                }
            }
        }
        hir_packages.insert(package.package, targets);
    }

    let mut package_scopes = BTreeMap::new();
    let mut inventory_items = BTreeSet::new();
    let mut key_items = BTreeMap::new();
    let mut planned = Vec::new();
    for package in &inventory.packages {
        if package_scopes.contains_key(&package.package) {
            return Err(DeclarationTableError::DuplicateInventoryPackage(
                package.package,
            ));
        }
        let hir_targets = hir_packages.get(&package.package).ok_or(
            DeclarationTableError::InventoryPackageWithoutHir(package.package),
        )?;
        let mut targets = BTreeMap::new();
        let mut target_roots = BTreeSet::new();
        for target in &package.targets {
            if targets
                .insert(target.target_id, target.target.clone())
                .is_some()
                || !target_roots.insert(target.target.clone())
            {
                return Err(DeclarationTableError::DuplicateInventoryTarget {
                    package: package.package,
                    target: target.target_id,
                });
            }
            let hir_root = hir_targets.get(&target.target_id).ok_or(
                DeclarationTableError::InventoryTargetWithoutHir {
                    package: package.package,
                    target: target.target_id,
                },
            )?;
            if hir_root != &target.target {
                return Err(DeclarationTableError::TargetRootMismatch {
                    package: package.package,
                    target: target.target_id,
                });
            }
        }
        for &target in hir_targets.keys() {
            if !targets.contains_key(&target) {
                return Err(DeclarationTableError::MissingInventoryTarget {
                    package: package.package,
                    target,
                });
            }
        }

        let dependencies = package
            .provenance
            .dependencies
            .iter()
            .map(|dependency| ScopedDependency {
                package: dependency.package,
                kind: dependency.kind,
            })
            .collect();
        package_scopes.insert(
            package.package,
            PackageScope {
                targets,
                dependencies,
            },
        );

        for definition in &package.definitions {
            if !inventory_items.insert(definition.hir_item) {
                return Err(DeclarationTableError::DuplicateInventoryItem(
                    definition.hir_item,
                ));
            }
            let item = hir_items.get(&definition.hir_item).ok_or(
                DeclarationTableError::InventoryDefinitionWithoutHir(definition.hir_item),
            )?;
            validate_definition(package.package, definition, item)?;
            let key_bytes = encode_semantic_definition_key_session(&definition.key)?;
            if let Some(&first) = key_items.get(&key_bytes) {
                return Err(DeclarationTableError::DuplicateSemanticKey {
                    first,
                    second: definition.hir_item,
                });
            }
            key_items.insert(key_bytes.clone(), definition.hir_item);
            planned.push(PlannedDeclaration {
                item: definition.hir_item,
                metadata: DeclarationMetadata {
                    package: item.package,
                    target: item.target,
                    target_root: item.target_root.clone(),
                    module_path: item.module_path.clone(),
                    kind: item.kind,
                    name: item.name.clone(),
                    key_bytes,
                },
            });
        }
    }

    for &package in hir_packages.keys() {
        if !package_scopes.contains_key(&package) {
            return Err(DeclarationTableError::MissingInventoryPackage(package));
        }
    }
    for &item in hir_items.keys() {
        if !inventory_items.contains(&item) {
            return Err(DeclarationTableError::MissingInventoryDefinition(item));
        }
    }
    planned.sort_by(|left, right| left.metadata.key_bytes.cmp(&right.metadata.key_bytes));
    Ok(DeclarationPlan {
        rows: planned,
        packages: package_scopes,
    })
}

fn validate_definition(
    inventory_package: PackageId,
    definition: &SemanticDefinitionInventorySkeleton,
    item: &HirItemAuthority,
) -> Result<(), DeclarationTableError> {
    let id = definition.hir_item;
    if definition.key.module.package != inventory_package
        || definition.key.module.package != item.package
    {
        return Err(DeclarationTableError::DefinitionPackageMismatch(id));
    }
    if definition.key.module.target != item.target_root {
        return Err(DeclarationTableError::DefinitionTargetMismatch(id));
    }
    if definition.key.module.path != item.module_path {
        return Err(DeclarationTableError::DefinitionModuleMismatch(id));
    }
    if definition.key.kind != item.kind {
        return Err(DeclarationTableError::DefinitionKindMismatch(id));
    }
    if definition.key.name != item.name {
        return Err(DeclarationTableError::DefinitionNameMismatch(id));
    }
    if definition.key.span != item.span {
        return Err(DeclarationTableError::DefinitionSpanMismatch(id));
    }
    let inventory_top_level = matches!(
        definition.key.owner_path,
        SymbolicDefinitionOwnerSkeleton::TopLevel
    );
    if inventory_top_level != item.top_level {
        return Err(DeclarationTableError::DefinitionOwnerMismatch(id));
    }
    if definition.declared_visibility != item.declared_visibility {
        return Err(DeclarationTableError::DefinitionVisibilityMismatch(id));
    }
    if !member_visibilities_match(&definition.member_visibilities, &item.member_visibilities) {
        return Err(DeclarationTableError::DefinitionMemberVisibilityMismatch(
            id,
        ));
    }
    Ok(())
}

fn member_visibilities_match(
    left: &[arche_frontend::SemanticMemberVisibility],
    right: &[arche_frontend::SemanticMemberVisibility],
) -> bool {
    let mut left = left
        .iter()
        .map(|member| (&member.path, &member.declared_visibility))
        .collect::<Vec<_>>();
    let mut right = right
        .iter()
        .map(|member| (&member.path, &member.declared_visibility))
        .collect::<Vec<_>>();
    left.sort();
    right.sort();
    left == right
}

fn select_candidate_offsets<R>(
    rows: &[R],
    packages: &BTreeMap<PackageId, PackageScope>,
    package: PackageId,
    target: TargetId,
) -> Result<Vec<u64>, CandidateUniverseError>
where
    R: HasDeclarationMetadata,
{
    let scope = packages
        .get(&package)
        .ok_or(CandidateUniverseError::UnknownPackage(package))?;
    if !scope.targets.contains_key(&target) {
        return Err(CandidateUniverseError::UnknownTarget { package, target });
    }

    let mut reachable = BTreeSet::from([package]);
    let mut pending = vec![package];
    while let Some(current) = pending.pop() {
        let current_scope =
            packages
                .get(&current)
                .ok_or(CandidateUniverseError::MissingNormalDependency {
                    package,
                    dependency: current,
                })?;
        for dependency in &current_scope.dependencies {
            if dependency.kind != DependencyKind::Normal {
                continue;
            }
            if !packages.contains_key(&dependency.package) {
                return Err(CandidateUniverseError::MissingNormalDependency {
                    package: current,
                    dependency: dependency.package,
                });
            }
            if reachable.insert(dependency.package) {
                pending.push(dependency.package);
            }
        }
    }
    reachable.remove(&package);

    let mut selected = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            let row = row.metadata();
            row.kind == DeclarationKind::Impl
                && ((row.package == package && row.target == target)
                    || (reachable.contains(&row.package) && row.target_root == TargetRoot::Library))
        })
        .collect::<Vec<_>>();
    selected.sort_by(|(_, left), (_, right)| {
        left.metadata().key_bytes.cmp(&right.metadata().key_bytes)
    });
    selected
        .into_iter()
        .map(|(offset, _)| {
            u64::try_from(offset).map_err(|_| CandidateUniverseError::CandidateIndexExhausted)
        })
        .collect()
}

trait HasDeclarationMetadata {
    fn metadata(&self) -> &DeclarationMetadata;
}

impl HasDeclarationMetadata for DeclarationRow {
    fn metadata(&self) -> &DeclarationMetadata {
        &self.metadata
    }
}

impl HasDeclarationMetadata for DeclarationMetadata {
    fn metadata(&self) -> &DeclarationMetadata {
        self
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use arche_frontend::{check_workspace_c1, FrontendOutput};
    use arche_package::{load_workspace, resolve, ManifestRequest, RegistrySnapshot};

    use super::*;

    fn corpus_frontend(name: &str) -> FrontendOutput {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../tests/m27c1")
            .join(name);
        let workspace = load_workspace(&ManifestRequest::discover_from(&root)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        check_workspace_c1(&workspace, &graph, &[]).unwrap()
    }

    fn package(byte: u8) -> PackageId {
        PackageId::from_bytes([byte; 16])
    }

    fn metadata(
        package: PackageId,
        target: u64,
        root: TargetRoot,
        module: &[&str],
        kind: DeclarationKind,
        key: &[u8],
    ) -> DeclarationMetadata {
        DeclarationMetadata {
            package,
            target: TargetId(target),
            target_root: root,
            module_path: module.iter().map(|segment| (*segment).to_owned()).collect(),
            kind,
            name: String::new(),
            key_bytes: key.to_vec(),
        }
    }

    fn scope(
        targets: &[(u64, TargetRoot)],
        dependencies: &[(PackageId, DependencyKind)],
    ) -> PackageScope {
        PackageScope {
            targets: targets
                .iter()
                .map(|(id, root)| (TargetId(*id), root.clone()))
                .collect(),
            dependencies: dependencies
                .iter()
                .map(|(package, kind)| ScopedDependency {
                    package: *package,
                    kind: *kind,
                })
                .collect(),
        }
    }

    #[test]
    fn real_c1_authority_builds_sorted_owner_branded_rows() {
        let handoff = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let table = DeclarationTable::build(&handoff).unwrap();
        assert_eq!(table.len(), handoff.indexes().item_count());

        let bytes = table
            .declarations()
            .map(|row| row.session_traversal_bytes())
            .collect::<Vec<_>>();
        assert!(bytes.windows(2).all(|pair| pair[0] < pair[1]));
        let first = table.declarations().next().unwrap();
        let first_item = handoff.indexes().item(first.session_item()).unwrap().id();
        let handle = table.handle_for_session_item(first_item).unwrap();
        assert_eq!(
            table
                .declaration(&handle)
                .unwrap()
                .session_traversal_bytes(),
            first.session_traversal_bytes()
        );

        let other = DeclarationTable::build(&handoff).unwrap();
        assert!(other.declaration(&handle).is_none());
    }

    #[test]
    fn duplicate_missing_and_cross_package_inventory_rows_fail_closed() {
        let frontend = corpus_frontend("language-game");
        let hir = frontend.hir().clone();
        let inventory = frontend.inventory().clone();
        let package_index = inventory
            .packages
            .iter()
            .position(|package| !package.definitions.is_empty())
            .expect("the language corpus has declarations");

        let mut duplicate = inventory.clone();
        let repeated = duplicate.packages[package_index].definitions[0].clone();
        duplicate.packages[package_index]
            .definitions
            .push(repeated.clone());
        assert_eq!(
            plan_declarations(&hir, &duplicate).unwrap_err(),
            DeclarationTableError::DuplicateInventoryItem(repeated.hir_item)
        );

        let mut missing = inventory.clone();
        let removed = missing.packages[package_index].definitions.remove(0);
        assert_eq!(
            plan_declarations(&hir, &missing).unwrap_err(),
            DeclarationTableError::MissingInventoryDefinition(removed.hir_item)
        );

        let mut crossed = inventory;
        let crossed_item = crossed.packages[package_index].definitions[0].hir_item;
        crossed.packages[package_index].definitions[0]
            .key
            .module
            .package = package(0xfe);
        assert_eq!(
            plan_declarations(&hir, &crossed).unwrap_err(),
            DeclarationTableError::DefinitionPackageMismatch(crossed_item)
        );
    }

    #[test]
    fn synthetic_inherited_scope_is_exact_and_reversal_deterministic() {
        let root = package(1);
        let normal = package(2);
        let transitive = package(3);
        let development = package(4);
        let unrelated = package(5);
        let scopes = BTreeMap::from([
            (
                root,
                scope(
                    &[
                        (10, TargetRoot::Binary("main".to_owned())),
                        (11, TargetRoot::Binary("sibling".to_owned())),
                        (12, TargetRoot::Library),
                        (13, TargetRoot::Environment("training".to_owned())),
                    ],
                    &[
                        (normal, DependencyKind::Normal),
                        (development, DependencyKind::Development),
                    ],
                ),
            ),
            (
                normal,
                scope(
                    &[
                        (20, TargetRoot::Library),
                        (21, TargetRoot::Binary("tool".to_owned())),
                    ],
                    &[(transitive, DependencyKind::Normal)],
                ),
            ),
            (transitive, scope(&[(30, TargetRoot::Library)], &[])),
            (development, scope(&[(40, TargetRoot::Library)], &[])),
            (unrelated, scope(&[(50, TargetRoot::Library)], &[])),
        ]);
        let rows = vec![
            metadata(
                root,
                10,
                TargetRoot::Binary("main".to_owned()),
                &["nested"],
                DeclarationKind::Impl,
                b"current-nested",
            ),
            metadata(
                root,
                10,
                TargetRoot::Binary("main".to_owned()),
                &[],
                DeclarationKind::Impl,
                b"current-root",
            ),
            metadata(
                root,
                11,
                TargetRoot::Binary("sibling".to_owned()),
                &[],
                DeclarationKind::Impl,
                b"sibling-bin",
            ),
            metadata(
                root,
                12,
                TargetRoot::Library,
                &[],
                DeclarationKind::Impl,
                b"sibling-library",
            ),
            metadata(
                root,
                13,
                TargetRoot::Environment("training".to_owned()),
                &[],
                DeclarationKind::Impl,
                b"sibling-environment",
            ),
            metadata(
                normal,
                20,
                TargetRoot::Library,
                &[],
                DeclarationKind::Impl,
                b"normal-library",
            ),
            metadata(
                normal,
                21,
                TargetRoot::Binary("tool".to_owned()),
                &[],
                DeclarationKind::Impl,
                b"normal-bin",
            ),
            metadata(
                transitive,
                30,
                TargetRoot::Library,
                &[],
                DeclarationKind::Impl,
                b"transitive-library",
            ),
            metadata(
                development,
                40,
                TargetRoot::Library,
                &[],
                DeclarationKind::Impl,
                b"development-library",
            ),
            metadata(
                unrelated,
                50,
                TargetRoot::Library,
                &[],
                DeclarationKind::Impl,
                b"unrelated-library",
            ),
            metadata(
                root,
                10,
                TargetRoot::Binary("main".to_owned()),
                &[],
                DeclarationKind::Trait,
                b"not-an-impl",
            ),
        ];

        let selected = select_candidate_offsets(&rows, &scopes, root, TargetId(10)).unwrap();
        let selected_keys = selected
            .iter()
            .map(|&offset| rows[usize::try_from(offset).unwrap()].key_bytes.as_slice())
            .collect::<Vec<_>>();
        assert_eq!(
            selected_keys,
            vec![
                b"current-nested".as_slice(),
                b"current-root".as_slice(),
                b"normal-library".as_slice(),
                b"transitive-library".as_slice(),
            ]
        );

        let mut reversed = rows.clone();
        reversed.reverse();
        let reversed_selected =
            select_candidate_offsets(&reversed, &scopes, root, TargetId(10)).unwrap();
        let reversed_keys = reversed_selected
            .iter()
            .map(|&offset| {
                reversed[usize::try_from(offset).unwrap()]
                    .key_bytes
                    .as_slice()
            })
            .collect::<Vec<_>>();
        assert_eq!(reversed_keys, selected_keys);
    }
}
