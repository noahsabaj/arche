//! M27-C2 semantic authority and consuming result boundary.
//!
//! The public surface is intentionally read-only: compiler checking modules
//! may construct terminal rows inside this crate, while consumers can only
//! inspect branded handles, borrow-tied views, deterministic diagnostics, and
//! closed compiler evidence.

mod binder;
mod body_check;
mod checker;
mod coercion;
mod declaration_check;
mod declarations;
mod diagnostic;
mod formation;
mod literal;
mod model;
mod pattern;
mod readiness;
mod sealed;
mod traits;
mod typing;

pub use binder::{
    trait_self_type, validate_body_symbolic_type, validate_generic_argument,
    validate_symbolic_const, validate_symbolic_lifetime, validate_symbolic_predicate,
    validate_symbolic_type, BinderFrame, BinderStack, BinderValidationError, TraitSelfContext,
};
pub use body_check::{
    BodyCheckIncompleteness, BodyCheckIncompletenessKind, C2BodyAttemptView, C2BodyCheckFailure,
    C2BodyHandle, C2BodyTable, C2BodyView, CheckedBodyCall, CheckedBodyCallee,
    CheckedBodyExpression, CheckedBodyLocal, CheckedBodyPattern, CheckedBodyPatternAnalysis,
};
pub use checker::{check_workspace_c2, C2BlockStage, C2BlockedWorkspace, C2CheckFailure};
pub use coercion::{classify_coercion, CheckedCoercion, CoercionKind, LifetimeOutlives};
pub use declaration_check::{
    check_declarations_c2, CheckedDeclarationFacts, CheckedDeclarationHandle,
    CheckedDeclarationView, ContextualSelfTemplateFailure, DeclarationCheckBlocker,
    DeclarationCheckBlockerReason, DeclarationCheckFailure, DeclarationCheckInternalError,
    PendingAuthorityDomain,
};
pub use declarations::{
    CandidateUniverseError, DeclarationHandle, DeclarationTable, DeclarationTableError,
    DeclarationView, OrdinaryImplCandidateUniverse,
};
pub use diagnostic::{
    CompilationPhase, NonEmptySemanticDiagnostics, ScopedPackageBytes, SemanticDiagnostic,
};
pub use formation::{
    generic_argument_kind, validate_generic_arguments, GenericFormationError,
    TraitFrameSubstitution,
};
pub use literal::{
    check_float_literal, check_integer_literal, FloatLiteralError, FloatType, IntegerLiteralError,
    TypedFloatLiteral, TypedIntegerLiteral,
};
pub use model::{
    C2CheckedWorkspace, C2Handoff, C2RejectedWorkspace, C2Resolution, C2TargetView, C2TypeProducer,
    NeedsCtfeObligation, NeedsCtfeObligations, PendingC4Dependencies, PendingC4Dependency,
    RetainedFrontend, SessionIndexError, SessionIndexFailure, SessionIndexTables, SessionItemIndex,
    SessionItemView, SessionTypeIndex, SessionTypeView,
};
pub use pattern::{
    analyze_pattern_match, check_irrefutable_pattern, ArmReachability, BindingAnnotation,
    BindingMode, CompletePatternMatch, DecisionTree, EnumType, EnumVariant, IntegerType,
    IrrefutablePatternAnalysis, OwnershipFactKind, Pattern, PatternArm, PatternBinding,
    PatternBindingFact, PatternConst, PatternDiagnosticCode, PatternError, PatternErrorKind,
    PatternErrors, PatternLiteral, PatternMatchAnalysis, PatternProjection, PatternScrutinee,
    PatternTest, PatternType, PendingPatternMatch, PendingPatternTest, PlaceMutability,
    RangeEndpoint, RecordField, RecordPatternField, RecordType, ReferenceMutability,
    SequenceLengthConstraint, TypedBinding, TypedPattern, TypedPatternArm, TypedPatternKind,
    TypedRangeEndpoint,
};
pub use readiness::{
    audit_declaration_shape, audit_definition_owner, ShapeGateAudit, ShapeReadinessInconsistency,
};
pub use sealed::{
    derive_sealed_copy, select_sealed_primitive_operator, PrimitiveDomain, PrimitiveOperatorTrait,
    SealedCopyBase, SealedCopyProof, SealedPrimitiveOperator,
};
pub use traits::{
    BoundWitness, CanonicalTraitObligation, OrdinaryImplCandidateSpec, OrdinaryImplSelection,
    OrdinarySemanticImplKey, PendingC4EcsKeyComparison, PredicateEvidence, SemanticPredicate,
    SemanticTraitKey, TraitEnvironment, TraitEvidence, TraitModelError, TraitPredicate,
    TraitSolveError, TraitSolveResult, TraitSolver, TraitSolverBuildError, TraitSubstitution,
};
pub use typing::{
    check_generic_instantiation, check_typed_expression, BinaryTypeOperator, CheckedExpression,
    CheckedExpressionKind, CheckedPrimitiveSelection, PrimitiveExpressionOperator, TypeCheckError,
    TypeCheckErrorKind, TypeDiagnosticCode, TypedExpressionInput, TypingContext, UnaryTypeOperator,
};
