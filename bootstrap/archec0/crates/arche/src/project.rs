use arche_foundation::status::ProcessStatus;
use arche_frontend::{check_workspace as check_frontend_workspace, FrontendError};
use arche_package::{
    load_workspace, resolve, source_tree_digest, Diagnostic, DiagnosticCode, Diagnostics,
    IntegrityDigest, ManifestRequest, PackageName, PackageNodeId, PortablePath, RegistrySnapshot,
    SourceTreeEntry, ToolchainLock, WorkspaceMember,
};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CheckSummary {
    pub packages: u64,
    pub targets: u64,
    pub modules: u64,
}

#[derive(Debug)]
pub(crate) enum ProjectError {
    Package(Diagnostics),
    Frontend(FrontendError),
}

impl From<Diagnostics> for ProjectError {
    fn from(error: Diagnostics) -> Self {
        Self::Package(error)
    }
}

impl From<FrontendError> for ProjectError {
    fn from(error: FrontendError) -> Self {
        Self::Frontend(error)
    }
}

impl ProjectError {
    /// Classifies user-controlled project configuration separately from build failures.
    /// Mixed diagnostic batches remain failures so dependency, registry, or I/O errors
    /// cannot be hidden behind a configuration diagnostic.
    pub(crate) fn status(&self) -> ProcessStatus {
        match self {
            Self::Frontend(_) => ProcessStatus::Failure,
            Self::Package(diagnostics)
                if diagnostics
                    .entries()
                    .iter()
                    .all(|diagnostic| package_code_is_usage(diagnostic.code)) =>
            {
                ProcessStatus::Usage
            }
            Self::Package(_) => ProcessStatus::Failure,
        }
    }
}

fn package_code_is_usage(code: DiagnosticCode) -> bool {
    matches!(
        code,
        DiagnosticCode::ManifestSyntax
            | DiagnosticCode::ManifestSchema
            | DiagnosticCode::ManifestUnknown
            | DiagnosticCode::ManifestValue
            | DiagnosticCode::ManifestTarget
            | DiagnosticCode::WorkspaceDiscovery
            | DiagnosticCode::WorkspacePath
            | DiagnosticCode::WorkspaceMember
            | DiagnosticCode::LockInvalid
    )
}

pub(crate) fn check_project(
    current_dir: &Path,
    explicit_manifest: Option<&Path>,
) -> Result<CheckSummary, ProjectError> {
    let request = match explicit_manifest {
        Some(path) => ManifestRequest::explicit(current_dir, path),
        None => ManifestRequest::discover_from(current_dir),
    };
    let workspace = load_workspace(&request)?;
    let toolchain = ToolchainLock::bootstrap_current();
    validate_toolchain_requirements(&workspace.members, &toolchain)?;
    let graph = resolve(&workspace, &RegistrySnapshot::empty())?;
    let workspace_source_digest =
        source_tree_digest(std::slice::from_ref(&workspace.source_entry))?;

    let hir = check_frontend_workspace(&workspace, &graph)?;
    let target_count = u64::try_from(hir.targets().len()).map_err(|_| count_overflow())?;
    let mut module_count = 0_u64;
    let mut semantic_inputs =
        BTreeMap::<PackageNodeId, BTreeMap<PortablePath, SourceTreeEntry>>::new();
    for member in &workspace.members {
        let package = member
            .manifest
            .package
            .as_ref()
            .expect("workspace members are validated packages");
        let node = graph
            .packages
            .iter()
            .find(|resolved| resolved.name == package.name)
            .map(|resolved| resolved.id)
            .ok_or_else(|| missing_hir_package(&package.name))?;
        semantic_inputs.insert(
            node,
            BTreeMap::from([(
                member.manifest.source_entry.path.clone(),
                member.manifest.source_entry.clone(),
            )]),
        );
    }
    for target in hir.targets() {
        module_count = module_count
            .checked_add(u64::try_from(target.modules().len()).map_err(|_| count_overflow())?)
            .ok_or_else(count_overflow)?;
        let inputs = semantic_inputs
            .get_mut(&target.target().package)
            .ok_or_else(|| missing_hir_node(target.target().package))?;
        for entry in target.source_entries() {
            match inputs.get(&entry.path) {
                Some(previous) if previous != entry => {
                    return Err(changed_source_entry(&entry.path));
                }
                Some(_) => {}
                None => {
                    inputs.insert(entry.path.clone(), entry.clone());
                }
            }
        }
    }
    let mut source_digests = BTreeMap::<PackageName, IntegrityDigest>::new();
    for (node, inputs) in semantic_inputs {
        let package = graph.package(node).ok_or_else(|| missing_hir_node(node))?;
        let entries = inputs.into_values().collect::<Vec<_>>();
        source_digests.insert(package.name.clone(), source_tree_digest(&entries)?);
    }

    let lock = graph.to_lockfile(toolchain, workspace_source_digest, &source_digests)?;
    lock.publish_atomic(&workspace.root.join("Arche.lock"))?;
    Ok(CheckSummary {
        packages: u64::try_from(graph.packages.len()).map_err(|_| count_overflow())?,
        targets: target_count,
        modules: module_count,
    })
}

fn validate_toolchain_requirements(
    members: &[WorkspaceMember],
    toolchain: &ToolchainLock,
) -> Result<(), ProjectError> {
    for member in members {
        let package = member
            .manifest
            .package
            .as_ref()
            .expect("workspace members are validated packages");
        if !package.arche.matches(&toolchain.version) {
            return Err(ProjectError::Package(
                Diagnostic::new(
                    DiagnosticCode::ManifestValue,
                    format!(
                        "package `{}` requires Arche `{}`, but selected toolchain is `{}`",
                        package.name, package.arche, toolchain.version
                    ),
                )
                .at_path(member.directory.join("Arche.toml"))
                .into(),
            ));
        }
    }
    Ok(())
}

fn changed_source_entry(path: &PortablePath) -> ProjectError {
    ProjectError::Package(
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("source `{path}` changed between immutable target snapshots; retry the check"),
        )
        .into(),
    )
}

fn missing_hir_package(package: &PackageName) -> ProjectError {
    ProjectError::Package(
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("checked workspace package `{package}` is absent from the resolved graph"),
        )
        .into(),
    )
}

fn missing_hir_node(node: PackageNodeId) -> ProjectError {
    ProjectError::Package(
        Diagnostic::new(
            DiagnosticCode::Io,
            format!(
                "checked target references absent package node {}",
                node.get()
            ),
        )
        .into(),
    )
}

fn count_overflow() -> ProjectError {
    ProjectError::Package(
        Diagnostic::new(
            DiagnosticCode::Io,
            "project count exceeds the checked u64 representation",
        )
        .into(),
    )
}

pub(crate) fn write_error(output: &mut impl Write, error: &ProjectError) -> io::Result<()> {
    match error {
        ProjectError::Package(diagnostics) => {
            for diagnostic in diagnostics.entries() {
                if let Some(label) = &diagnostic.primary {
                    match (
                        label.start_line,
                        label.start_column,
                        label.end_line,
                        label.end_column,
                    ) {
                        (Some(line), Some(column), Some(_), Some(_)) => writeln!(
                            output,
                            "{}:{line}:{column}: error[{}]: {}",
                            label.path.display(),
                            diagnostic.code.as_str(),
                            diagnostic.message
                        )?,
                        _ => writeln!(
                            output,
                            "{}: error[{}]: {}",
                            label.path.display(),
                            diagnostic.code.as_str(),
                            diagnostic.message
                        )?,
                    }
                } else {
                    writeln!(
                        output,
                        "error[{}]: {}",
                        diagnostic.code.as_str(),
                        diagnostic.message
                    )?;
                }
                for note in &diagnostic.notes {
                    writeln!(output, "note: {note}")?;
                }
            }
        }
        ProjectError::Frontend(error) => write_frontend_error(output, error)?,
    }
    Ok(())
}

fn write_frontend_error(output: &mut impl Write, error: &FrontendError) -> io::Result<()> {
    if let Some(span) = error.diagnostic.primary.span {
        let path = usize::try_from(span.file.0)
            .ok()
            .and_then(|index| error.files.get(index));
        if let Some(path) = path {
            writeln!(
                output,
                "{}:{}:{}: error[{}]: {}",
                path.display(),
                span.start.line,
                span.start.column,
                error.diagnostic.code,
                error.diagnostic.message
            )?;
        } else {
            writeln!(output, "{}", error.diagnostic)?;
        }
    } else {
        writeln!(output, "{}", error.diagnostic)?;
    }
    for label in &error.diagnostic.secondary {
        writeln!(output, "note: {}", label.message)?;
    }
    for note in &error.diagnostic.notes {
        writeln!(output, "note: {note}")?;
    }
    Ok(())
}

impl fmt::Display for ProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(error) => error.fmt(formatter),
            Self::Frontend(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ProjectError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_diagnostic_renders_exact_source_location() {
        let error = ProjectError::Package(
            Diagnostic::new(
                DiagnosticCode::IdentityInvalid,
                "identifier space exhausted",
            )
            .at_source_span(
                "registry/example/Arche.toml",
                arche_package::ManifestSpan {
                    start_byte: 12,
                    end_byte: 27,
                    start_line: 4,
                    start_column: 3,
                    end_line: 4,
                    end_column: 18,
                },
            )
            .into(),
        );
        let mut output = Vec::new();

        write_error(&mut output, &error).expect("render diagnostic");

        assert_eq!(
            String::from_utf8(output).expect("UTF-8 output"),
            "registry/example/Arche.toml:4:3: error[IDENTITY001]: identifier space exhausted\n"
        );
    }

    #[test]
    fn package_diagnostic_preserves_legacy_path_only_rendering() {
        let error = ProjectError::Package(
            Diagnostic::new(DiagnosticCode::ManifestSyntax, "invalid manifest")
                .at_span("Arche.toml", 2, 8)
                .into(),
        );
        let mut output = Vec::new();

        write_error(&mut output, &error).expect("render diagnostic");

        assert_eq!(
            String::from_utf8(output).expect("UTF-8 output"),
            "Arche.toml: error[MANIFEST001]: invalid manifest\n"
        );
    }
}
