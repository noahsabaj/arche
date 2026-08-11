//! M27 package-aware source frontend.
//!
//! This crate is deliberately independent of the closed M26 source checker and
//! Core pipeline. It snapshots and parses explicit target module trees, resolves
//! their namespaces into HIR identifiers, and performs the structural target
//! linking that precedes M27-C type/effect checking.

pub mod ast;
pub mod embedded_core;
mod hir;
pub mod include_inputs;
pub mod lexer;
mod modules;
mod package;
pub mod parser;
mod source;
mod symbol;
mod syntax;

pub use arche_package::SourceTreeEntry;
pub use hir::{
    check_workspace_c1, declaration_shape_readiness, dump_hir_c1, dump_resolved_target,
    encode_declaration_shape_preimage, encode_definition_owner_entry,
    encode_final_declaration_shape_identity, encode_final_definition_owner_identity,
    encode_generic_arguments, encode_generic_parameters, encode_inventory_c1, encode_method_entry,
    encode_symbolic_const, encode_symbolic_effect, encode_symbolic_effect_set,
    encode_symbolic_predicate, encode_symbolic_predicate_set, encode_symbolic_type,
    owner_shape_readiness, try_canonicalize_declaration_shape, try_canonicalize_definition_owner,
    AssociatedPathCandidate, AssociatedPathOwner, AssociatedPathResolution, BuiltinRes,
    BuiltinResTarget, CallTrait, CanonicalDeclarationShape, CanonicalDefinitionOwner, CaptureMode,
    CheckTargetRequest, CtfeBudgetsSkeleton, DeclarationKind, DependencyKind, EffectKind,
    EnvironmentSchedulePaths, FrontendOutput, GeneratorTarget, GenericArgumentShape,
    GenericParameterId, GenericParameterKind, GenericParameterShape, HiddenLifetimeBinder,
    HiddenLifetimeBinderSource, HirBinding, HirBindingOrigin, HirBindingTarget, HirBodyId,
    HirBodySource, HirDefinition, HirDefinitionId, HirDefinitionKind, HirGenericArgumentUse,
    HirGenericArgumentsUse, HirItemId, HirItemRes, HirItemSource, HirLocalBinding, HirModule,
    HirModuleId, HirNamespace, HirPathUse, HirSelfUse, IntegerType, LocalId, ManifestCapability,
    MaterializedRegistryPackage, MemberVisibilityPath, ModuleId, ModuleRef, Mutability, Namespace,
    PackageDependencySkeleton, PackageProvenanceSkeleton, PackageSourceSkeleton, PathResolution,
    PendingShapeKind, Res, ResolvedGenericArgument, ResolvedSymbolicBody, ResolvedSymbolicConst,
    ResolvedSymbolicEffect, ResolvedSymbolicItem, ResolvedSymbolicLifetime, ResolvedSymbolicModule,
    ResolvedSymbolicPackageHir, ResolvedSymbolicShape, ResolvedSymbolicTargetHir,
    ResolvedSymbolicType, ResolvedSymbolicWorkspaceHir, ResolvedTargetContract, ResolvedTargetHir,
    ResolvedWorkspaceHir, SemanticBindingInventorySkeleton, SemanticBindingOrigin,
    SemanticBindingPath, SemanticBindingTarget, SemanticBodyInventorySkeleton, SemanticBodyKey,
    SemanticBodyKind, SemanticDeclarationPath, SemanticDefinitionInventorySkeleton,
    SemanticDefinitionKey, SemanticInventorySkeleton, SemanticMemberVisibility,
    SemanticModuleInventorySkeleton, SemanticPackageInventorySkeleton,
    SemanticTargetContractSkeleton, SemanticTargetInventorySkeleton, ShapeEncodingError,
    SymbolicCallableKind, SymbolicCallableParameterMode, SymbolicCallableParameterSkeleton,
    SymbolicCallableShapeSkeleton, SymbolicCapabilityAccessMode, SymbolicCapture,
    SymbolicConstExpression, SymbolicConstNode, SymbolicDeclarationPayloadSkeleton,
    SymbolicDeclarationShapeSkeleton, SymbolicDefinitionOwnerSkeleton, SymbolicEffectAtom,
    SymbolicEffectSetsSkeleton, SymbolicEffectShapeSkeleton, SymbolicFieldShapeSkeleton,
    SymbolicImpliedCapabilityRequirementSkeleton, SymbolicLifetime, SymbolicMethodShapeSkeleton,
    SymbolicPendingShape, SymbolicPredicate, SymbolicPredicateShapeSkeleton, SymbolicQueryTermKind,
    SymbolicQueryTermShapeSkeleton, SymbolicRecordForm, SymbolicRecordShapeSkeleton,
    SymbolicShapeReadiness, SymbolicSourceSpan, SymbolicSystemAccessShapeSkeleton, SymbolicType,
    SymbolicTypeEffectSet, SymbolicTypeShapeSkeleton, SymbolicVariantShapeSkeleton, TargetId,
    TargetKind, TargetRoot, UnresolvedPathKind, Visibility, WorkspaceInventorySkeleton,
};
pub use modules::{check_target, FrontendError, FrontendErrorCode};
pub use package::{check_manifest_target, check_workspace, check_workspace_target};
pub use parser::parse_reader;
pub use source::{
    Diagnostic, FileId, Label, SourceDatabase, SourceDatabaseBuilder, SourceFile, SourcePosition,
    SourceRole, SourceSnippet, Span, EMBEDDED_CORE_FILE_ID,
};
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
