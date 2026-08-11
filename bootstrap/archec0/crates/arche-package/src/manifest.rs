use crate::diagnostic::{io_diagnostic, Diagnostic, DiagnosticCode, Diagnostics};
use crate::{
    DependencyAlias, DependencyPath, ItemPath, PackageName, PortablePath, SourceIdentifier,
    SourceTreeEntry,
};
use semver::{Version, VersionReq};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use toml_edit::{Array, Document, InlineTable, Item, Table, Value};

pub const MANIFEST_SCHEMA: i64 = 1;
pub const DEFAULT_CONST_EVAL_STEPS: u64 = 10_000_000;
pub const DEFAULT_CONST_EVAL_CALL_DEPTH: u64 = 1_024;
pub const DEFAULT_CONST_EVAL_HEAP_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishMetadata {
    pub enabled: bool,
    pub license: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Package {
    pub name: PackageName,
    pub version: Version,
    pub edition: String,
    pub arche: VersionReq,
    pub publish: PublishMetadata,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConstEvalBudgets {
    pub steps: u64,
    pub call_depth: u64,
    pub heap_bytes: u64,
}

impl Default for ConstEvalBudgets {
    fn default() -> Self {
        Self {
            steps: DEFAULT_CONST_EVAL_STEPS,
            call_depth: DEFAULT_CONST_EVAL_CALL_DEPTH,
            heap_bytes: DEFAULT_CONST_EVAL_HEAP_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Capability {
    Args,
    Environment,
    Stdio,
    Files,
    Subprocess,
    WallClock,
    MonotonicClock,
    Tcp,
    Udp,
    Threads,
    Atomics,
    Synchronization,
}

impl Capability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Args => "args",
            Self::Environment => "environment",
            Self::Stdio => "stdio",
            Self::Files => "files",
            Self::Subprocess => "subprocess",
            Self::WallClock => "wall-clock",
            Self::MonotonicClock => "monotonic-clock",
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Threads => "threads",
            Self::Atomics => "atomics",
            Self::Synchronization => "synchronization",
        }
    }
}

impl FromStr for Capability {
    type Err = Diagnostics;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "args" => Ok(Self::Args),
            "environment" => Ok(Self::Environment),
            "stdio" => Ok(Self::Stdio),
            "files" => Ok(Self::Files),
            "subprocess" => Ok(Self::Subprocess),
            "wall-clock" => Ok(Self::WallClock),
            "monotonic-clock" => Ok(Self::MonotonicClock),
            "tcp" => Ok(Self::Tcp),
            "udp" => Ok(Self::Udp),
            "threads" => Ok(Self::Threads),
            "atomics" => Ok(Self::Atomics),
            "synchronization" => Ok(Self::Synchronization),
            _ => Err(Diagnostic::new(
                DiagnosticCode::ManifestValue,
                format!("unknown capability `{value}`"),
            )
            .into()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibTarget {
    pub path: PortablePath,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BinaryTarget {
    pub name: SourceIdentifier,
    pub path: PortablePath,
    pub world: ItemPath,
    pub capabilities: BTreeSet<Capability>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentTarget {
    pub name: SourceIdentifier,
    pub path: PortablePath,
    pub world: ItemPath,
    pub profile: SourceIdentifier,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentProfile {
    pub name: SourceIdentifier,
    pub reset: ItemPath,
    pub step: ItemPath,
    pub self_play: ItemPath,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetKind {
    Library,
    Binary,
    Environment,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Target {
    Library(LibTarget),
    Binary(BinaryTarget),
    Environment(EnvironmentTarget),
}

impl Target {
    pub const fn kind(&self) -> TargetKind {
        match self {
            Self::Library(_) => TargetKind::Library,
            Self::Binary(_) => TargetKind::Binary,
            Self::Environment(_) => TargetKind::Environment,
        }
    }

    pub fn name(&self) -> Option<&SourceIdentifier> {
        match self {
            Self::Library(_) => None,
            Self::Binary(target) => Some(&target.name),
            Self::Environment(target) => Some(&target.name),
        }
    }

    pub fn path(&self) -> &PortablePath {
        match self {
            Self::Library(target) => &target.path,
            Self::Binary(target) => &target.path,
            Self::Environment(target) => &target.path,
        }
    }

    pub fn world(&self) -> Option<&ItemPath> {
        match self {
            Self::Library(_) => None,
            Self::Binary(target) => Some(&target.world),
            Self::Environment(target) => Some(&target.world),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DependencyKind {
    Registry,
    Path,
    PublishCompatiblePath,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dependency {
    pub alias: DependencyAlias,
    pub kind: DependencyKind,
    pub package: Option<PackageName>,
    pub requirement: Option<VersionReq>,
    pub path: Option<DependencyPath>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceDeclaration {
    pub members: Vec<PortablePath>,
    pub default_members: Option<Vec<PortablePath>>,
}

/// Exact half-open UTF-8 source range retained from one parsed manifest.
/// Lines and columns are one-based Unicode-scalar coordinates; bare carriage
/// returns advance only the byte coordinate, matching the C1 source contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManifestSpan {
    pub start_byte: u64,
    pub end_byte: u64,
    pub start_line: u64,
    pub start_column: u64,
    pub end_line: u64,
    pub end_column: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Manifest {
    pub path: PathBuf,
    pub source_entry: SourceTreeEntry,
    pub package: Option<Package>,
    pub const_eval: ConstEvalBudgets,
    pub library: Option<LibTarget>,
    pub binaries: Vec<BinaryTarget>,
    pub environments: Vec<EnvironmentTarget>,
    pub environment_profiles: BTreeMap<SourceIdentifier, EnvironmentProfile>,
    pub dependencies: BTreeMap<DependencyAlias, Dependency>,
    pub dev_dependencies: BTreeMap<DependencyAlias, Dependency>,
    package_span: Option<ManifestSpan>,
    library_span: Option<ManifestSpan>,
    binary_spans: Vec<ManifestSpan>,
    environment_spans: Vec<ManifestSpan>,
    pub(crate) workspace: Option<WorkspaceDeclaration>,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Self, Diagnostics> {
        let bytes = fs::read(path).map_err(|error| io_diagnostic(path, "read manifest", &error))?;
        let source = std::str::from_utf8(&bytes).map_err(|error| {
            Diagnostic::new(
                DiagnosticCode::ManifestSyntax,
                format!("manifest is not UTF-8: {error}"),
            )
            .at_span(
                path,
                error.valid_up_to(),
                error.valid_up_to().saturating_add(1),
            )
        })?;
        Self::parse(path, source)
    }

    pub fn parse(path: &Path, source: &str) -> Result<Self, Diagnostics> {
        if source.starts_with('\u{feff}') {
            return Err(Diagnostic::new(
                DiagnosticCode::ManifestSyntax,
                "manifest must be UTF-8 without a byte-order mark",
            )
            .at_span(path, 0, 3)
            .into());
        }
        let document = Document::parse(source).map_err(|error| {
            let mut diagnostic = Diagnostic::new(
                DiagnosticCode::ManifestSyntax,
                format!("invalid TOML: {error}"),
            );
            if let Some(span) = error.span() {
                diagnostic = diagnostic.at_span(path, span.start, span.end);
            } else {
                diagnostic = diagnostic.at_path(path);
            }
            Diagnostics::from(diagnostic)
        })?;
        let mut manifest = parse_document(path, &document, source)?;
        manifest.source_entry =
            SourceTreeEntry::from_bytes(PortablePath::new("Arche.toml")?, source.as_bytes())?;
        Ok(manifest)
    }

    pub fn targets(&self) -> impl Iterator<Item = Target> + '_ {
        self.library
            .iter()
            .cloned()
            .map(Target::Library)
            .chain(self.binaries.iter().cloned().map(Target::Binary))
            .chain(self.environments.iter().cloned().map(Target::Environment))
    }

    pub fn workspace_members(&self) -> Option<&[PortablePath]> {
        self.workspace
            .as_ref()
            .map(|workspace| workspace.members.as_slice())
    }

    pub fn workspace_default_members(&self) -> Option<Option<&[PortablePath]>> {
        self.workspace
            .as_ref()
            .map(|workspace| workspace.default_members.as_deref())
    }

    pub const fn package_span(&self) -> Option<ManifestSpan> {
        self.package_span
    }

    pub fn target_span(&self, target: &Target) -> Option<ManifestSpan> {
        match target {
            Target::Library(candidate) if self.library.as_ref() == Some(candidate) => {
                self.library_span
            }
            Target::Binary(candidate) => self
                .binaries
                .iter()
                .position(|target| target == candidate)
                .and_then(|index| self.binary_spans.get(index).copied()),
            Target::Environment(candidate) => self
                .environments
                .iter()
                .position(|target| target == candidate)
                .and_then(|index| self.environment_spans.get(index).copied()),
            _ => None,
        }
    }
}

fn parse_document(
    path: &Path,
    document: &Document<&str>,
    source: &str,
) -> Result<Manifest, Diagnostics> {
    reject_unknown(
        path,
        document.as_table(),
        &[
            "schema",
            "package",
            "workspace",
            "const-eval",
            "lib",
            "bin",
            "environment",
            "environment-profile",
            "dependencies",
            "dev-dependencies",
        ],
    )?;
    let schema = required_integer(path, document.get("schema"), "schema")?;
    if schema != MANIFEST_SCHEMA {
        return Err(Diagnostic::new(
            DiagnosticCode::ManifestSchema,
            format!("unsupported Arche.toml schema {schema}; expected schema 1"),
        )
        .at_path(path)
        .into());
    }

    let package = optional_table(path, document.get("package"), "package")?
        .map(|table| parse_package(path, table))
        .transpose()?;
    let workspace = optional_table(path, document.get("workspace"), "workspace")?
        .map(|table| parse_workspace(path, table))
        .transpose()?;
    if package.is_none() && workspace.is_none() {
        return Err(Diagnostic::new(
            DiagnosticCode::ManifestValue,
            "manifest must declare `[package]`, `[workspace]`, or both",
        )
        .at_path(path)
        .into());
    }

    let const_eval_table = optional_table(path, document.get("const-eval"), "const-eval")?;
    let const_eval = parse_const_eval(path, const_eval_table, package.as_ref())?;
    let library = optional_table(path, document.get("lib"), "lib")?
        .map(|table| parse_library(path, table))
        .transpose()?;
    let binaries = parse_binaries(path, document.get("bin"))?;
    let environments = parse_environments(path, document.get("environment"))?;
    let environment_profiles =
        parse_environment_profiles(path, document.get("environment-profile"))?;
    let dependencies = parse_dependencies(path, document.get("dependencies"))?;
    let dev_dependencies = parse_dependencies(path, document.get("dev-dependencies"))?;
    for development in dev_dependencies.keys() {
        if let Some(normal) = dependencies
            .keys()
            .find(|normal| normal.casefold_key() == development.casefold_key())
        {
            return Err(Diagnostic::new(
                DiagnosticCode::ManifestValue,
                format!(
                    "dependency alias `{development}` conflicts with normal dependency alias `{normal}`"
                ),
            )
            .at_path(path)
            .into());
        }
    }

    if package.is_none()
        && (library.is_some()
            || !binaries.is_empty()
            || !environments.is_empty()
            || !dependencies.is_empty()
            || !dev_dependencies.is_empty()
            || const_eval_table.is_some())
    {
        return Err(Diagnostic::new(
            DiagnosticCode::ManifestValue,
            "a virtual workspace cannot declare targets, dependencies, or const-eval budgets",
        )
        .at_path(path)
        .into());
    }

    if package.is_some() && library.is_none() && binaries.is_empty() && environments.is_empty() {
        return Err(Diagnostic::new(
            DiagnosticCode::ManifestTarget,
            "a package must declare at least one explicit `[lib]`, `[[bin]]`, or `[[environment]]` target",
        )
        .at_path(path)
        .into());
    }

    if let Some(package) = &package {
        if package.publish.enabled {
            for dependency in dependencies.values().chain(dev_dependencies.values()) {
                if dependency.kind == DependencyKind::Path {
                    return Err(Diagnostic::new(
                        DiagnosticCode::ManifestValue,
                        format!(
                            "publishable package `{}` must give path dependency `{}` registry package and version fields",
                            package.name, dependency.alias
                        ),
                    )
                    .at_path(path)
                    .into());
                }
            }
        }
    }

    validate_targets_and_profiles(path, &binaries, &environments, &environment_profiles)?;
    let package_range =
        manifest_optional_item_range(path, document.get("package"), package.is_some(), "package")?;
    let library_range = manifest_optional_item_range(
        path,
        document.get("lib"),
        library.is_some(),
        "library target",
    )?;
    let binary_ranges = manifest_table_ranges(path, document.get("bin"), binaries.len(), "binary")?;
    let environment_ranges = manifest_table_ranges(
        path,
        document.get("environment"),
        environments.len(),
        "environment",
    )?;
    let position_index = ManifestPositionIndex::new(
        source,
        package_range
            .iter()
            .chain(library_range.iter())
            .chain(binary_ranges.iter())
            .chain(environment_ranges.iter()),
    );
    let package_span = package_range.map(|range| position_index.span(range));
    let library_span = library_range.map(|range| position_index.span(range));
    let binary_spans = binary_ranges
        .into_iter()
        .map(|range| position_index.span(range))
        .collect();
    let environment_spans = environment_ranges
        .into_iter()
        .map(|range| position_index.span(range))
        .collect();
    Ok(Manifest {
        path: path.to_path_buf(),
        source_entry: SourceTreeEntry::from_bytes(PortablePath::new("Arche.toml")?, &[])?,
        package,
        const_eval,
        library,
        binaries,
        environments,
        environment_profiles,
        dependencies,
        dev_dependencies,
        package_span,
        library_span,
        binary_spans,
        environment_spans,
        workspace,
    })
}

fn manifest_optional_item_range(
    path: &Path,
    item: Option<&Item>,
    expected: bool,
    authority: &str,
) -> Result<Option<Range<usize>>, Diagnostics> {
    let range = item.and_then(Item::span);
    if range.is_some() == expected {
        Ok(range)
    } else {
        Err(missing_manifest_span(path, authority))
    }
}

fn manifest_table_ranges(
    path: &Path,
    item: Option<&Item>,
    expected: usize,
    authority: &str,
) -> Result<Vec<Range<usize>>, Diagnostics> {
    let ranges = item
        .and_then(Item::as_array_of_tables)
        .map(|tables| {
            tables
                .iter()
                .map(|table| {
                    table
                        .span()
                        .ok_or_else(|| missing_manifest_span(path, authority))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    if ranges.len() != expected {
        return Err(missing_manifest_span(path, authority));
    }
    Ok(ranges)
}

fn missing_manifest_span(path: &Path, authority: &str) -> Diagnostics {
    Diagnostic::new(
        DiagnosticCode::IdentityInvalid,
        format!("parser did not retain the exact {authority} manifest span"),
    )
    .at_path(path)
    .into()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ManifestPosition {
    byte: u64,
    line: u64,
    column: u64,
}

struct ManifestPositionIndex {
    endpoints: Vec<usize>,
    positions: Vec<ManifestPosition>,
}

impl ManifestPositionIndex {
    fn new<'a>(source: &str, ranges: impl Iterator<Item = &'a Range<usize>>) -> Self {
        let mut endpoints = ranges
            .flat_map(|range| [range.start, range.end])
            .collect::<Vec<_>>();
        endpoints.sort_unstable();
        endpoints.dedup();
        for endpoint in &endpoints {
            assert!(
                source.is_char_boundary(*endpoint),
                "toml_edit returns in-bounds UTF-8 boundaries"
            );
        }

        let mut positions = Vec::with_capacity(endpoints.len());
        let mut endpoint_index = 0;
        let mut offset = 0_usize;
        let mut position = ManifestPosition {
            byte: 0,
            line: 1,
            column: 1,
        };
        while endpoints.get(endpoint_index) == Some(&offset) {
            positions.push(position);
            endpoint_index += 1;
        }
        for character in source.chars() {
            offset = offset
                .checked_add(character.len_utf8())
                .expect("manifest length fits usize");
            position.byte = position
                .byte
                .checked_add(u64::try_from(character.len_utf8()).expect("UTF-8 width fits u64"))
                .expect("manifest length was already checked as u64");
            if character == '\n' {
                position.line = position
                    .line
                    .checked_add(1)
                    .expect("manifest line count fits u64");
                position.column = 1;
            } else if character != '\r' {
                position.column = position
                    .column
                    .checked_add(1)
                    .expect("manifest column count fits u64");
            }
            while endpoints.get(endpoint_index) == Some(&offset) {
                positions.push(position);
                endpoint_index += 1;
            }
        }
        assert_eq!(
            endpoint_index,
            endpoints.len(),
            "toml_edit returns in-bounds UTF-8 boundaries"
        );
        Self {
            endpoints,
            positions,
        }
    }

    fn position(&self, byte: usize) -> ManifestPosition {
        let index = self
            .endpoints
            .binary_search(&byte)
            .expect("manifest span endpoint was indexed");
        self.positions[index]
    }

    fn span(&self, range: Range<usize>) -> ManifestSpan {
        let start = self.position(range.start);
        let end = self.position(range.end);
        ManifestSpan {
            start_byte: start.byte,
            end_byte: end.byte,
            start_line: start.line,
            start_column: start.column,
            end_line: end.line,
            end_column: end.column,
        }
    }
}

#[cfg(test)]
fn manifest_position(source: &str, byte: usize) -> (u64, u64, u64) {
    let range = byte..byte;
    let index = ManifestPositionIndex::new(source, std::iter::once(&range));
    let position = index.position(byte);
    (position.byte, position.line, position.column)
}

fn parse_package(path: &Path, table: &Table) -> Result<Package, Diagnostics> {
    reject_unknown(
        path,
        table,
        &["name", "version", "edition", "arche", "publish", "license"],
    )?;
    let name = required_string(path, table.get("name"), "package.name")?.parse()?;
    let version_text = required_string(path, table.get("version"), "package.version")?;
    let version = Version::parse(version_text).map_err(|error| {
        Diagnostic::new(
            DiagnosticCode::ManifestValue,
            format!("invalid package version `{version_text}`: {error}"),
        )
        .at_path(path)
    })?;
    if !version.build.is_empty() || version.to_string() != version_text {
        return Err(Diagnostic::new(
            DiagnosticCode::ManifestValue,
            "package.version must be canonical SemVer without build metadata",
        )
        .at_path(path)
        .into());
    }
    let edition = required_string(path, table.get("edition"), "package.edition")?;
    if edition != "2026" {
        return Err(Diagnostic::new(
            DiagnosticCode::ManifestValue,
            format!("unsupported Arche edition `{edition}`; expected `2026`"),
        )
        .at_path(path)
        .into());
    }
    let arche_text = required_string(path, table.get("arche"), "package.arche")?;
    let arche = VersionReq::parse(arche_text).map_err(|error| {
        Diagnostic::new(
            DiagnosticCode::ManifestValue,
            format!("invalid Arche toolchain requirement `{arche_text}`: {error}"),
        )
        .at_path(path)
    })?;
    let enabled = optional_bool(path, table.get("publish"), "package.publish")?.unwrap_or(false);
    let license =
        optional_string(path, table.get("license"), "package.license")?.map(str::to_owned);
    if let Some(expression) = &license {
        spdx::Expression::parse(expression).map_err(|error| {
            Diagnostic::new(
                DiagnosticCode::ManifestValue,
                format!("invalid SPDX license expression `{expression}`: {error}"),
            )
            .at_path(path)
        })?;
    }
    if enabled && license.is_none() {
        return Err(Diagnostic::new(
            DiagnosticCode::ManifestValue,
            "publishable packages must declare package.license",
        )
        .at_path(path)
        .into());
    }
    Ok(Package {
        name,
        version,
        edition: edition.to_owned(),
        arche,
        publish: PublishMetadata { enabled, license },
    })
}

fn parse_workspace(path: &Path, table: &Table) -> Result<WorkspaceDeclaration, Diagnostics> {
    reject_unknown(path, table, &["members", "default-members"])?;
    let members = parse_portable_path_array(path, table.get("members"), "workspace.members", true)?;
    if members.is_empty() {
        return Err(Diagnostic::new(
            DiagnosticCode::WorkspaceMember,
            "workspace.members cannot be empty",
        )
        .at_path(path)
        .into());
    }
    require_sorted_unique(path, "workspace.members", &members)?;
    for (index, parent) in members.iter().enumerate() {
        if parent.as_str() == "." {
            continue;
        }
        let prefix = format!("{}/", parent.as_str());
        if let Some(child) = members
            .iter()
            .skip(index + 1)
            .find(|candidate| candidate.as_str().starts_with(&prefix))
        {
            return Err(Diagnostic::new(
                DiagnosticCode::WorkspaceMember,
                format!(
                    "workspace member `{child}` is nested inside member `{parent}`; nested members are not supported"
                ),
            )
            .at_path(path)
            .into());
        }
    }
    let default_members = table
        .get("default-members")
        .map(|item| parse_portable_path_array(path, Some(item), "workspace.default-members", true))
        .transpose()?;
    if let Some(defaults) = &default_members {
        if defaults.is_empty() {
            return Err(Diagnostic::new(
                DiagnosticCode::WorkspaceMember,
                "workspace.default-members cannot be empty",
            )
            .at_path(path)
            .into());
        }
        require_sorted_unique(path, "workspace.default-members", defaults)?;
        for default in defaults {
            if members.binary_search(default).is_err() {
                return Err(Diagnostic::new(
                    DiagnosticCode::WorkspaceMember,
                    format!("default workspace member `{default}` is not in workspace.members"),
                )
                .at_path(path)
                .into());
            }
        }
    }
    Ok(WorkspaceDeclaration {
        members,
        default_members,
    })
}

fn parse_const_eval(
    path: &Path,
    table: Option<&Table>,
    package: Option<&Package>,
) -> Result<ConstEvalBudgets, Diagnostics> {
    let Some(table) = table else {
        if package.is_some_and(|package| package.publish.enabled) {
            return Err(Diagnostic::new(
                DiagnosticCode::ManifestValue,
                "publishable packages must explicitly pin `[const-eval]` budgets",
            )
            .at_path(path)
            .into());
        }
        return Ok(ConstEvalBudgets::default());
    };
    reject_unknown(path, table, &["steps", "call-depth", "heap-bytes"])?;
    let defaults = ConstEvalBudgets::default();
    let require_all = package.is_some_and(|package| package.publish.enabled);
    let steps = budget(
        path,
        table.get("steps"),
        "const-eval.steps",
        defaults.steps,
        require_all,
    )?;
    let call_depth = budget(
        path,
        table.get("call-depth"),
        "const-eval.call-depth",
        defaults.call_depth,
        require_all,
    )?;
    let heap_bytes = budget(
        path,
        table.get("heap-bytes"),
        "const-eval.heap-bytes",
        defaults.heap_bytes,
        require_all,
    )?;
    Ok(ConstEvalBudgets {
        steps,
        call_depth,
        heap_bytes,
    })
}

fn budget(
    path: &Path,
    item: Option<&Item>,
    field: &str,
    default: u64,
    required: bool,
) -> Result<u64, Diagnostics> {
    if item.is_none() && !required {
        return Ok(default);
    }
    let value = required_integer(path, item, field)?;
    u64::try_from(value)
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::ManifestValue,
                format!("{field} must be a positive u64"),
            )
            .at_path(path)
            .into()
        })
}

fn parse_library(path: &Path, table: &Table) -> Result<LibTarget, Diagnostics> {
    reject_unknown(path, table, &["path"])?;
    let target_path =
        optional_string(path, table.get("path"), "lib.path")?.unwrap_or("src/lib.arc");
    Ok(LibTarget {
        path: PortablePath::new(target_path)?,
    })
}

fn parse_binaries(path: &Path, item: Option<&Item>) -> Result<Vec<BinaryTarget>, Diagnostics> {
    let Some(item) = item else {
        return Ok(Vec::new());
    };
    let tables = item.as_array_of_tables().ok_or_else(|| {
        Diagnostics::from(
            Diagnostic::new(
                DiagnosticCode::ManifestTarget,
                "`bin` must use `[[bin]]` tables",
            )
            .at_path(path),
        )
    })?;
    let count = tables.len();
    tables
        .iter()
        .map(|table| {
            reject_unknown(path, table, &["name", "path", "world", "capabilities"])?;
            let name =
                SourceIdentifier::new(required_string(path, table.get("name"), "bin.name")?)?;
            let target_path = match optional_string(path, table.get("path"), "bin.path")? {
                Some(value) => PortablePath::new(value)?,
                None if count == 1 => PortablePath::new("src/main.arc")?,
                None => {
                    return Err(Diagnostic::new(
                        DiagnosticCode::ManifestTarget,
                        "each of multiple binary targets must declare a path",
                    )
                    .at_path(path)
                    .into())
                }
            };
            let world = required_package_item_path(path, table.get("world"), "bin.world")?;
            let capabilities = parse_capabilities(path, table.get("capabilities"))?;
            Ok(BinaryTarget {
                name,
                path: target_path,
                world,
                capabilities,
            })
        })
        .collect()
}

fn parse_environments(
    path: &Path,
    item: Option<&Item>,
) -> Result<Vec<EnvironmentTarget>, Diagnostics> {
    let Some(item) = item else {
        return Ok(Vec::new());
    };
    let tables = item.as_array_of_tables().ok_or_else(|| {
        Diagnostics::from(
            Diagnostic::new(
                DiagnosticCode::ManifestTarget,
                "`environment` must use `[[environment]]` tables",
            )
            .at_path(path),
        )
    })?;
    tables
        .iter()
        .map(|table| {
            reject_unknown(path, table, &["name", "path", "world", "profile"])?;
            Ok(EnvironmentTarget {
                name: SourceIdentifier::new(required_string(
                    path,
                    table.get("name"),
                    "environment.name",
                )?)?,
                path: PortablePath::new(required_string(
                    path,
                    table.get("path"),
                    "environment.path",
                )?)?,
                world: required_package_item_path(path, table.get("world"), "environment.world")?,
                profile: SourceIdentifier::new(required_string(
                    path,
                    table.get("profile"),
                    "environment.profile",
                )?)?,
            })
        })
        .collect()
}

fn parse_environment_profiles(
    path: &Path,
    item: Option<&Item>,
) -> Result<BTreeMap<SourceIdentifier, EnvironmentProfile>, Diagnostics> {
    let Some(item) = item else {
        return Ok(BTreeMap::new());
    };
    let table = item.as_table().ok_or_else(|| {
        Diagnostics::from(
            Diagnostic::new(
                DiagnosticCode::ManifestTarget,
                "environment profiles must use `[environment-profile.NAME]` tables",
            )
            .at_path(path),
        )
    })?;
    let mut profiles = BTreeMap::new();
    let mut casefold = BTreeMap::<String, String>::new();
    for (raw_name, item) in table.iter() {
        let name = SourceIdentifier::new(raw_name)?;
        reject_casefold_alias(path, "environment profile", &name, &mut casefold)?;
        let profile_table = item.as_table().ok_or_else(|| {
            Diagnostics::from(
                Diagnostic::new(
                    DiagnosticCode::ManifestTarget,
                    format!("environment profile `{raw_name}` must be an explicit table"),
                )
                .at_path(path),
            )
        })?;
        reject_unknown(path, profile_table, &["reset", "step", "self-play"])?;
        let profile = EnvironmentProfile {
            name: name.clone(),
            reset: required_package_item_path(
                path,
                profile_table.get("reset"),
                "environment-profile.reset",
            )?,
            step: required_package_item_path(
                path,
                profile_table.get("step"),
                "environment-profile.step",
            )?,
            self_play: required_package_item_path(
                path,
                profile_table.get("self-play"),
                "environment-profile.self-play",
            )?,
        };
        profiles.insert(name, profile);
    }
    Ok(profiles)
}

fn parse_dependencies(
    path: &Path,
    item: Option<&Item>,
) -> Result<BTreeMap<DependencyAlias, Dependency>, Diagnostics> {
    let Some(item) = item else {
        return Ok(BTreeMap::new());
    };
    let table = item.as_table().ok_or_else(|| {
        Diagnostics::from(
            Diagnostic::new(
                DiagnosticCode::ManifestValue,
                "dependencies must use explicit `[dependencies.ALIAS]` tables",
            )
            .at_path(path),
        )
    })?;
    let mut output = BTreeMap::new();
    let mut casefold = BTreeMap::<String, String>::new();
    for (raw_alias, item) in table.iter() {
        let alias = SourceIdentifier::new(raw_alias)?;
        reject_casefold_alias(path, "dependency alias", &alias, &mut casefold)?;
        let (package_text, version_text, path_text) = if let Some(dependency_table) =
            item.as_table()
        {
            reject_unknown(path, dependency_table, &["package", "version", "path"])?;
            (
                optional_string(path, dependency_table.get("package"), "dependency.package")?,
                optional_string(path, dependency_table.get("version"), "dependency.version")?,
                optional_string(path, dependency_table.get("path"), "dependency.path")?,
            )
        } else if let Some(dependency_table) = item.as_value().and_then(Value::as_inline_table) {
            reject_unknown_inline(path, dependency_table, &["package", "version", "path"])?;
            (
                optional_inline_string(path, dependency_table, "package")?,
                optional_inline_string(path, dependency_table, "version")?,
                optional_inline_string(path, dependency_table, "path")?,
            )
        } else {
            return Err(Diagnostic::new(
                DiagnosticCode::ManifestValue,
                format!("dependency `{raw_alias}` must be a table with explicit fields"),
            )
            .at_path(path)
            .into());
        };
        let package = package_text.map(PackageName::from_str).transpose()?;
        let requirement = version_text
            .map(|value| {
                VersionReq::parse(value).map_err(|error| {
                    Diagnostics::from(
                        Diagnostic::new(
                            DiagnosticCode::ManifestValue,
                            format!("invalid dependency requirement `{value}`: {error}"),
                        )
                        .at_path(path),
                    )
                })
            })
            .transpose()?;
        let dependency_path = path_text.map(DependencyPath::new).transpose()?;
        let kind = match (&package, &requirement, &dependency_path) {
            (Some(_), Some(_), None) => DependencyKind::Registry,
            (None, None, Some(_)) => DependencyKind::Path,
            (Some(_), Some(_), Some(_)) => DependencyKind::PublishCompatiblePath,
            _ => {
                return Err(Diagnostic::new(
                    DiagnosticCode::ManifestValue,
                    format!(
                        "dependency `{raw_alias}` must contain package+version, path, or package+version+path"
                    ),
                )
                .at_path(path)
                .into())
            }
        };
        let dependency = Dependency {
            alias: alias.clone(),
            kind,
            package,
            requirement,
            path: dependency_path,
        };
        output.insert(alias, dependency);
    }
    Ok(output)
}

fn parse_capabilities(
    path: &Path,
    item: Option<&Item>,
) -> Result<BTreeSet<Capability>, Diagnostics> {
    let Some(item) = item else {
        return Ok(BTreeSet::new());
    };
    let array = item.as_array().ok_or_else(|| {
        Diagnostics::from(
            Diagnostic::new(
                DiagnosticCode::ManifestValue,
                "bin.capabilities must be an array of strings",
            )
            .at_path(path),
        )
    })?;
    let mut previous: Option<&str> = None;
    let mut output = BTreeSet::new();
    for value in array.iter() {
        let text = value.as_str().ok_or_else(|| {
            Diagnostics::from(
                Diagnostic::new(
                    DiagnosticCode::ManifestValue,
                    "bin.capabilities entries must be strings",
                )
                .at_path(path),
            )
        })?;
        if previous.is_some_and(|previous| previous >= text) {
            return Err(Diagnostic::new(
                DiagnosticCode::ManifestValue,
                "bin.capabilities must be strictly sorted and unique",
            )
            .at_path(path)
            .into());
        }
        previous = Some(text);
        output.insert(text.parse()?);
    }
    Ok(output)
}

fn validate_targets_and_profiles(
    path: &Path,
    binaries: &[BinaryTarget],
    environments: &[EnvironmentTarget],
    profiles: &BTreeMap<SourceIdentifier, EnvironmentProfile>,
) -> Result<(), Diagnostics> {
    let mut names = BTreeMap::<String, String>::new();
    for name in binaries
        .iter()
        .map(|target| &target.name)
        .chain(environments.iter().map(|target| &target.name))
    {
        reject_casefold_alias(path, "target", name, &mut names)?;
    }
    for environment in environments {
        if !profiles.contains_key(&environment.profile) {
            return Err(Diagnostic::new(
                DiagnosticCode::ManifestTarget,
                format!(
                    "environment `{}` selects missing profile `{}`",
                    environment.name, environment.profile
                ),
            )
            .at_path(path)
            .into());
        }
    }
    let used = environments
        .iter()
        .map(|target| &target.profile)
        .collect::<BTreeSet<_>>();
    if let Some(unused) = profiles.keys().find(|profile| !used.contains(profile)) {
        return Err(Diagnostic::new(
            DiagnosticCode::ManifestTarget,
            format!("environment profile `{unused}` is not selected by a target"),
        )
        .at_path(path)
        .into());
    }
    Ok(())
}

fn reject_casefold_alias(
    path: &Path,
    kind: &str,
    identifier: &SourceIdentifier,
    seen: &mut BTreeMap<String, String>,
) -> Result<(), Diagnostics> {
    let key = identifier.casefold_key();
    if let Some(previous) = seen.insert(key, identifier.as_str().to_owned()) {
        return Err(Diagnostic::new(
            DiagnosticCode::ManifestValue,
            format!(
                "{kind} `{}` case-folds to the same name as `{previous}`",
                identifier
            ),
        )
        .at_path(path)
        .into());
    }
    Ok(())
}

fn parse_portable_path_array(
    path: &Path,
    item: Option<&Item>,
    field: &str,
    workspace_member: bool,
) -> Result<Vec<PortablePath>, Diagnostics> {
    let array = required_array(path, item, field)?;
    array
        .iter()
        .map(|value| {
            let value = value.as_str().ok_or_else(|| {
                Diagnostics::from(
                    Diagnostic::new(
                        DiagnosticCode::ManifestValue,
                        format!("{field} entries must be strings"),
                    )
                    .at_path(path),
                )
            })?;
            if workspace_member {
                PortablePath::workspace_member(value)
            } else {
                PortablePath::new(value)
            }
        })
        .collect()
}

fn require_sorted_unique<T: Ord + std::fmt::Display>(
    path: &Path,
    field: &str,
    values: &[T],
) -> Result<(), Diagnostics> {
    for pair in values.windows(2) {
        if pair[0] >= pair[1] {
            return Err(Diagnostic::new(
                DiagnosticCode::WorkspaceMember,
                format!(
                    "{field} must be strictly sorted and unique near `{}`",
                    pair[1]
                ),
            )
            .at_path(path)
            .into());
        }
    }
    Ok(())
}

fn reject_unknown(path: &Path, table: &Table, allowed: &[&str]) -> Result<(), Diagnostics> {
    for (key, item) in table.iter() {
        if !allowed.contains(&key) {
            let mut diagnostic = Diagnostic::new(
                DiagnosticCode::ManifestUnknown,
                format!("unknown manifest field `{key}`"),
            );
            if let Some(span) = item.span() {
                diagnostic = diagnostic.at_span(path, span.start, span.end);
            } else {
                diagnostic = diagnostic.at_path(path);
            }
            return Err(diagnostic.into());
        }
    }
    Ok(())
}

fn reject_unknown_inline(
    path: &Path,
    table: &InlineTable,
    allowed: &[&str],
) -> Result<(), Diagnostics> {
    for (key, value) in table.iter() {
        if !allowed.contains(&key) {
            let mut diagnostic = Diagnostic::new(
                DiagnosticCode::ManifestUnknown,
                format!("unknown manifest field `{key}`"),
            );
            if let Some(span) = value.span() {
                diagnostic = diagnostic.at_span(path, span.start, span.end);
            } else {
                diagnostic = diagnostic.at_path(path);
            }
            return Err(diagnostic.into());
        }
    }
    Ok(())
}

fn optional_inline_string<'a>(
    path: &Path,
    table: &'a InlineTable,
    key: &str,
) -> Result<Option<&'a str>, Diagnostics> {
    match table.get(key) {
        None => Ok(None),
        Some(value) => value.as_str().map(Some).ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::ManifestValue,
                format!("dependency.{key} must be a string"),
            )
            .at_path(path)
            .into()
        }),
    }
}

fn optional_table<'a>(
    path: &Path,
    item: Option<&'a Item>,
    field: &str,
) -> Result<Option<&'a Table>, Diagnostics> {
    item.map(|item| {
        item.as_table().ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::ManifestValue,
                format!("`{field}` must be a table"),
            )
            .at_path(path)
            .into()
        })
    })
    .transpose()
}

fn required_string<'a>(
    path: &Path,
    item: Option<&'a Item>,
    field: &str,
) -> Result<&'a str, Diagnostics> {
    optional_string(path, item, field)?.ok_or_else(|| {
        Diagnostic::new(
            DiagnosticCode::ManifestValue,
            format!("missing required string `{field}`"),
        )
        .at_path(path)
        .into()
    })
}

fn required_package_item_path(
    path: &Path,
    item: Option<&Item>,
    field: &str,
) -> Result<ItemPath, Diagnostics> {
    let raw = required_string(path, item, field)?;
    let item_path = ItemPath::parse(raw)?;
    if !matches!(item_path.root(), crate::ItemPathRoot::Package) {
        return Err(Diagnostic::new(
            DiagnosticCode::ManifestTarget,
            format!("`{field}` must use an explicit `package::` path"),
        )
        .at_path(path)
        .into());
    }
    Ok(item_path)
}

fn optional_string<'a>(
    path: &Path,
    item: Option<&'a Item>,
    field: &str,
) -> Result<Option<&'a str>, Diagnostics> {
    item.map(|item| {
        item.as_str().ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::ManifestValue,
                format!("`{field}` must be a string"),
            )
            .at_path(path)
            .into()
        })
    })
    .transpose()
}

fn required_integer(path: &Path, item: Option<&Item>, field: &str) -> Result<i64, Diagnostics> {
    item.and_then(Item::as_integer).ok_or_else(|| {
        Diagnostic::new(
            DiagnosticCode::ManifestValue,
            format!("missing or non-integer `{field}`"),
        )
        .at_path(path)
        .into()
    })
}

fn optional_bool(
    path: &Path,
    item: Option<&Item>,
    field: &str,
) -> Result<Option<bool>, Diagnostics> {
    item.map(|item| {
        item.as_bool().ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::ManifestValue,
                format!("`{field}` must be a boolean"),
            )
            .at_path(path)
            .into()
        })
    })
    .transpose()
}

fn required_array<'a>(
    path: &Path,
    item: Option<&'a Item>,
    field: &str,
) -> Result<&'a Array, Diagnostics> {
    item.and_then(Item::as_array).ok_or_else(|| {
        Diagnostic::new(
            DiagnosticCode::ManifestValue,
            format!("missing or non-array `{field}`"),
        )
        .at_path(path)
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE: &str = r#"
schema = 1

[package]
name = "example/game"
version = "0.1.0"
edition = "2026"
arche = "^0.1"
publish = false

[lib]

[[bin]]
name = "server"
world = "package::ServerWorld"
capabilities = ["stdio", "udp"]

[[environment]]
name = "training"
path = "src/training.arc"
world = "package::TrainingWorld"
profile = "training"

[environment-profile.training]
reset = "package::Reset"
step = "package::Step"
self-play = "package::SelfPlay"

[dependencies.math]
package = "arche/math"
version = "^0.1"

[dev-dependencies.fixture]
path = "packages/fixture"
"#;

    #[test]
    fn parses_complete_schema_one_manifest() {
        let manifest = Manifest::parse(Path::new("Arche.toml"), PACKAGE).unwrap();
        assert_eq!(
            manifest.package.as_ref().unwrap().name.as_str(),
            "example/game"
        );
        assert_eq!(
            manifest.library.as_ref().unwrap().path.as_str(),
            "src/lib.arc"
        );
        assert_eq!(manifest.binaries[0].path.as_str(), "src/main.arc");
        assert_eq!(manifest.environments.len(), 1);
        assert_eq!(manifest.dependencies.len(), 1);
        assert_eq!(manifest.dev_dependencies.len(), 1);
    }

    #[test]
    fn retains_exact_package_and_target_table_spans() {
        let manifest = Manifest::parse(Path::new("Arche.toml"), PACKAGE).unwrap();
        let package = manifest.package_span().unwrap();
        let library = manifest
            .target_span(&Target::Library(manifest.library.clone().unwrap()))
            .unwrap();
        let binary = manifest
            .target_span(&Target::Binary(manifest.binaries[0].clone()))
            .unwrap();
        let environment = manifest
            .target_span(&Target::Environment(manifest.environments[0].clone()))
            .unwrap();

        let text = |span: ManifestSpan| {
            &PACKAGE
                [usize::try_from(span.start_byte).unwrap()..usize::try_from(span.end_byte).unwrap()]
        };
        assert_eq!(text(package), "[package]");
        assert_eq!(text(library), "[lib]");
        assert_eq!(text(binary), "[[bin]]");
        assert_eq!(text(environment), "[[environment]]");
        assert_eq!(
            package.start_byte,
            PACKAGE.find("[package]").unwrap() as u64
        );
        assert_eq!(library.start_byte, PACKAGE.find("[lib]").unwrap() as u64);
        assert_eq!(binary.start_byte, PACKAGE.find("[[bin]]").unwrap() as u64);
        assert_eq!(
            environment.start_byte,
            PACKAGE.find("[[environment]]").unwrap() as u64
        );
    }

    #[test]
    fn manifest_spans_pin_unicode_tabs_crlf_and_bare_cr_coordinates() {
        let source = concat!(
            "# π\tmanifest\r\n",
            "schema = 1\r\n",
            "\r\n",
            "[package]\r\n",
            "name = \"example/coordinates\"\r\n",
            "version = \"0.1.0\"\r\n",
            "edition = \"2026\"\r\n",
            "arche = \">=0.0.0\"\r\n",
            "publish = false\r\n",
            "\r\n",
            "[lib]\r\n",
            "path = \"src/lib.arc\"\r\n",
        );
        let manifest = Manifest::parse(Path::new("Arche.toml"), source).unwrap();
        let start = source.find("[package]").unwrap();
        assert_eq!(
            manifest.package_span(),
            Some(ManifestSpan {
                start_byte: u64::try_from(start).unwrap(),
                end_byte: u64::try_from(start + "[package]".len()).unwrap(),
                start_line: 4,
                start_column: 1,
                end_line: 4,
                end_column: 10,
            })
        );

        let mixed_newlines = "α\t\rβ\nγ";
        assert_eq!(manifest_position(mixed_newlines, "α".len()), (2, 1, 2));
        assert_eq!(
            manifest_position(mixed_newlines, "α\t\r".len()),
            (4, 1, 3),
            "tabs advance one scalar column while a bare carriage return advances bytes only"
        );
        assert_eq!(
            manifest_position(mixed_newlines, "α\t\rβ\n".len()),
            (7, 2, 1)
        );
    }

    #[test]
    fn manifest_position_index_resolves_many_unsorted_unicode_endpoints() {
        let mut source = String::new();
        let mut ranges = Vec::new();
        for ordinal in 0..256 {
            source.push_str("π\t");
            let start = source.len();
            source.push_str("[[bin]]");
            let end = source.len();
            ranges.push(start..end);
            if ordinal % 17 == 0 {
                ranges.push(start..end);
            }
            match ordinal % 3 {
                0 => source.push_str("\r\n"),
                1 => source.push_str("\rβ\n"),
                _ => source.push_str("終\t\n"),
            }
        }
        ranges.reverse();

        let index = ManifestPositionIndex::new(&source, ranges.iter());
        let reference = |byte: usize| {
            let mut position = ManifestPosition {
                byte: 0,
                line: 1,
                column: 1,
            };
            for character in source[..byte].chars() {
                position.byte += u64::try_from(character.len_utf8()).unwrap();
                if character == '\n' {
                    position.line += 1;
                    position.column = 1;
                } else if character != '\r' {
                    position.column += 1;
                }
            }
            position
        };

        for range in ranges {
            let start = reference(range.start);
            let end = reference(range.end);
            assert_eq!(
                index.span(range),
                ManifestSpan {
                    start_byte: start.byte,
                    end_byte: end.byte,
                    start_line: start.line,
                    start_column: start.column,
                    end_line: end.line,
                    end_column: end.column,
                }
            );
        }
    }

    #[test]
    fn rejects_unknown_and_shorthand_dependency_fields() {
        let unknown = PACKAGE.replace("publish = false", "publish = false\nfeatures = []");
        assert!(Manifest::parse(Path::new("Arche.toml"), &unknown).is_err());
        let shorthand = PACKAGE.replace(
            "[dependencies.math]\npackage = \"arche/math\"\nversion = \"^0.1\"",
            "[dependencies]\nmath = \"^0.1\"",
        );
        assert!(Manifest::parse(Path::new("Arche.toml"), &shorthand).is_err());

        let repeated = PACKAGE.replace(
            "[dev-dependencies.fixture]\npath = \"packages/fixture\"",
            "[dev-dependencies.Math]\npath = \"packages/fixture\"",
        );
        assert!(Manifest::parse(Path::new("Arche.toml"), &repeated).is_err());
    }

    #[test]
    fn dependency_aliases_reject_reserved_migration_keywords() {
        let reserved = PACKAGE.replace("[dependencies.math]", "[dependencies.startup]");
        assert!(Manifest::parse(Path::new("Arche.toml"), &reserved).is_err());
    }

    #[test]
    fn target_links_require_explicit_package_roots() {
        let dependency_world = PACKAGE.replace(
            "world = \"package::ServerWorld\"",
            "world = \"shared::ServerWorld\"",
        );
        assert!(Manifest::parse(Path::new("Arche.toml"), &dependency_world).is_err());
    }

    #[test]
    fn published_packages_pin_budgets_and_publishable_paths() {
        let published = PACKAGE.replace("publish = false", "publish = true\nlicense = \"MIT\"");
        assert!(Manifest::parse(Path::new("Arche.toml"), &published).is_err());
    }

    #[test]
    fn workspace_lists_are_sorted_and_defaults_are_members() {
        let valid = r#"
schema = 1
[workspace]
members = [".", "packages/a"]
default-members = ["."]
"#;
        assert!(Manifest::parse(Path::new("Arche.toml"), valid).is_ok());
        let invalid = valid.replace("[\".\", \"packages/a\"]", "[\"packages/a\", \".\"]");
        assert!(Manifest::parse(Path::new("Arche.toml"), &invalid).is_err());
    }

    #[test]
    fn workspace_members_cannot_nest_except_beneath_the_combined_root() {
        let nested = concat!(
            "schema = 1\n",
            "[workspace]\n",
            "members = [\"packages\", \"packages/app\"]\n",
        );
        assert!(Manifest::parse(Path::new("Arche.toml"), nested).is_err());

        let combined = concat!(
            "schema = 1\n",
            "[package]\n",
            "name = \"example/root\"\n",
            "version = \"0.1.0\"\n",
            "edition = \"2026\"\n",
            "arche = \">=0.0.0\"\n",
            "[workspace]\n",
            "members = [\".\", \"packages/app\"]\n",
            "[lib]\n",
        );
        assert!(Manifest::parse(Path::new("Arche.toml"), combined).is_ok());
    }
}
