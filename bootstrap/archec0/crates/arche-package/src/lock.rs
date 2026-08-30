use crate::atomic::publish_if_changed;
use crate::diagnostic::{Diagnostic, DiagnosticCode, Diagnostics};
use crate::{DependencyAlias, IntegrityDigest, PackageName, PortablePath, SourceIdentifier};
use semver::{Version, VersionReq};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::path::Path;
use std::str::FromStr;
use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

const LOCK_SCHEMA: i64 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolchainLock {
    pub version: Version,
    pub release_manifest_digest: IntegrityDigest,
}

impl ToolchainLock {
    pub fn bootstrap_current() -> Self {
        Self {
            version: Version::new(0, 0, 0),
            release_manifest_digest: IntegrityDigest::of_bytes(
                b"ARCHE-BOOTSTRAP-RELEASE-MANIFEST\0\x01\x00\x00\x00arche-0.0.0-rust-seed",
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryLock {
    pub identity: String,
    pub snapshot_digest: IntegrityDigest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceLock {
    pub source_digest: IntegrityDigest,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LockDependencyKind {
    Normal,
    Development,
}

impl LockDependencyKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Development => "development",
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DependencyRequirement {
    canonical: String,
    parsed: VersionReq,
}

impl DependencyRequirement {
    pub fn from_version_req(requirement: &VersionReq) -> Self {
        Self {
            canonical: requirement.to_string(),
            parsed: requirement.clone(),
        }
    }

    pub fn any() -> Self {
        Self::from_version_req(&VersionReq::STAR)
    }

    pub fn exact(version: &Version) -> Self {
        let parsed = VersionReq::parse(&format!("={version}"))
            .expect("a canonical package version forms an exact requirement");
        Self::from_version_req(&parsed)
    }

    pub fn as_str(&self) -> &str {
        &self.canonical
    }

    pub fn matches(&self, version: &Version) -> bool {
        self.parsed.matches(version)
    }

    fn parse_canonical(value: &str) -> Result<Self, Diagnostics> {
        let parsed = VersionReq::parse(value).map_err(|error| {
            lock_error(format!("invalid dependency requirement `{value}`: {error}"))
        })?;
        let requirement = Self::from_version_req(&parsed);
        if requirement.as_str() != value {
            return Err(lock_error(format!(
                "dependency requirement `{value}` is not canonical; use `{requirement}`"
            )));
        }
        Ok(requirement)
    }
}

impl std::fmt::Display for DependencyRequirement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Ord for DependencyRequirement {
    fn cmp(&self, other: &Self) -> Ordering {
        self.canonical.cmp(&other.canonical)
    }
}

impl PartialOrd for DependencyRequirement {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LockDependency {
    pub alias: DependencyAlias,
    pub package: PackageName,
    pub requirement: DependencyRequirement,
    pub kind: LockDependencyKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LockSource {
    Workspace {
        path: PortablePath,
        source_digest: IntegrityDigest,
    },
    Registry {
        archive_digest: IntegrityDigest,
        source_digest: IntegrityDigest,
        provenance_record_digest: IntegrityDigest,
        inclusion_record_digest: IntegrityDigest,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockPackage {
    pub name: PackageName,
    pub version: Version,
    pub source: LockSource,
    pub dependencies: Vec<LockDependency>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lockfile {
    pub toolchain: ToolchainLock,
    pub workspace: WorkspaceLock,
    pub registry: RegistryLock,
    pub packages: Vec<LockPackage>,
}

impl Lockfile {
    pub fn new(
        toolchain: ToolchainLock,
        workspace: WorkspaceLock,
        registry: RegistryLock,
        mut packages: Vec<LockPackage>,
    ) -> Result<Self, Diagnostics> {
        packages.sort_by(|left, right| left.name.cmp(&right.name));
        for package in &mut packages {
            package.dependencies.sort();
        }
        let lock = Self {
            toolchain,
            workspace,
            registry,
            packages,
        };
        lock.validate()?;
        Ok(lock)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, Diagnostics> {
        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) || bytes.contains(&b'\r') {
            return Err(lock_error(
                "lock must be UTF-8 without BOM and use LF endings",
            ));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| lock_error("lock is not valid UTF-8"))?;
        let document = text
            .parse::<DocumentMut>()
            .map_err(|error| lock_error(format!("invalid lock TOML: {error}")))?;
        ensure_keys(
            document.as_table(),
            &["schema", "toolchain", "workspace", "registry", "package"],
            "lock",
        )?;
        if required_integer(document.as_table(), "schema")? != LOCK_SCHEMA {
            return Err(lock_error("unsupported Arche.lock schema; expected 1"));
        }
        let toolchain_table = required_table(document.as_table(), "toolchain")?;
        ensure_keys(
            toolchain_table,
            &["version", "release-manifest-digest"],
            "toolchain",
        )?;
        let toolchain = ToolchainLock {
            version: parse_version(required_string(toolchain_table, "version")?)?,
            release_manifest_digest: required_string(toolchain_table, "release-manifest-digest")?
                .parse()?,
        };
        let workspace_table = required_table(document.as_table(), "workspace")?;
        ensure_keys(workspace_table, &["source-digest"], "workspace")?;
        let workspace = WorkspaceLock {
            source_digest: required_string(workspace_table, "source-digest")?.parse()?,
        };
        let registry_table = required_table(document.as_table(), "registry")?;
        ensure_keys(registry_table, &["identity", "snapshot-digest"], "registry")?;
        let registry = RegistryLock {
            identity: required_string(registry_table, "identity")?.to_owned(),
            snapshot_digest: required_string(registry_table, "snapshot-digest")?.parse()?,
        };
        let array = document
            .as_table()
            .get("package")
            .and_then(Item::as_array_of_tables)
            .ok_or_else(|| lock_error("lock requires one or more [[package]] rows"))?;
        let mut packages = Vec::with_capacity(array.len());
        for table in array {
            packages.push(parse_package(table)?);
        }
        let lock = Self {
            toolchain,
            workspace,
            registry,
            packages,
        };
        lock.validate()?;
        let canonical = lock.to_canonical_string()?;
        if canonical.as_bytes() != bytes {
            return Err(lock_error("lock is valid but not canonically encoded"));
        }
        Ok(lock)
    }

    pub fn to_canonical_string(&self) -> Result<String, Diagnostics> {
        self.validate()?;
        let mut output = String::new();
        writeln!(output, "schema = 1").unwrap();
        writeln!(output).unwrap();
        writeln!(output, "[toolchain]").unwrap();
        writeln!(
            output,
            "version = {}",
            quoted(&self.toolchain.version.to_string())
        )
        .unwrap();
        writeln!(
            output,
            "release-manifest-digest = {}",
            quoted(&self.toolchain.release_manifest_digest.to_string())
        )
        .unwrap();
        writeln!(output).unwrap();
        writeln!(output, "[workspace]").unwrap();
        writeln!(
            output,
            "source-digest = {}",
            quoted(&self.workspace.source_digest.to_string())
        )
        .unwrap();
        writeln!(output).unwrap();
        writeln!(output, "[registry]").unwrap();
        writeln!(output, "identity = {}", quoted(&self.registry.identity)).unwrap();
        writeln!(
            output,
            "snapshot-digest = {}",
            quoted(&self.registry.snapshot_digest.to_string())
        )
        .unwrap();
        for package in &self.packages {
            writeln!(output).unwrap();
            writeln!(output, "[[package]]").unwrap();
            writeln!(output, "name = {}", quoted(package.name.as_str())).unwrap();
            writeln!(output, "version = {}", quoted(&package.version.to_string())).unwrap();
            match &package.source {
                LockSource::Workspace {
                    path,
                    source_digest,
                } => {
                    writeln!(output, "source = \"workspace\"").unwrap();
                    writeln!(output, "path = {}", quoted(path.as_str())).unwrap();
                    writeln!(
                        output,
                        "source-digest = {}",
                        quoted(&source_digest.to_string())
                    )
                    .unwrap();
                }
                LockSource::Registry {
                    archive_digest,
                    source_digest,
                    provenance_record_digest,
                    inclusion_record_digest,
                } => {
                    writeln!(output, "source = \"registry\"").unwrap();
                    writeln!(
                        output,
                        "archive-digest = {}",
                        quoted(&archive_digest.to_string())
                    )
                    .unwrap();
                    writeln!(
                        output,
                        "source-digest = {}",
                        quoted(&source_digest.to_string())
                    )
                    .unwrap();
                    writeln!(
                        output,
                        "provenance-record-digest = {}",
                        quoted(&provenance_record_digest.to_string())
                    )
                    .unwrap();
                    writeln!(
                        output,
                        "inclusion-record-digest = {}",
                        quoted(&inclusion_record_digest.to_string())
                    )
                    .unwrap();
                }
            }
            if package.dependencies.is_empty() {
                writeln!(output, "dependencies = []").unwrap();
            } else {
                writeln!(output, "dependencies = [").unwrap();
                for dependency in &package.dependencies {
                    writeln!(
                        output,
                        "  {{ alias = {}, package = {}, requirement = {}, kind = {} }},",
                        quoted(dependency.alias.as_str()),
                        quoted(dependency.package.as_str()),
                        quoted(dependency.requirement.as_str()),
                        quoted(dependency.kind.as_str())
                    )
                    .unwrap();
                }
                writeln!(output, "]").unwrap();
            }
        }
        Ok(output)
    }

    pub fn publish_atomic(&self, path: &Path) -> Result<bool, Diagnostics> {
        let canonical = self.to_canonical_string()?;
        publish_if_changed(path, canonical.as_bytes())
    }

    fn validate(&self) -> Result<(), Diagnostics> {
        if !self.toolchain.version.build.is_empty() {
            return Err(lock_error(
                "toolchain version contains forbidden build metadata",
            ));
        }
        if self.registry.identity != crate::OFFICIAL_REGISTRY_IDENTITY {
            return Err(lock_error(format!(
                "lock registry identity must be `{}`",
                crate::OFFICIAL_REGISTRY_IDENTITY
            )));
        }
        if self.packages.is_empty() {
            return Err(lock_error("lock must contain at least one package"));
        }
        let mut names = BTreeSet::new();
        let mut rows = BTreeMap::new();
        let mut workspace_paths = BTreeSet::new();
        let mut folded_workspace_paths = BTreeMap::<String, String>::new();
        for package in &self.packages {
            if !package.version.build.is_empty() {
                return Err(lock_error(format!(
                    "package `{}` version contains forbidden build metadata",
                    package.name
                )));
            }
            if !names.insert(package.name.clone()) {
                return Err(lock_error(format!("duplicate package `{}`", package.name)));
            }
            if let LockSource::Workspace { path, .. } = &package.source {
                if !workspace_paths.insert(path.clone()) {
                    return Err(lock_error(format!(
                        "workspace path `{path}` is assigned to more than one package"
                    )));
                }
                let folded = path.casefold_key();
                if let Some(previous) =
                    folded_workspace_paths.insert(folded, path.as_str().to_owned())
                {
                    return Err(lock_error(format!(
                        "workspace paths `{previous}` and `{path}` are case-fold/NFC aliases"
                    )));
                }
            }
            let mut aliases = BTreeMap::<String, String>::new();
            for dependency in &package.dependencies {
                let folded = dependency.alias.casefold_key();
                if let Some(previous) = aliases.insert(folded, dependency.alias.as_str().to_owned())
                {
                    return Err(lock_error(format!(
                        "package `{}` dependency aliases `{previous}` and `{}` collide under NFC/case folding",
                        package.name, dependency.alias,
                    )));
                }
            }
            rows.insert(package.name.clone(), package);
        }
        if self
            .packages
            .windows(2)
            .any(|pair| pair[0].name >= pair[1].name)
        {
            return Err(lock_error("package rows are not in canonical name order"));
        }
        for package in &self.packages {
            if package
                .dependencies
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            {
                return Err(lock_error(format!(
                    "dependencies for `{}` are not in canonical order",
                    package.name
                )));
            }
            for dependency in &package.dependencies {
                let target = rows.get(&dependency.package).ok_or_else(|| {
                    lock_error(format!(
                        "package `{}` references missing dependency `{}`",
                        package.name, dependency.package
                    ))
                })?;
                if matches!(&package.source, LockSource::Registry { .. })
                    && matches!(&target.source, LockSource::Workspace { .. })
                {
                    return Err(lock_error(format!(
                        "registry package `{}` cannot depend on workspace package `{}`",
                        package.name, target.name
                    )));
                }
                if !dependency.requirement.matches(&target.version) {
                    return Err(lock_error(format!(
                        "package `{}` dependency `{}` requires `{}` but selected `{}` is version `{}`",
                        package.name,
                        dependency.alias,
                        dependency.requirement,
                        target.name,
                        target.version,
                    )));
                }
            }
        }

        let roots = self
            .packages
            .iter()
            .filter(|package| matches!(package.source, LockSource::Workspace { .. }))
            .map(|package| package.name.clone())
            .collect::<Vec<_>>();
        if roots.is_empty() {
            return Err(lock_error("lock has no workspace root package"));
        }
        let mut reachable = BTreeSet::new();
        let mut queue = VecDeque::from(roots);
        while let Some(name) = queue.pop_front() {
            if !reachable.insert(name.clone()) {
                continue;
            }
            let package = rows.get(&name).expect("queued package exists");
            queue.extend(package.dependencies.iter().map(|edge| edge.package.clone()));
        }
        if reachable.len() != self.packages.len() {
            let orphan = self
                .packages
                .iter()
                .find(|package| !reachable.contains(&package.name))
                .expect("length mismatch has orphan");
            return Err(lock_error(format!(
                "orphan package `{}` is unreachable",
                orphan.name
            )));
        }
        reject_dependency_cycles(&rows)?;
        Ok(())
    }
}

fn reject_dependency_cycles(rows: &BTreeMap<PackageName, &LockPackage>) -> Result<(), Diagnostics> {
    let mut incoming = rows
        .keys()
        .cloned()
        .map(|name| (name, 0_u64))
        .collect::<BTreeMap<_, _>>();
    for package in rows.values() {
        for dependency in &package.dependencies {
            let count = incoming
                .get_mut(&dependency.package)
                .expect("lock dependency targets were validated");
            *count = count
                .checked_add(1)
                .ok_or_else(|| lock_error("lock dependency count exceeds u64"))?;
        }
    }
    let mut ready = incoming
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    let mut visited = 0_u64;
    while let Some(name) = ready.pop_first() {
        visited = visited
            .checked_add(1)
            .ok_or_else(|| lock_error("lock package count exceeds u64"))?;
        for dependency in &rows[&name].dependencies {
            let count = incoming
                .get_mut(&dependency.package)
                .expect("lock dependency targets were validated");
            *count = count
                .checked_sub(1)
                .expect("incoming dependency count is consistent");
            if *count == 0 {
                ready.insert(dependency.package.clone());
            }
        }
    }
    if usize::try_from(visited).ok() != Some(rows.len()) {
        let cycle = incoming
            .iter()
            .find(|(_, count)| **count != 0)
            .map(|(name, _)| name)
            .expect("cycle leaves a nonzero incoming count");
        return Err(lock_error(format!(
            "package dependency graph contains a cycle involving `{cycle}`"
        )));
    }
    Ok(())
}

fn parse_package(table: &Table) -> Result<LockPackage, Diagnostics> {
    ensure_keys(
        table,
        &[
            "name",
            "version",
            "source",
            "path",
            "archive-digest",
            "source-digest",
            "provenance-record-digest",
            "inclusion-record-digest",
            "dependencies",
        ],
        "package row",
    )?;
    let name = required_string(table, "name")?.parse()?;
    let version = parse_version(required_string(table, "version")?)?;
    let source = match required_string(table, "source")? {
        "workspace" => LockSource::Workspace {
            path: PortablePath::workspace_member(required_string(table, "path")?)?,
            source_digest: required_string(table, "source-digest")?.parse()?,
        },
        "registry" => LockSource::Registry {
            archive_digest: required_string(table, "archive-digest")?.parse()?,
            source_digest: required_string(table, "source-digest")?.parse()?,
            provenance_record_digest: required_string(table, "provenance-record-digest")?
                .parse()?,
            inclusion_record_digest: required_string(table, "inclusion-record-digest")?.parse()?,
        },
        other => return Err(lock_error(format!("unknown package source `{other}`"))),
    };
    match &source {
        LockSource::Workspace { .. } => ensure_absent(
            table,
            &[
                "archive-digest",
                "provenance-record-digest",
                "inclusion-record-digest",
            ],
        )?,
        LockSource::Registry { .. } => ensure_absent(table, &["path"])?,
    }
    let dependencies = parse_dependencies(table.get("dependencies"))?;
    Ok(LockPackage {
        name,
        version,
        source,
        dependencies,
    })
}

fn parse_dependencies(item: Option<&Item>) -> Result<Vec<LockDependency>, Diagnostics> {
    let array = item
        .and_then(Item::as_array)
        .ok_or_else(|| lock_error("package dependencies must be an array"))?;
    let mut dependencies = Vec::with_capacity(array.len());
    for value in array {
        let table = value
            .as_inline_table()
            .ok_or_else(|| lock_error("dependency row must be an inline table"))?;
        ensure_inline_keys(table, &["alias", "package", "requirement", "kind"])?;
        let alias = SourceIdentifier::new(required_inline_string(table, "alias")?)?;
        let package = PackageName::from_str(required_inline_string(table, "package")?)?;
        let requirement =
            DependencyRequirement::parse_canonical(required_inline_string(table, "requirement")?)?;
        let kind = match required_inline_string(table, "kind")? {
            "normal" => LockDependencyKind::Normal,
            "development" => LockDependencyKind::Development,
            other => return Err(lock_error(format!("unknown dependency kind `{other}`"))),
        };
        dependencies.push(LockDependency {
            alias,
            package,
            requirement,
            kind,
        });
    }
    Ok(dependencies)
}

fn required_table<'a>(table: &'a Table, key: &str) -> Result<&'a Table, Diagnostics> {
    table
        .get(key)
        .and_then(Item::as_table)
        .ok_or_else(|| lock_error(format!("lock requires [{key}]")))
}

fn required_string<'a>(table: &'a Table, key: &str) -> Result<&'a str, Diagnostics> {
    table
        .get(key)
        .and_then(Item::as_str)
        .ok_or_else(|| lock_error(format!("lock field `{key}` must be a string")))
}

fn required_integer(table: &Table, key: &str) -> Result<i64, Diagnostics> {
    table
        .get(key)
        .and_then(Item::as_integer)
        .ok_or_else(|| lock_error(format!("lock field `{key}` must be an integer")))
}

fn required_inline_string<'a>(table: &'a InlineTable, key: &str) -> Result<&'a str, Diagnostics> {
    table
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| lock_error(format!("dependency field `{key}` must be a string")))
}

fn ensure_keys(table: &Table, allowed: &[&str], context: &str) -> Result<(), Diagnostics> {
    if let Some((key, _)) = table.iter().find(|(key, _)| !allowed.contains(key)) {
        return Err(lock_error(format!("unknown {context} field `{key}`")));
    }
    Ok(())
}

fn ensure_inline_keys(table: &InlineTable, allowed: &[&str]) -> Result<(), Diagnostics> {
    if let Some((key, _)) = table.iter().find(|(key, _)| !allowed.contains(key)) {
        return Err(lock_error(format!("unknown dependency field `{key}`")));
    }
    Ok(())
}

fn ensure_absent(table: &Table, keys: &[&str]) -> Result<(), Diagnostics> {
    if let Some(key) = keys.iter().find(|key| table.contains_key(key)) {
        return Err(lock_error(format!(
            "field `{key}` is invalid for this package source"
        )));
    }
    Ok(())
}

fn parse_version(value: &str) -> Result<Version, Diagnostics> {
    let version = Version::parse(value)
        .map_err(|error| lock_error(format!("invalid version `{value}`: {error}")))?;
    if version.to_string() != value || !version.build.is_empty() {
        return Err(lock_error(format!("version `{value}` is not canonical")));
    }
    Ok(version)
}

fn quoted(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                write!(output, "\\u{:04X}", u32::from(character)).unwrap();
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn lock_error(message: impl Into<String>) -> Diagnostics {
    Diagnostic::new(DiagnosticCode::LockInvalid, message).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(seed: &str) -> IntegrityDigest {
        IntegrityDigest::of_bytes(seed.as_bytes())
    }

    fn golden_lock() -> Lockfile {
        Lockfile::new(
            ToolchainLock {
                version: Version::new(0, 0, 0),
                release_manifest_digest: digest("toolchain"),
            },
            WorkspaceLock {
                source_digest: digest("workspace"),
            },
            RegistryLock {
                identity: crate::OFFICIAL_REGISTRY_IDENTITY.to_owned(),
                snapshot_digest: digest("registry"),
            },
            vec![LockPackage {
                name: "example/game".parse().unwrap(),
                version: Version::new(0, 1, 0),
                source: LockSource::Workspace {
                    path: PortablePath::workspace_member(".").unwrap(),
                    source_digest: digest("source"),
                },
                dependencies: Vec::new(),
            }],
        )
        .unwrap()
    }

    fn dependency_lock(
        requirement: &str,
        selected_version: Version,
    ) -> Result<Lockfile, Diagnostics> {
        Lockfile::new(
            ToolchainLock::bootstrap_current(),
            WorkspaceLock {
                source_digest: digest("workspace"),
            },
            RegistryLock {
                identity: crate::OFFICIAL_REGISTRY_IDENTITY.to_owned(),
                snapshot_digest: digest("registry"),
            },
            vec![
                LockPackage {
                    name: "example/app".parse().unwrap(),
                    version: Version::new(0, 1, 0),
                    source: LockSource::Workspace {
                        path: PortablePath::workspace_member(".").unwrap(),
                        source_digest: digest("app"),
                    },
                    dependencies: vec![LockDependency {
                        alias: SourceIdentifier::new("math").unwrap(),
                        package: "arche/math".parse().unwrap(),
                        requirement: DependencyRequirement::from_version_req(
                            &VersionReq::parse(requirement).unwrap(),
                        ),
                        kind: LockDependencyKind::Normal,
                    }],
                },
                LockPackage {
                    name: "arche/math".parse().unwrap(),
                    version: selected_version,
                    source: LockSource::Registry {
                        archive_digest: digest("archive"),
                        source_digest: digest("math"),
                        provenance_record_digest: digest("provenance"),
                        inclusion_record_digest: digest("inclusion"),
                    },
                    dependencies: Vec::new(),
                },
            ],
        )
    }

    #[test]
    fn canonical_lock_round_trips_exactly() {
        let text = golden_lock().to_canonical_string().unwrap();
        let parsed = Lockfile::parse(text.as_bytes()).unwrap();
        assert_eq!(parsed, golden_lock());
        assert!(text.ends_with('\n'));
        assert!(!text.contains('\r'));
        assert!(!text.contains("timestamp"));
    }

    #[test]
    fn dependency_requirements_round_trip_and_reject_stale_selections() {
        let lock = dependency_lock("^1.0.0", Version::new(1, 2, 0)).unwrap();
        let text = lock.to_canonical_string().unwrap();
        assert!(text.contains(concat!(
            "{ alias = \"math\", package = \"arche/math\", ",
            "requirement = \"^1.0.0\", kind = \"normal\" }",
        )));
        assert_eq!(Lockfile::parse(text.as_bytes()).unwrap(), lock);

        let tampered = text.replace("requirement = \"^1.0.0\"", "requirement = \"^2.0.0\"");
        assert!(Lockfile::parse(tampered.as_bytes()).is_err());
        assert!(dependency_lock("^1.0.0", Version::new(2, 0, 0)).is_err());
    }

    #[test]
    fn noncanonical_or_corrupt_locks_fail_closed() {
        let text = golden_lock().to_canonical_string().unwrap();
        assert!(Lockfile::parse(text.replace("schema = 1", "schema=1").as_bytes()).is_err());
        assert!(Lockfile::parse(
            text.replace(
                "dependencies = []",
                "dependencies = [{ alias = \"x\", package = \"missing/x\", requirement = \"*\", kind = \"normal\" }]"
            )
            .as_bytes()
        )
        .is_err());
        assert!(Lockfile::parse(text.replace("sha256:", "SHA256:").as_bytes()).is_err());
        assert!(Lockfile::parse(
            text.replace(
                "dependencies = []",
                "dependencies = [{ alias = \"self_dep\", package = \"example/game\", requirement = \"*\", kind = \"normal\" }]"
            )
            .as_bytes()
        )
        .is_err());
    }

    #[test]
    fn dependency_aliases_and_source_boundaries_fail_closed() {
        let workspace_one: PackageName = "example/one".parse().unwrap();
        let workspace_two: PackageName = "example/two".parse().unwrap();
        let registry: PackageName = "example/registry".parse().unwrap();
        let package = |name: PackageName, source: LockSource, dependencies| LockPackage {
            name,
            version: Version::new(1, 0, 0),
            source,
            dependencies,
        };
        let workspace_source = |path: &str| LockSource::Workspace {
            path: PortablePath::workspace_member(path).unwrap(),
            source_digest: digest(path),
        };
        let registry_source = || LockSource::Registry {
            archive_digest: digest("archive"),
            source_digest: digest("registry source"),
            provenance_record_digest: digest("provenance"),
            inclusion_record_digest: digest("inclusion"),
        };
        let dependency = |alias: &str, package: PackageName, requirement: &str| LockDependency {
            alias: SourceIdentifier::new(alias).unwrap(),
            package,
            requirement: DependencyRequirement::from_version_req(
                &VersionReq::parse(requirement).unwrap(),
            ),
            kind: LockDependencyKind::Normal,
        };
        let toolchain = ToolchainLock::bootstrap_current();
        let workspace = WorkspaceLock {
            source_digest: digest("workspace"),
        };
        let registry_lock = RegistryLock {
            identity: crate::OFFICIAL_REGISTRY_IDENTITY.to_owned(),
            snapshot_digest: digest("registry"),
        };

        let aliases = Lockfile::new(
            toolchain.clone(),
            workspace,
            registry_lock.clone(),
            vec![
                package(
                    workspace_one.clone(),
                    workspace_source("packages/one"),
                    vec![
                        dependency("Foo", registry.clone(), "*"),
                        dependency("foo", registry.clone(), "*"),
                    ],
                ),
                package(registry.clone(), registry_source(), Vec::new()),
            ],
        );
        assert!(aliases.is_err());

        let source_boundary = Lockfile::new(
            toolchain,
            workspace,
            registry_lock,
            vec![
                package(
                    workspace_one,
                    workspace_source("packages/one"),
                    vec![dependency("registry", registry.clone(), "^1.0.0")],
                ),
                package(
                    workspace_two.clone(),
                    workspace_source("packages/two"),
                    Vec::new(),
                ),
                package(
                    registry,
                    registry_source(),
                    vec![dependency("local", workspace_two, "^1.0.0")],
                ),
            ],
        );
        assert!(source_boundary.is_err());
    }
}
