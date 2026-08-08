use crate::diagnostic::{io_diagnostic, Diagnostic, DiagnosticCode, Diagnostics};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const ATTEMPTS: u32 = 128;

pub(crate) fn publish_if_changed(path: &Path, bytes: &[u8]) -> Result<bool, Diagnostics> {
    match read_existing(path) {
        Ok(Some(existing)) if existing == bytes => return Ok(false),
        Ok(_) => {}
        Err(error) => return Err(io_diagnostic(path, "read existing lock", &error)),
    }

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(Diagnostic::new(
            DiagnosticCode::WorkspacePath,
            format!(
                "lock parent `{}` is not an existing directory",
                parent.display()
            ),
        )
        .at_path(parent)
        .into());
    }
    let file_name = path.file_name().ok_or_else(|| {
        Diagnostics::from(Diagnostic::new(
            DiagnosticCode::WorkspacePath,
            "lock path must have a file name",
        ))
    })?;

    let (temporary_path, mut temporary) = create_temporary(parent, file_name)?;
    let mut guard = TemporaryGuard::new(temporary_path);
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.sync_all())
        .map_err(|error| io_diagnostic(guard.path(), "write temporary lock", &error))?;
    drop(temporary);
    fs::rename(guard.path(), path)
        .map_err(|error| io_diagnostic(path, "atomically replace lock", &error))?;
    guard.commit();
    Ok(true)
}

fn read_existing(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let length = file.metadata()?.len();
    let capacity = usize::try_from(length)
        .map_err(|_| std::io::Error::other("existing lock does not fit host address space"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| std::io::Error::other("could not allocate existing lock buffer"))?;
    file.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

fn create_temporary(
    parent: &Path,
    file_name: &std::ffi::OsStr,
) -> Result<(PathBuf, File), Diagnostics> {
    for attempt in 0..ATTEMPTS {
        let mut name = OsString::from(".");
        name.push(file_name);
        name.push(format!(".arche-tmp-{}-{attempt}", std::process::id()));
        let path = parent.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_diagnostic(&path, "create temporary lock", &error)),
        }
    }
    Err(Diagnostic::new(
        DiagnosticCode::Io,
        "could not create a unique sibling temporary lock",
    )
    .into())
}

struct TemporaryGuard {
    path: PathBuf,
    committed: bool,
}

impl TemporaryGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for TemporaryGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn publication_replaces_atomically_and_skips_identical_bytes() {
        let directory = test_directory();
        let lock = directory.join("Arche.lock");
        assert!(publish_if_changed(&lock, b"first\n").unwrap());
        assert_eq!(fs::read(&lock).unwrap(), b"first\n");
        assert!(!publish_if_changed(&lock, b"first\n").unwrap());
        assert!(publish_if_changed(&lock, b"second\n").unwrap());
        assert_eq!(fs::read(&lock).unwrap(), b"second\n");
        assert!(fs::read_dir(&directory).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("arche-tmp")));
        fs::remove_dir_all(directory).unwrap();
    }

    fn test_directory() -> PathBuf {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("arche-package-atomic-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        path
    }
}
