use crate::output::SourceIdentity;
use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Seek, SeekFrom, Write};
#[cfg(test)]
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const SNAPSHOT_ATTEMPTS: u64 = 128;
const COPY_BUFFER_BYTES: usize = 1024 * 1024;
static NEXT_SNAPSHOT: AtomicU64 = AtomicU64::new(0);

/// One immutable compiler view of a source file.
///
/// The original handle remains open for output-alias checks. All compiler
/// phases read a private spool, so later edits or replacements of the source
/// path cannot change the program halfway through a build.
#[derive(Debug)]
pub struct SourceSnapshot {
    spool_path: PathBuf,
    spool: File,
    identity: SourceIdentity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourcePosition {
    pub byte: u64,
    pub line: u64,
    pub column: u64,
}

impl SourceSnapshot {
    pub fn capture(path: &Path) -> io::Result<Self> {
        let mut original = File::open(path)?;
        let identity = SourceIdentity::from_open_file(path, &original)?;
        let (spool_path, spool_writer) = create_spool(path)?;
        let mut cleanup = PendingSpool::new(spool_path, spool_writer);
        let mut spool = File::open(cleanup.path())?;
        detach_private_spool(cleanup.path())?;

        let _copied_byte_len = {
            let mut writer = BufWriter::with_capacity(COPY_BUFFER_BYTES, cleanup.file_mut());
            let copied = io::copy(&mut original, &mut writer)?;
            writer.flush()?;
            copied
        };
        cleanup.close();
        spool.seek(SeekFrom::Start(0))?;
        let spool_path = cleanup.commit();
        original.seek(SeekFrom::Start(0))?;

        Ok(Self {
            spool_path,
            spool,
            identity,
        })
    }

    #[cfg(test)]
    pub fn byte_len(&self) -> io::Result<u64> {
        Ok(self.spool.metadata()?.len())
    }

    pub fn reader(&self) -> io::Result<BufReader<File>> {
        let mut spool = self.spool.try_clone()?;
        spool.seek(SeekFrom::Start(0))?;
        Ok(BufReader::with_capacity(COPY_BUFFER_BYTES, spool))
    }

    #[cfg(test)]
    pub fn read_to_string(&self) -> io::Result<String> {
        let capacity = usize::try_from(self.byte_len()?).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "source snapshot does not fit in this host address space",
            )
        })?;
        let mut source = String::new();
        source.try_reserve_exact(capacity).map_err(|error| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                format!("could not reserve source snapshot memory: {error}"),
            )
        })?;
        self.reader()?.read_to_string(&mut source)?;
        Ok(source)
    }

    #[cfg(test)]
    pub fn location(&self, byte_offset: u64) -> io::Result<SourcePosition> {
        if byte_offset > self.byte_len()? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "diagnostic byte offset exceeds the source snapshot",
            ));
        }

        let mut reader = self.reader()?;
        let mut line = 1u64;
        let mut column = 1u64;
        let mut remaining = byte_offset;
        let mut utf8_bytes = [0u8; 4];
        let mut utf8_len = 0usize;
        let mut utf8_expected = 0usize;

        while remaining != 0 {
            let buffer = reader.fill_buf()?;
            if buffer.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "source snapshot ended before the diagnostic offset",
                ));
            }
            let consumed =
                usize::try_from(remaining.min(u64::try_from(buffer.len()).map_err(|_| {
                    io::Error::new(io::ErrorKind::OutOfMemory, "reader buffer exceeds u64")
                })?))
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::OutOfMemory,
                        "diagnostic offset exceeds this host address space",
                    )
                })?;

            for &byte in &buffer[..consumed] {
                if utf8_expected != 0 {
                    if byte & 0xc0 != 0x80 {
                        return Err(invalid_utf8_location(byte_offset - remaining));
                    }
                    utf8_bytes[utf8_len] = byte;
                    utf8_len += 1;
                    if utf8_len == utf8_expected {
                        let character = std::str::from_utf8(&utf8_bytes[..utf8_len])
                            .map_err(|_| invalid_utf8_location(byte_offset - remaining))?
                            .chars()
                            .next()
                            .ok_or_else(|| invalid_utf8_location(byte_offset - remaining))?;
                        advance_location(character, &mut line, &mut column)?;
                        utf8_len = 0;
                        utf8_expected = 0;
                    }
                } else if byte.is_ascii() {
                    advance_location(char::from(byte), &mut line, &mut column)?;
                } else {
                    utf8_expected = utf8_width(byte)
                        .ok_or_else(|| invalid_utf8_location(byte_offset - remaining))?;
                    utf8_bytes[0] = byte;
                    utf8_len = 1;
                }
            }
            reader.consume(consumed);
            remaining -= u64::try_from(consumed).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "consumed byte count exceeds u64",
                )
            })?;
        }

        if utf8_expected != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "diagnostic byte offset is inside a UTF-8 character",
            ));
        }
        Ok(SourcePosition {
            byte: byte_offset,
            line,
            column,
        })
    }

    pub fn identity(&self) -> &SourceIdentity {
        &self.identity
    }
}

#[cfg(unix)]
fn detach_private_spool(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

#[cfg(not(unix))]
fn detach_private_spool(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

#[cfg(test)]
fn utf8_width(first: u8) -> Option<usize> {
    match first {
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

#[cfg(test)]
fn advance_location(character: char, line: &mut u64, column: &mut u64) -> io::Result<()> {
    if character == '\n' {
        *line = line.checked_add(1).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "source line number overflows u64",
            )
        })?;
        *column = 1;
    } else if character != '\r' {
        *column = column.checked_add(1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::OutOfMemory, "source column overflows u64")
        })?;
    }
    Ok(())
}

#[cfg(test)]
fn invalid_utf8_location(offset: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("source is not valid UTF-8 near byte {offset}"),
    )
}

impl Drop for SourceSnapshot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.spool_path);
    }
}

fn create_spool(source_path: &Path) -> io::Result<(PathBuf, File)> {
    let temp = env::temp_dir();
    let source_name = source_path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("source"));
    let first = NEXT_SNAPSHOT.fetch_add(1, Ordering::Relaxed);

    for attempt in 0..SNAPSHOT_ATTEMPTS {
        let mut name = OsString::from(".archec0-source-");
        name.push(std::process::id().to_string());
        name.push("-");
        name.push((first + attempt).to_string());
        name.push("-");
        name.push(source_name);
        let path = temp.join(name);
        match create_private_spool_file(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique source snapshot",
    ))
}

#[cfg(windows)]
fn create_private_spool_file(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const FILE_ATTRIBUTE_TEMPORARY: u32 = 0x0000_0100;
    const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;

    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_DELETE_ON_CLOSE)
        .open(path)
}

#[cfg(unix)]
fn create_private_spool_file(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(all(not(unix), not(windows)))]
fn create_private_spool_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
}

struct PendingSpool {
    path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl PendingSpool {
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

    fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("source spool is open")
    }

    fn close(&mut self) {
        drop(self.file.take());
    }

    fn commit(mut self) -> PathBuf {
        self.committed = true;
        self.path.clone()
    }
}

impl Drop for PendingSpool {
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

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn source_mutation_after_capture_does_not_change_snapshot() {
        let directory = TestDirectory::new();
        let source_path = directory.path.join("source.arc");
        fs::write(&source_path, "world Before\nstartup { exit 0 }\n").unwrap();

        let snapshot = SourceSnapshot::capture(&source_path).unwrap();
        fs::write(&source_path, "world After\nstartup { exit 1 }\n").unwrap();

        assert_eq!(
            snapshot.read_to_string().unwrap(),
            "world Before\nstartup { exit 0 }\n"
        );
        assert_eq!(snapshot.byte_len().unwrap(), 32);
    }

    #[test]
    fn snapshot_readers_cannot_mutate_the_private_spool() {
        let directory = TestDirectory::new();
        let source_path = directory.path.join("source.arc");
        fs::write(&source_path, "world Main\nstartup { exit 0 }\n").unwrap();

        let snapshot = SourceSnapshot::capture(&source_path).unwrap();
        let mut reader = snapshot.reader().unwrap();
        reader
            .get_mut()
            .write_all(b"!")
            .expect_err("snapshot readers must retain read-only handles");
        assert_eq!(
            snapshot.read_to_string().unwrap(),
            "world Main\nstartup { exit 0 }\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_spool_creation_is_owner_only_before_detachment() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TestDirectory::new();
        let spool_path = directory.path.join("private-spool");
        let spool = create_private_spool_file(&spool_path).unwrap();
        let mode = spool.metadata().unwrap().permissions().mode() & 0o777;

        assert_eq!(mode, 0o600);
    }

    #[test]
    fn dropping_snapshot_removes_private_spool() {
        let directory = TestDirectory::new();
        let source_path = directory.path.join("source.arc");
        fs::write(&source_path, "world Main\nstartup { exit 0 }\n").unwrap();

        let spool_path = {
            let snapshot = SourceSnapshot::capture(&source_path).unwrap();
            assert!(
                !snapshot.spool_path.exists(),
                "private snapshots detach from the filesystem namespace before copying"
            );
            snapshot.spool_path.clone()
        };

        assert!(!spool_path.exists());
    }

    #[test]
    fn private_spool_cannot_be_reopened_through_its_temporary_name() {
        let directory = TestDirectory::new();
        let source_path = directory.path.join("source.arc");
        fs::write(&source_path, "world Main\nstartup { exit 0 }\n").unwrap();

        let snapshot = SourceSnapshot::capture(&source_path).unwrap();
        assert!(OpenOptions::new()
            .read(true)
            .write(true)
            .open(&snapshot.spool_path)
            .is_err());

        let mut source = String::new();
        snapshot
            .reader()
            .unwrap()
            .read_to_string(&mut source)
            .unwrap();
        assert_eq!(source, "world Main\nstartup { exit 0 }\n");
    }

    #[test]
    fn repeated_readers_start_at_the_beginning_and_locations_are_utf8_aware() {
        let directory = TestDirectory::new();
        let source_path = directory.path.join("source.arc");
        fs::write(
            &source_path,
            "world Main\n\u{2003}\u{2003}startup { exit 0 }\n",
        )
        .unwrap();
        let snapshot = SourceSnapshot::capture(&source_path).unwrap();

        let mut first = String::new();
        snapshot
            .reader()
            .unwrap()
            .read_to_string(&mut first)
            .unwrap();
        let mut second = String::new();
        snapshot
            .reader()
            .unwrap()
            .read_to_string(&mut second)
            .unwrap();

        assert_eq!(first, second);
        let startup_offset = u64::try_from(first.find("startup").unwrap()).unwrap();
        assert_eq!(
            snapshot.location(startup_offset).unwrap(),
            SourcePosition {
                byte: startup_offset,
                line: 2,
                column: 3,
            }
        );
    }

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new() -> Self {
            let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "archec0-source-snapshot-test-{}-{id}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
