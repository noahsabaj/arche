use same_file::Handle;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

const TEMP_FILE_ATTEMPTS: u32 = 128;

#[derive(Debug)]
pub struct SourceIdentity {
    lexical_path: PathBuf,
    handle: Handle,
}

impl SourceIdentity {
    pub fn from_open_file(path: &Path, file: &File) -> io::Result<Self> {
        Ok(Self {
            lexical_path: normalized_absolute_path(path)?,
            handle: Handle::from_file(file.try_clone()?)?,
        })
    }
}

#[derive(Debug)]
pub enum PublishError {
    SourceOutputAlias,
    Io(io::Error),
}

impl fmt::Display for PublishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceOutputAlias => {
                formatter.write_str("output resolves to the input source file")
            }
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PublishError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SourceOutputAlias => None,
            Self::Io(error) => Some(error),
        }
    }
}

impl From<io::Error> for PublishError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Publish bytes without exposing a partially written output.
///
/// The repeated identity checks prevent ordinary path, symlink, and hard-link
/// alias mistakes. They are not a defense against a malicious process racing
/// namespace replacements between the final check and rename.
pub fn publish(source: &SourceIdentity, output: &Path, bytes: &[u8]) -> Result<(), PublishError> {
    publish_with_failure(source, output, bytes, FailurePoint::None)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailurePoint {
    None,
    #[cfg(test)]
    AfterPartialWrite,
    #[cfg(test)]
    BeforeRename,
}

fn publish_with_failure(
    source: &SourceIdentity,
    output: &Path,
    bytes: &[u8],
    _failure: FailurePoint,
) -> Result<(), PublishError> {
    ensure_distinct_from_source(source, output)?;

    let parent = output_parent(output);
    fs::create_dir_all(parent)?;
    ensure_distinct_from_source(source, output)?;

    let (temporary_path, temporary_file) = create_sibling_temporary(output, parent)?;
    let mut temporary = TemporaryOutput::new(temporary_path, temporary_file);

    #[cfg(test)]
    if _failure == FailurePoint::AfterPartialWrite {
        let partial_len = bytes.len().div_ceil(2);
        temporary.file_mut().write_all(&bytes[..partial_len])?;
        return Err(PublishError::Io(io::Error::other(
            "injected partial-write failure",
        )));
    }

    temporary.file_mut().write_all(bytes)?;
    temporary.file_mut().flush()?;
    make_executable(temporary.file())?;
    temporary.file().sync_all()?;
    temporary.close();

    ensure_distinct_from_source(source, output)?;
    #[cfg(test)]
    if _failure == FailurePoint::BeforeRename {
        return Err(PublishError::Io(io::Error::other(
            "injected rename failure",
        )));
    }

    fs::rename(temporary.path(), output)?;
    temporary.commit();
    Ok(())
}

fn ensure_distinct_from_source(source: &SourceIdentity, output: &Path) -> Result<(), PublishError> {
    let output_path = normalized_absolute_path(output)?;
    if lexical_paths_equal(&source.lexical_path, &output_path) {
        return Err(PublishError::SourceOutputAlias);
    }

    match Handle::from_path(output) {
        Ok(output_handle) if output_handle == source.handle => Err(PublishError::SourceOutputAlias),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PublishError::Io(error)),
    }
}

fn normalized_absolute_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };

    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    Ok(normalized)
}

#[cfg(windows)]
fn lexical_paths_equal(left: &Path, right: &Path) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

#[cfg(not(windows))]
fn lexical_paths_equal(left: &Path, right: &Path) -> bool {
    left == right
}

fn output_parent(output: &Path) -> &Path {
    output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn create_sibling_temporary(output: &Path, parent: &Path) -> io::Result<(PathBuf, File)> {
    let file_name = output.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "output path must have a file name",
        )
    })?;

    for attempt in 0..TEMP_FILE_ATTEMPTS {
        let mut temporary_name = OsString::from(".");
        temporary_name.push(file_name);
        temporary_name.push(format!(".archec0-tmp-{}-{attempt}", std::process::id()));
        let temporary_path = parent.join(temporary_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique sibling temporary output",
    ))
}

#[cfg(unix)]
fn make_executable(file: &File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = file.metadata()?.permissions();
    let mode = permissions.mode();
    let executable_mode = mode | 0o100 | ((mode & 0o044) >> 2);
    permissions.set_mode(executable_mode);
    file.set_permissions(permissions)
}

#[cfg(not(unix))]
fn make_executable(_file: &File) -> io::Result<()> {
    Ok(())
}

struct TemporaryOutput {
    path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl TemporaryOutput {
    fn new(path: PathBuf, file: File) -> Self {
        Self {
            path,
            file: Some(file),
            committed: false,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn file(&self) -> &File {
        self.file.as_ref().expect("temporary output is still open")
    }

    fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("temporary output is still open")
    }

    fn close(&mut self) {
        drop(self.file.take());
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        self.close();
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn publishes_distinct_output_and_replaces_existing_artifact() {
        let directory = TestDirectory::new();
        let source_path = directory.path().join("source.arc");
        let output_path = directory.path().join("program");
        fs::write(&source_path, "world Main {}\n").unwrap();
        fs::write(&output_path, b"old artifact").unwrap();
        let (_source_file, source) = open_source(&source_path);

        publish(&source, &output_path, b"new artifact").unwrap();

        assert_eq!(fs::read(output_path).unwrap(), b"new artifact");
        assert_eq!(fs::read(source_path).unwrap(), b"world Main {}\n");
        assert_no_temporary_files(directory.path());
    }

    #[test]
    fn rejects_exact_and_relative_source_aliases() {
        let directory = TestDirectory::new();
        let source_path = directory.path().join("source.arc");
        fs::write(&source_path, "source stays intact").unwrap();
        let (_source_file, source) = open_source(&source_path);

        assert!(matches!(
            publish(&source, &source_path, b"replacement"),
            Err(PublishError::SourceOutputAlias)
        ));

        let relative_alias = directory
            .path()
            .join("missing")
            .join("..")
            .join("source.arc");
        assert!(matches!(
            publish(&source, &relative_alias, b"replacement"),
            Err(PublishError::SourceOutputAlias)
        ));
        assert!(!directory.path().join("missing").exists());
        assert_eq!(fs::read(source_path).unwrap(), b"source stays intact");
    }

    #[test]
    fn rejects_hard_link_source_alias() {
        let directory = TestDirectory::new();
        let source_path = directory.path().join("source.arc");
        let output_path = directory.path().join("source-hard-link.arc");
        fs::write(&source_path, "source stays intact").unwrap();
        fs::hard_link(&source_path, &output_path).unwrap();
        let (_source_file, source) = open_source(&source_path);

        assert!(matches!(
            publish(&source, &output_path, b"replacement"),
            Err(PublishError::SourceOutputAlias)
        ));
        assert_eq!(fs::read(source_path).unwrap(), b"source stays intact");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symbolic_link_source_alias() {
        use std::os::unix::fs::symlink;

        let directory = TestDirectory::new();
        let source_path = directory.path().join("source.arc");
        let output_path = directory.path().join("source-symbolic-link.arc");
        fs::write(&source_path, "source stays intact").unwrap();
        symlink(&source_path, &output_path).unwrap();
        let (_source_file, source) = open_source(&source_path);

        assert!(matches!(
            publish(&source, &output_path, b"replacement"),
            Err(PublishError::SourceOutputAlias)
        ));
        assert_eq!(fs::read(source_path).unwrap(), b"source stays intact");
    }

    #[test]
    fn partial_write_failure_preserves_existing_artifact_and_cleans_temporary() {
        assert_failure_preserves_output(FailurePoint::AfterPartialWrite);
    }

    #[test]
    fn rename_failure_preserves_existing_artifact_and_cleans_temporary() {
        assert_failure_preserves_output(FailurePoint::BeforeRename);
    }

    #[test]
    fn real_rename_failure_preserves_source_destination_and_cleans_temporary() {
        let directory = TestDirectory::new();
        let source_path = directory.path().join("source.arc");
        let output_path = directory.path().join("existing-directory");
        let destination_entry = output_path.join("sentinel");
        fs::write(&source_path, "source stays intact").unwrap();
        fs::create_dir(&output_path).unwrap();
        fs::write(&destination_entry, "destination stays intact").unwrap();
        let (_source_file, source) = open_source(&source_path);

        let result = publish(&source, &output_path, b"replacement");

        assert!(matches!(result, Err(PublishError::Io(_))));
        assert_eq!(fs::read(source_path).unwrap(), b"source stays intact");
        assert_eq!(
            fs::read(destination_entry).unwrap(),
            b"destination stays intact"
        );
        assert_no_temporary_files(directory.path());
    }

    #[cfg(unix)]
    #[test]
    fn published_output_is_executable() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TestDirectory::new();
        let source_path = directory.path().join("source.arc");
        let output_path = directory.path().join("program");
        fs::write(&source_path, "world Main {}\n").unwrap();
        let (_source_file, source) = open_source(&source_path);

        publish(&source, &output_path, b"artifact").unwrap();

        let mode = fs::metadata(output_path).unwrap().permissions().mode();
        assert_ne!(mode & 0o100, 0, "owner execute bit was not set");
        if mode & 0o040 != 0 {
            assert_ne!(mode & 0o010, 0, "group read did not imply execute");
        }
        if mode & 0o004 != 0 {
            assert_ne!(mode & 0o001, 0, "other read did not imply execute");
        }
    }

    fn assert_failure_preserves_output(failure: FailurePoint) {
        let directory = TestDirectory::new();
        let source_path = directory.path().join("source.arc");
        let output_path = directory.path().join("program");
        fs::write(&source_path, "world Main {}\n").unwrap();
        fs::write(&output_path, b"old artifact").unwrap();
        let (_source_file, source) = open_source(&source_path);

        let result = publish_with_failure(&source, &output_path, b"new artifact", failure);

        assert!(matches!(result, Err(PublishError::Io(_))));
        assert_eq!(fs::read(output_path).unwrap(), b"old artifact");
        assert_no_temporary_files(directory.path());
    }

    fn open_source(path: &Path) -> (File, SourceIdentity) {
        let file = File::open(path).unwrap();
        let identity = SourceIdentity::from_open_file(path, &file).unwrap();
        (file, identity)
    }

    fn assert_no_temporary_files(directory: &Path) {
        let temporary_count = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().contains(".archec0-tmp-"))
            .count();
        assert_eq!(temporary_count, 0);
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            for _ in 0..TEMP_FILE_ATTEMPTS {
                let unique = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
                let path = env::temp_dir().join(format!(
                    "archec0-output-test-{}-{unique}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("could not create test directory: {error}"),
                }
            }
            panic!("could not create a unique test directory");
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
