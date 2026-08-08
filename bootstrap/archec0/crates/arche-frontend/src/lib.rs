//! M27 package-aware source frontend.
//!
//! This crate is deliberately independent of the closed M26 source checker and
//! Core pipeline. It snapshots and parses explicit target module trees, resolves
//! their namespaces into HIR identifiers, and performs the structural target
//! linking that precedes M27-C type/effect checking.

mod hir;
mod modules;
mod package;
mod source;
mod symbol;
mod syntax;

pub use arche_package::SourceTreeEntry;
pub use hir::{
    dump_resolved_target, CheckTargetRequest, EnvironmentSchedulePaths, HirDefinition,
    HirDefinitionId, HirDefinitionKind, HirModule, HirNamespace, ModuleId, ResolvedTargetHir,
    ResolvedWorkspaceHir, TargetId, TargetKind,
};
pub use modules::{check_target, FrontendError, FrontendErrorCode};
pub use package::{check_manifest_target, check_workspace, check_workspace_target};
pub use source::{Diagnostic, FileId, Label, SourcePosition, Span};
pub use symbol::{case_fold_nfc, normalize_identifier, Symbol};

/// The exact Unicode tables are a source/package compatibility contract.
pub const UNICODE_IDENT_VERSION: (u8, u8, u8) = unicode_ident::UNICODE_VERSION;
pub const UNICODE_NORMALIZATION_VERSION: (u8, u8, u8) = unicode_normalization::UNICODE_VERSION;
pub const UNICODE_CASEFOLD_VERSION: (u64, u64, u64) = unicode_casefold::UNICODE_VERSION;

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn pins_the_selected_unicode_data_versions() {
        assert_eq!(UNICODE_IDENT_VERSION, (17, 0, 0));
        assert_eq!(UNICODE_NORMALIZATION_VERSION, (17, 0, 0));
        assert_eq!(UNICODE_CASEFOLD_VERSION, (9, 0, 0));
    }
}
