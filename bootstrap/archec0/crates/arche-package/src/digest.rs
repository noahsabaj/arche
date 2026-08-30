use crate::diagnostic::{Diagnostic, DiagnosticCode, Diagnostics};
use crate::PortablePath;
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

const SOURCE_TREE_DOMAIN: &[u8] = b"ARCHE-SOURCE-TREE\0";
const SOURCE_TREE_VERSION: u32 = 1;

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IntegrityDigest([u8; 32]);

impl IntegrityDigest {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn of_bytes(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut output = [0_u8; 32];
        output.copy_from_slice(&digest);
        Self(output)
    }
}

impl fmt::Display for IntegrityDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sha256:")?;
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for IntegrityDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "IntegrityDigest({self})")
    }
}

impl FromStr for IntegrityDigest {
    type Err = Diagnostics;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some(hex) = value.strip_prefix("sha256:") else {
            return Err(invalid_digest(value));
        };
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid_digest(value));
        }
        let mut bytes = [0_u8; 32];
        for (index, target) in bytes.iter_mut().enumerate() {
            let offset = index * 2;
            *target = u8::from_str_radix(&hex[offset..offset + 2], 16)
                .map_err(|_| invalid_digest(value))?;
        }
        Ok(Self(bytes))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SourceTreeEntry {
    pub path: PortablePath,
    pub byte_length: u64,
    pub content_digest: IntegrityDigest,
}

impl SourceTreeEntry {
    pub fn from_bytes(path: PortablePath, bytes: &[u8]) -> Result<Self, Diagnostics> {
        Ok(Self {
            path,
            byte_length: u64::try_from(bytes.len()).map_err(|_| {
                Diagnostics::from(Diagnostic::new(
                    DiagnosticCode::WorkspacePath,
                    "source input length does not fit u64",
                ))
            })?,
            content_digest: IntegrityDigest::of_bytes(bytes),
        })
    }
}

fn invalid_digest(value: &str) -> Diagnostics {
    Diagnostic::new(
        DiagnosticCode::ManifestValue,
        format!(
            "invalid integrity digest `{value}`; expected `sha256:` and 64 lowercase hex digits"
        ),
    )
    .into()
}

/// Computes the schema-1 source-tree digest from immutable per-input commits.
pub fn source_tree_digest(entries: &[SourceTreeEntry]) -> Result<IntegrityDigest, Diagnostics> {
    let mut sorted = entries.to_vec();
    sorted.sort();
    if sorted.windows(2).any(|pair| pair[0].path == pair[1].path) {
        return Err(Diagnostic::new(
            DiagnosticCode::WorkspacePath,
            "source-tree input paths are not unique",
        )
        .into());
    }

    let mut hasher = Sha256::new();
    hasher.update(SOURCE_TREE_DOMAIN);
    hasher.update(SOURCE_TREE_VERSION.to_le_bytes());
    hasher.update(
        u64::try_from(sorted.len())
            .map_err(|_| {
                Diagnostics::from(Diagnostic::new(
                    DiagnosticCode::WorkspacePath,
                    "source-tree entry count does not fit u64",
                ))
            })?
            .to_le_bytes(),
    );

    for entry in sorted {
        let path_bytes = entry.path.as_str().as_bytes();
        hasher.update(
            u64::try_from(path_bytes.len())
                .map_err(|_| {
                    Diagnostics::from(Diagnostic::new(
                        DiagnosticCode::WorkspacePath,
                        "source path length does not fit u64",
                    ))
                })?
                .to_le_bytes(),
        );
        hasher.update(path_bytes);
        hasher.update(entry.byte_length.to_le_bytes());
        hasher.update(entry.content_digest.as_bytes());
    }
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&digest);
    Ok(IntegrityDigest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_text_is_canonical_and_round_trips() {
        let digest = IntegrityDigest::of_bytes(b"Arche");
        let text = digest.to_string();
        assert_eq!(text.len(), 71);
        assert_eq!(text.parse::<IntegrityDigest>().unwrap(), digest);
        assert!(text.to_uppercase().parse::<IntegrityDigest>().is_err());
    }

    #[test]
    fn sha256_golden_is_exact() {
        assert_eq!(
            IntegrityDigest::of_bytes(b"abc").to_string(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn source_tree_is_order_independent_but_path_sensitive() {
        let first =
            SourceTreeEntry::from_bytes(PortablePath::new("src/a.arc").unwrap(), b"a").unwrap();
        let second =
            SourceTreeEntry::from_bytes(PortablePath::new("src/b.arc").unwrap(), b"b").unwrap();
        assert_eq!(
            source_tree_digest(&[first.clone(), second.clone()]).unwrap(),
            source_tree_digest(&[second.clone(), first.clone()]).unwrap()
        );
        let renamed = SourceTreeEntry {
            path: PortablePath::new("src/c.arc").unwrap(),
            ..second.clone()
        };
        assert_ne!(
            source_tree_digest(&[first.clone(), renamed]).unwrap(),
            source_tree_digest(&[first, second]).unwrap()
        );
    }
}
