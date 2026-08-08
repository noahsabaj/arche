use crate::diagnostic::{Diagnostic, DiagnosticCode, Diagnostics};
use arche_foundation::identity::PackageId;
use std::fmt;
use std::str::FromStr;
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

pub const OFFICIAL_REGISTRY_IDENTITY: &str = "registry+https://packages.arche-lang.org";

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackageName(String);

impl PackageName {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn leaf(&self) -> &str {
        self.0
            .split_once('/')
            .map_or(self.0.as_str(), |(_, leaf)| leaf)
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for PackageName {
    type Err = Diagnostics;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut pieces = value.split('/');
        let Some(scope) = pieces.next() else {
            return Err(invalid_name(value));
        };
        let Some(name) = pieces.next() else {
            return Err(invalid_name(value));
        };
        if pieces.next().is_some() || !valid_package_segment(scope) || !valid_package_segment(name)
        {
            return Err(invalid_name(value));
        }
        Ok(Self(value.to_owned()))
    }
}

fn invalid_name(value: &str) -> Diagnostics {
    Diagnostic::new(
        DiagnosticCode::ManifestValue,
        format!("invalid package name `{value}`; expected lowercase ASCII `scope/name`"),
    )
    .into()
}

fn valid_package_segment(segment: &str) -> bool {
    if segment.is_empty() || segment.starts_with(['-', '_']) || segment.ends_with(['-', '_']) {
        return false;
    }
    segment.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
    })
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceIdentifier(String);

impl SourceIdentifier {
    pub fn new(value: &str) -> Result<Self, Diagnostics> {
        let normalized = value.nfc().collect::<String>();
        if normalized != value {
            return Err(Diagnostic::new(
                DiagnosticCode::ManifestValue,
                format!("identifier `{value}` is not NFC-normalized; use `{normalized}`"),
            )
            .into());
        }
        let mut chars = value.chars();
        let Some(first) = chars.next() else {
            return Err(invalid_identifier(value));
        };
        if first != '_' && !unicode_ident::is_xid_start(first) {
            return Err(invalid_identifier(value));
        }
        if !chars.all(|character| character == '_' || unicode_ident::is_xid_continue(character)) {
            return Err(invalid_identifier(value));
        }
        if KEYWORDS.binary_search(&value).is_ok() {
            return Err(Diagnostic::new(
                DiagnosticCode::ManifestValue,
                format!("`{value}` is a reserved Arche keyword"),
            )
            .into());
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn casefold_key(&self) -> String {
        self.0.nfc().case_fold().nfc().collect()
    }
}

impl fmt::Display for SourceIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

fn invalid_identifier(value: &str) -> Diagnostics {
    Diagnostic::new(
        DiagnosticCode::ManifestValue,
        format!("invalid Arche identifier `{value}`"),
    )
    .into()
}

// Sorted for binary_search.
const KEYWORDS: &[&str] = &[
    "as",
    "bool",
    "catch",
    "component",
    "const",
    "else",
    "enum",
    "exit",
    "false",
    "fn",
    "for",
    "if",
    "impl",
    "in",
    "init",
    "let",
    "match",
    "mod",
    "mut",
    "package",
    "pub",
    "query",
    "requires",
    "resource",
    "return",
    "schedule",
    "self",
    "spawn",
    "startup",
    "static",
    "struct",
    "super",
    "system",
    "tag",
    "throw",
    "throws",
    "trait",
    "true",
    "type",
    "unsafe",
    "use",
    "while",
    "world",
    "yield",
];

pub type DependencyAlias = SourceIdentifier;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PortablePath(String);

impl PortablePath {
    pub fn new(value: &str) -> Result<Self, Diagnostics> {
        Self::parse(value, false)
    }

    pub fn workspace_member(value: &str) -> Result<Self, Diagnostics> {
        Self::parse(value, true)
    }

    fn parse(value: &str, allow_workspace_dot: bool) -> Result<Self, Diagnostics> {
        if allow_workspace_dot && value == "." {
            return Ok(Self(value.to_owned()));
        }
        let invalid = value.is_empty()
            || value.contains('\0')
            || value.contains('\\')
            || value.starts_with('/')
            || value.starts_with("//")
            || value.as_bytes().get(1) == Some(&b':')
            || value
                .split('/')
                .any(|piece| piece.is_empty() || piece == "." || piece == "..");
        if invalid {
            return Err(Diagnostic::new(
                DiagnosticCode::WorkspacePath,
                format!("`{value}` is not a safe portable relative path"),
            )
            .into());
        }
        if value.nfc().collect::<String>() != value {
            return Err(Diagnostic::new(
                DiagnosticCode::WorkspacePath,
                format!("portable path `{value}` is not NFC-normalized"),
            )
            .into());
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }

    pub fn casefold_key(&self) -> String {
        self.0.nfc().case_fold().nfc().collect()
    }
}

impl fmt::Display for PortablePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A package-relative development dependency path.
///
/// Unlike source and workspace-member paths, a dependency may begin with one
/// or more `..` segments so sibling workspace members can depend on each
/// other. Parent segments are permitted only as a leading canonical prefix;
/// workspace loading still proves that the resolved path stays inside the
/// workspace and names an explicitly declared member.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DependencyPath(String);

impl DependencyPath {
    pub fn new(value: &str) -> Result<Self, Diagnostics> {
        let invalid_host_form = value.is_empty()
            || value.contains('\0')
            || value.contains('\\')
            || value.starts_with('/')
            || value.starts_with("//")
            || value.as_bytes().get(1) == Some(&b':');
        let mut saw_normal = false;
        let mut invalid_segment = false;
        for segment in value.split('/') {
            match segment {
                "" | "." => invalid_segment = true,
                ".." if saw_normal => invalid_segment = true,
                ".." => {}
                _ => saw_normal = true,
            }
        }
        if invalid_host_form || invalid_segment {
            return Err(Diagnostic::new(
                DiagnosticCode::WorkspacePath,
                format!(
                    "`{value}` is not a canonical portable dependency path; parent segments may appear only as a leading prefix"
                ),
            )
            .into());
        }
        if value.nfc().collect::<String>() != value {
            return Err(Diagnostic::new(
                DiagnosticCode::WorkspacePath,
                format!("dependency path `{value}` is not NFC-normalized"),
            )
            .into());
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl fmt::Display for DependencyPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ItemPathRoot {
    Package,
    SelfModule,
    Super(u64),
    Dependency(DependencyAlias),
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ItemPath {
    root: ItemPathRoot,
    segments: Vec<SourceIdentifier>,
}

impl ItemPath {
    pub fn parse(value: &str) -> Result<Self, Diagnostics> {
        let raw = value.split("::").collect::<Vec<_>>();
        if raw.len() < 2 || raw.iter().any(|segment| segment.is_empty()) {
            return Err(invalid_item_path(value));
        }
        let (root, start) = match raw[0] {
            "package" => (ItemPathRoot::Package, 1),
            "self" => (ItemPathRoot::SelfModule, 1),
            "super" => {
                let count = raw
                    .iter()
                    .take_while(|segment| **segment == "super")
                    .count();
                (
                    ItemPathRoot::Super(u64::try_from(count).unwrap_or(u64::MAX)),
                    count,
                )
            }
            dependency => (
                ItemPathRoot::Dependency(SourceIdentifier::new(dependency)?),
                1,
            ),
        };
        if start >= raw.len() {
            return Err(invalid_item_path(value));
        }
        let segments = raw[start..]
            .iter()
            .map(|segment| SourceIdentifier::new(segment))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { root, segments })
    }

    pub fn root(&self) -> &ItemPathRoot {
        &self.root
    }

    pub fn segments(&self) -> &[SourceIdentifier] {
        &self.segments
    }

    pub fn canonical(&self) -> String {
        let mut output = match &self.root {
            ItemPathRoot::Package => "package".to_owned(),
            ItemPathRoot::SelfModule => "self".to_owned(),
            ItemPathRoot::Super(count) => {
                std::iter::repeat_n("super", usize::try_from(*count).unwrap_or(usize::MAX))
                    .collect::<Vec<_>>()
                    .join("::")
            }
            ItemPathRoot::Dependency(alias) => alias.as_str().to_owned(),
        };
        for segment in &self.segments {
            output.push_str("::");
            output.push_str(segment.as_str());
        }
        output
    }
}

fn invalid_item_path(value: &str) -> Diagnostics {
    Diagnostic::new(
        DiagnosticCode::ManifestValue,
        format!("invalid item path `{value}`"),
    )
    .into()
}

pub fn canonical_package_id(name: &PackageName) -> PackageId {
    let mut preimage =
        Vec::with_capacity(16 + OFFICIAL_REGISTRY_IDENTITY.len() + name.as_str().len());
    extend_length_prefixed(&mut preimage, OFFICIAL_REGISTRY_IDENTITY.as_bytes());
    extend_length_prefixed(&mut preimage, name.as_str().as_bytes());
    PackageId::from_canonical_preimage(&preimage)
}

fn extend_length_prefixed(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&u64::try_from(value.len()).unwrap_or(u64::MAX).to_le_bytes());
    output.extend_from_slice(value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_package_names_are_strict() {
        assert_eq!("arche/math".parse::<PackageName>().unwrap().leaf(), "math");
        for invalid in ["math", "Arche/math", "arche/-math", "arche/math/"] {
            assert!(invalid.parse::<PackageName>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn source_identifiers_reject_migration_keywords() {
        assert!(SourceIdentifier::new("startup").is_err());
    }

    #[test]
    fn portable_paths_reject_host_dependent_forms() {
        assert!(PortablePath::new("src/main.arc").is_ok());
        for invalid in ["", ".", "../x", "a/../b", "C:/x", "a\\b", "/x"] {
            assert!(PortablePath::new(invalid).is_err(), "{invalid}");
        }
        assert!(PortablePath::workspace_member(".").is_ok());
    }

    #[test]
    fn dependency_paths_allow_only_a_canonical_parent_prefix() {
        assert!(DependencyPath::new("../shared").is_ok());
        assert!(DependencyPath::new("../../shared/core").is_ok());
        for invalid in ["", ".", "a/../b", "../a/..", "C:/x", "a\\b", "/x"] {
            assert!(DependencyPath::new(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn package_id_is_version_independent_and_domain_canonical() {
        let package: PackageName = "example/game".parse().unwrap();
        assert_eq!(
            canonical_package_id(&package),
            canonical_package_id(&package)
        );
        assert_eq!(
            canonical_package_id(&package).to_string(),
            "596EEC45570E56E04A372341227D2DCF"
        );
    }

    #[test]
    fn item_paths_use_arche_roots() {
        let path = ItemPath::parse("package::physics::Position").unwrap();
        assert_eq!(path.canonical(), "package::physics::Position");
        assert!(ItemPath::parse("package").is_err());
    }
}
