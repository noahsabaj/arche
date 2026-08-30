use crate::diagnostic::{io_diagnostic, Diagnostic, DiagnosticCode, Diagnostics};
use crate::{DependencyKind, DependencyPath, Manifest, PackageName, PortablePath, SourceTreeEntry};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

pub const MANIFEST_FILE_NAME: &str = "Arche.toml";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestRequest {
    pub start: PathBuf,
    pub explicit_manifest: Option<PathBuf>,
}

impl ManifestRequest {
    pub fn discover_from(start: impl Into<PathBuf>) -> Self {
        Self {
            start: start.into(),
            explicit_manifest: None,
        }
    }

    pub fn explicit(start: impl Into<PathBuf>, manifest: impl Into<PathBuf>) -> Self {
        Self {
            start: start.into(),
            explicit_manifest: Some(manifest.into()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceMember {
    pub relative_path: PortablePath,
    /// Canonical physical package directory. It never enters artifact identity.
    pub directory: PathBuf,
    pub manifest: Manifest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
    /// Canonical physical workspace directory. It never enters lock bytes.
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    /// Exact bytes of the workspace authority manifest. This is separate from
    /// member package source digests for virtual workspaces.
    pub source_entry: SourceTreeEntry,
    pub members: Vec<WorkspaceMember>,
    pub default_members: Vec<usize>,
}

impl Workspace {
    pub fn selected_members(&self) -> impl Iterator<Item = &WorkspaceMember> {
        self.default_members
            .iter()
            .map(|index| &self.members[*index])
    }

    pub fn member(&self, package: &PackageName) -> Option<&WorkspaceMember> {
        self.members.iter().find(|member| {
            member
                .manifest
                .package
                .as_ref()
                .is_some_and(|candidate| &candidate.name == package)
        })
    }
}

pub fn discover_manifest(request: &ManifestRequest) -> Result<PathBuf, Diagnostics> {
    if let Some(explicit) = &request.explicit_manifest {
        let path = if explicit.is_absolute() {
            explicit.clone()
        } else {
            request.start.join(explicit)
        };
        return validate_manifest_file(&path);
    }
    let start = fs::canonicalize(&request.start)
        .map_err(|error| io_diagnostic(&request.start, "resolve discovery directory", &error))?;
    let mut directory = if start.is_dir() {
        start
    } else {
        start.parent().map(Path::to_path_buf).ok_or_else(|| {
            workspace_error(
                DiagnosticCode::WorkspaceDiscovery,
                "manifest discovery start has no parent directory",
            )
        })?
    };
    loop {
        let candidate = directory.join(MANIFEST_FILE_NAME);
        if candidate.exists() {
            return validate_manifest_file(&candidate);
        }
        if !directory.pop() {
            break;
        }
    }
    Err(workspace_error(
        DiagnosticCode::WorkspaceDiscovery,
        format!(
            "could not find `{MANIFEST_FILE_NAME}` at or above `{}`",
            request.start.display()
        ),
    ))
}

pub fn load_workspace(request: &ManifestRequest) -> Result<Workspace, Diagnostics> {
    let selected_manifest_path = discover_manifest(request)?;
    let selected_manifest = Manifest::load(&selected_manifest_path)?;
    let selected_directory = selected_manifest_path
        .parent()
        .expect("validated manifest has parent")
        .to_path_buf();
    let (root_manifest_path, root_manifest) = if selected_manifest.workspace_members().is_some() {
        (selected_manifest_path.clone(), selected_manifest)
    } else if let Some((path, manifest)) = find_enclosing_workspace(&selected_directory)? {
        (path, manifest)
    } else {
        return singleton_workspace(selected_manifest_path, selected_manifest);
    };
    load_declared_workspace(&root_manifest_path, root_manifest, &selected_directory)
}

fn singleton_workspace(
    manifest_path: PathBuf,
    manifest: Manifest,
) -> Result<Workspace, Diagnostics> {
    let package = manifest.package.as_ref().ok_or_else(|| {
        workspace_error(
            DiagnosticCode::WorkspaceMember,
            "a virtual workspace must declare workspace members",
        )
    })?;
    let directory = manifest_path
        .parent()
        .expect("validated manifest has parent")
        .to_path_buf();
    let root = fs::canonicalize(&directory)
        .map_err(|error| io_diagnostic(&directory, "resolve package directory", &error))?;
    let source_entry = manifest.source_entry.clone();
    let _ = package;
    Ok(Workspace {
        root: root.clone(),
        manifest_path,
        source_entry,
        members: vec![WorkspaceMember {
            relative_path: PortablePath::workspace_member(".")?,
            directory: root,
            manifest,
        }],
        default_members: vec![0],
    })
}

fn find_enclosing_workspace(
    selected_directory: &Path,
) -> Result<Option<(PathBuf, Manifest)>, Diagnostics> {
    let selected = fs::canonicalize(selected_directory)
        .map_err(|error| io_diagnostic(selected_directory, "resolve selected package", &error))?;
    let mut ancestor = selected.parent();
    while let Some(directory) = ancestor {
        let candidate = directory.join(MANIFEST_FILE_NAME);
        if candidate.is_file() {
            let manifest = Manifest::load(&candidate)?;
            if let Some(members) = manifest.workspace_members() {
                let listed = members.iter().any(|member| {
                    resolve_member_lexically(directory, member)
                        .and_then(|path| fs::canonicalize(path).ok())
                        .is_some_and(|path| path == selected)
                });
                if listed {
                    return Ok(Some((validate_manifest_file(&candidate)?, manifest)));
                }
            }
        }
        ancestor = directory.parent();
    }
    Ok(None)
}

fn load_declared_workspace(
    root_manifest_path: &Path,
    root_manifest: Manifest,
    selected_directory: &Path,
) -> Result<Workspace, Diagnostics> {
    let root_directory = root_manifest_path
        .parent()
        .expect("validated manifest has parent");
    let root = fs::canonicalize(root_directory)
        .map_err(|error| io_diagnostic(root_directory, "resolve workspace root", &error))?;
    let source_entry = root_manifest.source_entry.clone();
    let declared = root_manifest
        .workspace_members()
        .expect("workspace loader received workspace manifest")
        .to_vec();
    let defaults = root_manifest
        .workspace_default_members()
        .expect("workspace loader received workspace manifest")
        .map(ToOwned::to_owned);
    let has_root = declared.iter().any(|path| path.as_str() == ".");
    if root_manifest.package.is_some() != has_root {
        return Err(workspace_error(
            DiagnosticCode::WorkspaceMember,
            "a combined package/workspace must list `.`, while a virtual workspace must not",
        ));
    }

    let mut members = Vec::with_capacity(declared.len());
    let mut physical = BTreeSet::new();
    let mut folded = BTreeMap::<String, String>::new();
    let mut package_names = BTreeSet::new();
    for relative in &declared {
        let key = relative.casefold_key();
        if let Some(previous) = folded.insert(key, relative.as_str().to_owned()) {
            return Err(workspace_error(
                DiagnosticCode::WorkspaceMember,
                format!("workspace paths `{previous}` and `{relative}` are case-fold/NFC aliases"),
            ));
        }
        let lexical = resolve_exact_member_path(&root, relative)?;
        let directory = fs::canonicalize(&lexical)
            .map_err(|error| io_diagnostic(&lexical, "resolve workspace member", &error))?;
        if !directory.starts_with(&root) || !directory.is_dir() {
            return Err(workspace_error(
                DiagnosticCode::WorkspacePath,
                format!(
                    "workspace member `{relative}` is not a directory contained by the workspace"
                ),
            ));
        }
        if !physical.insert(directory.clone()) {
            return Err(workspace_error(
                DiagnosticCode::WorkspaceMember,
                format!("workspace member `{relative}` aliases another physical directory"),
            ));
        }
        let manifest_path = validate_manifest_file(&directory.join(MANIFEST_FILE_NAME))?;
        let manifest = if relative.as_str() == "." {
            root_manifest.clone()
        } else {
            let manifest = Manifest::load(&manifest_path)?;
            if manifest.workspace_members().is_some() {
                return Err(workspace_error(
                    DiagnosticCode::WorkspaceMember,
                    format!("nested workspace at `{relative}` is not supported"),
                ));
            }
            manifest
        };
        let package = manifest.package.as_ref().ok_or_else(|| {
            workspace_error(
                DiagnosticCode::WorkspaceMember,
                format!("workspace member `{relative}` does not declare [package]"),
            )
        })?;
        if !package_names.insert(package.name.clone()) {
            return Err(workspace_error(
                DiagnosticCode::WorkspaceMember,
                format!("workspace repeats package identity `{}`", package.name),
            ));
        }
        members.push(WorkspaceMember {
            relative_path: relative.clone(),
            directory,
            manifest,
        });
    }

    let selected = fs::canonicalize(selected_directory)
        .map_err(|error| io_diagnostic(selected_directory, "resolve selected package", &error))?;
    if !members.iter().any(|member| member.directory == selected) && selected != root {
        return Err(workspace_error(
            DiagnosticCode::WorkspaceMember,
            "selected package is not a declared member of its enclosing workspace",
        ));
    }

    validate_path_dependencies(&root, &members)?;
    let selected_paths = defaults.unwrap_or_else(|| declared.clone());
    let default_members = selected_paths
        .iter()
        .map(|path| {
            members
                .iter()
                .position(|member| member.relative_path == *path)
                .expect("manifest parser validated defaults are members")
        })
        .collect();
    Ok(Workspace {
        root,
        manifest_path: root_manifest_path.to_path_buf(),
        source_entry,
        members,
        default_members,
    })
}

fn validate_path_dependencies(root: &Path, members: &[WorkspaceMember]) -> Result<(), Diagnostics> {
    for member in members {
        for dependency in member
            .manifest
            .dependencies
            .values()
            .chain(member.manifest.dev_dependencies.values())
            .filter(|dependency| dependency.kind != DependencyKind::Registry)
        {
            let path = dependency.path.as_ref().expect("path dependency has path");
            let lexical = resolve_exact_dependency_path(
                root,
                &member.directory,
                path,
                dependency.alias.as_str(),
            )?;
            let target = fs::canonicalize(&lexical)
                .map_err(|error| io_diagnostic(&lexical, "resolve path dependency", &error))?;
            if !target.starts_with(root) {
                return Err(workspace_error(
                    DiagnosticCode::WorkspacePath,
                    format!(
                        "path dependency `{}` escapes the workspace",
                        dependency.alias
                    ),
                ));
            }
            let target_member = members
                .iter()
                .find(|candidate| candidate.directory == target)
                .ok_or_else(|| {
                    workspace_error(
                        DiagnosticCode::WorkspaceMember,
                        format!(
                            "path dependency `{}` does not target a declared workspace member",
                            dependency.alias
                        ),
                    )
                })?;
            let target_package = target_member
                .manifest
                .package
                .as_ref()
                .expect("member has package");
            if let Some(expected) = &dependency.package {
                if expected != &target_package.name {
                    return Err(workspace_error(
                        DiagnosticCode::WorkspaceMember,
                        format!(
                            "path dependency `{}` names `{expected}` but targets `{}`",
                            dependency.alias, target_package.name
                        ),
                    ));
                }
            }
            if let Some(requirement) = &dependency.requirement {
                if !requirement.matches(&target_package.version) {
                    return Err(workspace_error(
                        DiagnosticCode::WorkspaceMember,
                        format!("path dependency `{}` requires `{requirement}` but targets version `{}`", dependency.alias, target_package.version),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_manifest_file(path: &Path) -> Result<PathBuf, Diagnostics> {
    if path.file_name().and_then(|name| name.to_str()) != Some(MANIFEST_FILE_NAME) {
        return Err(workspace_error(
            DiagnosticCode::WorkspacePath,
            format!("manifest path must name `{MANIFEST_FILE_NAME}` exactly"),
        ));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let exact = resolve_exact_child(
        parent,
        MANIFEST_FILE_NAME,
        "manifest",
        &path.display().to_string(),
    )?;
    let metadata = fs::symlink_metadata(&exact)
        .map_err(|error| io_diagnostic(&exact, "inspect manifest", &error))?;
    if is_link_or_reparse_point(&metadata) || !metadata.is_file() {
        return Err(workspace_error(
            DiagnosticCode::WorkspacePath,
            format!(
                "manifest `{}` must be a regular non-symlink file",
                exact.display()
            ),
        ));
    }
    fs::canonicalize(&exact).map_err(|error| io_diagnostic(&exact, "resolve manifest", &error))
}

fn resolve_member_lexically(root: &Path, path: &PortablePath) -> Option<PathBuf> {
    if path.as_str() == "." {
        return Some(root.to_path_buf());
    }
    Some(
        path.segments()
            .fold(root.to_path_buf(), |base, segment| base.join(segment)),
    )
}

fn resolve_exact_member_path(root: &Path, path: &PortablePath) -> Result<PathBuf, Diagnostics> {
    if path.as_str() == "." {
        return Ok(root.to_path_buf());
    }
    let mut current = root.to_path_buf();
    for segment in path.segments() {
        current = resolve_exact_child(&current, segment, "workspace member", path.as_str())?;
    }
    Ok(current)
}

fn resolve_exact_dependency_path(
    workspace_root: &Path,
    package_root: &Path,
    path: &DependencyPath,
    alias: &str,
) -> Result<PathBuf, Diagnostics> {
    let mut current = package_root.to_path_buf();
    for segment in path.segments() {
        if segment == ".." {
            if !current.pop() || !current.starts_with(workspace_root) {
                return Err(workspace_error(
                    DiagnosticCode::WorkspacePath,
                    format!("path dependency `{alias}` escapes the workspace"),
                ));
            }
        } else {
            current = resolve_exact_child(&current, segment, "path dependency", alias)?;
        }
    }
    if !current.starts_with(workspace_root) {
        return Err(workspace_error(
            DiagnosticCode::WorkspacePath,
            format!("path dependency `{alias}` escapes the workspace"),
        ));
    }
    Ok(current)
}

fn resolve_exact_child(
    parent: &Path,
    expected: &str,
    kind: &str,
    owner: &str,
) -> Result<PathBuf, Diagnostics> {
    let expected_nfc = expected.nfc().collect::<String>();
    let expected_fold = expected_nfc.case_fold().nfc().collect::<String>();
    let mut aliases = Vec::new();
    let entries = fs::read_dir(parent)
        .map_err(|error| io_diagnostic(parent, "inspect path component directory", &error))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| io_diagnostic(parent, "inspect path component", &error))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let nfc = name.nfc().collect::<String>();
        let folded = nfc.case_fold().nfc().collect::<String>();
        if nfc == expected_nfc || folded == expected_fold {
            aliases.push(name);
        }
    }
    aliases.sort();
    if aliases.len() != 1 || aliases[0] != expected {
        let observed = if aliases.is_empty() {
            "none".to_owned()
        } else {
            aliases.join(", ")
        };
        return Err(workspace_error(
            DiagnosticCode::WorkspacePath,
            format!(
                "{kind} `{owner}` requires exact NFC/case path component `{expected}`; colliding entries: {observed}"
            ),
        ));
    }
    let child = parent.join(expected);
    let metadata = fs::symlink_metadata(&child)
        .map_err(|error| io_diagnostic(&child, "inspect path component", &error))?;
    if is_link_or_reparse_point(&metadata) {
        return Err(workspace_error(
            DiagnosticCode::WorkspacePath,
            format!(
                "path component `{}` is a symlink or junction",
                child.display()
            ),
        ));
    }
    Ok(child)
}

fn is_link_or_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn workspace_error(code: DiagnosticCode, message: impl Into<String>) -> Diagnostics {
    Diagnostic::new(code, message).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn discovers_singleton_from_nested_directory() {
        let root = temp_directory();
        fs::create_dir(root.join("src")).unwrap();
        write_manifest(
            &root.join(MANIFEST_FILE_NAME),
            "example/one",
            "\n[lib]\npath = \"src/lib.arc\"\n",
        );
        fs::write(root.join("src/lib.arc"), "pub component Marker {}\n").unwrap();
        let workspace = load_workspace(&ManifestRequest::discover_from(root.join("src"))).unwrap();
        assert_eq!(workspace.members.len(), 1);
        assert_eq!(workspace.selected_members().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_workspace_members_and_defaults_are_stable() {
        let root = temp_directory();
        fs::create_dir(root.join("packages")).unwrap();
        fs::create_dir(root.join("packages/a")).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("packages/a/src")).unwrap();
        write_manifest(
            &root.join(MANIFEST_FILE_NAME),
            "example/root",
            "\n[workspace]\nmembers = [\".\", \"packages/a\"]\ndefault-members = [\"packages/a\"]\n\n[lib]\npath = \"src/lib.arc\"\n",
        );
        write_manifest(
            &root.join("packages/a/Arche.toml"),
            "example/a",
            "\n[lib]\npath = \"src/lib.arc\"\n",
        );
        fs::write(root.join("src/lib.arc"), "pub component Root {}\n").unwrap();
        fs::write(root.join("packages/a/src/lib.arc"), "pub component A {}\n").unwrap();
        let workspace =
            load_workspace(&ManifestRequest::discover_from(root.join("packages/a"))).unwrap();
        assert_eq!(workspace.members.len(), 2);
        assert_eq!(
            workspace
                .selected_members()
                .next()
                .unwrap()
                .manifest
                .package
                .as_ref()
                .unwrap()
                .name
                .as_str(),
            "example/a"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sibling_dependency_parent_path_stays_inside_the_workspace() {
        let root = temp_directory();
        fs::create_dir_all(root.join("packages/app/src")).unwrap();
        fs::create_dir_all(root.join("packages/shared/src")).unwrap();
        fs::write(
            root.join(MANIFEST_FILE_NAME),
            concat!(
                "schema = 1\n\n",
                "[workspace]\n",
                "members = [\"packages/app\", \"packages/shared\"]\n",
                "default-members = [\"packages/app\"]\n",
            ),
        )
        .unwrap();
        write_manifest(
            &root.join("packages/app/Arche.toml"),
            "example/app",
            concat!(
                "\n[lib]\npath = \"src/lib.arc\"\n",
                "\n[dependencies]\nshared = { path = \"../shared\" }\n",
            ),
        );
        write_manifest(
            &root.join("packages/shared/Arche.toml"),
            "example/shared",
            "\n[lib]\npath = \"src/lib.arc\"\n",
        );
        fs::write(
            root.join("packages/app/src/lib.arc"),
            "pub component App {}\n",
        )
        .unwrap();
        fs::write(
            root.join("packages/shared/src/lib.arc"),
            "pub component Shared {}\n",
        )
        .unwrap();

        let workspace = load_workspace(&ManifestRequest::discover_from(
            root.join("packages/app/src"),
        ))
        .unwrap();
        assert_eq!(workspace.members.len(), 2);
        assert_eq!(
            workspace
                .selected_members()
                .next()
                .unwrap()
                .manifest
                .package
                .as_ref()
                .unwrap()
                .name
                .as_str(),
            "example/app"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_member_components_require_exact_case_and_nfc() {
        let root = temp_directory();
        fs::create_dir_all(root.join("Packages/app/src")).unwrap();
        fs::write(
            root.join(MANIFEST_FILE_NAME),
            "schema = 1\n\n[workspace]\nmembers = [\"packages/app\"]\n",
        )
        .unwrap();
        write_manifest(
            &root.join("Packages/app/Arche.toml"),
            "example/app",
            "\n[lib]\npath = \"src/lib.arc\"\n",
        );
        fs::write(
            root.join("Packages/app/src/lib.arc"),
            "pub component App {}\n",
        )
        .unwrap();

        let error = load_workspace(&ManifestRequest::discover_from(&root)).unwrap_err();
        assert_eq!(error.entries()[0].code, DiagnosticCode::WorkspacePath);
        fs::remove_dir_all(root).unwrap();
    }

    fn write_manifest(path: &Path, name: &str, suffix: &str) {
        let mut file = fs::File::create(path).unwrap();
        write!(
            file,
            "schema = 1\n\n[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\narche = \">=0.0.0\"\npublish = false\n{suffix}"
        )
        .unwrap();
    }

    fn temp_directory() -> PathBuf {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("arche-workspace-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        path
    }
}
