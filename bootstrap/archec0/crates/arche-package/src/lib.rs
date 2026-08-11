//! Deterministic package, workspace, resolver, and lock contracts for M27.
//!
//! This crate deliberately knows nothing about Arche source syntax. The public
//! driver and the M27 frontend consume it; the closed M26 `archec0` compiler
//! does not.

mod atomic;
mod diagnostic;
mod digest;
mod lock;
mod manifest;
mod name;
mod resolver;
mod workspace;

pub use diagnostic::{Diagnostic, DiagnosticCode, Diagnostics, SourceLabel};
pub use digest::{source_tree_digest, IntegrityDigest, SourceTreeEntry};
pub use lock::{
    DependencyRequirement, LockDependency, LockDependencyKind, LockPackage, LockSource, Lockfile,
    RegistryLock, ToolchainLock, WorkspaceLock,
};
pub use manifest::{
    BinaryTarget, Capability, ConstEvalBudgets, Dependency, DependencyKind, EnvironmentProfile,
    EnvironmentTarget, LibTarget, Manifest, ManifestSpan, Package, PublishMetadata, Target,
    TargetKind,
};
pub use name::{
    canonical_package_id, DependencyAlias, DependencyPath, ItemPath, ItemPathRoot, PackageName,
    PortablePath, SourceIdentifier, OFFICIAL_REGISTRY_IDENTITY,
};
pub use resolver::{
    resolve, PackageNodeId, RegistryDependency, RegistryRelease, RegistrySnapshot,
    ResolvedDependency, ResolvedGraph, ResolvedPackage, ResolvedSource,
};
pub use workspace::{
    discover_manifest, load_workspace, ManifestRequest, Workspace, WorkspaceMember,
};

/// Exact Unicode table versions used by schema 1.
pub const UNICODE_IDENT_VERSION: (u8, u8, u8) = unicode_ident::UNICODE_VERSION;
pub const UNICODE_NORMALIZATION_VERSION: (u8, u8, u8) = unicode_normalization::UNICODE_VERSION;
pub const UNICODE_CASEFOLD_VERSION: (u64, u64, u64) = unicode_casefold::UNICODE_VERSION;

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn unicode_tables_are_deliberately_pinned() {
        assert_eq!(UNICODE_IDENT_VERSION, (17, 0, 0));
        assert_eq!(UNICODE_NORMALIZATION_VERSION, (17, 0, 0));
        assert_eq!(UNICODE_CASEFOLD_VERSION, (9, 0, 0));
    }
}
