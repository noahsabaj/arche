use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arche_package::{IntegrityDigest, PackageNodeId, PortablePath, SourceTreeEntry};
use same_file::Handle;
use sha2::{Digest, Sha256};
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

use crate::embedded_core::VerifiedEmbeddedCoreAuthority;

static NEXT_SNAPSHOT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FileId(pub u64);

/// The compiler-owned embedded Core snapshot is the sole user of this ID.
/// Ordinary workspace and include acquisition must fail before reaching it.
pub const EMBEDDED_CORE_FILE_ID: FileId = FileId(u64::MAX);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourcePosition {
    pub byte: u64,
    pub line: u64,
    pub column: u64,
}

impl SourcePosition {
    pub const START: Self = Self {
        byte: 0,
        line: 1,
        column: 1,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    pub file: FileId,
    pub start: SourcePosition,
    pub end: SourcePosition,
}

impl Span {
    pub fn join(self, other: Self) -> Self {
        debug_assert_eq!(self.file, other.file);
        Self {
            file: self.file,
            start: self.start,
            end: other.end,
        }
    }
}

/// Why a retained input was acquired. A path used in more than one role still
/// has one snapshot and one workspace-global [`FileId`].
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SourceRole {
    Module,
    Include,
    EmbeddedCore,
}

/// Immutable metadata for one retained source or include input. Host paths,
/// physical identities, and spool handles remain private compiler authority.
#[derive(Debug)]
pub struct SourceFile {
    id: FileId,
    package: Option<PackageNodeId>,
    portable_path: PortablePath,
    roles: Vec<SourceRole>,
    canonical_path: Option<PathBuf>,
    identity: Option<Handle>,
    snapshot: SourceSnapshot,
}

impl SourceFile {
    pub const fn id(&self) -> FileId {
        self.id
    }

    pub const fn package(&self) -> Option<PackageNodeId> {
        self.package
    }

    pub fn is_embedded_core(&self) -> bool {
        self.id == EMBEDDED_CORE_FILE_ID
    }

    pub fn portable_path(&self) -> &PortablePath {
        &self.portable_path
    }

    pub fn roles(&self) -> &[SourceRole] {
        &self.roles
    }

    pub const fn byte_length(&self) -> u64 {
        self.snapshot.byte_length()
    }

    pub const fn content_digest(&self) -> IntegrityDigest {
        self.snapshot.content_digest()
    }

    /// Host-dependent paths are exposed only for human diagnostics and never
    /// enter semantic dumps or identity encodings.
    pub fn diagnostic_path(&self) -> Option<&Path> {
        self.canonical_path.as_deref()
    }
}

/// A bounded read of a retained source span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSnippet {
    pub bytes: Vec<u8>,
    pub truncated: bool,
}

/// Sealed workspace source authority. It has no mutating API; every reader is
/// cloned from the retained private spool and never reopens the original path.
#[derive(Debug)]
pub struct SourceDatabase {
    files: Vec<SourceFile>,
}

impl SourceDatabase {
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    pub fn file(&self, id: FileId) -> Option<&SourceFile> {
        if id == EMBEDDED_CORE_FILE_ID {
            return self.files.last().filter(|source| source.id == id);
        }
        let index = usize::try_from(id.0).ok()?;
        self.files.get(index).filter(|source| source.id == id)
    }

    pub fn reader(&self, id: FileId) -> io::Result<BufReader<File>> {
        self.file(id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown source FileId"))?
            .snapshot
            .reader()
    }

    pub fn bounded_snippet(&self, span: Span, maximum_bytes: u64) -> io::Result<SourceSnippet> {
        let source = self
            .file(span.file)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown source FileId"))?;
        if span.start.byte > span.end.byte || span.end.byte > source.byte_length() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "source span lies outside its retained snapshot",
            ));
        }
        let length = span.end.byte - span.start.byte;
        let retained = length.min(maximum_bytes);
        let capacity = usize::try_from(retained).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "bounded source snippet does not fit host address space",
            )
        })?;
        let mut reader = source.snapshot.reader()?;
        reader.seek(SeekFrom::Start(span.start.byte))?;
        let mut bytes = Vec::with_capacity(capacity);
        reader.take(retained).read_to_end(&mut bytes)?;
        Ok(SourceSnippet {
            bytes,
            truncated: retained != length,
        })
    }

    /// Returns package-relative commitments derived only from retained bytes.
    pub fn source_entries(&self, package: PackageNodeId) -> Vec<SourceTreeEntry> {
        let mut entries = self
            .files
            .iter()
            .filter(|source| source.package == Some(package))
            .map(|source| SourceTreeEntry {
                path: source.portable_path.clone(),
                byte_length: source.byte_length(),
                content_digest: source.content_digest(),
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        entries
    }
}

/// Mutable acquisition authority used only until all ordinary inputs have been
/// assigned their canonical workspace-global IDs and the database is sealed.
#[derive(Debug, Default)]
pub struct SourceDatabaseBuilder {
    files: Vec<SourceFile>,
    paths: BTreeMap<(PackageNodeId, PortablePath), FileId>,
    package_roots: BTreeMap<PackageNodeId, PackageRootBinding>,
}

#[derive(Debug)]
struct PackageRootBinding {
    requested: PathBuf,
    canonical: PathBuf,
    manifest: Option<PackageManifestBinding>,
}

#[derive(Debug)]
struct PackageManifestBinding {
    portable_path: PortablePath,
    identity: Handle,
}

impl SourceDatabaseBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retains the package-root manifest's physical identity without assigning
    /// it a [`FileId`]. A manifest receives an ordinary dense ID only when the
    /// program also acquires it as a module or include input.
    pub(crate) fn bind_package_manifest(
        &mut self,
        package: PackageNodeId,
        package_root: &Path,
        manifest_entry: &SourceTreeEntry,
    ) -> Result<(), Diagnostic> {
        let canonical_root = self.canonical_package_root(package, package_root)?;
        let retained = self
            .package_roots
            .get(&package)
            .expect("canonical package root has a retained binding");
        if let Some(manifest) = &retained.manifest {
            if manifest.portable_path != manifest_entry.path {
                return Err(Diagnostic::path(
                    "SOURCE004",
                    format!(
                        "package node {} was already bound to manifest {}; refusing {}",
                        package.get(),
                        manifest.portable_path,
                        manifest_entry.path
                    ),
                ));
            }
            return Ok(());
        }

        let manifest_path = portable_path_to_host_path(&canonical_root, &manifest_entry.path);
        let opened =
            OpenedSource::open_exact(&manifest_path, &canonical_root).map_err(|error| {
                Diagnostic::path(
                    "SOURCE001",
                    format!(
                        "could not retain package manifest {}: {error}",
                        manifest_path.display()
                    ),
                )
            })?;
        let manifest_identity = opened.into_identity().map_err(|error| {
            Diagnostic::path(
                "SOURCE001",
                format!(
                    "could not retain package manifest {}: {error}",
                    manifest_path.display()
                ),
            )
        })?;

        if let Some((previous_package, previous)) =
            self.package_roots.iter().find(|(node, root)| {
                **node != package
                    && root
                        .manifest
                        .as_ref()
                        .is_some_and(|manifest| manifest.identity == manifest_identity)
            })
        {
            let previous_manifest = previous
                .manifest
                .as_ref()
                .expect("selected package root has a manifest binding");
            return Err(Diagnostic::path(
                "SOURCE004",
                format!(
                    "package manifest {} for node {} aliases retained manifest {} for node {}",
                    manifest_entry.path,
                    package.get(),
                    previous_manifest.portable_path,
                    previous_package.get()
                ),
            ));
        }
        if let Some(previous) = self
            .files
            .iter()
            .find(|source| source.identity.as_ref() == Some(&manifest_identity))
        {
            return Err(Diagnostic::path(
                "SOURCE004",
                format!(
                    "package manifest {} aliases already retained source {}",
                    manifest_entry.path, previous.portable_path
                ),
            ));
        }

        self.package_roots
            .get_mut(&package)
            .expect("canonical package root has a retained binding")
            .manifest = Some(PackageManifestBinding {
            portable_path: manifest_entry.path.clone(),
            identity: manifest_identity,
        });
        Ok(())
    }

    /// Acquires `portable_path` relative to `package_root` exactly once. A
    /// repeated exact package/path request reuses its first ID without touching
    /// the original filesystem path again.
    pub fn acquire(
        &mut self,
        package: PackageNodeId,
        package_root: &Path,
        portable_path: PortablePath,
        role: SourceRole,
    ) -> Result<FileId, Diagnostic> {
        if self.files.last().is_some_and(SourceFile::is_embedded_core) {
            return Err(Diagnostic::path(
                "IDENTITY001",
                "ordinary source acquisition cannot follow embedded-Core source sealing",
            ));
        }
        let key = (package, portable_path.clone());
        if let Some(id) = self.paths.get(&key).copied() {
            let retained_root = self
                .package_roots
                .get(&package)
                .expect("retained source path has a package-root binding");
            if package_root != retained_root.requested && package_root != retained_root.canonical {
                return Err(Diagnostic::path(
                    "SOURCE004",
                    format!(
                        "package node {} was already retained from {}; refusing different root {}",
                        package.get(),
                        retained_root.canonical.display(),
                        package_root.display()
                    ),
                ));
            }
            let source = self
                .files
                .get_mut(usize::try_from(id.0).expect("ordinary FileId originated from usize"))
                .expect("source path index references retained source");
            if !source.roles.contains(&role) {
                source.roles.push(role);
                source.roles.sort();
            }
            return Ok(id);
        }

        let canonical_root = self.canonical_package_root(package, package_root)?;
        let file = checked_ordinary_file_id(self.files.len())?;
        let path = portable_path_to_host_path(&canonical_root, &portable_path);
        let opened = OpenedSource::open_exact(&path, &canonical_root).map_err(|error| {
            Diagnostic::path(
                "SOURCE001",
                format!("could not open source {}: {error}", path.display()),
            )
        })?;
        if let Some((manifest_package, manifest)) =
            self.package_roots
                .iter()
                .find_map(|(manifest_package, root)| {
                    root.manifest.as_ref().and_then(|manifest| {
                        (manifest.identity == *opened.identity())
                            .then_some((manifest_package, manifest))
                    })
                })
        {
            if *manifest_package != package || manifest.portable_path != portable_path {
                return Err(Diagnostic::path(
                    "SOURCE004",
                    format!(
                        "source {} for package node {} aliases retained manifest {} for package node {}",
                        portable_path,
                        package.get(),
                        manifest.portable_path,
                        manifest_package.get()
                    ),
                ));
            }
        }
        if self
            .package_roots
            .get(&package)
            .and_then(|root| root.manifest.as_ref())
            .is_some_and(|manifest| {
                manifest.portable_path == portable_path && manifest.identity != *opened.identity()
            })
        {
            return Err(Diagnostic::path(
                "SOURCE004",
                format!(
                    "package manifest {} changed physical identity before source acquisition",
                    portable_path
                ),
            ));
        }
        if let Some(previous) = self
            .files
            .iter()
            .find(|previous| previous.identity.as_ref() == Some(opened.identity()))
        {
            return Err(Diagnostic::path(
                "SOURCE004",
                format!(
                    "source {} aliases already retained {}",
                    portable_path, previous.portable_path
                ),
            ));
        }
        let canonical_path = opened.canonical_path().to_path_buf();
        let (snapshot, identity) = opened.into_snapshot().map_err(|error| {
            Diagnostic::path(
                "SOURCE001",
                format!("could not snapshot source {}: {error}", path.display()),
            )
        })?;
        self.paths.insert(key, file);
        self.files.push(SourceFile {
            id: file,
            package: Some(package),
            portable_path,
            roles: vec![role],
            canonical_path: Some(canonical_path),
            identity: Some(identity),
            snapshot,
        });
        Ok(file)
    }

    /// Installs the one compiler-owned hostless snapshot after every ordinary
    /// module/include input has received its dense workspace ID.
    pub(crate) fn install_embedded_core(
        &mut self,
        authority: &VerifiedEmbeddedCoreAuthority,
    ) -> Result<(), Diagnostic> {
        if self.files.last().is_some_and(SourceFile::is_embedded_core) {
            return Err(Diagnostic::path(
                "IDENTITY001",
                "embedded-Core synthetic source was installed more than once",
            ));
        }
        let source = authority.projection().source();
        let portable_path = PortablePath::new(source.package_path()).map_err(|diagnostics| {
            Diagnostic::path(
                "IDENTITY001",
                format!("embedded-Core package path is invalid: {diagnostics}"),
            )
        })?;
        let snapshot = SourceSnapshot::capture_embedded(source.bytes()).map_err(|error| {
            Diagnostic::path(
                "SOURCE001",
                format!("could not retain embedded-Core synthetic source: {error}"),
            )
        })?;
        if snapshot.content_digest().as_bytes() != source.digest() {
            return Err(Diagnostic::path(
                "IDENTITY001",
                "retained embedded-Core bytes differ from the verified release authority",
            ));
        }
        self.files.push(SourceFile {
            id: EMBEDDED_CORE_FILE_ID,
            package: None,
            portable_path,
            roles: vec![SourceRole::EmbeddedCore],
            canonical_path: None,
            identity: None,
            snapshot,
        });
        Ok(())
    }

    fn canonical_package_root(
        &mut self,
        package: PackageNodeId,
        package_root: &Path,
    ) -> Result<PathBuf, Diagnostic> {
        if let Some(retained) = self.package_roots.get(&package) {
            if package_root == retained.requested || package_root == retained.canonical {
                return Ok(retained.canonical.clone());
            }
            let requested = fs::canonicalize(package_root).map_err(|error| {
                Diagnostic::path(
                    "SOURCE001",
                    format!(
                        "could not resolve package root {}: {error}",
                        package_root.display()
                    ),
                )
            })?;
            if requested != retained.canonical {
                return Err(Diagnostic::path(
                    "SOURCE004",
                    format!(
                        "package node {} was already retained from {}; refusing different root {}",
                        package.get(),
                        retained.canonical.display(),
                        requested.display()
                    ),
                ));
            }
            return Ok(retained.canonical.clone());
        }

        let canonical = fs::canonicalize(package_root).map_err(|error| {
            Diagnostic::path(
                "SOURCE001",
                format!(
                    "could not resolve package root {}: {error}",
                    package_root.display()
                ),
            )
        })?;
        self.package_roots.insert(
            package,
            PackageRootBinding {
                requested: package_root.to_path_buf(),
                canonical: canonical.clone(),
                manifest: None,
            },
        );
        Ok(canonical)
    }

    /// Opens a fresh reader over bytes already retained by this builder.
    /// Parsing during canonical module traversal uses this accessor so it never
    /// reopens the original source path before the database is sealed.
    pub(crate) fn reader(&self, id: FileId) -> io::Result<BufReader<File>> {
        let index = usize::try_from(id.0)
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "unknown source FileId"))?;
        self.files
            .get(index)
            .filter(|source| source.id == id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown source FileId"))?
            .snapshot
            .reader()
    }

    /// Validates an already-retained include as exact UTF-8 with bounded
    /// memory. Incomplete scalars may cross spool read boundaries.
    pub(crate) fn validate_utf8(&self, id: FileId) -> io::Result<()> {
        validate_utf8_reader(self.reader(id)?)
    }

    pub fn seal(self) -> Arc<SourceDatabase> {
        Arc::new(SourceDatabase { files: self.files })
    }
}

fn checked_ordinary_file_id(index: usize) -> Result<FileId, Diagnostic> {
    let value = u64::try_from(index).map_err(|_| {
        Diagnostic::path(
            "IDENTITY001",
            "workspace source-file count exceeds the checked u64 representation",
        )
    })?;
    checked_ordinary_file_id_value(value)
}

fn checked_ordinary_file_id_value(value: u64) -> Result<FileId, Diagnostic> {
    if value == EMBEDDED_CORE_FILE_ID.0 {
        return Err(Diagnostic::path(
            "IDENTITY001",
            "ordinary source-file allocation reached the embedded-Core reserved FileId",
        ));
    }
    Ok(FileId(value))
}

fn portable_path_to_host_path(root: &Path, path: &PortablePath) -> PathBuf {
    let mut output = root.to_path_buf();
    for segment in path.as_str().split('/') {
        output.push(segment);
    }
    output
}

fn validate_utf8_reader(mut reader: impl Read) -> io::Result<()> {
    const CHUNK: usize = 64 * 1024;
    let mut buffer = [0_u8; CHUNK];
    let mut pending = Vec::<u8>::with_capacity(3);
    let mut combined = Vec::<u8>::with_capacity(CHUNK + 3);
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            if pending.is_empty() {
                return Ok(());
            }
            return std::str::from_utf8(&pending).map(|_| ()).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("included string is not exact UTF-8: {error}"),
                )
            });
        }

        combined.clear();
        combined.extend_from_slice(&pending);
        combined.extend_from_slice(&buffer[..count]);
        pending.clear();
        if let Err(error) = std::str::from_utf8(&combined) {
            if error.error_len().is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("included string is not exact UTF-8: {error}"),
                ));
            }
            let incomplete = &combined[error.valid_up_to()..];
            if incomplete.len() > 3 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "included string ends in a noncanonical UTF-8 prefix",
                ));
            }
            pending.extend_from_slice(incomplete);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Label {
    pub span: Option<Span>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub primary: Box<Label>,
    pub secondary: Vec<Label>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn at(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            code,
            primary: Box::new(Label {
                span: Some(span),
                message: message.clone(),
            }),
            message,
            secondary: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn path(code: &'static str, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            code,
            primary: Box::new(Label {
                span: None,
                message: message.clone(),
            }),
            message,
            secondary: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn with_secondary(mut self, span: Span, message: impl Into<String>) -> Self {
        self.secondary.push(Label {
            span: Some(span),
            message: message.into(),
        });
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "error[{}]: {}", self.code, self.message)
    }
}

impl std::error::Error for Diagnostic {}

#[derive(Debug)]
pub(crate) struct SourceSnapshot {
    original_path: Option<PathBuf>,
    spool_path: PathBuf,
    spool: Option<File>,
    byte_length: u64,
    content_digest: IntegrityDigest,
}

#[derive(Debug)]
pub(crate) struct OpenedSource {
    binding: SourcePathBinding,
    input: File,
    identity: Handle,
}

#[derive(Debug)]
struct SourcePathBinding {
    package_root: PathBuf,
    source_path: PathBuf,
    canonical_destination: PathBuf,
    parent_identity: Handle,
    entry_identity: Handle,
}

impl SourcePathBinding {
    fn bind(path: &Path, package_root: &Path) -> io::Result<Self> {
        let relative = path.strip_prefix(package_root).map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "source {} is not lexically contained by package root {}",
                    path.display(),
                    package_root.display()
                ),
            )
        })?;
        let root_metadata = fs::symlink_metadata(package_root)?;
        if is_link_or_reparse_point(&root_metadata) {
            return Err(path_alias_error(path, package_root));
        }
        if !root_metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("package root {} is not a directory", package_root.display()),
            ));
        }

        let mut current = package_root.to_path_buf();
        for component in relative.components() {
            let std::path::Component::Normal(expected) = component else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "source {} contains a non-portable path component",
                        path.display()
                    ),
                ));
            };
            let expected = expected.to_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "source {} contains a non-Unicode path component",
                        path.display()
                    ),
                )
            })?;
            current = resolve_exact_child(&current, expected, path)?;
        }
        if current == package_root {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a package root cannot be acquired as a source file",
            ));
        }

        let canonical_destination = fs::canonicalize(&current)?;
        if !canonical_destination.starts_with(package_root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "source {} resolves outside package root {}",
                    path.display(),
                    package_root.display()
                ),
            ));
        }
        let parent = current.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("source {} has no parent directory entry", path.display()),
            )
        })?;
        let parent_identity = Handle::from_path(parent)?;
        let entry_identity = Handle::from_path(&current)?;
        Ok(Self {
            package_root: package_root.to_path_buf(),
            source_path: current,
            canonical_destination,
            parent_identity,
            entry_identity,
        })
    }

    fn revalidate(&self, source_identity: &Handle, phase: &str) -> io::Result<()> {
        let current = Self::bind(&self.source_path, &self.package_root)?;
        if current.canonical_destination != self.canonical_destination {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "source {} changed its package-contained destination {phase}",
                    self.source_path.display()
                ),
            ));
        }
        if current.parent_identity != self.parent_identity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "source {} changed its parent directory entry {phase}",
                    self.source_path.display()
                ),
            ));
        }
        if current.entry_identity != self.entry_identity
            || &current.entry_identity != source_identity
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "source {} changed identity {phase} or changed its directory entry",
                    self.source_path.display()
                ),
            ));
        }
        Ok(())
    }
}

impl OpenedSource {
    pub(crate) fn open(
        path: &Path,
        expected_canonical: &Path,
        package_root: &Path,
    ) -> io::Result<Self> {
        Self::open_with(path, expected_canonical, package_root, |source| {
            File::open(source)
        })
    }

    fn open_exact(path: &Path, package_root: &Path) -> io::Result<Self> {
        Self::open_exact_with_after(path, package_root, |source| File::open(source), |_| Ok(()))
    }

    fn open_with<F>(
        path: &Path,
        expected_canonical: &Path,
        package_root: &Path,
        opener: F,
    ) -> io::Result<Self>
    where
        F: FnOnce(&Path) -> io::Result<File>,
    {
        let opened = Self::open_exact_with_after(path, package_root, opener, |_| Ok(()))?;
        opened.require_canonical(expected_canonical)
    }

    fn open_exact_with_after<F, A>(
        path: &Path,
        package_root: &Path,
        opener: F,
        after_open: A,
    ) -> io::Result<Self>
    where
        F: FnOnce(&Path) -> io::Result<File>,
        A: FnOnce(&File) -> io::Result<()>,
    {
        let binding = SourcePathBinding::bind(path, package_root)?;
        let input = opener(&binding.source_path)?;
        if !input.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("source {} is not a regular file", path.display()),
            ));
        }
        let identity = Handle::from_file(input.try_clone()?)?;
        after_open(&input)?;
        binding.revalidate(&identity, "while opening")?;

        Ok(Self {
            binding,
            input,
            identity,
        })
    }

    fn require_canonical(self, expected_canonical: &Path) -> io::Result<Self> {
        if self.binding.canonical_destination != expected_canonical {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "source {} changed its package-contained destination before opening",
                    self.binding.source_path.display()
                ),
            ));
        }
        Ok(self)
    }

    pub(crate) const fn identity(&self) -> &Handle {
        &self.identity
    }

    fn into_identity(self) -> io::Result<Handle> {
        self.binding
            .revalidate(&self.identity, "while retaining its physical identity")?;
        Ok(self.identity)
    }

    fn canonical_path(&self) -> &Path {
        &self.binding.canonical_destination
    }

    pub(crate) fn into_snapshot(self) -> io::Result<(SourceSnapshot, Handle)> {
        self.into_snapshot_with(|_| Ok(()))
    }

    fn into_snapshot_with<F>(mut self, after_spool: F) -> io::Result<(SourceSnapshot, Handle)>
    where
        F: FnOnce(&SourceSnapshot) -> io::Result<()>,
    {
        let snapshot =
            SourceSnapshot::capture(&self.binding.canonical_destination, &mut self.input)?;
        after_spool(&snapshot)?;
        self.binding
            .revalidate(&self.identity, "after source spooling")?;
        Ok((snapshot, self.identity))
    }

    #[cfg(test)]
    fn into_snapshot_with_hooks<D, F>(
        mut self,
        during_spool: D,
        after_spool: F,
    ) -> io::Result<(SourceSnapshot, Handle)>
    where
        D: FnMut(u64) -> io::Result<()>,
        F: FnOnce(&SourceSnapshot) -> io::Result<()>,
    {
        let snapshot = SourceSnapshot::capture_with(
            &self.binding.canonical_destination,
            &mut self.input,
            during_spool,
        )?;
        after_spool(&snapshot)?;
        self.binding
            .revalidate(&self.identity, "after source spooling")?;
        Ok((snapshot, self.identity))
    }
}

impl SourceSnapshot {
    fn capture(path: &Path, input: &mut File) -> io::Result<Self> {
        Self::capture_reader(Some(path.to_path_buf()), input)
    }

    #[cfg(test)]
    fn capture_with<F>(path: &Path, input: &mut File, after_chunk: F) -> io::Result<Self>
    where
        F: FnMut(u64) -> io::Result<()>,
    {
        Self::capture_reader_with(Some(path.to_path_buf()), input, after_chunk)
    }

    fn capture_embedded(bytes: &[u8]) -> io::Result<Self> {
        Self::capture_reader(None, std::io::Cursor::new(bytes))
    }

    fn capture_reader(original_path: Option<PathBuf>, input: impl Read) -> io::Result<Self> {
        Self::capture_reader_with(original_path, input, |_| Ok(()))
    }

    fn capture_reader_with<F>(
        original_path: Option<PathBuf>,
        mut input: impl Read,
        mut after_chunk: F,
    ) -> io::Result<Self>
    where
        F: FnMut(u64) -> io::Result<()>,
    {
        let (spool_path, spool) = create_spool()?;
        let mut cleanup = SpoolCleanup::new(spool_path.clone());
        let mut writer = BufWriter::new(spool);
        let mut hasher = Sha256::new();
        let mut byte_length = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = io::Read::read(&mut input, &mut buffer)?;
            if count == 0 {
                break;
            }
            writer.write_all(&buffer[..count])?;
            hasher.update(&buffer[..count]);
            byte_length = byte_length
                .checked_add(u64::try_from(count).map_err(|_| {
                    io::Error::other("source snapshot read length does not fit u64")
                })?)
                .ok_or_else(|| io::Error::other("source snapshot length exceeds u64"))?;
            after_chunk(byte_length)?;
        }
        writer.flush()?;
        let mut spool = writer.into_inner().map_err(|error| error.into_error())?;
        spool.seek(SeekFrom::Start(0))?;
        let mut digest = [0_u8; 32];
        digest.copy_from_slice(&hasher.finalize());
        cleanup.disarm();
        Ok(Self {
            original_path,
            spool_path,
            spool: Some(spool),
            byte_length,
            content_digest: IntegrityDigest::from_bytes(digest),
        })
    }

    pub(crate) fn reader(&self) -> io::Result<BufReader<File>> {
        let mut file = self
            .spool
            .as_ref()
            .expect("source snapshot spool remains open while readable")
            .try_clone()?;
        file.seek(SeekFrom::Start(0))?;
        Ok(BufReader::with_capacity(1024 * 1024, file))
    }

    pub(crate) fn path(&self) -> &Path {
        self.original_path
            .as_deref()
            .expect("ordinary source snapshots retain one host path")
    }

    pub(crate) const fn byte_length(&self) -> u64 {
        self.byte_length
    }

    pub(crate) const fn content_digest(&self) -> IntegrityDigest {
        self.content_digest
    }
}

fn resolve_exact_child(parent: &Path, expected: &str, source: &Path) -> io::Result<PathBuf> {
    let expected_nfc = expected.nfc().collect::<String>();
    if expected_nfc != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "source {} requires NFC path component `{expected_nfc}`, not `{expected}`",
                source.display()
            ),
        ));
    }
    let expected_fold = expected_nfc.case_fold().nfc().collect::<String>();
    let mut aliases = Vec::new();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
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
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "source {} requires exact NFC/case path component `{expected}`; colliding entries: {observed}",
                source.display()
            ),
        ));
    }

    let child = parent.join(expected);
    let metadata = fs::symlink_metadata(&child)?;
    if is_link_or_reparse_point(&metadata) {
        return Err(path_alias_error(source, &child));
    }
    Ok(child)
}

fn path_alias_error(source: &Path, alias: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "source path {} contains a symbolic-link or reparse-point alias at {}",
            source.display(),
            alias.display()
        ),
    )
}

fn is_link_or_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

impl Drop for SourceSnapshot {
    fn drop(&mut self) {
        drop(self.spool.take());
        let _ = fs::remove_file(&self.spool_path);
    }
}

struct SpoolCleanup {
    path: PathBuf,
    armed: bool,
}

impl SpoolCleanup {
    const fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SpoolCleanup {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn create_spool() -> io::Result<(PathBuf, File)> {
    let root = std::env::temp_dir();
    for _ in 0..128 {
        let ordinal = NEXT_SNAPSHOT.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!(
            "arche-frontend-{}-{ordinal}.snapshot",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create private source snapshot",
    ))
}

pub(crate) fn advance(position: &mut SourcePosition, character: char) -> io::Result<()> {
    position.byte =
        position
            .byte
            .checked_add(u64::try_from(character.len_utf8()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "UTF-8 width exceeds u64")
            })?)
            .ok_or_else(|| io::Error::other("source byte position overflow"))?;
    if character == '\n' {
        position.line = position
            .line
            .checked_add(1)
            .ok_or_else(|| io::Error::other("source line position overflow"))?;
        position.column = 1;
    } else if character != '\r' {
        position.column = position
            .column
            .checked_add(1)
            .ok_or_else(|| io::Error::other("source column position overflow"))?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub(crate) fn unique_directory(name: &str) -> PathBuf {
        let ordinal = NEXT_SNAPSHOT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "arche-frontend-test-{}-{ordinal}-{name}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("test directory is created");
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replace_entry(source: &Path, replacement: &Path, displaced: &Path) -> io::Result<()> {
        fs::rename(source, displaced)?;
        if let Err(error) = fs::rename(replacement, source) {
            let _ = fs::rename(displaced, source);
            return Err(error);
        }
        Ok(())
    }

    #[cfg(unix)]
    fn create_file_symlink(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_file_symlink(target: &Path, link: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }

    #[cfg(unix)]
    fn create_directory_symlink(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_directory_symlink(target: &Path, link: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    #[test]
    fn snapshot_digest_and_cleanup_bind_the_spooled_bytes() {
        let directory = test_support::unique_directory("snapshot-cleanup");
        let package_root = fs::canonicalize(&directory).unwrap();
        let source = package_root.join("input.arc");
        fs::write(&source, b"abc").unwrap();
        let canonical = fs::canonicalize(&source).unwrap();
        let opened = OpenedSource::open(&source, &canonical, &package_root).unwrap();
        let (snapshot, _) = opened.into_snapshot().unwrap();
        let spool = snapshot.spool_path.clone();
        assert_eq!(snapshot.byte_length(), 3);
        assert_eq!(snapshot.content_digest(), IntegrityDigest::of_bytes(b"abc"));
        assert!(spool.exists());
        drop(snapshot);
        assert!(!spool.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn checked_identity_is_the_file_that_would_be_spooled() {
        let directory = test_support::unique_directory("snapshot-identity");
        let package_root = fs::canonicalize(&directory).unwrap();
        let source = package_root.join("input.arc");
        let replacement = package_root.join("replacement.arc");
        fs::write(&source, b"original").unwrap();
        fs::write(&replacement, b"replacement").unwrap();
        let canonical = fs::canonicalize(&source).unwrap();
        let open_count = std::cell::Cell::new(0_u8);

        let error = OpenedSource::open_with(&source, &canonical, &package_root, |_| {
            open_count.set(open_count.get() + 1);
            File::open(&replacement)
        })
        .unwrap_err();
        assert_eq!(open_count.get(), 1);
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("changed identity while opening"));

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn exact_component_resolution_rejects_wrong_case_and_non_nfc_spelling() {
        let directory = test_support::unique_directory("snapshot-exact-components");
        let cases = [
            ("case-directory", "Data", "input.arc", "data", "input.arc"),
            ("case-basename", "data", "Input.arc", "data", "input.arc"),
            (
                "nfc-directory",
                "Cafe\u{301}",
                "input.arc",
                "Caf\u{e9}",
                "input.arc",
            ),
            (
                "nfc-basename",
                "data",
                "Cafe\u{301}.arc",
                "data",
                "Caf\u{e9}.arc",
            ),
        ];
        for (fixture, actual_directory, actual_file, requested_directory, requested_file) in cases {
            let root = directory.join(fixture);
            fs::create_dir_all(root.join(actual_directory)).unwrap();
            fs::write(root.join(actual_directory).join(actual_file), b"source").unwrap();
            let root = fs::canonicalize(root).unwrap();
            let requested = root.join(requested_directory).join(requested_file);
            let error = OpenedSource::open_exact(&requested, &root).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{fixture}");
            assert!(
                error.to_string().contains("exact NFC/case"),
                "{fixture}: {error}"
            );
        }

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn exact_component_resolution_rejects_casefold_and_nfc_colliding_siblings() {
        let directory = test_support::unique_directory("snapshot-component-collisions");
        let fixtures = [
            ("casefold", "File.arc", "file.arc", "File.arc"),
            (
                "normalization",
                "Caf\u{e9}.arc",
                "Cafe\u{301}.arc",
                "Caf\u{e9}.arc",
            ),
        ];
        let mut exercised = 0_u8;
        for (fixture, first, second, requested) in fixtures {
            let root = directory.join(fixture);
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join(first), b"first").unwrap();
            fs::write(root.join(second), b"second").unwrap();
            let observed = fs::read_dir(&root)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            if !observed.iter().any(|name| name == first)
                || !observed.iter().any(|name| name == second)
            {
                continue;
            }
            exercised += 1;
            let root = fs::canonicalize(root).unwrap();
            let error = OpenedSource::open_exact(&root.join(requested), &root).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{fixture}");
            assert!(
                error.to_string().contains("colliding entries"),
                "{fixture}: {error}"
            );
        }
        assert_ne!(exercised, 0, "host supports neither collision fixture");

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn replacement_after_open_is_rejected_before_spooling() {
        let directory = test_support::unique_directory("snapshot-replace-after-open");
        let package_root = fs::canonicalize(&directory).unwrap();
        let source = package_root.join("input.arc");
        let replacement = package_root.join("replacement.arc");
        let displaced = package_root.join("displaced.arc");
        fs::write(&source, b"original").unwrap();
        fs::write(&replacement, b"replacement").unwrap();
        let open_count = std::cell::Cell::new(0_u8);

        let error = OpenedSource::open_exact_with_after(
            &source,
            &package_root,
            |path| {
                open_count.set(open_count.get() + 1);
                File::open(path)
            },
            |_| replace_entry(&source, &replacement, &displaced),
        )
        .unwrap_err();
        assert_eq!(open_count.get(), 1);
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("changed identity while opening"));

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn replacement_during_capture_discards_the_completed_spool() {
        let directory = test_support::unique_directory("snapshot-replace-during-capture");
        let package_root = fs::canonicalize(&directory).unwrap();
        let source = package_root.join("input.arc");
        let replacement = package_root.join("replacement.arc");
        let displaced = package_root.join("displaced.arc");
        fs::write(&source, vec![b'o'; 128 * 1024 + 17]).unwrap();
        fs::write(&replacement, b"replacement").unwrap();
        let opened = OpenedSource::open_exact(&source, &package_root).unwrap();
        let captured_spool = std::cell::RefCell::new(None::<PathBuf>);
        let replaced = std::cell::Cell::new(false);

        let error = opened
            .into_snapshot_with_hooks(
                |captured| {
                    if !replaced.replace(true) {
                        assert_eq!(captured, 64 * 1024);
                        replace_entry(&source, &replacement, &displaced)?;
                    }
                    Ok(())
                },
                |snapshot| {
                    captured_spool.replace(Some(snapshot.spool_path.clone()));
                    Ok(())
                },
            )
            .unwrap_err();
        assert!(replaced.get());
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("after source spooling"));
        let spool = captured_spool.into_inner().unwrap();
        assert!(!spool.exists());

        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn parent_entry_replacement_is_rejected_even_when_the_file_identity_is_reused() {
        let directory = test_support::unique_directory("snapshot-replace-parent");
        let package_root = fs::canonicalize(&directory).unwrap();
        let parent = package_root.join("data");
        let displaced_parent = package_root.join("displaced-data");
        fs::create_dir(&parent).unwrap();
        let source = parent.join("input.arc");
        fs::write(&source, b"original").unwrap();
        let opened = OpenedSource::open_exact(&source, &package_root).unwrap();
        let captured_spool = std::cell::RefCell::new(None::<PathBuf>);

        let error = opened
            .into_snapshot_with(|snapshot| {
                captured_spool.replace(Some(snapshot.spool_path.clone()));
                fs::rename(&parent, &displaced_parent)?;
                fs::create_dir(&parent)?;
                fs::hard_link(displaced_parent.join("input.arc"), &source)
            })
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("parent directory entry"));
        assert!(!captured_spool.into_inner().unwrap().exists());

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retained_database_reuses_exact_paths_without_reopening() {
        let directory = test_support::unique_directory("database-reuse");
        let package_root = fs::canonicalize(&directory).unwrap();
        let lexical_marker = package_root.join("lexical-marker");
        fs::create_dir(&lexical_marker).unwrap();
        let requested_root = lexical_marker.join("..");
        fs::write(package_root.join("input.arc"), b"original").unwrap();
        let portable = PortablePath::new("input.arc").unwrap();
        let mut builder = SourceDatabaseBuilder::new();
        let first = builder
            .acquire(
                PackageNodeId::new(0),
                &requested_root,
                portable.clone(),
                SourceRole::Module,
            )
            .unwrap();

        fs::remove_dir(&lexical_marker).unwrap();
        fs::write(package_root.join("input.arc"), b"replacement").unwrap();
        let second = builder
            .acquire(
                PackageNodeId::new(0),
                &requested_root,
                portable,
                SourceRole::Include,
            )
            .unwrap();
        assert_eq!(first, second);

        let database = builder.seal();
        assert_eq!(database.files().len(), 1);
        assert_eq!(
            database.file(first).unwrap().roles(),
            &[SourceRole::Module, SourceRole::Include]
        );
        let mut retained = Vec::new();
        database
            .reader(first)
            .unwrap()
            .read_to_end(&mut retained)
            .unwrap();
        assert_eq!(retained, b"original");
        assert_eq!(
            database.source_entries(PackageNodeId::new(0))[0].content_digest,
            IntegrityDigest::of_bytes(b"original")
        );

        drop(database);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retained_database_rejects_a_second_path_to_one_physical_file() {
        let directory = test_support::unique_directory("database-alias");
        let package_root = fs::canonicalize(&directory).unwrap();
        let original = package_root.join("original.arc");
        let alias = package_root.join("alias.arc");
        fs::write(&original, b"same file").unwrap();
        fs::hard_link(&original, &alias).unwrap();

        let mut builder = SourceDatabaseBuilder::new();
        builder
            .acquire(
                PackageNodeId::new(0),
                &package_root,
                PortablePath::new("original.arc").unwrap(),
                SourceRole::Module,
            )
            .unwrap();
        let error = builder
            .acquire(
                PackageNodeId::new(0),
                &package_root,
                PortablePath::new("alias.arc").unwrap(),
                SourceRole::Module,
            )
            .unwrap_err();
        assert_eq!(error.code, "SOURCE004");

        drop(builder);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retained_database_rejects_reusing_one_package_id_under_another_root() {
        let directory = test_support::unique_directory("database-package-root");
        let first_root = directory.join("first");
        let second_root = directory.join("second");
        fs::create_dir_all(&first_root).unwrap();
        fs::create_dir_all(&second_root).unwrap();
        fs::write(first_root.join("lib.arc"), b"first").unwrap();
        fs::write(second_root.join("lib.arc"), b"second").unwrap();

        let mut builder = SourceDatabaseBuilder::new();
        let first = builder
            .acquire(
                PackageNodeId::new(0),
                &first_root,
                PortablePath::new("lib.arc").unwrap(),
                SourceRole::Module,
            )
            .unwrap();
        let error = builder
            .acquire(
                PackageNodeId::new(0),
                &second_root,
                PortablePath::new("lib.arc").unwrap(),
                SourceRole::Module,
            )
            .unwrap_err();
        assert_eq!(error.code, "SOURCE004");
        assert!(error.message.contains("refusing different root"));

        let database = builder.seal();
        assert_eq!(database.files().len(), 1);
        assert_eq!(
            database.file(first).unwrap().package(),
            Some(PackageNodeId::new(0))
        );
        drop(database);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn ordinary_file_ids_are_workspace_global_across_packages() {
        let directory = test_support::unique_directory("database-global-ids");
        let first_root = directory.join("first");
        let second_root = directory.join("second");
        fs::create_dir_all(&first_root).unwrap();
        fs::create_dir_all(&second_root).unwrap();
        fs::write(first_root.join("lib.arc"), b"first").unwrap();
        fs::write(second_root.join("lib.arc"), b"second").unwrap();

        let mut builder = SourceDatabaseBuilder::new();
        let first = builder
            .acquire(
                PackageNodeId::new(0),
                &first_root,
                PortablePath::new("lib.arc").unwrap(),
                SourceRole::Module,
            )
            .unwrap();
        let second = builder
            .acquire(
                PackageNodeId::new(1),
                &second_root,
                PortablePath::new("lib.arc").unwrap(),
                SourceRole::Module,
            )
            .unwrap();
        assert_eq!(first, FileId(0));
        assert_eq!(second, FileId(1));

        let database = builder.seal();
        assert_eq!(database.source_entries(PackageNodeId::new(0)).len(), 1);
        assert_eq!(database.source_entries(PackageNodeId::new(1)).len(), 1);
        drop(database);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retained_database_bounds_snippets_and_cleans_spools() {
        let directory = test_support::unique_directory("database-snippet");
        let package_root = fs::canonicalize(&directory).unwrap();
        fs::write(package_root.join("input.arc"), "aéz".as_bytes()).unwrap();
        let mut builder = SourceDatabaseBuilder::new();
        let file = builder
            .acquire(
                PackageNodeId::new(0),
                &package_root,
                PortablePath::new("input.arc").unwrap(),
                SourceRole::Module,
            )
            .unwrap();
        let spool = builder.files[0].snapshot.spool_path.clone();
        let database = builder.seal();
        let snippet = database
            .bounded_snippet(
                Span {
                    file,
                    start: SourcePosition::START,
                    end: SourcePosition {
                        byte: 4,
                        line: 1,
                        column: 4,
                    },
                },
                3,
            )
            .unwrap();
        assert_eq!(snippet.bytes, "aé".as_bytes());
        assert!(snippet.truncated);
        assert!(spool.exists());
        drop(database);
        assert!(!spool.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn ordinary_file_ids_never_reach_the_embedded_core_reservation() {
        let error = checked_ordinary_file_id_value(u64::MAX).unwrap_err();
        assert_eq!(error.code, "IDENTITY001");
        assert_eq!(
            checked_ordinary_file_id_value(u64::MAX - 1).unwrap(),
            FileId(u64::MAX - 1)
        );
    }

    #[test]
    fn embedded_core_is_hostless_last_and_excluded_from_source_trees() {
        let directory = test_support::unique_directory("database-embedded-core");
        fs::write(directory.join("lib.arc"), b"ordinary").unwrap();
        let mut builder = SourceDatabaseBuilder::new();
        let ordinary = builder
            .acquire(
                PackageNodeId::new(0),
                &directory,
                PortablePath::new("lib.arc").unwrap(),
                SourceRole::Module,
            )
            .unwrap();
        let authority = crate::embedded_core::verified_embedded_core_authority().unwrap();
        builder.install_embedded_core(&authority).unwrap();
        let database = builder.seal();

        assert_eq!(ordinary, FileId(0));
        assert_eq!(
            database
                .files()
                .iter()
                .map(SourceFile::id)
                .collect::<Vec<_>>(),
            vec![ordinary, EMBEDDED_CORE_FILE_ID]
        );
        let embedded = database.file(EMBEDDED_CORE_FILE_ID).unwrap();
        assert!(embedded.is_embedded_core());
        assert_eq!(embedded.package(), None);
        assert_eq!(embedded.diagnostic_path(), None);
        assert_eq!(embedded.roles(), &[SourceRole::EmbeddedCore]);
        let mut retained = Vec::new();
        database
            .reader(EMBEDDED_CORE_FILE_ID)
            .unwrap()
            .read_to_end(&mut retained)
            .unwrap();
        assert_eq!(retained, authority.projection().source().bytes());
        assert_eq!(database.source_entries(PackageNodeId::new(0)).len(), 1);

        drop(database);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn source_positions_pin_cr_lf_tabs_and_unicode_scalars() {
        let mut position = SourcePosition::START;
        advance(&mut position, '\r').unwrap();
        assert_eq!(
            position,
            SourcePosition {
                byte: 1,
                line: 1,
                column: 1
            }
        );
        advance(&mut position, '\n').unwrap();
        assert_eq!(
            position,
            SourcePosition {
                byte: 2,
                line: 2,
                column: 1
            }
        );
        advance(&mut position, '\t').unwrap();
        assert_eq!(
            position,
            SourcePosition {
                byte: 3,
                line: 2,
                column: 2
            }
        );
        advance(&mut position, 'é').unwrap();
        assert_eq!(
            position,
            SourcePosition {
                byte: 5,
                line: 2,
                column: 3
            }
        );
    }

    #[test]
    fn included_string_utf8_validation_is_streaming_and_exact() {
        let mut valid = vec![b'a'; 64 * 1024 - 1];
        valid.extend_from_slice("é".as_bytes());
        valid.extend_from_slice(b"z");
        validate_utf8_reader(std::io::Cursor::new(valid)).unwrap();

        let mut invalid = vec![b'a'; 64 * 1024 - 1];
        invalid.extend_from_slice(&[0xc3, 0x28]);
        let error = validate_utf8_reader(std::io::Cursor::new(invalid)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);

        let truncated = validate_utf8_reader(std::io::Cursor::new([0xf0, 0x9f, 0x92])).unwrap_err();
        assert_eq!(truncated.kind(), io::ErrorKind::InvalidData);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn final_and_parent_links_are_rejected_before_spooling() {
        let directory = test_support::unique_directory("snapshot-links");
        let package_root = fs::canonicalize(&directory).unwrap();
        let target = package_root.join("target.arc");
        let alias = package_root.join("input.arc");
        fs::write(&target, b"target").unwrap();
        if create_file_symlink(&target, &alias).is_err() {
            fs::remove_dir_all(directory).unwrap();
            return;
        }

        let error = OpenedSource::open_exact(&alias, &package_root).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("symbolic-link"));
        fs::remove_file(&alias).unwrap();

        let real_parent = package_root.join("real-parent");
        let parent_alias = package_root.join("parent");
        fs::create_dir(&real_parent).unwrap();
        fs::write(real_parent.join("nested.arc"), b"nested").unwrap();
        if create_directory_symlink(&real_parent, &parent_alias).is_err() {
            fs::remove_dir_all(directory).unwrap();
            return;
        }
        let error =
            OpenedSource::open_exact(&parent_alias.join("nested.arc"), &package_root).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("symbolic-link"));

        fs::remove_dir_all(directory).unwrap();
    }
}
