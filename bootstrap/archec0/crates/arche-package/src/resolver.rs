use crate::diagnostic::{Diagnostic, DiagnosticCode, Diagnostics};
use crate::lock::{
    DependencyRequirement, LockDependency, LockDependencyKind, LockPackage, LockSource, Lockfile,
    RegistryLock, ToolchainLock, WorkspaceLock,
};
use crate::{
    canonical_package_id, Dependency, DependencyAlias, DependencyKind, IntegrityDigest,
    PackageName, PortablePath, Workspace, OFFICIAL_REGISTRY_IDENTITY,
};
use arche_foundation::identity::PackageId;
use semver::{Version, VersionReq};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

const REGISTRY_SNAPSHOT_DOMAIN: &[u8] = b"ARCHE-REGISTRY-SNAPSHOT\0";
const REGISTRY_SNAPSHOT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackageNodeId(u64);

impl PackageNodeId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryDependency {
    pub alias: DependencyAlias,
    pub package: PackageName,
    pub requirement: VersionReq,
    pub kind: LockDependencyKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryRelease {
    pub version: Version,
    pub yanked: bool,
    pub archive_digest: IntegrityDigest,
    pub source_digest: IntegrityDigest,
    pub provenance_record_digest: IntegrityDigest,
    pub inclusion_record_digest: IntegrityDigest,
    pub dependencies: Vec<RegistryDependency>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistrySnapshot {
    identity: String,
    snapshot_digest: IntegrityDigest,
    releases: BTreeMap<PackageName, Vec<RegistryRelease>>,
}

impl RegistrySnapshot {
    pub fn empty() -> Self {
        let releases = BTreeMap::new();
        Self {
            identity: OFFICIAL_REGISTRY_IDENTITY.to_owned(),
            snapshot_digest: registry_snapshot_commitment(&releases)
                .expect("the empty registry snapshot has a canonical commitment"),
            releases,
        }
    }

    #[cfg(test)]
    fn from_test_releases(
        releases: BTreeMap<PackageName, Vec<RegistryRelease>>,
    ) -> Result<Self, Diagnostics> {
        validate_registry_releases(&releases)?;
        Ok(Self {
            identity: OFFICIAL_REGISTRY_IDENTITY.to_owned(),
            snapshot_digest: registry_snapshot_commitment(&releases)?,
            releases,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedSource {
    Workspace {
        relative_path: PortablePath,
    },
    Registry {
        archive_digest: IntegrityDigest,
        source_digest: IntegrityDigest,
        provenance_record_digest: IntegrityDigest,
        inclusion_record_digest: IntegrityDigest,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedPackage {
    pub id: PackageNodeId,
    pub package_id: PackageId,
    pub name: PackageName,
    pub version: Version,
    pub source: ResolvedSource,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ResolvedDependency {
    pub from: PackageNodeId,
    pub alias: DependencyAlias,
    pub to: PackageNodeId,
    pub requirement: DependencyRequirement,
    pub kind: LockDependencyKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedGraph {
    pub packages: Vec<ResolvedPackage>,
    pub roots: Vec<PackageNodeId>,
    pub dependencies: Vec<ResolvedDependency>,
    pub registry_identity: String,
    pub registry_snapshot_digest: IntegrityDigest,
}

impl ResolvedGraph {
    pub fn package(&self, id: PackageNodeId) -> Option<&ResolvedPackage> {
        usize::try_from(id.get())
            .ok()
            .and_then(|index| self.packages.get(index))
    }

    /// Revalidates the complete graph before a downstream authority consumes
    /// it. `ResolvedGraph` is serializable/tool-facing data, so consumers do
    /// not rely on it having been produced by this process's resolver.
    pub fn validate(&self) -> Result<(), Diagnostics> {
        if self.registry_identity != OFFICIAL_REGISTRY_IDENTITY {
            return Err(dependency_error(format!(
                "resolved graph registry must be `{OFFICIAL_REGISTRY_IDENTITY}`"
            )));
        }
        if self.packages.is_empty() {
            return Err(dependency_error("resolved graph has no packages"));
        }
        let mut names = BTreeSet::new();
        let mut ids = BTreeMap::new();
        let mut workspace_paths = BTreeSet::new();
        let mut folded_workspace_paths = BTreeSet::new();
        for (index, package) in self.packages.iter().enumerate() {
            let expected_id = PackageNodeId::new(
                u64::try_from(index)
                    .map_err(|_| dependency_error("resolved package count exceeds u64"))?,
            );
            if package.id != expected_id {
                return Err(dependency_error(format!(
                    "resolved package `{}` has non-dense node ID {} instead of {}",
                    package.name,
                    package.id.get(),
                    expected_id.get()
                )));
            }
            if !names.insert(package.name.clone()) {
                return Err(dependency_error(format!(
                    "resolved graph repeats package `{}`",
                    package.name
                )));
            }
            if package.package_id != canonical_package_id(&package.name) {
                return Err(dependency_error(format!(
                    "resolved package `{}` has the wrong canonical PackageId",
                    package.name
                )));
            }
            if !package.version.build.is_empty() {
                return Err(dependency_error(format!(
                    "resolved package `{}` has forbidden version build metadata",
                    package.name
                )));
            }
            if let ResolvedSource::Workspace { relative_path } = &package.source {
                if !workspace_paths.insert(relative_path.clone())
                    || !folded_workspace_paths.insert(relative_path.casefold_key())
                {
                    return Err(dependency_error(format!(
                        "resolved workspace path `{relative_path}` is duplicated or case-fold/NFC aliased"
                    )));
                }
            }
            ids.insert(package.name.clone(), package.id);
        }
        if self
            .packages
            .windows(2)
            .any(|pair| pair[0].name >= pair[1].name)
        {
            return Err(dependency_error(
                "resolved packages are not in canonical name order",
            ));
        }

        let expected_roots = self
            .packages
            .iter()
            .filter(|package| matches!(&package.source, ResolvedSource::Workspace { .. }))
            .map(|package| package.id)
            .collect::<Vec<_>>();
        if self.roots != expected_roots {
            return Err(dependency_error(
                "resolved roots must be the sorted, unique workspace package IDs",
            ));
        }
        if self.dependencies.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(dependency_error(
                "resolved dependencies are not in canonical order",
            ));
        }
        let mut aliases = BTreeMap::<(PackageNodeId, String), String>::new();
        for dependency in &self.dependencies {
            let from = self.package(dependency.from).ok_or_else(|| {
                dependency_error("resolved dependency source is outside the package graph")
            })?;
            let to = self.package(dependency.to).ok_or_else(|| {
                dependency_error("resolved dependency target is outside the package graph")
            })?;
            let folded = dependency.alias.casefold_key();
            if let Some(previous) = aliases.insert(
                (dependency.from, folded),
                dependency.alias.as_str().to_owned(),
            ) {
                return Err(dependency_error(format!(
                    "resolved package `{}` dependency aliases `{previous}` and `{}` collide under NFC/case folding",
                    from.name, dependency.alias,
                )));
            }
            if matches!(&from.source, ResolvedSource::Registry { .. })
                && matches!(&to.source, ResolvedSource::Workspace { .. })
            {
                return Err(dependency_error(format!(
                    "registry package `{}` cannot depend on workspace package `{}`",
                    from.name, to.name
                )));
            }
            if !dependency.requirement.matches(&to.version) {
                return Err(dependency_error(format!(
                    "resolved package `{}` dependency `{}` requires `{}` but selected `{}` is version `{}`",
                    from.name,
                    dependency.alias,
                    dependency.requirement,
                    to.name,
                    to.version,
                )));
            }
        }
        let package_names = self
            .packages
            .iter()
            .map(|package| package.name.clone())
            .collect::<Vec<_>>();
        reject_cycles(&package_names, &ids, &self.dependencies)?;

        let mut reachable = self.roots.iter().copied().collect::<BTreeSet<_>>();
        loop {
            let before = reachable.len();
            for dependency in &self.dependencies {
                if reachable.contains(&dependency.from) {
                    reachable.insert(dependency.to);
                }
            }
            if reachable.len() == before {
                break;
            }
        }
        if reachable.len() != self.packages.len() {
            let orphan = self
                .packages
                .iter()
                .find(|package| !reachable.contains(&package.id))
                .expect("reachability length mismatch has an orphan");
            return Err(dependency_error(format!(
                "resolved package `{}` is unreachable from workspace roots",
                orphan.name
            )));
        }
        Ok(())
    }

    pub fn to_lockfile(
        &self,
        toolchain: ToolchainLock,
        workspace_source_digest: IntegrityDigest,
        workspace_source_digests: &BTreeMap<PackageName, IntegrityDigest>,
    ) -> Result<Lockfile, Diagnostics> {
        self.validate()?;
        let mut edges = BTreeMap::<PackageNodeId, Vec<LockDependency>>::new();
        for dependency in &self.dependencies {
            let target = self.package(dependency.to).ok_or_else(|| {
                dependency_error("resolved dependency target is outside the package graph")
            })?;
            edges
                .entry(dependency.from)
                .or_default()
                .push(LockDependency {
                    alias: dependency.alias.clone(),
                    package: target.name.clone(),
                    requirement: dependency.requirement.clone(),
                    kind: dependency.kind,
                });
        }
        let packages = self
            .packages
            .iter()
            .map(|package| {
                let source = match &package.source {
                    ResolvedSource::Workspace { relative_path } => {
                        let source_digest = workspace_source_digests
                            .get(&package.name)
                            .copied()
                            .ok_or_else(|| {
                                dependency_error(format!(
                                    "missing source digest for workspace package `{}`",
                                    package.name
                                ))
                            })?;
                        LockSource::Workspace {
                            path: relative_path.clone(),
                            source_digest,
                        }
                    }
                    ResolvedSource::Registry {
                        archive_digest,
                        source_digest,
                        provenance_record_digest,
                        inclusion_record_digest,
                    } => LockSource::Registry {
                        archive_digest: *archive_digest,
                        source_digest: *source_digest,
                        provenance_record_digest: *provenance_record_digest,
                        inclusion_record_digest: *inclusion_record_digest,
                    },
                };
                Ok(LockPackage {
                    name: package.name.clone(),
                    version: package.version.clone(),
                    source,
                    dependencies: edges.remove(&package.id).unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>, Diagnostics>>()?;
        if let Some(source) = edges.keys().next() {
            return Err(dependency_error(format!(
                "resolved dependency source {} is outside the package graph",
                source.get()
            )));
        }
        Lockfile::new(
            toolchain,
            WorkspaceLock {
                source_digest: workspace_source_digest,
            },
            RegistryLock {
                identity: self.registry_identity.clone(),
                snapshot_digest: self.registry_snapshot_digest,
            },
            packages,
        )
    }
}

#[derive(Clone, Debug)]
struct Constraint {
    requirement: VersionReq,
}

#[derive(Clone, Debug)]
struct SelectedRegistry {
    release: RegistryRelease,
}

pub fn resolve(
    workspace: &Workspace,
    snapshot: &RegistrySnapshot,
) -> Result<ResolvedGraph, Diagnostics> {
    validate_snapshot(snapshot)?;
    let workspace_by_name = workspace
        .members
        .iter()
        .map(|member| {
            let package = member
                .manifest
                .package
                .as_ref()
                .expect("workspace member is a package");
            (package.name.clone(), member)
        })
        .collect::<BTreeMap<_, _>>();

    let mut constraints = BTreeMap::<PackageName, Vec<Constraint>>::new();
    for member in &workspace.members {
        for dependency in member
            .manifest
            .dependencies
            .values()
            .chain(member.manifest.dev_dependencies.values())
        {
            if dependency.kind == DependencyKind::Registry {
                let package = dependency
                    .package
                    .as_ref()
                    .expect("registry dependency has package");
                if workspace_by_name.contains_key(package) {
                    return Err(dependency_error(format!(
                        "package `{package}` is both a workspace and registry source"
                    )));
                }
                constraints
                    .entry(package.clone())
                    .or_default()
                    .push(Constraint {
                        requirement: dependency
                            .requirement
                            .clone()
                            .expect("registry dependency has requirement"),
                    });
            }
        }
    }

    let first_requirement = constraints.iter().next().map(|(name, values)| {
        let requirements = values
            .iter()
            .map(|constraint| constraint.requirement.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!("`{name}` ({requirements})")
    });
    let selected = solve(snapshot, &workspace_by_name, constraints, BTreeMap::new()).ok_or_else(|| {
        dependency_error(match first_requirement {
            Some(requirement) => format!(
                "no single-version solution satisfies package {requirement} and the complete dependency graph"
            ),
            None => "no single-version solution satisfies the complete dependency graph".to_owned(),
        })
    })?;

    let mut package_names = workspace_by_name.keys().cloned().collect::<Vec<_>>();
    package_names.extend(selected.keys().cloned());
    package_names.sort();
    package_names.dedup();
    let ids = package_names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            (
                name.clone(),
                PackageNodeId::new(u64::try_from(index).expect("package graph index fits u64")),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut packages = Vec::with_capacity(package_names.len());
    for name in &package_names {
        let id = ids[name];
        if let Some(member) = workspace_by_name.get(name) {
            let package = member
                .manifest
                .package
                .as_ref()
                .expect("workspace member has package");
            packages.push(ResolvedPackage {
                id,
                package_id: canonical_package_id(name),
                name: name.clone(),
                version: package.version.clone(),
                source: ResolvedSource::Workspace {
                    relative_path: member.relative_path.clone(),
                },
            });
        } else {
            let release = &selected[name].release;
            packages.push(ResolvedPackage {
                id,
                package_id: canonical_package_id(name),
                name: name.clone(),
                version: release.version.clone(),
                source: ResolvedSource::Registry {
                    archive_digest: release.archive_digest,
                    source_digest: release.source_digest,
                    provenance_record_digest: release.provenance_record_digest,
                    inclusion_record_digest: release.inclusion_record_digest,
                },
            });
        }
    }

    let mut dependencies = Vec::new();
    for member in &workspace.members {
        let from_name = &member
            .manifest
            .package
            .as_ref()
            .expect("member has package")
            .name;
        let from = ids[from_name];
        add_manifest_edges(member, from, &ids, &workspace_by_name, &mut dependencies)?;
    }
    for (name, selected) in &selected {
        let from = ids[name];
        for dependency in &selected.release.dependencies {
            dependencies.push(ResolvedDependency {
                from,
                alias: dependency.alias.clone(),
                to: *ids.get(&dependency.package).ok_or_else(|| {
                    dependency_error(format!(
                        "registry package `{name}` resolved an absent dependency `{}`",
                        dependency.package
                    ))
                })?,
                requirement: DependencyRequirement::from_version_req(&dependency.requirement),
                kind: dependency.kind,
            });
        }
    }
    dependencies.sort();
    reject_cycles(&package_names, &ids, &dependencies)?;

    let mut roots = workspace_by_name
        .keys()
        .map(|name| ids[name])
        .collect::<Vec<_>>();
    roots.sort();
    let graph = ResolvedGraph {
        packages,
        roots,
        dependencies,
        registry_identity: snapshot.identity.clone(),
        registry_snapshot_digest: snapshot.snapshot_digest,
    };
    graph.validate()?;
    Ok(graph)
}

fn solve(
    snapshot: &RegistrySnapshot,
    workspace: &BTreeMap<PackageName, &crate::WorkspaceMember>,
    constraints: BTreeMap<PackageName, Vec<Constraint>>,
    selected: BTreeMap<PackageName, SelectedRegistry>,
) -> Option<BTreeMap<PackageName, SelectedRegistry>> {
    for (name, assignment) in &selected {
        if constraints.get(name).is_some_and(|values| {
            values
                .iter()
                .any(|constraint| !constraint.requirement.matches(&assignment.release.version))
        }) {
            return None;
        }
    }
    let next = constraints
        .keys()
        .find(|name| !selected.contains_key(*name))
        .cloned();
    let Some(name) = next else {
        return Some(selected);
    };
    if workspace.contains_key(&name) {
        return None;
    }
    let requirements = &constraints[&name];
    let mut candidates = snapshot.releases.get(&name)?.clone();
    candidates.sort_by(|left, right| right.version.cmp(&left.version));
    let mut best = None;
    for release in candidates {
        if release.yanked
            || requirements
                .iter()
                .any(|constraint| !constraint.requirement.matches(&release.version))
        {
            continue;
        }
        let mut next_constraints = constraints.clone();
        let mut invalid = false;
        for dependency in &release.dependencies {
            if workspace.contains_key(&dependency.package) {
                invalid = true;
                break;
            }
            next_constraints
                .entry(dependency.package.clone())
                .or_default()
                .push(Constraint {
                    requirement: dependency.requirement.clone(),
                });
        }
        if invalid {
            continue;
        }
        let mut next_selected = selected.clone();
        next_selected.insert(name.clone(), SelectedRegistry { release });
        if let Some(solution) = solve(snapshot, workspace, next_constraints, next_selected) {
            if best
                .as_ref()
                .is_none_or(|current| solution_is_better(&solution, current))
            {
                best = Some(solution);
            }
        }
    }
    best
}

fn solution_is_better(
    candidate: &BTreeMap<PackageName, SelectedRegistry>,
    current: &BTreeMap<PackageName, SelectedRegistry>,
) -> bool {
    let names = candidate
        .keys()
        .chain(current.keys())
        .collect::<BTreeSet<_>>();
    for name in names {
        match (candidate.get(name), current.get(name)) {
            (Some(left), Some(right)) => match left.release.version.cmp(&right.release.version) {
                std::cmp::Ordering::Greater => return true,
                std::cmp::Ordering::Less => return false,
                std::cmp::Ordering::Equal => {}
            },
            (Some(_), None) => return true,
            (None, Some(_)) => return false,
            (None, None) => unreachable!("name came from one complete solution"),
        }
    }
    false
}

fn add_manifest_edges(
    member: &crate::WorkspaceMember,
    from: PackageNodeId,
    ids: &BTreeMap<PackageName, PackageNodeId>,
    workspace_by_name: &BTreeMap<PackageName, &crate::WorkspaceMember>,
    output: &mut Vec<ResolvedDependency>,
) -> Result<(), Diagnostics> {
    for (dependency, kind) in member
        .manifest
        .dependencies
        .values()
        .map(|dependency| (dependency, LockDependencyKind::Normal))
        .chain(
            member
                .manifest
                .dev_dependencies
                .values()
                .map(|dependency| (dependency, LockDependencyKind::Development)),
        )
    {
        let target_name = dependency_target(member, dependency, workspace_by_name)?;
        let to = ids.get(&target_name).copied().ok_or_else(|| {
            dependency_error(format!("dependency `{}` did not resolve", dependency.alias))
        })?;
        let requirement = match &dependency.requirement {
            Some(requirement) => DependencyRequirement::from_version_req(requirement),
            None => {
                let target_version = workspace_by_name
                    .get(&target_name)
                    .and_then(|target| target.manifest.package.as_ref())
                    .map(|package| &package.version)
                    .ok_or_else(|| {
                        dependency_error(format!(
                            "path dependency `{}` resolved to a package without a version",
                            dependency.alias
                        ))
                    })?;
                DependencyRequirement::exact(target_version)
            }
        };
        output.push(ResolvedDependency {
            from,
            alias: dependency.alias.clone(),
            to,
            requirement,
            kind,
        });
    }
    Ok(())
}

fn dependency_target(
    member: &crate::WorkspaceMember,
    dependency: &Dependency,
    workspace_by_name: &BTreeMap<PackageName, &crate::WorkspaceMember>,
) -> Result<PackageName, Diagnostics> {
    if dependency.kind == DependencyKind::Registry {
        return Ok(dependency
            .package
            .clone()
            .expect("registry dependency has package"));
    }
    let portable = dependency.path.as_ref().expect("path dependency has path");
    let target_path = portable
        .segments()
        .fold(member.directory.clone(), |base, segment| base.join(segment));
    let target_canonical = fs::canonicalize(&target_path).map_err(|error| {
        Diagnostic::new(
            DiagnosticCode::WorkspacePath,
            format!(
                "could not resolve dependency path `{}`: {error}",
                target_path.display()
            ),
        )
        .at_path(&target_path)
    })?;
    let target = workspace_by_name
        .values()
        .find(|candidate| candidate.directory == target_canonical)
        .ok_or_else(|| {
            dependency_error(format!(
                "path dependency `{}` does not target a declared workspace member",
                dependency.alias
            ))
        })?;
    let package = target
        .manifest
        .package
        .as_ref()
        .expect("workspace member has package");
    if let Some(expected) = &dependency.package {
        if expected != &package.name {
            return Err(dependency_error(format!(
                "path dependency `{}` names `{expected}` but targets `{}`",
                dependency.alias, package.name
            )));
        }
    }
    if let Some(requirement) = &dependency.requirement {
        if !requirement.matches(&package.version) {
            return Err(dependency_error(format!(
                "path dependency `{}` requires `{requirement}` but targets version `{}`",
                dependency.alias, package.version
            )));
        }
    }
    Ok(package.name.clone())
}

fn validate_snapshot(snapshot: &RegistrySnapshot) -> Result<(), Diagnostics> {
    if snapshot.identity != OFFICIAL_REGISTRY_IDENTITY {
        return Err(Diagnostic::new(
            DiagnosticCode::RegistryInvalid,
            format!("registry identity must be `{OFFICIAL_REGISTRY_IDENTITY}`"),
        )
        .into());
    }
    validate_registry_releases(&snapshot.releases)?;
    let expected = registry_snapshot_commitment(&snapshot.releases)?;
    if snapshot.snapshot_digest != expected {
        return Err(Diagnostic::new(
            DiagnosticCode::RegistryInvalid,
            "registry snapshot contents do not match their validated commitment",
        )
        .into());
    }
    Ok(())
}

fn validate_registry_releases(
    release_sets: &BTreeMap<PackageName, Vec<RegistryRelease>>,
) -> Result<(), Diagnostics> {
    for (name, releases) in release_sets {
        let mut versions = BTreeSet::new();
        for release in releases {
            if !release.version.build.is_empty() || !versions.insert(release.version.clone()) {
                return Err(Diagnostic::new(
                    DiagnosticCode::RegistryInvalid,
                    format!(
                        "registry metadata for `{name}` has a duplicate or noncanonical version"
                    ),
                )
                .into());
            }
            let mut aliases = BTreeMap::<String, String>::new();
            for dependency in &release.dependencies {
                let folded = dependency.alias.casefold_key();
                if let Some(previous) = aliases.insert(folded, dependency.alias.as_str().to_owned())
                {
                    return Err(Diagnostic::new(
                        DiagnosticCode::RegistryInvalid,
                        format!(
                            "registry release `{name} {}` dependency aliases `{previous}` and `{}` collide under NFC/case folding",
                            release.version, dependency.alias,
                        ),
                    )
                    .into());
                }
            }
        }
    }
    Ok(())
}

// This is an internal M27-B integrity commitment over the injected logical
// snapshot, not a public registry wire format. M27-J's verified decoder owns
// the production index serialization contract.
fn registry_snapshot_commitment(
    release_sets: &BTreeMap<PackageName, Vec<RegistryRelease>>,
) -> Result<IntegrityDigest, Diagnostics> {
    let mut hasher = Sha256::new();
    hasher.update(REGISTRY_SNAPSHOT_DOMAIN);
    hasher.update(REGISTRY_SNAPSHOT_VERSION.to_le_bytes());
    update_registry_count(&mut hasher, release_sets.len(), "registry package count")?;
    for (name, releases) in release_sets {
        update_registry_text(&mut hasher, name.as_str(), "registry package name")?;
        update_registry_count(&mut hasher, releases.len(), "registry release count")?;
        let mut canonical_releases = releases.iter().collect::<Vec<_>>();
        canonical_releases.sort_by(|left, right| left.version.cmp(&right.version));
        for release in canonical_releases {
            update_registry_text(
                &mut hasher,
                &release.version.to_string(),
                "registry release version",
            )?;
            hasher.update([u8::from(release.yanked)]);
            hasher.update(release.archive_digest.as_bytes());
            hasher.update(release.source_digest.as_bytes());
            hasher.update(release.provenance_record_digest.as_bytes());
            hasher.update(release.inclusion_record_digest.as_bytes());
            update_registry_count(
                &mut hasher,
                release.dependencies.len(),
                "registry dependency count",
            )?;
            let mut dependencies = release
                .dependencies
                .iter()
                .map(|dependency| (dependency, dependency.requirement.to_string()))
                .collect::<Vec<_>>();
            dependencies.sort_by(|(left, left_requirement), (right, right_requirement)| {
                left.alias
                    .cmp(&right.alias)
                    .then_with(|| left.package.cmp(&right.package))
                    .then_with(|| left_requirement.cmp(right_requirement))
                    .then_with(|| left.kind.cmp(&right.kind))
            });
            for (dependency, requirement) in dependencies {
                update_registry_text(
                    &mut hasher,
                    dependency.alias.as_str(),
                    "registry dependency alias",
                )?;
                update_registry_text(
                    &mut hasher,
                    dependency.package.as_str(),
                    "registry dependency package",
                )?;
                update_registry_text(&mut hasher, &requirement, "registry dependency requirement")?;
                hasher.update([match dependency.kind {
                    LockDependencyKind::Normal => 1,
                    LockDependencyKind::Development => 2,
                }]);
            }
        }
    }
    let mut digest = [0_u8; 32];
    digest.copy_from_slice(&hasher.finalize());
    Ok(IntegrityDigest::from_bytes(digest))
}

fn update_registry_count(
    hasher: &mut Sha256,
    value: usize,
    context: &str,
) -> Result<(), Diagnostics> {
    let value = u64::try_from(value).map_err(|_| {
        Diagnostic::new(
            DiagnosticCode::RegistryInvalid,
            format!("{context} exceeds u64"),
        )
    })?;
    hasher.update(value.to_le_bytes());
    Ok(())
}

fn update_registry_text(
    hasher: &mut Sha256,
    value: &str,
    context: &str,
) -> Result<(), Diagnostics> {
    update_registry_count(hasher, value.len(), context)?;
    hasher.update(value.as_bytes());
    Ok(())
}

fn reject_cycles(
    names: &[PackageName],
    ids: &BTreeMap<PackageName, PackageNodeId>,
    dependencies: &[ResolvedDependency],
) -> Result<(), Diagnostics> {
    let mut adjacency = BTreeMap::<PackageNodeId, Vec<PackageNodeId>>::new();
    for dependency in dependencies {
        adjacency
            .entry(dependency.from)
            .or_default()
            .push(dependency.to);
    }
    let reverse = ids
        .iter()
        .map(|(name, id)| (*id, name))
        .collect::<BTreeMap<_, _>>();
    let mut complete = BTreeSet::new();
    for name in names {
        let id = ids[name];
        let mut active = Vec::new();
        if let Some(cycle) = find_cycle(id, &adjacency, &mut active, &mut complete) {
            let text = cycle
                .iter()
                .map(|id| reverse[id].as_str())
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(Diagnostic::new(
                DiagnosticCode::DependencyCycle,
                format!("package dependency cycle: {text}"),
            )
            .into());
        }
    }
    Ok(())
}

fn find_cycle(
    node: PackageNodeId,
    adjacency: &BTreeMap<PackageNodeId, Vec<PackageNodeId>>,
    active: &mut Vec<PackageNodeId>,
    complete: &mut BTreeSet<PackageNodeId>,
) -> Option<Vec<PackageNodeId>> {
    if let Some(index) = active.iter().position(|candidate| *candidate == node) {
        let mut cycle = active[index..].to_vec();
        cycle.push(node);
        return Some(cycle);
    }
    if complete.contains(&node) {
        return None;
    }
    active.push(node);
    if let Some(children) = adjacency.get(&node) {
        for child in children {
            if let Some(cycle) = find_cycle(*child, adjacency, active, complete) {
                return Some(cycle);
            }
        }
    }
    active.pop();
    complete.insert(node);
    None
}

fn dependency_error(message: impl Into<String>) -> Diagnostics {
    Diagnostic::new(DiagnosticCode::DependencyConflict, message).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ManifestRequest, Workspace};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn selects_highest_compatible_non_yanked_release_independent_of_input_order() {
        let (root, workspace) = workspace_with_dependencies(
            "math = { package = \"arche/math\", version = \">=1.0.0, <3.0.0\" }",
        );
        let package: PackageName = "arche/math".parse().unwrap();
        let releases = vec![
            release("1.0.0", false, vec![]),
            release("3.0.0", false, vec![]),
            release("2.0.0", false, vec![]),
            release("2.5.0", true, vec![]),
        ];
        let first = snapshot([(package.clone(), releases.clone())]);
        let second = snapshot([(package, releases.into_iter().rev().collect())]);
        let left = resolve(&workspace, &first).unwrap();
        let right = resolve(&workspace, &second).unwrap();
        assert_eq!(left, right);
        assert_eq!(
            left.packages
                .iter()
                .find(|package| package.name.as_str() == "arche/math")
                .unwrap()
                .version,
            Version::new(2, 0, 0)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backtracks_when_the_highest_local_choice_breaks_a_later_constraint() {
        let (root, workspace) = workspace_with_dependencies(concat!(
            "a = { package = \"example/a\", version = \"^1.0.0\" }\n",
            "b = { package = \"example/b\", version = \"^1.0.0\" }",
        ));
        let shared_one = registry_dependency("shared", "example/shared", "^1.0.0");
        let shared_two = registry_dependency("shared", "example/shared", "^2.0.0");
        let snapshot = snapshot([
            (
                "example/a".parse().unwrap(),
                vec![
                    release("1.1.0", false, vec![shared_two]),
                    release("1.0.0", false, vec![shared_one.clone()]),
                ],
            ),
            (
                "example/b".parse().unwrap(),
                vec![release("1.0.0", false, vec![shared_one])],
            ),
            (
                "example/shared".parse().unwrap(),
                vec![
                    release("2.0.0", false, vec![]),
                    release("1.0.0", false, vec![]),
                ],
            ),
        ]);
        let graph = resolve(&workspace, &snapshot).unwrap();
        assert_eq!(
            graph
                .packages
                .iter()
                .find(|package| package.name.as_str() == "example/a")
                .unwrap()
                .version,
            Version::new(1, 0, 0)
        );
        assert_eq!(
            graph
                .packages
                .iter()
                .find(|package| package.name.as_str() == "example/shared")
                .unwrap()
                .version,
            Version::new(1, 0, 0)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn complete_solution_maximizes_versions_in_package_name_order() {
        let (root, workspace) =
            workspace_with_dependencies("z = { package = \"example/z\", version = \"^1.0.0\" }");
        let snapshot = snapshot([
            (
                "example/z".parse().unwrap(),
                vec![
                    release(
                        "1.1.0",
                        false,
                        vec![registry_dependency("a", "example/a", "=1.0.0")],
                    ),
                    release(
                        "1.0.0",
                        false,
                        vec![registry_dependency("a", "example/a", "=2.0.0")],
                    ),
                ],
            ),
            (
                "example/a".parse().unwrap(),
                vec![
                    release("2.0.0", false, vec![]),
                    release("1.0.0", false, vec![]),
                ],
            ),
        ]);

        let graph = resolve(&workspace, &snapshot).unwrap();
        assert_eq!(
            graph
                .packages
                .iter()
                .find(|package| package.name.as_str() == "example/a")
                .unwrap()
                .version,
            Version::new(2, 0, 0)
        );
        assert_eq!(
            graph
                .packages
                .iter()
                .find(|package| package.name.as_str() == "example/z")
                .unwrap()
                .version,
            Version::new(1, 0, 0)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_registry_cycles_and_names_the_first_unsatisfied_package() {
        let (root, workspace) =
            workspace_with_dependencies("a = { package = \"example/a\", version = \"^1.0.0\" }");
        let cyclic = snapshot([
            (
                "example/a".parse().unwrap(),
                vec![release(
                    "1.0.0",
                    false,
                    vec![registry_dependency("b", "example/b", "^1.0.0")],
                )],
            ),
            (
                "example/b".parse().unwrap(),
                vec![release(
                    "1.0.0",
                    false,
                    vec![registry_dependency("a", "example/a", "^1.0.0")],
                )],
            ),
        ]);
        let error = resolve(&workspace, &cyclic).unwrap_err();
        assert!(error.to_string().contains("cycle"));

        let missing = resolve(&workspace, &RegistrySnapshot::empty()).unwrap_err();
        assert!(missing.to_string().contains("example/a"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolved_graph_revalidates_publicly_representable_structure() {
        let (root, workspace) =
            workspace_with_dependencies("a = { package = \"example/a\", version = \"^1.0.0\" }");
        let snapshot = snapshot([(
            "example/a".parse().unwrap(),
            vec![release("1.0.0", false, vec![])],
        )]);
        let graph = resolve(&workspace, &snapshot).unwrap();
        graph.validate().unwrap();

        let mut non_dense = graph.clone();
        non_dense.packages[0].id = PackageNodeId::new(99);
        assert!(non_dense.validate().is_err());

        let mut orphan = graph.clone();
        orphan.dependencies.clear();
        assert!(orphan.validate().is_err());

        let mut cyclic = graph.clone();
        let root_id = *cyclic.roots.first().unwrap();
        cyclic.dependencies.push(ResolvedDependency {
            from: root_id,
            alias: crate::SourceIdentifier::new("self_dep").unwrap(),
            to: root_id,
            requirement: DependencyRequirement::any(),
            kind: LockDependencyKind::Normal,
        });
        cyclic.dependencies.sort();
        assert!(cyclic.validate().is_err());

        let target_id = graph
            .packages
            .iter()
            .find(|package| package.name.as_str() == "example/a")
            .unwrap()
            .id;
        let mut stale_selection = graph.clone();
        stale_selection.packages[usize::try_from(target_id.get()).unwrap()].version =
            Version::new(2, 0, 0);
        assert!(stale_selection.validate().is_err());

        let mut folded_alias = graph.clone();
        folded_alias.dependencies.push(ResolvedDependency {
            from: root_id,
            alias: crate::SourceIdentifier::new("A").unwrap(),
            to: target_id,
            requirement: DependencyRequirement::any(),
            kind: LockDependencyKind::Normal,
        });
        folded_alias.dependencies.sort();
        assert!(folded_alias.validate().is_err());

        let mut invalid_source = graph.clone();
        invalid_source.dependencies.push(ResolvedDependency {
            from: PackageNodeId::new(99),
            alias: crate::SourceIdentifier::new("outside").unwrap(),
            to: target_id,
            requirement: DependencyRequirement::any(),
            kind: LockDependencyKind::Normal,
        });
        invalid_source.dependencies.sort();
        let source_digests = BTreeMap::from([(
            "example/root".parse().unwrap(),
            IntegrityDigest::of_bytes(b"root source"),
        )]);
        let error = invalid_source
            .to_lockfile(
                ToolchainLock::bootstrap_current(),
                IntegrityDigest::of_bytes(b"workspace"),
                &source_digests,
            )
            .unwrap_err();
        assert!(error.to_string().contains("source is outside"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registry_snapshot_commitment_and_aliases_fail_closed() {
        let (root, workspace) =
            workspace_with_dependencies("a = { package = \"example/a\", version = \"^1.0.0\" }");
        let package: PackageName = "example/a".parse().unwrap();
        let snapshot = snapshot([(package.clone(), vec![release("1.0.0", false, vec![])])]);
        validate_snapshot(&snapshot).unwrap();

        let mut mutated = snapshot.clone();
        mutated
            .releases
            .get_mut(&package)
            .unwrap()
            .push(release("1.1.0", false, vec![]));
        assert!(resolve(&workspace, &mutated).is_err());

        let aliases = BTreeMap::from([(
            package,
            vec![release(
                "1.0.0",
                false,
                vec![
                    registry_dependency("Foo", "example/left", "^1.0.0"),
                    registry_dependency("foo", "example/right", "^1.0.0"),
                ],
            )],
        )]);
        assert!(RegistrySnapshot::from_test_releases(aliases).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    fn workspace_with_dependencies(dependencies: &str) -> (PathBuf, Workspace) {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("arche-resolver-{}-{id}", std::process::id()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.arc"), "pub component Marker {}\n").unwrap();
        fs::write(
            root.join("Arche.toml"),
            format!(
                concat!(
                    "schema = 1\n\n",
                    "[package]\n",
                    "name = \"example/root\"\n",
                    "version = \"0.1.0\"\n",
                    "edition = \"2026\"\n",
                    "arche = \">=0.0.0\"\n",
                    "publish = false\n\n",
                    "[lib]\n",
                    "path = \"src/lib.arc\"\n\n",
                    "[dependencies]\n",
                    "{}\n",
                ),
                dependencies
            ),
        )
        .unwrap();
        let workspace = crate::load_workspace(&ManifestRequest::discover_from(&root)).unwrap();
        (root, workspace)
    }

    fn registry_dependency(alias: &str, package: &str, requirement: &str) -> RegistryDependency {
        RegistryDependency {
            alias: crate::SourceIdentifier::new(alias).unwrap(),
            package: package.parse().unwrap(),
            requirement: VersionReq::parse(requirement).unwrap(),
            kind: LockDependencyKind::Normal,
        }
    }

    fn snapshot(
        releases: impl IntoIterator<Item = (PackageName, Vec<RegistryRelease>)>,
    ) -> RegistrySnapshot {
        RegistrySnapshot::from_test_releases(releases.into_iter().collect()).unwrap()
    }

    fn release(
        version: &str,
        yanked: bool,
        dependencies: Vec<RegistryDependency>,
    ) -> RegistryRelease {
        let digest = IntegrityDigest::of_bytes(version.as_bytes());
        RegistryRelease {
            version: Version::parse(version).unwrap(),
            yanked,
            archive_digest: digest,
            source_digest: digest,
            provenance_record_digest: digest,
            inclusion_record_digest: digest,
            dependencies,
        }
    }
}
