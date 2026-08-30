use crate::archive::decode_archepkg;
use crate::digest::IntegrityDigest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Credentials {
    pub token: Option<String>,
    pub username: Option<String>,
}

impl Credentials {
    pub fn default_path() -> PathBuf {
        let base = if let Ok(custom) = std::env::var("ARCHE_HOME") {
            PathBuf::from(custom)
        } else if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            PathBuf::from(local_app_data).join("arche")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".arche")
        } else {
            PathBuf::from(".arche")
        };
        base.join("credentials.toml")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::default_path())
    }

    pub fn load_from(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            return Self::default();
        };

        let mut creds = Self::default();
        for line in content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("token =") {
                creds.token = Some(rest.trim().trim_matches('"').to_string());
            } else if let Some(rest) = line.strip_prefix("username =") {
                creds.username = Some(rest.trim().trim_matches('"').to_string());
            }
        }
        creds
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::default_path())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut content = String::new();
        content.push_str("# Arche registry credentials\n");
        if let Some(token) = &self.token {
            content.push_str(&format!("token = \"{token}\"\n"));
        }
        if let Some(username) = &self.username {
            content.push_str(&format!("username = \"{username}\"\n"));
        }
        std::fs::write(path, content)
    }

    pub fn clear(&self) -> std::io::Result<()> {
        let p = Self::default_path();
        if p.exists() {
            std::fs::remove_file(p)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct RegistryPackageRelease {
    pub version: String,
    pub checksum: IntegrityDigest,
    pub yanked: bool,
    pub archive_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct RegistryPackageEntry {
    pub name: String,
    pub owners: Vec<String>,
    pub trusted_publishers: Vec<String>,
    pub releases: BTreeMap<String, RegistryPackageRelease>,
}

#[derive(Clone, Debug, Default)]
pub struct LocalRegistryServer {
    pub root: PathBuf,
    pub scopes: Vec<String>,
    pub packages: BTreeMap<String, RegistryPackageEntry>,
}

impl LocalRegistryServer {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            scopes: vec![
                "core".to_string(),
                "std".to_string(),
                "app".to_string(),
                "example".to_string(),
            ],
            packages: BTreeMap::new(),
        }
    }

    pub fn create_scope(&mut self, scope: &str) -> Result<(), String> {
        if self.scopes.iter().any(|s| s == scope) {
            return Err(format!("scope `{scope}` already exists"));
        }
        self.scopes.push(scope.to_string());
        Ok(())
    }

    pub fn publish_package(
        &mut self,
        archive_bytes: &[u8],
        user: &str,
    ) -> Result<(String, String), String> {
        let decoded =
            decode_archepkg(archive_bytes).map_err(|e| format!("invalid package archive: {e}"))?;
        let manifest = crate::Manifest::parse(Path::new("Arche.toml"), &decoded.manifest_toml)
            .map_err(|e| format!("invalid manifest: {e}"))?;
        let pkg = manifest
            .package
            .as_ref()
            .ok_or_else(|| "manifest missing [package]".to_string())?;

        let name = pkg.name.as_str();
        let version = pkg.version.to_string();

        let entry = self
            .packages
            .entry(name.to_string())
            .or_insert_with(|| RegistryPackageEntry {
                name: name.to_string(),
                owners: vec![user.to_string()],
                trusted_publishers: Vec::new(),
                releases: BTreeMap::new(),
            });

        if entry.releases.contains_key(&version) {
            return Err(format!("release `{name}@{version}` already published"));
        }

        let checksum = IntegrityDigest::of_bytes(archive_bytes);
        entry.releases.insert(
            version.clone(),
            RegistryPackageRelease {
                version: version.clone(),
                checksum,
                yanked: false,
                archive_bytes: archive_bytes.to_vec(),
            },
        );

        Ok((name.to_string(), version))
    }

    pub fn yank_release(&mut self, name: &str, version: &str) -> Result<(), String> {
        let pkg = self
            .packages
            .get_mut(name)
            .ok_or_else(|| format!("package `{name}` not found"))?;
        let rel = pkg
            .releases
            .get_mut(version)
            .ok_or_else(|| format!("release `{name}@{version}` not found"))?;
        rel.yanked = true;
        Ok(())
    }

    pub fn unyank_release(&mut self, name: &str, version: &str) -> Result<(), String> {
        let pkg = self
            .packages
            .get_mut(name)
            .ok_or_else(|| format!("package `{name}` not found"))?;
        let rel = pkg
            .releases
            .get_mut(version)
            .ok_or_else(|| format!("release `{name}@{version}` not found"))?;
        rel.yanked = false;
        Ok(())
    }

    pub fn search(&self, query: &str) -> Vec<&RegistryPackageEntry> {
        self.packages
            .values()
            .filter(|p| p.name.contains(query))
            .collect()
    }
}
