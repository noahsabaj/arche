//! ARCHEPKG v1 format encoder, decoder, and integrity validator.

use crate::diagnostic::{Diagnostic, DiagnosticCode, Diagnostics};
use crate::digest::IntegrityDigest;
use crate::name::PortablePath;
use std::fmt;
use std::path::{Path, PathBuf};

pub const ARCHEPKG_MAGIC: &[u8; 9] = b"ARCHEPKG\x01";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveFileEntry {
    pub path: PortablePath,
    pub digest: IntegrityDigest,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedArchive {
    pub manifest_toml: String,
    pub files: Vec<ArchiveFileEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArchiveError {
    MagicMismatch,
    TruncatedData,
    InvalidUtf8,
    InvalidPath(String),
    DuplicatePath(String),
    ChecksumMismatch {
        path: String,
        expected: IntegrityDigest,
        actual: IntegrityDigest,
    },
    Io(String),
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MagicMismatch => write!(f, "invalid ARCHEPKG v1 magic header"),
            Self::TruncatedData => write!(f, "archive binary stream is truncated or malformed"),
            Self::InvalidUtf8 => write!(f, "archive contains non-UTF-8 manifest or filename"),
            Self::InvalidPath(p) => {
                write!(f, "archive contains invalid or unsafe relative path {p}")
            }
            Self::DuplicatePath(p) => write!(f, "archive contains duplicate path {p}"),
            Self::ChecksumMismatch {
                path,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "checksum mismatch for {path}: expected {expected}, actual {actual}"
                )
            }
            Self::Io(e) => write!(f, "I/O error during archive extraction: {e}"),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<ArchiveError> for Diagnostics {
    fn from(err: ArchiveError) -> Self {
        Diagnostic::new(DiagnosticCode::Io, err.to_string()).into()
    }
}

/// Encodes an ARCHEPKG v1 archive.
pub fn encode_archepkg(
    manifest_toml: &str,
    files: &[(PortablePath, &[u8])],
) -> Result<Vec<u8>, ArchiveError> {
    let mut output = Vec::new();
    // 1. Magic (9 bytes)
    output.extend_from_slice(ARCHEPKG_MAGIC);

    // 2. Manifest TOML (u64le length + bytes)
    let manifest_bytes = manifest_toml.as_bytes();
    output.extend_from_slice(&(manifest_bytes.len() as u64).to_le_bytes());
    output.extend_from_slice(manifest_bytes);

    // 3. File count (u64le)
    let file_count = files.len() as u64;
    output.extend_from_slice(&file_count.to_le_bytes());

    // 4. File metadata headers
    let mut seen_paths = std::collections::BTreeSet::new();
    for (path, data) in files {
        let path_str = path.as_str();
        validate_archive_path(path_str)?;
        if !seen_paths.insert(path.clone()) {
            return Err(ArchiveError::DuplicatePath(path_str.to_string()));
        }

        let path_bytes = path_str.as_bytes();
        output.extend_from_slice(&(path_bytes.len() as u64).to_le_bytes());
        output.extend_from_slice(path_bytes);

        output.extend_from_slice(&(data.len() as u64).to_le_bytes());
        let digest = IntegrityDigest::of_bytes(data);
        output.extend_from_slice(digest.as_bytes());
    }

    // 5. File data payloads
    for (_, data) in files {
        output.extend_from_slice(data);
    }

    Ok(output)
}

/// Decodes an ARCHEPKG v1 archive and verifies integrity.
pub fn decode_archepkg(bytes: &[u8]) -> Result<DecodedArchive, ArchiveError> {
    if bytes.len() < ARCHEPKG_MAGIC.len() || &bytes[..ARCHEPKG_MAGIC.len()] != ARCHEPKG_MAGIC {
        return Err(ArchiveError::MagicMismatch);
    }
    let mut offset = ARCHEPKG_MAGIC.len();

    // Read manifest
    if offset + 8 > bytes.len() {
        return Err(ArchiveError::TruncatedData);
    }
    let manifest_len = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) as usize;
    offset += 8;

    if offset + manifest_len > bytes.len() {
        return Err(ArchiveError::TruncatedData);
    }
    let manifest_toml = std::str::from_utf8(&bytes[offset..offset + manifest_len])
        .map_err(|_| ArchiveError::InvalidUtf8)?
        .to_string();
    offset += manifest_len;

    // Read file count
    if offset + 8 > bytes.len() {
        return Err(ArchiveError::TruncatedData);
    }
    let file_count = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) as usize;
    offset += 8;

    // Read file headers
    struct PendingHeader {
        path: PortablePath,
        length: usize,
        digest: IntegrityDigest,
    }
    let mut headers = Vec::with_capacity(file_count);
    let mut seen_paths = std::collections::BTreeSet::new();

    for _ in 0..file_count {
        if offset + 8 > bytes.len() {
            return Err(ArchiveError::TruncatedData);
        }
        let path_len = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) as usize;
        offset += 8;

        if offset + path_len > bytes.len() {
            return Err(ArchiveError::TruncatedData);
        }
        let path_str = std::str::from_utf8(&bytes[offset..offset + path_len])
            .map_err(|_| ArchiveError::InvalidUtf8)?;
        validate_archive_path(path_str)?;
        let path = PortablePath::new(path_str)
            .map_err(|_| ArchiveError::InvalidPath(path_str.to_string()))?;
        offset += path_len;

        if !seen_paths.insert(path.clone()) {
            return Err(ArchiveError::DuplicatePath(path_str.to_string()));
        }

        if offset + 8 + 32 > bytes.len() {
            return Err(ArchiveError::TruncatedData);
        }
        let data_len = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) as usize;
        offset += 8;

        let mut digest_bytes = [0_u8; 32];
        digest_bytes.copy_from_slice(&bytes[offset..offset + 32]);
        let digest = IntegrityDigest::from_bytes(digest_bytes);
        offset += 32;

        headers.push(PendingHeader {
            path,
            length: data_len,
            digest,
        });
    }

    // Read payloads and verify digests
    let mut files = Vec::with_capacity(file_count);
    for header in headers {
        if offset + header.length > bytes.len() {
            return Err(ArchiveError::TruncatedData);
        }
        let payload = bytes[offset..offset + header.length].to_vec();
        offset += header.length;

        let actual_digest = IntegrityDigest::of_bytes(&payload);
        if actual_digest != header.digest {
            return Err(ArchiveError::ChecksumMismatch {
                path: header.path.as_str().to_string(),
                expected: header.digest,
                actual: actual_digest,
            });
        }

        files.push(ArchiveFileEntry {
            path: header.path,
            digest: header.digest,
            data: payload,
        });
    }

    Ok(DecodedArchive {
        manifest_toml,
        files,
    })
}

/// Unpacks a decoded archive to the specified directory.
pub fn unpack_archive(archive: &DecodedArchive, target_dir: &Path) -> Result<(), ArchiveError> {
    std::fs::create_dir_all(target_dir).map_err(|e| ArchiveError::Io(e.to_string()))?;

    // Write Arche.toml
    let manifest_path = target_dir.join("Arche.toml");
    std::fs::write(&manifest_path, &archive.manifest_toml)
        .map_err(|e| ArchiveError::Io(e.to_string()))?;

    // Write files
    for file in &archive.files {
        let rel_path = PathBuf::from(file.path.as_str());
        let full_path = target_dir.join(rel_path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ArchiveError::Io(e.to_string()))?;
        }
        std::fs::write(&full_path, &file.data).map_err(|e| ArchiveError::Io(e.to_string()))?;
    }

    Ok(())
}

fn validate_archive_path(path: &str) -> Result<(), ArchiveError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains("..")
        || path.contains(':')
        || path.contains('\\')
    {
        return Err(ArchiveError::InvalidPath(path.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_round_trips_cleanly() {
        let manifest = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";
        let path1 = PortablePath::new("src/lib.arc").unwrap();
        let path2 = PortablePath::new("src/util.arc").unwrap();
        let files = [
            (path1, b"fn add(a: i32, b: i32) -> i32 { a + b }".as_slice()),
            (path2, b"const ANSWER: i32 = 42;".as_slice()),
        ];

        let encoded = encode_archepkg(manifest, &files).unwrap();
        let decoded = decode_archepkg(&encoded).unwrap();

        assert_eq!(decoded.manifest_toml, manifest);
        assert_eq!(decoded.files.len(), 2);
        assert_eq!(decoded.files[0].path.as_str(), "src/lib.arc");
        assert_eq!(
            decoded.files[0].data,
            b"fn add(a: i32, b: i32) -> i32 { a + b }"
        );
        assert_eq!(decoded.files[1].path.as_str(), "src/util.arc");
        assert_eq!(decoded.files[1].data, b"const ANSWER: i32 = 42;");
    }

    #[test]
    fn archive_rejects_tampered_payload() {
        let manifest = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";
        let path = PortablePath::new("src/lib.arc").unwrap();
        let files = [(path, b"hello".as_slice())];

        let mut encoded = encode_archepkg(manifest, &files).unwrap();
        let last = encoded.len() - 1;
        encoded[last] ^= 0xFF;

        let err = decode_archepkg(&encoded).unwrap_err();
        assert!(matches!(err, ArchiveError::ChecksumMismatch { .. }));
    }

    #[test]
    fn archive_rejects_corrupted_magic() {
        let corrupted = b"ARCHEPKG\x00corrupt_payload_data".to_vec();
        let err = decode_archepkg(&corrupted).unwrap_err();
        assert!(matches!(err, ArchiveError::MagicMismatch));
    }

    #[test]
    fn archive_rejects_truncated_data() {
        let truncated = b"ARCHEPKG\x01".to_vec();
        let err = decode_archepkg(&truncated).unwrap_err();
        assert!(matches!(err, ArchiveError::TruncatedData));
    }

    #[test]
    fn archive_path_validator_rejects_unsafe_paths() {
        assert!(validate_archive_path("../secret.txt").is_err());
        assert!(validate_archive_path("/etc/passwd").is_err());
        assert!(validate_archive_path("C:\\Windows\\System32").is_err());
        assert!(validate_archive_path("nested/../../hack.arc").is_err());
        assert!(validate_archive_path("src/valid.arc").is_ok());
    }
}
