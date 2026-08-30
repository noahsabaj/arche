//! Content-addressed package cache management for Arche.

use crate::archive::{decode_archepkg, unpack_archive, ArchiveError};
use crate::diagnostic::{Diagnostic, DiagnosticCode, Diagnostics};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageCache {
    root: PathBuf,
    offline: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CacheError {
    Archive(ArchiveError),
    Io(String),
    OfflineMissingPackage { name: String, version: String },
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Archive(e) => write!(f, "cache archive error: {e}"),
            Self::Io(e) => write!(f, "cache I/O error: {e}"),
            Self::OfflineMissingPackage { name, version } => {
                write!(f, "package {name}@{version} is missing from offline cache; cannot resolve in offline mode")
            }
        }
    }
}

impl std::error::Error for CacheError {}

impl From<ArchiveError> for CacheError {
    fn from(err: ArchiveError) -> Self {
        Self::Archive(err)
    }
}

impl From<CacheError> for Diagnostics {
    fn from(err: CacheError) -> Self {
        Diagnostic::new(DiagnosticCode::Io, err.to_string()).into()
    }
}

impl PackageCache {
    /// Creates a PackageCache using the standard platform cache location.
    pub fn default_location(offline: bool) -> Self {
        let root = match std::env::var_os("ARCHE_HOME") {
            Some(home) => PathBuf::from(home).join("cache"),
            None => {
                #[cfg(windows)]
                {
                    match std::env::var_os("LOCALAPPDATA") {
                        Some(app_data) => PathBuf::from(app_data).join("arche").join("cache"),
                        None => PathBuf::from(".").join(".arche").join("cache"),
                    }
                }
                #[cfg(not(windows))]
                {
                    match std::env::var_os("HOME") {
                        Some(home) => PathBuf::from(home).join(".arche").join("cache"),
                        None => PathBuf::from(".").join(".arche").join("cache"),
                    }
                }
            }
        };
        Self { root, offline }
    }

    /// Creates a PackageCache at an explicit path.
    pub fn at_path(root: PathBuf, offline: bool) -> Self {
        Self { root, offline }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn is_offline(&self) -> bool {
        self.offline
    }

    pub fn archives_dir(&self) -> PathBuf {
        self.root.join("archives")
    }

    pub fn sources_dir(&self) -> PathBuf {
        self.root.join("src")
    }

    pub fn archive_path(&self, package_name: &str, version: &str) -> PathBuf {
        self.archives_dir()
            .join(format!("{package_name}-{version}.archepkg"))
    }

    pub fn source_path(&self, package_name: &str, version: &str) -> PathBuf {
        self.sources_dir().join(package_name).join(version)
    }

    /// Checks if a package version source directory exists and is valid.
    pub fn has_package(&self, package_name: &str, version: &str) -> bool {
        let src = self.source_path(package_name, version);
        src.join("Arche.toml").is_file()
    }

    /// Retrieves package source directory if present, or errors in offline mode.
    pub fn get_package_source(
        &self,
        package_name: &str,
        version: &str,
    ) -> Result<Option<PathBuf>, CacheError> {
        let src = self.source_path(package_name, version);
        if src.join("Arche.toml").is_file() {
            Ok(Some(src))
        } else if self.offline {
            Err(CacheError::OfflineMissingPackage {
                name: package_name.to_string(),
                version: version.to_string(),
            })
        } else {
            Ok(None)
        }
    }

    /// Stores an ARCHEPKG archive and unpacks its source into cache.
    pub fn store_package(
        &self,
        package_name: &str,
        version: &str,
        archive_bytes: &[u8],
    ) -> Result<PathBuf, CacheError> {
        let decoded = decode_archepkg(archive_bytes)?;

        // Ensure directories
        std::fs::create_dir_all(self.archives_dir()).map_err(|e| CacheError::Io(e.to_string()))?;
        let src_dir = self.source_path(package_name, version);
        std::fs::create_dir_all(&src_dir).map_err(|e| CacheError::Io(e.to_string()))?;

        // Write archive file
        let archive_file = self.archive_path(package_name, version);
        std::fs::write(&archive_file, archive_bytes).map_err(|e| CacheError::Io(e.to_string()))?;

        // Unpack sources
        unpack_archive(&decoded, &src_dir)?;

        Ok(src_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::encode_archepkg;
    use crate::name::PortablePath;

    #[test]
    fn cache_stores_and_retrieves_packages() {
        let temp_dir = std::env::temp_dir().join("arche_test_cache_1");
        let _ = std::fs::remove_dir_all(&temp_dir);

        let cache = PackageCache::at_path(temp_dir.clone(), false);
        assert!(!cache.has_package("math", "1.0.0"));

        let manifest = "[package]\nname = \"math\"\nversion = \"1.0.0\"\narche = \"0.0.0\"\nkind = \"library\"\n";
        let path = PortablePath::new("src/lib.arc").unwrap();
        let files = [(
            path,
            b"pub fn add(a: i32, b: i32) -> i32 { a + b }".as_slice(),
        )];

        let archive_bytes = encode_archepkg(manifest, &files).unwrap();
        let src_dir = cache
            .store_package("math", "1.0.0", &archive_bytes)
            .unwrap();

        assert!(cache.has_package("math", "1.0.0"));
        assert!(src_dir.join("Arche.toml").is_file());
        assert!(src_dir.join("src").join("lib.arc").is_file());

        let retrieved = cache.get_package_source("math", "1.0.0").unwrap();
        assert_eq!(retrieved, Some(src_dir));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn offline_cache_fails_on_missing_package() {
        let temp_dir = std::env::temp_dir().join("arche_test_cache_offline");
        let _ = std::fs::remove_dir_all(&temp_dir);

        let cache = PackageCache::at_path(temp_dir.clone(), true);
        let err = cache
            .get_package_source("nonexistent", "0.1.0")
            .unwrap_err();
        assert!(matches!(err, CacheError::OfflineMissingPackage { .. }));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
