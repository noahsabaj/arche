use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use arche_package::{
    LockDependencyKind, Manifest, PackageName, PackageNodeId, ResolvedGraph, ResolvedSource,
    Target, Workspace, WorkspaceMember,
};

use crate::hir::DependencyExport;
use crate::modules::{check_target_with_dependencies, package_export_surface};
use crate::{
    check_target, CheckTargetRequest, Diagnostic, EnvironmentSchedulePaths, FileId, FrontendError,
    FrontendErrorCode, Label, ResolvedTargetHir, ResolvedWorkspaceHir, SourcePosition, Span,
    TargetId, TargetKind,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct TargetIdAllocator {
    next: Option<u64>,
}

impl TargetIdAllocator {
    pub(crate) const fn new() -> Self {
        Self { next: Some(0) }
    }

    #[cfg(test)]
    const fn near_exhaustion() -> Self {
        Self {
            next: Some(u64::MAX - 1),
        }
    }

    pub(crate) fn allocate(
        &mut self,
        manifest: &Manifest,
        target: &Target,
    ) -> Result<TargetId, FrontendError> {
        let value = self
            .next
            .ok_or_else(|| target_id_exhausted(manifest, target))?;
        self.next = value.checked_add(1);
        Ok(TargetId(value))
    }
}

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
    for node in &ordered_nodes {
        let manifest = &members[node].manifest;
        let mut allocator = TargetIdAllocator::new();
        let mut ids = Vec::new();
        for target in manifest.targets() {
            ids.push(allocator.allocate(manifest, &target)?);
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

fn target_id_exhausted(manifest: &Manifest, target: &Target) -> FrontendError {
    let manifest_span = manifest.target_span(target);
    let target = match target {
        Target::Library(_) => "library target".to_owned(),
        Target::Binary(target) => format!("binary target `{}`", target.name),
        Target::Environment(target) => format!("environment target `{}`", target.name),
    };
    let mut error = target_error(
        "IDENTITY001",
        format!("TargetId allocation for {target} exceeds the checked u64 domain"),
    );
    if let Some(manifest_span) = manifest_span {
        error.diagnostic.primary.span = Some(Span {
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
        });
    } else {
        error
            .diagnostic
            .notes
            .push("the manifest target has no retained table span".to_owned());
    }
    error.files.push(manifest.path.clone());
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dump_resolved_target, HirNamespace};
    use arche_package::{load_workspace, resolve, ManifestRequest, RegistrySnapshot};
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn target_id_allocator_accepts_its_maximum_then_fails_closed() {
        let manifest_path = PathBuf::from("packages/app/Arche.toml");
        let manifest_source = manifest(
            "example/app",
            concat!(
                "\n[lib]\npath = \"src/lib.arc\"\n",
                "\n[[bin]]\nname = \"one\"\npath = \"src/one.arc\"\nworld = \"package::One\"\n",
                "\n[[bin]]\nname = \"overflow\"\npath = \"src/overflow.arc\"\nworld = \"package::Overflow\"\n",
            ),
        );
        let parsed_manifest = Manifest::parse(&manifest_path, &manifest_source).unwrap();
        let targets = parsed_manifest.targets().collect::<Vec<_>>();
        let mut allocator = TargetIdAllocator::near_exhaustion();

        assert_eq!(
            allocator.allocate(&parsed_manifest, &targets[0]).unwrap(),
            TargetId(u64::MAX - 1)
        );
        assert_eq!(
            allocator.allocate(&parsed_manifest, &targets[1]).unwrap(),
            TargetId(u64::MAX)
        );

        let error = allocator
            .allocate(&parsed_manifest, &targets[2])
            .unwrap_err();
        assert_eq!(error.diagnostic.code, "IDENTITY001");
        assert_eq!(error.files, vec![manifest_path]);
        let target_span = parsed_manifest.target_span(&targets[2]).unwrap();
        assert_eq!(
            &manifest_source[usize::try_from(target_span.start_byte).unwrap()
                ..usize::try_from(target_span.end_byte).unwrap()],
            "[[bin]]"
        );
        assert_eq!(
            error.diagnostic.primary.span,
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
        assert!(error.diagnostic.message.contains("overflow"));

        let repeated = allocator
            .allocate(&parsed_manifest, &targets[2])
            .unwrap_err();
        assert_eq!(
            error, repeated,
            "exhaustion must be deterministic and sticky"
        );
    }

    #[test]
    fn manifest_target_table_reordering_preserves_canonical_target_ids() {
        let vectors = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../tests/m27b");
        let load = |name: &str| {
            let path = vectors.join(name);
            let source = fs::read_to_string(&path).unwrap();
            Manifest::parse(&path, &source).unwrap()
        };
        let first = load("target-order-a.toml");
        let second = load("target-order-b.toml");
        let first_targets = first.targets().collect::<Vec<_>>();
        let second_targets = second.targets().collect::<Vec<_>>();
        assert_eq!(first_targets, second_targets);

        let allocate = |manifest: &Manifest, targets: &[Target]| {
            let mut allocator = TargetIdAllocator::new();
            targets
                .iter()
                .map(|target| allocator.allocate(manifest, target).unwrap())
                .collect::<Vec<_>>()
        };
        let first_ids = allocate(&first, &first_targets);
        let second_ids = allocate(&second, &second_targets);
        assert_eq!(first_ids, second_ids);
        assert_eq!(
            first_ids,
            vec![
                TargetId(0),
                TargetId(1),
                TargetId(2),
                TargetId(3),
                TargetId(4)
            ]
        );
    }

    #[test]
    fn multi_package_multi_target_ids_restart_and_remain_package_qualified() {
        let fixture = Fixture::new("multi-package-multi-target");
        fs::write(
            fixture.root.join("Arche.toml"),
            manifest(
                "example/app",
                concat!(
                    "\n[workspace]\n",
                    "members = [\".\", \"packages/dep\"]\n\n",
                    "[lib]\n",
                    "path = \"src/lib.arc\"\n\n",
                    "[[bin]]\n",
                    "name = \"server\"\n",
                    "path = \"src/main.arc\"\n",
                    "world = \"package::ServerWorld\"\n\n",
                    "[dependencies.dep]\n",
                    "path = \"packages/dep\"\n",
                ),
            ),
        )
        .unwrap();
        fs::write(
            fixture.root.join("packages/dep/Arche.toml"),
            manifest(
                "example/dep",
                concat!(
                    "\n[lib]\n",
                    "path = \"src/lib.arc\"\n\n",
                    "[[bin]]\n",
                    "name = \"server\"\n",
                    "path = \"src/main.arc\"\n",
                    "world = \"package::ServerWorld\"\n",
                ),
            ),
        )
        .unwrap();
        fixture.write_app("pub component LibraryMarker { }");
        fixture.write_dep("src/lib.arc", "pub component LibraryMarker { }");
        fs::write(
            fixture.root.join("src/main.arc"),
            "pub world ServerWorld { init { } } pub fn main() { } pub component BinaryMarker { }",
        )
        .unwrap();
        fixture.write_dep(
            "src/main.arc",
            "pub world ServerWorld { init { } } pub fn main() { } pub component BinaryMarker { }",
        );

        let (workspace, graph) = fixture.load();
        let hir = check_workspace(&workspace, &graph).unwrap();
        let target_keys = hir
            .targets()
            .iter()
            .map(|target| (target.target().package.get(), target.target().id.0))
            .collect::<Vec<_>>();
        assert_eq!(target_keys, [(0, 0), (0, 1), (1, 0), (1, 1)]);

        let definition_ids = hir
            .targets()
            .iter()
            .flat_map(|target| target.definitions().iter().map(|definition| definition.id))
            .collect::<Vec<_>>();
        assert_eq!(
            definition_ids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len(),
            definition_ids.len(),
            "package and target coordinates must keep repeated local IDs globally distinct"
        );
        let repeated_zero_ids = definition_ids
            .iter()
            .copied()
            .filter(|id| id.local() == 0)
            .map(|id| id.to_string())
            .collect::<Vec<_>>();
        assert_eq!(repeated_zero_ids, ["p0t0d0", "p0t1d0", "p1t0d0", "p1t1d0"]);
    }

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
        assert!(
            first
                .targets()
                .iter()
                .all(|target| target.target().id == TargetId(0)),
            "TargetId must restart at zero for each single-target package"
        );
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
