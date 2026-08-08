use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use arche_package::{
    LockDependencyKind, Manifest, PackageName, PackageNodeId, ResolvedGraph, ResolvedSource,
    Target, Workspace, WorkspaceMember,
};

use crate::hir::DependencyExport;
use crate::modules::{check_target_with_dependencies, package_export_surface};
use crate::{
    check_target, CheckTargetRequest, Diagnostic, EnvironmentSchedulePaths, FrontendError,
    FrontendErrorCode, Label, ResolvedTargetHir, ResolvedWorkspaceHir, TargetId, TargetKind,
};

/// Checks one target in an already identified package-graph node. This entry
/// point is useful for focused tooling; callers checking a workspace with
/// dependencies should use [`check_workspace`] so dependency exports are
/// retained and linked rather than represented by aliases alone.
pub fn check_manifest_target(
    package_root: &Path,
    manifest: &Manifest,
    target: &Target,
    package: PackageNodeId,
    target_id: TargetId,
) -> Result<ResolvedTargetHir, FrontendError> {
    let request = target_request(package_root, manifest, target, package, target_id)?;
    check_target(request)
}

/// Checks every source target in a resolved workspace graph. Workspace
/// libraries are checked dependency-first, while IDs and returned targets stay
/// in deterministic package-node/manifest-target order.
pub fn check_workspace(
    workspace: &Workspace,
    graph: &ResolvedGraph,
) -> Result<ResolvedWorkspaceHir, FrontendError> {
    graph.validate().map_err(|diagnostics| {
        target_error(
            "TARGET012",
            format!("resolved package graph is invalid: {diagnostics}"),
        )
    })?;
    let mut members = BTreeMap::<PackageNodeId, &WorkspaceMember>::new();
    let mut member_names = BTreeSet::new();
    for member in &workspace.members {
        let package = member
            .manifest
            .package
            .as_ref()
            .expect("workspace members are validated packages");
        let matches = graph
            .packages
            .iter()
            .filter(|resolved| resolved.name == package.name)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(target_error(
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
            return Err(target_error(
                "TARGET012",
                format!(
                    "workspace package `{}` is not a workspace source in the resolved graph",
                    package.name
                ),
            ));
        };
        if relative_path != &member.relative_path || resolved.version != package.version {
            return Err(target_error(
                "TARGET012",
                format!(
                    "resolved graph identity for workspace package `{}` does not match its loaded manifest",
                    package.name
                ),
            ));
        }
        if members.insert(resolved.id, member).is_some()
            || !member_names.insert(package.name.clone())
        {
            return Err(target_error(
                "TARGET012",
                "resolved workspace package nodes are not unique",
            ));
        }
    }

    for dependency in &graph.dependencies {
        if graph
            .packages
            .iter()
            .all(|package| package.id != dependency.from)
            || graph
                .packages
                .iter()
                .all(|package| package.id != dependency.to)
        {
            return Err(target_error(
                "TARGET012",
                "resolved dependency edge references an absent package node",
            ));
        }
    }

    let mut ordered_nodes = members.keys().copied().collect::<Vec<_>>();
    ordered_nodes.sort();
    let mut target_ids = BTreeMap::<PackageNodeId, Vec<TargetId>>::new();
    let mut next_target = 0_u64;
    for node in &ordered_nodes {
        let count = members[node].manifest.targets().count();
        let mut ids = Vec::with_capacity(count);
        for _ in 0..count {
            ids.push(TargetId(next_target));
            next_target = next_target
                .checked_add(1)
                .ok_or_else(|| target_error("TARGET013", "workspace target count exceeds u64"))?;
        }
        target_ids.insert(*node, ids);
    }

    let mut pending = ordered_nodes.iter().copied().collect::<BTreeSet<_>>();
    let mut exports = BTreeMap::new();
    let mut checked = Vec::new();
    while !pending.is_empty() {
        let ready = pending.iter().copied().find(|node| {
            graph
                .dependencies
                .iter()
                .filter(|dependency| dependency.from == *node)
                .filter(|dependency| members.contains_key(&dependency.to))
                .all(|dependency| !pending.contains(&dependency.to))
        });
        let node = ready.ok_or_else(|| {
            target_error(
                "TARGET014",
                "workspace dependency graph is cyclic during frontend linking",
            )
        })?;
        let member = members[&node];
        let dependencies = dependency_exports(node, member, graph, &members, &exports)?;
        for (target, target_id) in member
            .manifest
            .targets()
            .zip(target_ids[&node].iter().copied())
        {
            let request = target_request(
                &member.directory,
                &member.manifest,
                &target,
                node,
                target_id,
            )?;
            let hir = check_target_with_dependencies(request, dependencies.clone())?;
            if matches!(target, Target::Library(_)) {
                exports.insert(node, package_export_surface(&hir, &dependencies));
            }
            checked.push(hir);
        }
        pending.remove(&node);
    }

    Ok(ResolvedWorkspaceHir::new(checked))
}

/// Focused target adapter that validates the target/member pairing. It does
/// not synthesize dependency exports; graph-aware callers use
/// [`check_workspace`].
pub fn check_workspace_target(
    workspace: &Workspace,
    package: &PackageName,
    target: &Target,
    package_node: PackageNodeId,
    target_id: TargetId,
) -> Result<ResolvedTargetHir, FrontendError> {
    let member = workspace
        .members
        .iter()
        .find(|member| {
            member
                .manifest
                .package
                .as_ref()
                .is_some_and(|candidate| &candidate.name == package)
        })
        .ok_or_else(|| {
            target_error(
                "TARGET010",
                format!("workspace does not contain package `{package}`"),
            )
        })?;
    check_manifest_target(
        &member.directory,
        &member.manifest,
        target,
        package_node,
        target_id,
    )
}

fn dependency_exports(
    node: PackageNodeId,
    member: &WorkspaceMember,
    graph: &ResolvedGraph,
    workspace_members: &BTreeMap<PackageNodeId, &WorkspaceMember>,
    surfaces: &BTreeMap<PackageNodeId, crate::hir::PackageExportSurface>,
) -> Result<Vec<DependencyExport>, FrontendError> {
    let mut output = Vec::new();
    for alias in member.manifest.dependencies.keys() {
        let matches = graph
            .dependencies
            .iter()
            .filter(|dependency| {
                dependency.from == node
                    && dependency.kind == LockDependencyKind::Normal
                    && dependency.alias == *alias
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(target_error(
                "TARGET012",
                format!(
                    "resolved graph contains {} normal edges for dependency alias `{alias}`",
                    matches.len()
                ),
            ));
        }
        let target = matches[0].to;
        let surface = if workspace_members.contains_key(&target) {
            let surface = surfaces.get(&target).cloned().ok_or_else(|| {
                target_error(
                    "NAME008",
                    format!(
                        "workspace dependency `{alias}` has no checked library target to export"
                    ),
                )
            })?;
            if surface.package != target {
                return Err(target_error(
                    "TARGET012",
                    format!("dependency export surface for `{alias}` has the wrong package node"),
                ));
            }
            Some(surface)
        } else {
            return Err(target_error(
                "NAME008",
                format!(
                    "registry dependency `{alias}` requires the later package-cache/object export adapter"
                ),
            ));
        };
        output.push(DependencyExport {
            alias: alias.as_str().to_owned(),
            surface,
        });
    }
    Ok(output)
}

fn target_request(
    package_root: &Path,
    manifest: &Manifest,
    target: &Target,
    package: PackageNodeId,
    target_id: TargetId,
) -> Result<CheckTargetRequest, FrontendError> {
    if !manifest.targets().any(|candidate| candidate == *target) {
        return Err(target_error(
            "TARGET011",
            "selected target does not belong to the supplied package manifest",
        ));
    }
    let (kind, target_name, root_world, environment_schedules) = match target {
        Target::Library(_) => (TargetKind::Library, "lib".to_owned(), None, None),
        Target::Binary(binary) => (
            TargetKind::Binary,
            binary.name.as_str().to_owned(),
            Some(binary.world.canonical()),
            None,
        ),
        Target::Environment(environment) => {
            let profile = manifest
                .environment_profiles
                .get(&environment.profile)
                .ok_or_else(|| {
                    target_error(
                        "TARGET006",
                        format!(
                            "environment target `{}` names missing profile `{}`",
                            environment.name, environment.profile
                        ),
                    )
                })?;
            (
                TargetKind::Environment,
                environment.name.as_str().to_owned(),
                Some(environment.world.canonical()),
                Some(EnvironmentSchedulePaths {
                    reset: profile.reset.canonical(),
                    step: profile.step.canonical(),
                    self_play: profile.self_play.canonical(),
                }),
            )
        }
    };
    Ok(CheckTargetRequest {
        package_root: package_root.to_path_buf(),
        package,
        target_id,
        target_name,
        kind,
        source_root: target.path().as_str().into(),
        root_world,
        environment_schedules,
        dependency_aliases: manifest
            .dependencies
            .keys()
            .map(|alias| alias.as_str().to_owned())
            .collect(),
    })
}

fn target_error(code: &'static str, message: impl Into<String>) -> FrontendError {
    let message = message.into();
    FrontendError {
        kind: if code.starts_with("NAME") {
            FrontendErrorCode::Name
        } else {
            FrontendErrorCode::Target
        },
        diagnostic: Box::new(Diagnostic {
            code,
            message: message.clone(),
            primary: Box::new(Label {
                span: None,
                message,
            }),
            secondary: Vec::new(),
            notes: Vec::new(),
        }),
        files: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dump_resolved_target, HirNamespace};
    use arche_package::{load_workspace, resolve, ManifestRequest, RegistrySnapshot};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let ordinal = NEXT.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "arche-frontend-workspace-{}-{ordinal}-{name}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("src")).unwrap();
            fs::create_dir_all(root.join("packages/dep/src")).unwrap();
            fs::write(
                root.join("Arche.toml"),
                manifest(
                    "example/app",
                    concat!(
                        "\n[workspace]\n",
                        "members = [\".\", \"packages/dep\"]\n\n",
                        "[lib]\n",
                        "path = \"src/lib.arc\"\n\n",
                        "[dependencies.dep]\n",
                        "path = \"packages/dep\"\n",
                    ),
                ),
            )
            .unwrap();
            fs::write(
                root.join("packages/dep/Arche.toml"),
                manifest("example/dep", "\n[lib]\npath = \"src/lib.arc\"\n"),
            )
            .unwrap();
            Self { root }
        }

        fn write_app(&self, source: &str) {
            fs::write(self.root.join("src/lib.arc"), source).unwrap();
        }

        fn write_dep(&self, relative: &str, source: &str) {
            let path = self.root.join("packages/dep").join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, source).unwrap();
        }

        fn load(&self) -> (Workspace, ResolvedGraph) {
            let workspace = load_workspace(&ManifestRequest::discover_from(&self.root)).unwrap();
            let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
            (workspace, graph)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn manifest(name: &str, suffix: &str) -> String {
        format!(
            concat!(
                "schema = 1\n\n",
                "[package]\n",
                "name = \"{}\"\n",
                "version = \"0.1.0\"\n",
                "edition = \"2026\"\n",
                "arche = \">=0.0.0\"\n",
                "publish = false\n",
                "{}",
            ),
            name, suffix
        )
    }

    #[test]
    fn resolves_public_path_dependency_imports_with_stable_package_qualified_ids() {
        let fixture = Fixture::new("public-import");
        fixture.write_app(
            "use dep::Public; use dep::nested::Nested; use dep::Both; pub component Local { }",
        );
        fixture.write_dep(
            "src/lib.arc",
            "pub mod nested; pub component Public { } component Private { } pub struct Both { } pub fn Both() { }",
        );
        fixture.write_dep("src/nested.arc", "pub component Nested { }");
        let (workspace, graph) = fixture.load();
        let dep = graph
            .packages
            .iter()
            .find(|package| package.name.as_str() == "example/dep")
            .unwrap()
            .id;

        let first = check_workspace(&workspace, &graph).unwrap();
        let second = check_workspace(&workspace, &graph).unwrap();
        let app = first
            .targets()
            .iter()
            .find(|target| target.target().package != dep)
            .unwrap();
        let imported = app.modules()[0].scopes[&HirNamespace::Type]
            .iter()
            .find(|entry| entry.name.as_str() == "Public")
            .unwrap();
        assert!(imported.imported);
        assert_eq!(imported.definition.package(), dep);
        assert_eq!(
            first.definition(imported.definition).unwrap().name.as_str(),
            "Public"
        );
        for namespace in [HirNamespace::Type, HirNamespace::Value] {
            assert!(app.modules()[0].scopes[&namespace]
                .iter()
                .any(|entry| entry.imported && entry.name.as_str() == "Both"));
        }
        assert_eq!(
            first
                .targets()
                .iter()
                .map(dump_resolved_target)
                .collect::<Vec<_>>(),
            second
                .targets()
                .iter()
                .map(dump_resolved_target)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rejects_nonpublic_dependency_items_and_private_module_traversal() {
        let fixture = Fixture::new("private-imports");
        fixture.write_dep(
            "src/lib.arc",
            "component Private { } pub(package) component PackageOnly { } mod hidden;",
        );
        fixture.write_dep("src/hidden.arc", "pub component HiddenPublic { }");
        let (workspace, graph) = fixture.load();

        for (path, code) in [
            ("dep::Private", "VISIBILITY003"),
            ("dep::PackageOnly", "VISIBILITY003"),
            ("dep::hidden::HiddenPublic", "VISIBILITY003"),
        ] {
            fixture.write_app(&format!("use {path}; pub component Local {{ }}"));
            assert_eq!(
                check_workspace(&workspace, &graph)
                    .unwrap_err()
                    .diagnostic
                    .code,
                code
            );
        }
    }

    #[test]
    fn public_reexport_can_cross_a_private_module_but_cannot_widen_a_private_item() {
        let fixture = Fixture::new("reexports");
        fixture.write_app("use dep::Visible; pub component Local { }");
        fixture.write_dep("src/lib.arc", "mod hidden; pub use self::hidden::Visible;");
        fixture.write_dep("src/hidden.arc", "pub component Visible { }");
        let (workspace, graph) = fixture.load();
        assert!(check_workspace(&workspace, &graph).is_ok());

        fixture.write_dep("src/hidden.arc", "component Visible { }");
        assert_eq!(
            check_workspace(&workspace, &graph)
                .unwrap_err()
                .diagnostic
                .code,
            "VISIBILITY003"
        );
    }

    #[test]
    fn public_module_reexport_projects_its_descendants_for_dependents() {
        let fixture = Fixture::new("module-reexport-descendants");
        fixture.write_app("use dep::api::Visible; pub component Local { }");
        fixture.write_dep("src/lib.arc", "mod hidden; pub use self::hidden::api;");
        fixture.write_dep("src/hidden.arc", "pub mod api;");
        fixture.write_dep("src/hidden/api.arc", "pub component Visible { }");
        let (workspace, graph) = fixture.load();

        assert!(check_workspace(&workspace, &graph).is_ok());
    }

    #[test]
    fn cyclic_public_module_reexports_project_once_without_recursing_forever() {
        let fixture = Fixture::new("module-reexport-cycle");
        fixture.write_app("use dep::a::Leaf; use dep::a::a; pub component Local { }");
        fixture.write_dep("src/lib.arc", "pub mod a;");
        fixture.write_dep("src/a.arc", "pub use package::a; pub component Leaf { }");
        let (workspace, graph) = fixture.load();

        assert!(check_workspace(&workspace, &graph).is_ok());
    }
}
