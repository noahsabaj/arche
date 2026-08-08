use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use arche_package::IntegrityDigest;
use same_file::Handle;
use sha2::{Digest, Sha256};

static NEXT_SNAPSHOT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FileId(pub u64);

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
    pub(crate) fn at(code: &'static str, span: Span, message: impl Into<String>) -> Self {
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

    pub(crate) fn path(code: &'static str, message: impl Into<String>) -> Self {
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

    pub(crate) fn with_secondary(mut self, span: Span, message: impl Into<String>) -> Self {
        self.secondary.push(Label {
            span: Some(span),
            message: message.into(),
        });
        self
    }

    pub(crate) fn with_note(mut self, note: impl Into<String>) -> Self {
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
    original_path: PathBuf,
    spool_path: PathBuf,
    spool: Option<File>,
    byte_length: u64,
    content_digest: IntegrityDigest,
}

#[derive(Debug)]
pub(crate) struct OpenedSource {
    canonical_path: PathBuf,
    input: File,
    identity: Handle,
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

    fn open_with<F>(
        path: &Path,
        expected_canonical: &Path,
        package_root: &Path,
        opener: F,
    ) -> io::Result<Self>
    where
        F: FnOnce(&Path) -> io::Result<File>,
    {
        ensure_no_path_aliases(path, package_root)?;
        let canonical_before = fs::canonicalize(path)?;
        if canonical_before != expected_canonical || !canonical_before.starts_with(package_root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "source {} changed its package-contained destination before opening",
                    path.display()
                ),
            ));
        }

        let expected_identity = Handle::from_path(&canonical_before)?;
        let input = opener(path)?;
        if !input.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("source {} is not a regular file", path.display()),
            ));
        }
        let identity = Handle::from_file(input.try_clone()?)?;

        ensure_no_path_aliases(path, package_root)?;
        let canonical_after = fs::canonicalize(path)?;
        if canonical_after != canonical_before || !canonical_after.starts_with(package_root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "source {} changed its package-contained destination while opening",
                    path.display()
                ),
            ));
        }
        let current_identity = Handle::from_path(&canonical_after)?;
        if identity != expected_identity || identity != current_identity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("source {} changed identity while opening", path.display()),
            ));
        }

        Ok(Self {
            canonical_path: canonical_after,
            input,
            identity,
        })
    }

    pub(crate) const fn identity(&self) -> &Handle {
        &self.identity
    }

    pub(crate) fn into_snapshot(self) -> io::Result<(SourceSnapshot, Handle)> {
        let Self {
            canonical_path,
            input,
            identity,
        } = self;
        let snapshot = SourceSnapshot::capture(&canonical_path, input)?;
        Ok((snapshot, identity))
    }
}

impl SourceSnapshot {
    fn capture(path: &Path, mut input: File) -> io::Result<Self> {
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
        }
        writer.flush()?;
        let mut spool = writer.into_inner().map_err(|error| error.into_error())?;
        spool.seek(SeekFrom::Start(0))?;
        let mut digest = [0_u8; 32];
        digest.copy_from_slice(&hasher.finalize());
        cleanup.disarm();
        Ok(Self {
            original_path: path.to_path_buf(),
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
        &self.original_path
    }

    pub(crate) const fn byte_length(&self) -> u64 {
        self.byte_length
    }

    pub(crate) const fn content_digest(&self) -> IntegrityDigest {
        self.content_digest
    }
}

fn ensure_no_path_aliases(path: &Path, package_root: &Path) -> io::Result<()> {
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
    let mut current = package_root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current)?;
        if is_link_or_reparse_point(&metadata) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "source path {} contains a symbolic-link or reparse-point alias at {}",
                    path.display(),
                    current.display()
                ),
            ));
        }
    }
    Ok(())
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
    } else {
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

        let error = OpenedSource::open_with(&source, &canonical, &package_root, |_| {
            File::open(&replacement)
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("changed identity while opening"));

        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_sources_are_rejected_before_spooling() {
        use std::os::unix::fs::symlink;

        let directory = test_support::unique_directory("snapshot-symlink");
        let package_root = fs::canonicalize(&directory).unwrap();
        let target = package_root.join("target.arc");
        let alias = package_root.join("input.arc");
        fs::write(&target, b"target").unwrap();
        symlink(&target, &alias).unwrap();
        let canonical = fs::canonicalize(&alias).unwrap();

        let error = OpenedSource::open(&alias, &canonical, &package_root).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("symbolic-link"));

        fs::remove_dir_all(directory).unwrap();
    }
}
