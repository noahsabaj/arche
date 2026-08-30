//! Type-driven C2 pattern checking, usefulness, and decision-tree construction.
//!
//! This module deliberately stops at the C2/C3 boundary. It records whether a
//! binding moves, shares, or mutably borrows a matched place, but it does not
//! decide `Copy`, partial-move, or loan legality. Const-dependent tests remain
//! explicit [`DecisionTree::NeedsCtfe`] nodes for C5 to discharge.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The two diagnostic classes owned by C2 pattern checking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PatternDiagnosticCode {
    /// The pattern is ill-typed or violates a structural/binding rule.
    Pattern001,
    /// A concrete match is not exhaustive.
    Pattern002,
}

impl PatternDiagnosticCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pattern001 => "PATTERN001",
            Self::Pattern002 => "PATTERN002",
        }
    }
}

impl fmt::Display for PatternDiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A deterministic, source-position-independent pattern failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternError {
    code: PatternDiagnosticCode,
    arm_index: Option<usize>,
    kind: PatternErrorKind,
    message: Box<str>,
}

impl PatternError {
    fn at_arm(
        code: PatternDiagnosticCode,
        arm_index: usize,
        kind: PatternErrorKind,
        message: impl Into<Box<str>>,
    ) -> Self {
        Self {
            code,
            arm_index: Some(arm_index),
            kind,
            message: message.into(),
        }
    }

    fn without_arm(
        code: PatternDiagnosticCode,
        kind: PatternErrorKind,
        message: impl Into<Box<str>>,
    ) -> Self {
        Self {
            code,
            arm_index: None,
            kind,
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn code(&self) -> PatternDiagnosticCode {
        self.code
    }

    #[must_use]
    pub const fn arm_index(&self) -> Option<usize> {
        self.arm_index
    }

    #[must_use]
    pub const fn kind(&self) -> &PatternErrorKind {
        &self.kind
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PatternError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(arm_index) = self.arm_index {
            write!(
                formatter,
                "{} at arm {arm_index}: {}",
                self.code, self.message
            )
        } else {
            write!(formatter, "{}: {}", self.code, self.message)
        }
    }
}

impl std::error::Error for PatternError {}

/// Machine-readable reason for a [`PatternError`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternErrorKind {
    UnsupportedType,
    InvalidType,
    TypeMismatch,
    InvalidLiteral,
    InvalidRange,
    WrongArity,
    UnknownVariant,
    UnknownField,
    DuplicateField,
    MissingField,
    DuplicateBinding,
    OrBindingMismatch,
    DuplicateOrAlternative,
    MutableBorrowOfImmutablePath,
    ReferenceMutabilityMismatch,
    RefutablePattern,
    UnreachableArm,
    NonExhaustiveMatch,
    EmptyMatch,
}

/// A nonempty deterministic failure list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternErrors(Box<[PatternError]>);

impl PatternErrors {
    fn one(error: PatternError) -> Self {
        Self(vec![error].into_boxed_slice())
    }

    fn from_vec(errors: Vec<PatternError>) -> Self {
        debug_assert!(!errors.is_empty());
        Self(errors.into_boxed_slice())
    }

    #[must_use]
    pub fn as_slice(&self) -> &[PatternError] {
        &self.0
    }
}

impl fmt::Display for PatternErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.0.iter().enumerate() {
            if index != 0 {
                formatter.write_str("; ")?;
            }
            write!(formatter, "{error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PatternErrors {}

/// Signed or unsigned integer domain used by pattern literals and ranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IntegerType {
    Signed(u8),
    Unsigned(u8),
}

/// IEEE binary float width admitted as an opaque, binding-only pattern domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FloatType {
    F32,
    F64,
}

impl IntegerType {
    fn validate(self) -> bool {
        matches!(self.bits(), 8 | 16 | 32 | 64 | 128)
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            Self::Signed(bits) | Self::Unsigned(bits) => bits,
        }
    }

    #[must_use]
    pub const fn is_signed(self) -> bool {
        matches!(self, Self::Signed(_))
    }

    fn signed_bounds(self) -> Option<(i128, i128)> {
        let Self::Signed(bits) = self else {
            return None;
        };
        if bits == 128 {
            Some((i128::MIN, i128::MAX))
        } else {
            let magnitude = 1_i128 << (bits - 1);
            Some((-magnitude, magnitude - 1))
        }
    }

    fn unsigned_max(self) -> Option<u128> {
        let Self::Unsigned(bits) = self else {
            return None;
        };
        if bits == 128 {
            Some(u128::MAX)
        } else {
            Some((1_u128 << bits) - 1)
        }
    }
}

/// Reference layer in a scrutinee type or typed pattern projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReferenceMutability {
    Shared,
    Mutable,
}

/// A user enum variant. Order in [`EnumType`] is declaration order.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnumVariant {
    name: Box<str>,
    fields: Box<[PatternType]>,
    record_field_names: Option<Box<[Box<str>]>>,
}

impl EnumVariant {
    #[must_use]
    pub fn new(name: impl Into<Box<str>>, fields: Vec<PatternType>) -> Self {
        Self {
            name: name.into(),
            fields: fields.into_boxed_slice(),
            record_field_names: None,
        }
    }

    /// Construct a record-form enum variant. Field order is declaration order.
    #[must_use]
    pub fn record(name: impl Into<Box<str>>, fields: Vec<RecordField>) -> Self {
        let (record_field_names, fields): (Vec<_>, Vec<_>) = fields
            .into_iter()
            .map(|field| (field.name, field.ty))
            .unzip();
        Self {
            name: name.into(),
            fields: fields.into_boxed_slice(),
            record_field_names: Some(record_field_names.into_boxed_slice()),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn fields(&self) -> &[PatternType] {
        &self.fields
    }

    /// The declaration-order names for a record-form variant.
    #[must_use]
    pub fn record_field_names(&self) -> Option<&[Box<str>]> {
        self.record_field_names.as_deref()
    }

    #[must_use]
    pub fn is_record(&self) -> bool {
        self.record_field_names.is_some()
    }
}

/// One declaration-order field of a nominal record or record enum variant.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordField {
    name: Box<str>,
    ty: PatternType,
}

impl RecordField {
    #[must_use]
    pub fn new(name: impl Into<Box<str>>, ty: PatternType) -> Self {
        Self {
            name: name.into(),
            ty,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn ty(&self) -> &PatternType {
        &self.ty
    }
}

/// A nominal record description used for structural C2 matching.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordType {
    name: Box<str>,
    fields: Box<[RecordField]>,
}

impl RecordType {
    #[must_use]
    pub fn new(name: impl Into<Box<str>>, fields: Vec<RecordField>) -> Self {
        Self {
            name: name.into(),
            fields: fields.into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn fields(&self) -> &[RecordField] {
        &self.fields
    }
}

/// A nominal user enum description used only for C2 structural matching.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnumType {
    name: Box<str>,
    variants: Box<[EnumVariant]>,
}

impl EnumType {
    #[must_use]
    pub fn new(name: impl Into<Box<str>>, variants: Vec<EnumVariant>) -> Self {
        Self {
            name: name.into(),
            variants: variants.into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn variants(&self) -> &[EnumVariant] {
        &self.variants
    }
}

/// Closed pattern type algebra understood by the C2 engine.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PatternType {
    Unit,
    Bool,
    Integer(IntegerType),
    Char,
    /// The owned embedded-Core string type.
    String,
    /// The dynamically sized UTF-8 string slice type.
    Str,
    Tuple(Box<[PatternType]>),
    Array {
        element: Box<PatternType>,
        length: usize,
    },
    /// An array whose canonical length expression is retained for C5.
    SymbolicArray {
        element: Box<PatternType>,
        length: Box<PatternConst>,
    },
    Slice(Box<PatternType>),
    Record(RecordType),
    Enum(EnumType),
    Reference {
        mutability: ReferenceMutability,
        referent: Box<PatternType>,
    },
    /// An IEEE float scrutinee: bindable and wildcard-matchable, with no
    /// structural constructor space. Float structural patterns are rejected
    /// by the surface contract, so no literal or range can inhabit this
    /// domain; exhaustiveness requires a binding or wildcard row.
    Float(FloatType),
    /// An opaque scrutinee domain outside structural matching (currently a
    /// bound generic parameter): bindable and wildcard-matchable, with no
    /// structural constructor space.
    Opaque(Box<str>),
    /// Explicit fail-closed representation for a type outside C2's algebra.
    Unsupported(Box<str>),
}

impl PatternType {
    #[must_use]
    pub fn tuple(fields: Vec<Self>) -> Self {
        Self::Tuple(fields.into_boxed_slice())
    }

    #[must_use]
    pub fn array(element: Self, length: usize) -> Self {
        Self::Array {
            element: Box::new(element),
            length,
        }
    }

    #[must_use]
    pub fn symbolic_array(element: Self, length: PatternConst) -> Self {
        Self::SymbolicArray {
            element: Box::new(element),
            length: Box::new(length),
        }
    }

    #[must_use]
    pub fn slice(element: Self) -> Self {
        Self::Slice(Box::new(element))
    }

    #[must_use]
    pub fn reference(mutability: ReferenceMutability, referent: Self) -> Self {
        Self::Reference {
            mutability,
            referent: Box::new(referent),
        }
    }
}

/// Mutability of the evaluated scrutinee place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlaceMutability {
    Immutable,
    Mutable,
}

/// A typed match input. The expression is evaluated exactly once by later phases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternScrutinee {
    ty: PatternType,
    mutability: PlaceMutability,
}

impl PatternScrutinee {
    #[must_use]
    pub const fn new(ty: PatternType, mutability: PlaceMutability) -> Self {
        Self { ty, mutability }
    }

    #[must_use]
    pub const fn ty(&self) -> &PatternType {
        &self.ty
    }

    #[must_use]
    pub const fn mutability(&self) -> PlaceMutability {
        self.mutability
    }
}

/// Concrete scalar value accepted in a C2 pattern.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PatternLiteral {
    Bool(bool),
    Signed(i128),
    Unsigned(u128),
    Char(char),
    String(Box<str>),
}

/// Canonical dependency retained for C5. No compiler-stable identity is minted.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatternConst {
    dependency: Box<str>,
    ty: PatternType,
}

impl PatternConst {
    #[must_use]
    pub fn new(dependency: impl Into<Box<str>>, ty: PatternType) -> Self {
        Self {
            dependency: dependency.into(),
            ty,
        }
    }

    #[must_use]
    pub fn dependency(&self) -> &str {
        &self.dependency
    }

    #[must_use]
    pub const fn ty(&self) -> &PatternType {
        &self.ty
    }
}

/// Literal or CTFE-dependent integer/character range endpoint.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RangeEndpoint {
    Literal(PatternLiteral),
    Const(PatternConst),
}

/// Source binding annotation. `mut` affects only local assignability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BindingAnnotation {
    Inferred,
    Ref,
    RefMut,
}

/// An untyped source binding.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatternBinding {
    name: Box<str>,
    annotation: BindingAnnotation,
    variable_mutable: bool,
}

/// One source-spelled named field in a record pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordPatternField {
    name: Box<str>,
    pattern: Pattern,
}

impl RecordPatternField {
    #[must_use]
    pub fn new(name: impl Into<Box<str>>, pattern: Pattern) -> Self {
        Self {
            name: name.into(),
            pattern,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn pattern(&self) -> &Pattern {
        &self.pattern
    }
}

impl PatternBinding {
    #[must_use]
    pub fn new(
        name: impl Into<Box<str>>,
        annotation: BindingAnnotation,
        variable_mutable: bool,
    ) -> Self {
        Self {
            name: name.into(),
            annotation,
            variable_mutable,
        }
    }

    #[must_use]
    pub fn inferred(name: impl Into<Box<str>>) -> Self {
        Self::new(name, BindingAnnotation::Inferred, false)
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Untyped C2 pattern algebra.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pattern {
    Wildcard,
    Unit,
    Binding(PatternBinding),
    Literal(PatternLiteral),
    Const(PatternConst),
    Reference {
        mutability: ReferenceMutability,
        pattern: Box<Pattern>,
    },
    Tuple(Box<[Pattern]>),
    Slice {
        prefix: Box<[Pattern]>,
        has_rest: bool,
        suffix: Box<[Pattern]>,
    },
    Constructor {
        variant: Box<str>,
        fields: Box<[Pattern]>,
    },
    Record {
        constructor: Box<str>,
        fields: Box<[RecordPatternField]>,
    },
    Range {
        start: RangeEndpoint,
        end: RangeEndpoint,
        inclusive: bool,
    },
    At {
        binding: PatternBinding,
        pattern: Box<Pattern>,
    },
    Or(Box<[Pattern]>),
}

impl Pattern {
    #[must_use]
    pub fn tuple(fields: Vec<Self>) -> Self {
        Self::Tuple(fields.into_boxed_slice())
    }

    #[must_use]
    pub fn slice(prefix: Vec<Self>, has_rest: bool, suffix: Vec<Self>) -> Self {
        Self::Slice {
            prefix: prefix.into_boxed_slice(),
            has_rest,
            suffix: suffix.into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn constructor(variant: impl Into<Box<str>>, fields: Vec<Self>) -> Self {
        Self::Constructor {
            variant: variant.into(),
            fields: fields.into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn record(constructor: impl Into<Box<str>>, fields: Vec<RecordPatternField>) -> Self {
        Self::Record {
            constructor: constructor.into(),
            fields: fields.into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn or(alternatives: Vec<Self>) -> Self {
        Self::Or(alternatives.into_boxed_slice())
    }
}

/// A source-order arm. Guard expressions are checked elsewhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternArm {
    pattern: Pattern,
    has_guard: bool,
}

impl PatternArm {
    #[must_use]
    pub const fn new(pattern: Pattern, has_guard: bool) -> Self {
        Self { pattern, has_guard }
    }

    #[must_use]
    pub const fn pattern(&self) -> &Pattern {
        &self.pattern
    }

    #[must_use]
    pub const fn has_guard(&self) -> bool {
        self.has_guard
    }
}

/// Final binding mode after match ergonomics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BindingMode {
    Move,
    Ref,
    RefMut,
}

/// C2 ownership fact. C3 decides whether the recorded operation is legal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OwnershipFactKind {
    Move,
    Ref,
    RefMut,
}

/// A type- and mode-checked binding.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypedBinding {
    name: Box<str>,
    matched_type: PatternType,
    binding_type: PatternType,
    mode: BindingMode,
    variable_mutable: bool,
}

impl TypedBinding {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn matched_type(&self) -> &PatternType {
        &self.matched_type
    }

    #[must_use]
    pub const fn binding_type(&self) -> &PatternType {
        &self.binding_type
    }

    #[must_use]
    pub const fn mode(&self) -> BindingMode {
        self.mode
    }

    #[must_use]
    pub const fn variable_mutable(&self) -> bool {
        self.variable_mutable
    }
}

/// A typed and normalized range endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedRangeEndpoint {
    Literal(PatternLiteral),
    NeedsCtfe(PatternConst),
}

/// Typed pattern shape. Inserted and explicit dereferences are never re-inferred.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedPatternKind {
    Wildcard,
    Unit,
    Binding(TypedBinding),
    Literal(PatternLiteral),
    NeedsCtfe(PatternConst),
    Dereference {
        mutability: ReferenceMutability,
        inserted: bool,
        pattern: Box<TypedPattern>,
    },
    Tuple(Box<[TypedPattern]>),
    Slice {
        elements: Box<[TypedPattern]>,
        prefix_length: usize,
        suffix_length: usize,
    },
    /// A slice pattern over a dynamically sized slice.
    DynamicSlice {
        prefix: Box<[TypedPattern]>,
        has_rest: bool,
        suffix: Box<[TypedPattern]>,
    },
    /// A slice pattern whose array length comparison must be discharged by C5.
    SymbolicSlice {
        prefix: Box<[TypedPattern]>,
        has_rest: bool,
        suffix: Box<[TypedPattern]>,
        length: PatternConst,
    },
    Record {
        record_name: Box<str>,
        field_names: Box<[Box<str>]>,
        fields: Box<[TypedPattern]>,
    },
    Constructor {
        enum_name: Box<str>,
        variant_index: usize,
        variant: Box<str>,
        fields: Box<[TypedPattern]>,
    },
    RecordConstructor {
        enum_name: Box<str>,
        variant_index: usize,
        variant: Box<str>,
        field_names: Box<[Box<str>]>,
        fields: Box<[TypedPattern]>,
    },
    Range {
        start: TypedRangeEndpoint,
        end: TypedRangeEndpoint,
        inclusive: bool,
    },
    At {
        binding: TypedBinding,
        pattern: Box<TypedPattern>,
    },
    Or(Box<[TypedPattern]>),
}

/// A type-checked C2 pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedPattern {
    ty: PatternType,
    kind: TypedPatternKind,
}

impl TypedPattern {
    #[must_use]
    pub const fn ty(&self) -> &PatternType {
        &self.ty
    }

    #[must_use]
    pub const fn kind(&self) -> &TypedPatternKind {
        &self.kind
    }
}

/// One checked source-order arm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedPatternArm {
    pattern: TypedPattern,
    has_guard: bool,
}

impl TypedPatternArm {
    #[must_use]
    pub const fn pattern(&self) -> &TypedPattern {
        &self.pattern
    }

    #[must_use]
    pub const fn has_guard(&self) -> bool {
        self.has_guard
    }
}

/// A canonical projection from the once-evaluated scrutinee.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PatternProjection {
    InsertedDeref(ReferenceMutability),
    ExplicitDeref(ReferenceMutability),
    TupleField(usize),
    ArrayElement(usize),
    SliceElementFromStart(usize),
    /// Zero denotes the last element, one the penultimate element, and so on.
    SliceElementFromEnd(usize),
    RecordField {
        record_name: Box<str>,
        field_index: usize,
        field: Box<str>,
    },
    EnumField {
        variant_index: usize,
        field_index: usize,
    },
}

/// A checked sequence-length predicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SequenceLengthConstraint {
    Exact(usize),
    AtLeast(usize),
}

/// Canonical structural runtime test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternTest {
    Bool(bool),
    SignedRange {
        start: i128,
        end: i128,
        inclusive: bool,
    },
    UnsignedRange {
        start: u128,
        end: u128,
        inclusive: bool,
    },
    CharRange {
        start: char,
        end: char,
        inclusive: bool,
    },
    String(Box<str>),
    SliceLength(SequenceLengthConstraint),
    EnumVariant {
        enum_name: Box<str>,
        variant_index: usize,
        variant: Box<str>,
    },
}

/// A symbolic C2 test that only C5 may replace with a concrete test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingPatternTest {
    ConstEquals(PatternConst),
    Range {
        ty: PatternType,
        start: TypedRangeEndpoint,
        end: TypedRangeEndpoint,
        inclusive: bool,
    },
    ArrayLength {
        length: PatternConst,
        constraint: SequenceLengthConstraint,
    },
}

/// Binding plus the canonical matched-place projection stored at a leaf.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatternBindingFact {
    binding: TypedBinding,
    path: Box<[PatternProjection]>,
    ownership: OwnershipFactKind,
}

impl PatternBindingFact {
    #[must_use]
    pub const fn binding(&self) -> &TypedBinding {
        &self.binding
    }

    #[must_use]
    pub fn path(&self) -> &[PatternProjection] {
        &self.path
    }

    #[must_use]
    pub const fn ownership(&self) -> OwnershipFactKind {
        self.ownership
    }
}

/// Typed canonical decision tree consumed by MIR construction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecisionTree {
    Fail,
    Leaf {
        arm_index: usize,
        bindings: Box<[PatternBindingFact]>,
    },
    Guard {
        arm_index: usize,
        bindings: Box<[PatternBindingFact]>,
        on_true: Box<DecisionTree>,
        on_false: Box<DecisionTree>,
    },
    Test {
        path: Box<[PatternProjection]>,
        test: PatternTest,
        on_match: Box<DecisionTree>,
        on_mismatch: Box<DecisionTree>,
    },
    NeedsCtfe {
        path: Box<[PatternProjection]>,
        test: PendingPatternTest,
        on_match: Box<DecisionTree>,
        on_mismatch: Box<DecisionTree>,
    },
}

/// Concrete arm reachability. This is absent from pending analyses by design.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmReachability {
    Reachable,
}

/// Fully concrete and exhaustive match analysis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletePatternMatch {
    arms: Box<[TypedPatternArm]>,
    reachability: Box<[ArmReachability]>,
    tree: DecisionTree,
}

impl CompletePatternMatch {
    #[must_use]
    pub fn arms(&self) -> &[TypedPatternArm] {
        &self.arms
    }

    #[must_use]
    pub fn reachability(&self) -> &[ArmReachability] {
        &self.reachability
    }

    #[must_use]
    pub const fn tree(&self) -> &DecisionTree {
        &self.tree
    }
}

/// Checked pending match. No reachability or exhaustiveness claim is exposed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingPatternMatch {
    arms: Box<[TypedPatternArm]>,
    dependencies: Box<[PatternConst]>,
    tree: DecisionTree,
}

impl PendingPatternMatch {
    #[must_use]
    pub fn arms(&self) -> &[TypedPatternArm] {
        &self.arms
    }

    #[must_use]
    pub fn dependencies(&self) -> &[PatternConst] {
        &self.dependencies
    }

    #[must_use]
    pub const fn tree(&self) -> &DecisionTree {
        &self.tree
    }
}

/// Terminal C2 match status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternMatchAnalysis {
    Complete(CompletePatternMatch),
    NeedsCtfe(PendingPatternMatch),
}

/// Fully checked irrefutable binding context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrrefutablePatternAnalysis {
    Complete(TypedPattern),
    NeedsCtfe {
        pattern: TypedPattern,
        dependencies: Box<[PatternConst]>,
    },
}

#[derive(Clone, Copy)]
struct CheckContext {
    default_mode: BindingMode,
    mutable_path: bool,
}

/// Check an irrefutable parameter, `for`, or plain-`let` pattern.
pub fn check_irrefutable_pattern(
    scrutinee: &PatternScrutinee,
    pattern: &Pattern,
) -> Result<IrrefutablePatternAnalysis, PatternErrors> {
    validate_type(&scrutinee.ty).map_err(PatternErrors::one)?;
    let context = CheckContext {
        default_mode: BindingMode::Move,
        mutable_path: scrutinee.mutability == PlaceMutability::Mutable,
    };
    let typed = check_pattern(pattern, &scrutinee.ty, context).map_err(PatternErrors::one)?;
    validate_unique_bindings(&typed).map_err(PatternErrors::one)?;
    validate_or_bindings(&typed).map_err(PatternErrors::one)?;
    validate_duplicate_or_alternatives(&typed).map_err(PatternErrors::one)?;
    let dependencies = collect_dependencies(&typed);
    let optimistic_matrix = expand_cover(&replace_pending_with_wildcard(&to_cover(&typed)))
        .into_iter()
        .map(|cover| vec![cover])
        .collect::<Vec<_>>();
    if useful(
        &optimistic_matrix,
        &[CoverPattern::Wildcard],
        std::slice::from_ref(&scrutinee.ty),
    ) {
        return Err(PatternErrors::one(PatternError::without_arm(
            PatternDiagnosticCode::Pattern001,
            PatternErrorKind::RefutablePattern,
            "pattern is refutable in an irrefutable context",
        )));
    }
    if !dependencies.is_empty() {
        return Ok(IrrefutablePatternAnalysis::NeedsCtfe {
            pattern: typed,
            dependencies: dependencies.into_boxed_slice(),
        });
    }
    let covers = expand_cover(&to_cover(&typed));
    let matrix = covers
        .into_iter()
        .map(|cover| vec![cover])
        .collect::<Vec<_>>();
    if useful(
        &matrix,
        &[CoverPattern::Wildcard],
        std::slice::from_ref(&scrutinee.ty),
    ) {
        return Err(PatternErrors::one(PatternError::without_arm(
            PatternDiagnosticCode::Pattern001,
            PatternErrorKind::RefutablePattern,
            "pattern is refutable in an irrefutable context",
        )));
    }
    Ok(IrrefutablePatternAnalysis::Complete(typed))
}

/// Check, type, and compile a complete match arm list.
pub fn analyze_pattern_match(
    scrutinee: &PatternScrutinee,
    arms: &[PatternArm],
) -> Result<PatternMatchAnalysis, PatternErrors> {
    validate_type(&scrutinee.ty).map_err(PatternErrors::one)?;
    if arms.is_empty() {
        return Err(PatternErrors::one(PatternError::without_arm(
            PatternDiagnosticCode::Pattern002,
            PatternErrorKind::EmptyMatch,
            "match has no arms",
        )));
    }

    let context = CheckContext {
        default_mode: BindingMode::Move,
        mutable_path: scrutinee.mutability == PlaceMutability::Mutable,
    };
    let mut typed_arms = Vec::with_capacity(arms.len());
    for (arm_index, arm) in arms.iter().enumerate() {
        let typed = check_pattern(&arm.pattern, &scrutinee.ty, context)
            .map_err(|error| PatternErrors::one(with_arm(error, arm_index)))?;
        validate_unique_bindings(&typed)
            .map_err(|error| PatternErrors::one(with_arm(error, arm_index)))?;
        validate_or_bindings(&typed)
            .map_err(|error| PatternErrors::one(with_arm(error, arm_index)))?;
        validate_duplicate_or_alternatives(&typed)
            .map_err(|error| PatternErrors::one(with_arm(error, arm_index)))?;
        typed_arms.push(TypedPatternArm {
            pattern: typed,
            has_guard: arm.has_guard,
        });
    }

    let dependencies = typed_arms
        .iter()
        .flat_map(|arm| collect_dependencies(&arm.pattern))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let tree = build_decision_tree(&typed_arms);
    if !dependencies.is_empty() {
        let mut optimistic_matrix = Vec::new();
        for (arm_index, arm) in typed_arms.iter().enumerate() {
            let cover = to_cover(&arm.pattern);
            if cover_is_definitely_empty(&cover) {
                return Err(PatternErrors::one(PatternError::at_arm(
                    PatternDiagnosticCode::Pattern001,
                    arm_index,
                    PatternErrorKind::UnreachableArm,
                    "arm pattern is unreachable",
                )));
            }
            if !arm.has_guard {
                optimistic_matrix.extend(
                    expand_cover(&replace_pending_with_wildcard(&cover))
                        .into_iter()
                        .map(|alternative| vec![alternative]),
                );
            }
        }
        if useful(
            &optimistic_matrix,
            &[CoverPattern::Wildcard],
            std::slice::from_ref(&scrutinee.ty),
        ) {
            return Err(PatternErrors::one(PatternError::without_arm(
                PatternDiagnosticCode::Pattern002,
                PatternErrorKind::NonExhaustiveMatch,
                "match is structurally non-exhaustive before CTFE",
            )));
        }
        return Ok(PatternMatchAnalysis::NeedsCtfe(PendingPatternMatch {
            arms: typed_arms.into_boxed_slice(),
            dependencies: dependencies.into_boxed_slice(),
            tree,
        }));
    }

    let mut matrix = Vec::<Vec<CoverPattern>>::new();
    let mut errors = Vec::new();
    for (arm_index, arm) in typed_arms.iter().enumerate() {
        let alternatives = expand_cover(&to_cover(&arm.pattern));
        let reachable = alternatives.iter().any(|alternative| {
            useful(
                &matrix,
                std::slice::from_ref(alternative),
                std::slice::from_ref(&scrutinee.ty),
            )
        });
        if !reachable {
            errors.push(PatternError::at_arm(
                PatternDiagnosticCode::Pattern001,
                arm_index,
                PatternErrorKind::UnreachableArm,
                "arm pattern is unreachable",
            ));
        }
        if !arm.has_guard {
            matrix.extend(
                alternatives
                    .into_iter()
                    .map(|alternative| vec![alternative]),
            );
        }
    }
    if useful(
        &matrix,
        &[CoverPattern::Wildcard],
        std::slice::from_ref(&scrutinee.ty),
    ) {
        errors.push(PatternError::without_arm(
            PatternDiagnosticCode::Pattern002,
            PatternErrorKind::NonExhaustiveMatch,
            "match is not exhaustive",
        ));
    }
    if !errors.is_empty() {
        return Err(PatternErrors::from_vec(errors));
    }

    Ok(PatternMatchAnalysis::Complete(CompletePatternMatch {
        reachability: vec![ArmReachability::Reachable; typed_arms.len()].into_boxed_slice(),
        arms: typed_arms.into_boxed_slice(),
        tree,
    }))
}

fn with_arm(mut error: PatternError, arm_index: usize) -> PatternError {
    error.arm_index = Some(arm_index);
    error
}

fn pattern001(kind: PatternErrorKind, message: impl Into<Box<str>>) -> PatternError {
    PatternError::without_arm(PatternDiagnosticCode::Pattern001, kind, message)
}

fn validate_type(ty: &PatternType) -> Result<(), PatternError> {
    match ty {
        PatternType::Unit
        | PatternType::Bool
        | PatternType::Char
        | PatternType::String
        | PatternType::Str
        | PatternType::Float(_)
        | PatternType::Opaque(_) => Ok(()),
        PatternType::Integer(integer) if integer.validate() => Ok(()),
        PatternType::Integer(integer) => Err(pattern001(
            PatternErrorKind::InvalidType,
            format!("unsupported {}-bit integer pattern type", integer.bits()),
        )),
        PatternType::Tuple(fields) => {
            for field in fields {
                validate_type(field)?;
            }
            Ok(())
        }
        PatternType::Array { element, .. } | PatternType::Slice(element) => validate_type(element),
        PatternType::SymbolicArray { element, length } => {
            validate_type(element)?;
            if length.dependency.is_empty() {
                return Err(pattern001(
                    PatternErrorKind::InvalidType,
                    "symbolic array length has an empty canonical dependency",
                ));
            }
            if !matches!(length.ty, PatternType::Integer(IntegerType::Unsigned(_))) {
                return Err(pattern001(
                    PatternErrorKind::InvalidType,
                    "symbolic array length dependency is not an unsigned integer",
                ));
            }
            validate_type(&length.ty)
        }
        PatternType::Record(record) => {
            if record.name.is_empty() {
                return Err(pattern001(
                    PatternErrorKind::InvalidType,
                    "record type has an empty name",
                ));
            }
            validate_record_fields(&record.name, &record.fields)
        }
        PatternType::Reference { referent, .. } => validate_type(referent),
        PatternType::Enum(enum_ty) => {
            if enum_ty.name.is_empty() {
                return Err(pattern001(
                    PatternErrorKind::InvalidType,
                    "enum type has an empty name",
                ));
            }
            if enum_ty.variants.is_empty() {
                return Err(pattern001(
                    PatternErrorKind::InvalidType,
                    format!("enum `{}` has no variants", enum_ty.name),
                ));
            }
            let mut names = BTreeSet::new();
            for variant in &enum_ty.variants {
                if variant.name.is_empty() || !names.insert(variant.name.as_ref()) {
                    return Err(pattern001(
                        PatternErrorKind::InvalidType,
                        format!("enum `{}` has an empty or duplicate variant", enum_ty.name),
                    ));
                }
                if let Some(field_names) = &variant.record_field_names {
                    if field_names.len() != variant.fields.len() {
                        return Err(pattern001(
                            PatternErrorKind::InvalidType,
                            format!(
                                "record variant `{}::{}` has a field-name/type count mismatch",
                                enum_ty.name, variant.name
                            ),
                        ));
                    }
                    let mut names = BTreeSet::new();
                    for field_name in field_names {
                        if field_name.is_empty() || !names.insert(field_name.as_ref()) {
                            return Err(pattern001(
                                PatternErrorKind::InvalidType,
                                format!(
                                    "record variant `{}::{}` has an empty or duplicate field",
                                    enum_ty.name, variant.name
                                ),
                            ));
                        }
                    }
                }
                for field in &variant.fields {
                    validate_type(field)?;
                }
            }
            Ok(())
        }
        PatternType::Unsupported(description) => Err(pattern001(
            PatternErrorKind::UnsupportedType,
            format!("type `{description}` has no C2 constructor space"),
        )),
    }
}

fn validate_record_fields(record_name: &str, fields: &[RecordField]) -> Result<(), PatternError> {
    let mut names = BTreeSet::new();
    for field in fields {
        if field.name.is_empty() || !names.insert(field.name.as_ref()) {
            return Err(pattern001(
                PatternErrorKind::InvalidType,
                format!("record `{record_name}` has an empty or duplicate field"),
            ));
        }
        validate_type(&field.ty)?;
    }
    Ok(())
}

fn check_pattern(
    pattern: &Pattern,
    expected: &PatternType,
    context: CheckContext,
) -> Result<TypedPattern, PatternError> {
    if !has_explicit_reference_head(pattern) {
        if let PatternType::Reference {
            mutability,
            referent,
        } = expected
        {
            let (default_mode, mutable_path) = match mutability {
                ReferenceMutability::Shared => (
                    match context.default_mode {
                        BindingMode::Move | BindingMode::RefMut => BindingMode::Ref,
                        BindingMode::Ref => BindingMode::Ref,
                    },
                    false,
                ),
                ReferenceMutability::Mutable => (
                    match context.default_mode {
                        BindingMode::Move => BindingMode::RefMut,
                        BindingMode::Ref => BindingMode::Ref,
                        BindingMode::RefMut => BindingMode::RefMut,
                    },
                    true,
                ),
            };
            let nested = check_pattern(
                pattern,
                referent,
                CheckContext {
                    default_mode,
                    mutable_path,
                },
            )?;
            return Ok(TypedPattern {
                ty: expected.clone(),
                kind: TypedPatternKind::Dereference {
                    mutability: *mutability,
                    inserted: true,
                    pattern: Box::new(nested),
                },
            });
        }
    }

    let kind = match pattern {
        Pattern::Wildcard => TypedPatternKind::Wildcard,
        Pattern::Unit => {
            require_type(expected, &PatternType::Unit, "unit pattern")?;
            TypedPatternKind::Unit
        }
        Pattern::Binding(binding) => {
            TypedPatternKind::Binding(check_binding(binding, expected, context)?)
        }
        Pattern::Literal(literal) => {
            check_literal(literal, expected)?;
            TypedPatternKind::Literal(literal.clone())
        }
        Pattern::Const(constant) => {
            check_const(constant, expected)?;
            TypedPatternKind::NeedsCtfe(constant.clone())
        }
        Pattern::Reference {
            mutability,
            pattern,
        } => {
            let PatternType::Reference {
                mutability: expected_mutability,
                referent,
            } = expected
            else {
                return Err(pattern001(
                    PatternErrorKind::TypeMismatch,
                    "reference pattern requires a reference scrutinee",
                ));
            };
            if mutability != expected_mutability {
                return Err(pattern001(
                    PatternErrorKind::ReferenceMutabilityMismatch,
                    "reference pattern mutability does not match the scrutinee layer",
                ));
            }
            let nested = check_pattern(
                pattern,
                referent,
                CheckContext {
                    default_mode: BindingMode::Move,
                    mutable_path: *mutability == ReferenceMutability::Mutable,
                },
            )?;
            TypedPatternKind::Dereference {
                mutability: *mutability,
                inserted: false,
                pattern: Box::new(nested),
            }
        }
        Pattern::Tuple(fields) => {
            let PatternType::Tuple(field_types) = expected else {
                return Err(pattern001(
                    PatternErrorKind::TypeMismatch,
                    "tuple pattern requires a tuple scrutinee",
                ));
            };
            if fields.len() != field_types.len() {
                return Err(pattern001(
                    PatternErrorKind::WrongArity,
                    format!(
                        "tuple pattern has {} fields but the type has {}",
                        fields.len(),
                        field_types.len()
                    ),
                ));
            }
            TypedPatternKind::Tuple(
                fields
                    .iter()
                    .zip(field_types)
                    .map(|(field, ty)| check_pattern(field, ty, context))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
            )
        }
        Pattern::Slice {
            prefix,
            has_rest,
            suffix,
        } => match expected {
            PatternType::Array { element, length } => {
                let explicit = prefix.len().saturating_add(suffix.len());
                if (*has_rest && explicit > *length) || (!*has_rest && explicit != *length) {
                    return Err(pattern001(
                    PatternErrorKind::WrongArity,
                    format!(
                        "slice pattern describes {explicit} fixed elements for array length {length}"
                    ),
                ));
                }
                let mut elements = Vec::with_capacity(*length);
                for source in prefix {
                    elements.push(check_pattern(source, element, context)?);
                }
                elements.extend((0..length.saturating_sub(explicit)).map(|_| TypedPattern {
                    ty: element.as_ref().clone(),
                    kind: TypedPatternKind::Wildcard,
                }));
                for source in suffix {
                    elements.push(check_pattern(source, element, context)?);
                }
                TypedPatternKind::Slice {
                    elements: elements.into_boxed_slice(),
                    prefix_length: prefix.len(),
                    suffix_length: suffix.len(),
                }
            }
            PatternType::Slice(element) => TypedPatternKind::DynamicSlice {
                prefix: prefix
                    .iter()
                    .map(|field| check_pattern(field, element, context))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
                has_rest: *has_rest,
                suffix: suffix
                    .iter()
                    .map(|field| check_pattern(field, element, context))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
            },
            PatternType::SymbolicArray { element, length } => {
                if *has_rest && prefix.is_empty() && suffix.is_empty() {
                    TypedPatternKind::Wildcard
                } else {
                    TypedPatternKind::SymbolicSlice {
                        prefix: prefix
                            .iter()
                            .map(|field| check_pattern(field, element, context))
                            .collect::<Result<Vec<_>, _>>()?
                            .into_boxed_slice(),
                        has_rest: *has_rest,
                        suffix: suffix
                            .iter()
                            .map(|field| check_pattern(field, element, context))
                            .collect::<Result<Vec<_>, _>>()?
                            .into_boxed_slice(),
                        length: length.as_ref().clone(),
                    }
                }
            }
            _ => {
                return Err(pattern001(
                    PatternErrorKind::TypeMismatch,
                    "slice pattern requires an array or slice scrutinee",
                ));
            }
        },
        Pattern::Constructor { variant, fields } => {
            let PatternType::Enum(enum_ty) = expected else {
                return Err(pattern001(
                    PatternErrorKind::TypeMismatch,
                    "constructor pattern requires an enum scrutinee",
                ));
            };
            let Some((variant_index, definition)) = enum_ty
                .variants
                .iter()
                .enumerate()
                .find(|(_, definition)| definition.name.as_ref() == variant.as_ref())
            else {
                return Err(pattern001(
                    PatternErrorKind::UnknownVariant,
                    format!("enum `{}` has no variant `{variant}`", enum_ty.name),
                ));
            };
            if definition.is_record() {
                return Err(pattern001(
                    PatternErrorKind::TypeMismatch,
                    format!("variant `{variant}` requires a record-field pattern"),
                ));
            }
            if fields.len() != definition.fields.len() {
                return Err(pattern001(
                    PatternErrorKind::WrongArity,
                    format!(
                        "variant `{variant}` expects {} fields but pattern has {}",
                        definition.fields.len(),
                        fields.len()
                    ),
                ));
            }
            TypedPatternKind::Constructor {
                enum_name: enum_ty.name.clone(),
                variant_index,
                variant: variant.clone(),
                fields: fields
                    .iter()
                    .zip(&definition.fields)
                    .map(|(field, ty)| check_pattern(field, ty, context))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
            }
        }
        Pattern::Record {
            constructor,
            fields,
        } => match expected {
            PatternType::Record(record) => {
                if constructor.as_ref() != record.name.as_ref() {
                    return Err(pattern001(
                        PatternErrorKind::TypeMismatch,
                        format!(
                            "record pattern names `{constructor}` but the scrutinee type is `{}`",
                            record.name
                        ),
                    ));
                }
                let field_names = record
                    .fields
                    .iter()
                    .map(|field| field.name.clone())
                    .collect::<Vec<_>>();
                let field_types = record
                    .fields
                    .iter()
                    .map(|field| field.ty.clone())
                    .collect::<Vec<_>>();
                let fields = check_named_pattern_fields(
                    &record.name,
                    fields,
                    &field_names,
                    &field_types,
                    context,
                )?;
                TypedPatternKind::Record {
                    record_name: record.name.clone(),
                    field_names: field_names.into_boxed_slice(),
                    fields,
                }
            }
            PatternType::Enum(enum_ty) => {
                let Some((variant_index, definition)) = enum_ty
                    .variants
                    .iter()
                    .enumerate()
                    .find(|(_, definition)| definition.name.as_ref() == constructor.as_ref())
                else {
                    return Err(pattern001(
                        PatternErrorKind::UnknownVariant,
                        format!("enum `{}` has no variant `{constructor}`", enum_ty.name),
                    ));
                };
                let Some(field_names) = definition.record_field_names.as_deref() else {
                    return Err(pattern001(
                        PatternErrorKind::TypeMismatch,
                        format!("variant `{constructor}` requires a positional pattern"),
                    ));
                };
                let owner = format!("{}::{constructor}", enum_ty.name);
                let checked_fields = check_named_pattern_fields(
                    &owner,
                    fields,
                    field_names,
                    &definition.fields,
                    context,
                )?;
                TypedPatternKind::RecordConstructor {
                    enum_name: enum_ty.name.clone(),
                    variant_index,
                    variant: constructor.clone(),
                    field_names: field_names.to_vec().into_boxed_slice(),
                    fields: checked_fields,
                }
            }
            _ => {
                return Err(pattern001(
                    PatternErrorKind::TypeMismatch,
                    "record pattern requires a nominal record or record enum variant",
                ));
            }
        },
        Pattern::Range {
            start,
            end,
            inclusive,
        } => check_range(start, end, *inclusive, expected)?,
        Pattern::At { binding, pattern } => TypedPatternKind::At {
            binding: check_binding(binding, expected, context)?,
            pattern: Box::new(check_pattern(pattern, expected, context)?),
        },
        Pattern::Or(alternatives) => {
            if alternatives.len() < 2 {
                return Err(pattern001(
                    PatternErrorKind::InvalidType,
                    "or-pattern requires at least two alternatives",
                ));
            }
            TypedPatternKind::Or(
                alternatives
                    .iter()
                    .map(|alternative| check_pattern(alternative, expected, context))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
            )
        }
    };
    Ok(TypedPattern {
        ty: expected.clone(),
        kind,
    })
}

fn check_named_pattern_fields(
    owner: &str,
    source_fields: &[RecordPatternField],
    field_names: &[Box<str>],
    field_types: &[PatternType],
    context: CheckContext,
) -> Result<Box<[TypedPattern]>, PatternError> {
    debug_assert_eq!(field_names.len(), field_types.len());
    let mut source_by_name = BTreeMap::new();
    for source in source_fields {
        if !field_names
            .iter()
            .any(|name| name.as_ref() == source.name.as_ref())
        {
            return Err(pattern001(
                PatternErrorKind::UnknownField,
                format!(
                    "record pattern for `{owner}` has no field `{}`",
                    source.name
                ),
            ));
        }
        if source_by_name
            .insert(source.name.as_ref(), &source.pattern)
            .is_some()
        {
            return Err(pattern001(
                PatternErrorKind::DuplicateField,
                format!(
                    "record pattern for `{owner}` names field `{}` more than once",
                    source.name
                ),
            ));
        }
    }

    field_names
        .iter()
        .zip(field_types)
        .map(|(name, ty)| {
            let Some(pattern) = source_by_name.get(name.as_ref()) else {
                return Err(pattern001(
                    PatternErrorKind::MissingField,
                    format!("record pattern for `{owner}` is missing field `{name}`"),
                ));
            };
            check_pattern(pattern, ty, context)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn has_explicit_reference_head(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Reference { .. } => true,
        Pattern::At { pattern, .. } => has_explicit_reference_head(pattern),
        Pattern::Or(alternatives) => {
            !alternatives.is_empty() && alternatives.iter().all(has_explicit_reference_head)
        }
        _ => false,
    }
}

fn require_type(
    actual: &PatternType,
    expected: &PatternType,
    description: &str,
) -> Result<(), PatternError> {
    if actual == expected {
        Ok(())
    } else {
        Err(pattern001(
            PatternErrorKind::TypeMismatch,
            format!("{description} does not match the scrutinee type"),
        ))
    }
}

fn check_const(constant: &PatternConst, expected: &PatternType) -> Result<(), PatternError> {
    if constant.dependency.is_empty() {
        return Err(pattern001(
            PatternErrorKind::InvalidLiteral,
            "const pattern has an empty canonical dependency",
        ));
    }
    require_type(&constant.ty, expected, "const pattern")?;
    if !is_finite_structural_type(expected) {
        return Err(pattern001(
            PatternErrorKind::UnsupportedType,
            "const pattern type is not finite structural",
        ));
    }
    Ok(())
}

fn is_finite_structural_type(ty: &PatternType) -> bool {
    match ty {
        PatternType::Unit | PatternType::Bool | PatternType::Integer(_) | PatternType::Char => true,
        PatternType::Tuple(fields) => fields.iter().all(is_finite_structural_type),
        PatternType::Array { element, .. } => is_finite_structural_type(element),
        PatternType::SymbolicArray { .. } => false,
        PatternType::Record(record) => record
            .fields
            .iter()
            .all(|field| is_finite_structural_type(&field.ty)),
        PatternType::Enum(enum_ty) => enum_ty
            .variants
            .iter()
            .all(|variant| variant.fields.iter().all(is_finite_structural_type)),
        PatternType::String
        | PatternType::Str
        | PatternType::Slice(_)
        | PatternType::Reference { .. }
        | PatternType::Float(_)
        | PatternType::Opaque(_)
        | PatternType::Unsupported(_) => false,
    }
}

fn check_binding(
    binding: &PatternBinding,
    expected: &PatternType,
    context: CheckContext,
) -> Result<TypedBinding, PatternError> {
    if binding.name.is_empty() {
        return Err(pattern001(
            PatternErrorKind::DuplicateBinding,
            "binding name is empty",
        ));
    }
    let mode = match binding.annotation {
        BindingAnnotation::Inferred => context.default_mode,
        BindingAnnotation::Ref => BindingMode::Ref,
        BindingAnnotation::RefMut => BindingMode::RefMut,
    };
    if mode == BindingMode::RefMut && !context.mutable_path {
        return Err(pattern001(
            PatternErrorKind::MutableBorrowOfImmutablePath,
            format!("binding `{}` requires a mutable path", binding.name),
        ));
    }
    let binding_type = match mode {
        BindingMode::Move => expected.clone(),
        BindingMode::Ref => PatternType::reference(ReferenceMutability::Shared, expected.clone()),
        BindingMode::RefMut => {
            PatternType::reference(ReferenceMutability::Mutable, expected.clone())
        }
    };
    Ok(TypedBinding {
        name: binding.name.clone(),
        matched_type: expected.clone(),
        binding_type,
        mode,
        variable_mutable: binding.variable_mutable,
    })
}

fn check_literal(literal: &PatternLiteral, expected: &PatternType) -> Result<(), PatternError> {
    let valid = match (literal, expected) {
        (PatternLiteral::Bool(_), PatternType::Bool)
        | (PatternLiteral::Char(_), PatternType::Char) => true,
        (PatternLiteral::Signed(value), PatternType::Integer(integer)) => integer
            .signed_bounds()
            .is_some_and(|(minimum, maximum)| (minimum..=maximum).contains(value)),
        (PatternLiteral::Unsigned(value), PatternType::Integer(integer)) => integer
            .unsigned_max()
            .is_some_and(|maximum| *value <= maximum),
        (PatternLiteral::String(_), PatternType::String | PatternType::Str) => true,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(pattern001(
            PatternErrorKind::InvalidLiteral,
            "literal pattern does not fit the scrutinee type",
        ))
    }
}

fn check_range(
    start: &RangeEndpoint,
    end: &RangeEndpoint,
    inclusive: bool,
    expected: &PatternType,
) -> Result<TypedPatternKind, PatternError> {
    if !matches!(expected, PatternType::Integer(_) | PatternType::Char) {
        return Err(pattern001(
            PatternErrorKind::TypeMismatch,
            "range pattern requires an integer or character scrutinee",
        ));
    }
    let start = check_endpoint(start, expected)?;
    let end = check_endpoint(end, expected)?;
    if let (TypedRangeEndpoint::Literal(start), TypedRangeEndpoint::Literal(end)) = (&start, &end) {
        match compare_literals(start, end) {
            Some(std::cmp::Ordering::Greater) => {
                return Err(pattern001(
                    PatternErrorKind::InvalidRange,
                    "range pattern descends",
                ));
            }
            Some(std::cmp::Ordering::Equal) if !inclusive => {
                // An empty exclusive range is well-typed and is rejected by usefulness.
            }
            Some(_) => {}
            None => {
                return Err(pattern001(
                    PatternErrorKind::TypeMismatch,
                    "range endpoints have different scalar types",
                ));
            }
        }
    }
    Ok(TypedPatternKind::Range {
        start,
        end,
        inclusive,
    })
}

fn check_endpoint(
    endpoint: &RangeEndpoint,
    expected: &PatternType,
) -> Result<TypedRangeEndpoint, PatternError> {
    match endpoint {
        RangeEndpoint::Literal(literal) => {
            check_literal(literal, expected)?;
            Ok(TypedRangeEndpoint::Literal(literal.clone()))
        }
        RangeEndpoint::Const(constant) => {
            check_const(constant, expected)?;
            Ok(TypedRangeEndpoint::NeedsCtfe(constant.clone()))
        }
    }
}

fn compare_literals(first: &PatternLiteral, second: &PatternLiteral) -> Option<std::cmp::Ordering> {
    match (first, second) {
        (PatternLiteral::Signed(first), PatternLiteral::Signed(second)) => {
            first.partial_cmp(second)
        }
        (PatternLiteral::Unsigned(first), PatternLiteral::Unsigned(second)) => {
            first.partial_cmp(second)
        }
        (PatternLiteral::Char(first), PatternLiteral::Char(second)) => first.partial_cmp(second),
        _ => None,
    }
}

fn binding_map(pattern: &TypedPattern) -> Result<BTreeMap<&str, &TypedBinding>, PatternError> {
    let mut bindings = BTreeMap::new();
    collect_binding_map(pattern, &mut bindings)?;
    Ok(bindings)
}

fn collect_binding_map<'a>(
    pattern: &'a TypedPattern,
    bindings: &mut BTreeMap<&'a str, &'a TypedBinding>,
) -> Result<(), PatternError> {
    match &pattern.kind {
        TypedPatternKind::Binding(binding) => insert_binding(bindings, binding),
        TypedPatternKind::At { binding, pattern } => {
            insert_binding(bindings, binding)?;
            collect_binding_map(pattern, bindings)
        }
        TypedPatternKind::Dereference { pattern, .. } => collect_binding_map(pattern, bindings),
        TypedPatternKind::Tuple(fields) => {
            for field in fields {
                collect_binding_map(field, bindings)?;
            }
            Ok(())
        }
        TypedPatternKind::Slice { elements, .. }
        | TypedPatternKind::Record {
            fields: elements, ..
        }
        | TypedPatternKind::Constructor {
            fields: elements, ..
        }
        | TypedPatternKind::RecordConstructor {
            fields: elements, ..
        } => {
            for element in elements {
                collect_binding_map(element, bindings)?;
            }
            Ok(())
        }
        TypedPatternKind::DynamicSlice { prefix, suffix, .. }
        | TypedPatternKind::SymbolicSlice { prefix, suffix, .. } => {
            for element in prefix.iter().chain(suffix.iter()) {
                collect_binding_map(element, bindings)?;
            }
            Ok(())
        }
        TypedPatternKind::Or(_) => Ok(()),
        TypedPatternKind::Wildcard
        | TypedPatternKind::Unit
        | TypedPatternKind::Literal(_)
        | TypedPatternKind::NeedsCtfe(_)
        | TypedPatternKind::Range { .. } => Ok(()),
    }
}

fn insert_binding<'a>(
    bindings: &mut BTreeMap<&'a str, &'a TypedBinding>,
    binding: &'a TypedBinding,
) -> Result<(), PatternError> {
    if bindings.insert(binding.name(), binding).is_some() {
        Err(pattern001(
            PatternErrorKind::DuplicateBinding,
            format!("binding `{}` appears more than once", binding.name()),
        ))
    } else {
        Ok(())
    }
}

fn validate_unique_bindings(pattern: &TypedPattern) -> Result<(), PatternError> {
    for alternative in expand_typed(pattern) {
        binding_map(&alternative)?;
    }
    Ok(())
}

fn validate_or_bindings(pattern: &TypedPattern) -> Result<(), PatternError> {
    if !contains_or(pattern) {
        return Ok(());
    }
    let alternatives = expand_typed(pattern);
    let first = binding_map(&alternatives[0])?;
    for alternative in &alternatives[1..] {
        let candidate = binding_map(alternative)?;
        if candidate.len() != first.len()
            || first.iter().any(|(name, binding)| {
                candidate.get(name).is_none_or(|candidate| {
                    binding.matched_type != candidate.matched_type
                        || binding.binding_type != candidate.binding_type
                        || binding.mode != candidate.mode
                        || binding.variable_mutable != candidate.variable_mutable
                })
            })
        {
            return Err(pattern001(
                PatternErrorKind::OrBindingMismatch,
                "or-pattern alternatives bind different names, types, or modes",
            ));
        }
    }
    Ok(())
}

fn contains_or(pattern: &TypedPattern) -> bool {
    match &pattern.kind {
        TypedPatternKind::Or(_) => true,
        TypedPatternKind::Dereference { pattern, .. } | TypedPatternKind::At { pattern, .. } => {
            contains_or(pattern)
        }
        TypedPatternKind::Tuple(fields) => fields.iter().any(contains_or),
        TypedPatternKind::Slice { elements, .. }
        | TypedPatternKind::Record {
            fields: elements, ..
        }
        | TypedPatternKind::Constructor {
            fields: elements, ..
        }
        | TypedPatternKind::RecordConstructor {
            fields: elements, ..
        } => elements.iter().any(contains_or),
        TypedPatternKind::DynamicSlice { prefix, suffix, .. }
        | TypedPatternKind::SymbolicSlice { prefix, suffix, .. } => {
            prefix.iter().chain(suffix.iter()).any(contains_or)
        }
        _ => false,
    }
}

fn validate_duplicate_or_alternatives(pattern: &TypedPattern) -> Result<(), PatternError> {
    if let TypedPatternKind::Or(alternatives) = &pattern.kind {
        let mut prior = Vec::new();
        for alternative in alternatives {
            for canonical in expand_cover(&to_cover(alternative)) {
                if cover_contains_pending(&canonical) {
                    continue;
                }
                if prior.contains(&canonical) {
                    return Err(PatternError::without_arm(
                        PatternDiagnosticCode::Pattern001,
                        PatternErrorKind::DuplicateOrAlternative,
                        "or-pattern contains a duplicate alternative",
                    ));
                }
                prior.push(canonical);
            }
        }
    }
    match &pattern.kind {
        TypedPatternKind::Dereference { pattern, .. } | TypedPatternKind::At { pattern, .. } => {
            validate_duplicate_or_alternatives(pattern)
        }
        TypedPatternKind::Tuple(fields) => {
            for field in fields {
                validate_duplicate_or_alternatives(field)?;
            }
            Ok(())
        }
        TypedPatternKind::Slice { elements, .. }
        | TypedPatternKind::Record {
            fields: elements, ..
        }
        | TypedPatternKind::Constructor {
            fields: elements, ..
        }
        | TypedPatternKind::RecordConstructor {
            fields: elements, ..
        }
        | TypedPatternKind::Or(elements) => {
            for element in elements {
                validate_duplicate_or_alternatives(element)?;
            }
            Ok(())
        }
        TypedPatternKind::DynamicSlice { prefix, suffix, .. }
        | TypedPatternKind::SymbolicSlice { prefix, suffix, .. } => {
            for element in prefix.iter().chain(suffix.iter()) {
                validate_duplicate_or_alternatives(element)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn cover_contains_pending(pattern: &CoverPattern) -> bool {
    match pattern {
        CoverPattern::Pending => true,
        CoverPattern::Constructor(_, fields) => fields.iter().any(cover_contains_pending),
        CoverPattern::Slice { prefix, suffix, .. } => prefix
            .iter()
            .chain(suffix.iter())
            .any(cover_contains_pending),
        CoverPattern::Or(alternatives) => alternatives.iter().any(cover_contains_pending),
        CoverPattern::Empty
        | CoverPattern::Wildcard
        | CoverPattern::Signed(_, _)
        | CoverPattern::Unsigned(_, _)
        | CoverPattern::Char(_, _)
        | CoverPattern::String(_) => false,
    }
}

fn collect_dependencies(pattern: &TypedPattern) -> Vec<PatternConst> {
    let mut dependencies = BTreeSet::new();
    collect_dependencies_into(pattern, &mut dependencies);
    dependencies.into_iter().collect()
}

fn collect_dependencies_into(pattern: &TypedPattern, output: &mut BTreeSet<PatternConst>) {
    match &pattern.kind {
        TypedPatternKind::NeedsCtfe(constant) => {
            output.insert(constant.clone());
        }
        TypedPatternKind::Range { start, end, .. } => {
            if let TypedRangeEndpoint::NeedsCtfe(constant) = start {
                output.insert(constant.clone());
            }
            if let TypedRangeEndpoint::NeedsCtfe(constant) = end {
                output.insert(constant.clone());
            }
        }
        TypedPatternKind::Dereference { pattern, .. } | TypedPatternKind::At { pattern, .. } => {
            collect_dependencies_into(pattern, output)
        }
        TypedPatternKind::Tuple(fields) => {
            for field in fields {
                collect_dependencies_into(field, output);
            }
        }
        TypedPatternKind::Slice { elements, .. }
        | TypedPatternKind::Record {
            fields: elements, ..
        }
        | TypedPatternKind::Constructor {
            fields: elements, ..
        }
        | TypedPatternKind::RecordConstructor {
            fields: elements, ..
        }
        | TypedPatternKind::Or(elements) => {
            for element in elements {
                collect_dependencies_into(element, output);
            }
        }
        TypedPatternKind::DynamicSlice { prefix, suffix, .. } => {
            for element in prefix.iter().chain(suffix.iter()) {
                collect_dependencies_into(element, output);
            }
        }
        TypedPatternKind::SymbolicSlice {
            prefix,
            suffix,
            length,
            ..
        } => {
            output.insert(length.clone());
            for element in prefix.iter().chain(suffix.iter()) {
                collect_dependencies_into(element, output);
            }
        }
        _ => {}
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CoverPattern {
    Empty,
    Wildcard,
    Constructor(ConstructorTag, Box<[CoverPattern]>),
    Signed(i128, i128),
    Unsigned(u128, u128),
    Char(u32, u32),
    String(Box<str>),
    Slice {
        prefix: Box<[CoverPattern]>,
        has_rest: bool,
        suffix: Box<[CoverPattern]>,
    },
    Or(Box<[CoverPattern]>),
    Pending,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConstructorTag {
    Unit,
    Bool(bool),
    Tuple,
    Array,
    Record,
    Enum(usize),
    Reference(ReferenceMutability),
    String(Box<str>),
    StringOther,
    /// The single catch-all constructor of an opaque domain (floats and
    /// bound generic parameters): only a wildcard or binding covers it, so
    /// exhaustiveness always requires one.
    Opaque,
    SliceLength {
        minimum: usize,
        maximum: Option<usize>,
    },
    Signed(i128, i128),
    Unsigned(u128, u128),
    Char(u32, u32),
}

fn to_cover(pattern: &TypedPattern) -> CoverPattern {
    match &pattern.kind {
        TypedPatternKind::Wildcard | TypedPatternKind::Binding(_) => CoverPattern::Wildcard,
        TypedPatternKind::Unit => CoverPattern::Constructor(ConstructorTag::Unit, Box::default()),
        TypedPatternKind::Literal(literal) => match literal {
            PatternLiteral::Bool(value) => {
                CoverPattern::Constructor(ConstructorTag::Bool(*value), Box::default())
            }
            PatternLiteral::Signed(value) => CoverPattern::Signed(*value, *value),
            PatternLiteral::Unsigned(value) => CoverPattern::Unsigned(*value, *value),
            PatternLiteral::Char(value) => CoverPattern::Char(*value as u32, *value as u32),
            PatternLiteral::String(value) => CoverPattern::String(value.clone()),
        },
        TypedPatternKind::NeedsCtfe(_) => CoverPattern::Pending,
        TypedPatternKind::Dereference {
            mutability,
            pattern,
            ..
        } => CoverPattern::Constructor(
            ConstructorTag::Reference(*mutability),
            vec![to_cover(pattern)].into_boxed_slice(),
        ),
        TypedPatternKind::Tuple(fields) => CoverPattern::Constructor(
            ConstructorTag::Tuple,
            fields
                .iter()
                .map(to_cover)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        TypedPatternKind::Slice { elements, .. } => CoverPattern::Constructor(
            ConstructorTag::Array,
            elements
                .iter()
                .map(to_cover)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        TypedPatternKind::DynamicSlice {
            prefix,
            has_rest,
            suffix,
        } => CoverPattern::Slice {
            prefix: prefix
                .iter()
                .map(to_cover)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            has_rest: *has_rest,
            suffix: suffix
                .iter()
                .map(to_cover)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        },
        TypedPatternKind::SymbolicSlice { prefix, suffix, .. } => {
            if prefix
                .iter()
                .chain(suffix.iter())
                .map(to_cover)
                .any(|cover| cover_is_definitely_empty(&cover))
            {
                CoverPattern::Empty
            } else {
                CoverPattern::Pending
            }
        }
        TypedPatternKind::Record { fields, .. } => CoverPattern::Constructor(
            ConstructorTag::Record,
            fields
                .iter()
                .map(to_cover)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        TypedPatternKind::Constructor {
            variant_index,
            fields,
            ..
        } => CoverPattern::Constructor(
            ConstructorTag::Enum(*variant_index),
            fields
                .iter()
                .map(to_cover)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        TypedPatternKind::RecordConstructor {
            variant_index,
            fields,
            ..
        } => CoverPattern::Constructor(
            ConstructorTag::Enum(*variant_index),
            fields
                .iter()
                .map(to_cover)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        TypedPatternKind::Range {
            start,
            end,
            inclusive,
        } => literal_range_cover(start, end, *inclusive),
        TypedPatternKind::At { pattern, .. } => to_cover(pattern),
        TypedPatternKind::Or(alternatives) => CoverPattern::Or(
            alternatives
                .iter()
                .map(to_cover)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
    }
}

fn literal_range_cover(
    start: &TypedRangeEndpoint,
    end: &TypedRangeEndpoint,
    inclusive: bool,
) -> CoverPattern {
    let (TypedRangeEndpoint::Literal(start), TypedRangeEndpoint::Literal(end)) = (start, end)
    else {
        return CoverPattern::Pending;
    };
    match (start, end) {
        (PatternLiteral::Signed(start), PatternLiteral::Signed(end)) => {
            if inclusive {
                CoverPattern::Signed(*start, *end)
            } else if start == end {
                CoverPattern::Empty
            } else {
                CoverPattern::Signed(*start, *end - 1)
            }
        }
        (PatternLiteral::Unsigned(start), PatternLiteral::Unsigned(end)) => {
            if inclusive {
                CoverPattern::Unsigned(*start, *end)
            } else if start == end {
                CoverPattern::Empty
            } else {
                CoverPattern::Unsigned(*start, *end - 1)
            }
        }
        (PatternLiteral::Char(start), PatternLiteral::Char(end)) => {
            if inclusive {
                CoverPattern::Char(*start as u32, *end as u32)
            } else if start == end {
                CoverPattern::Empty
            } else {
                CoverPattern::Char(*start as u32, (*end as u32) - 1)
            }
        }
        _ => CoverPattern::Empty,
    }
}

fn cover_is_definitely_empty(pattern: &CoverPattern) -> bool {
    match pattern {
        CoverPattern::Empty => true,
        CoverPattern::Constructor(_, fields) => fields.iter().any(cover_is_definitely_empty),
        CoverPattern::Slice { prefix, suffix, .. } => prefix
            .iter()
            .chain(suffix.iter())
            .any(cover_is_definitely_empty),
        CoverPattern::Or(alternatives) => alternatives.iter().all(cover_is_definitely_empty),
        CoverPattern::Wildcard
        | CoverPattern::Signed(_, _)
        | CoverPattern::Unsigned(_, _)
        | CoverPattern::Char(_, _)
        | CoverPattern::String(_)
        | CoverPattern::Pending => false,
    }
}

fn replace_pending_with_wildcard(pattern: &CoverPattern) -> CoverPattern {
    match pattern {
        CoverPattern::Pending => CoverPattern::Wildcard,
        CoverPattern::Constructor(tag, fields) => CoverPattern::Constructor(
            tag.clone(),
            fields
                .iter()
                .map(replace_pending_with_wildcard)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        CoverPattern::Slice {
            prefix,
            has_rest,
            suffix,
        } => CoverPattern::Slice {
            prefix: prefix
                .iter()
                .map(replace_pending_with_wildcard)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            has_rest: *has_rest,
            suffix: suffix
                .iter()
                .map(replace_pending_with_wildcard)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        },
        CoverPattern::Or(alternatives) => CoverPattern::Or(
            alternatives
                .iter()
                .map(replace_pending_with_wildcard)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        other => other.clone(),
    }
}

fn expand_cover(pattern: &CoverPattern) -> Vec<CoverPattern> {
    match pattern {
        CoverPattern::Or(alternatives) => alternatives.iter().flat_map(expand_cover).collect(),
        CoverPattern::Constructor(tag, fields) => expand_cover_product(fields)
            .into_iter()
            .map(|fields| CoverPattern::Constructor(tag.clone(), fields.into_boxed_slice()))
            .collect(),
        CoverPattern::Slice {
            prefix,
            has_rest,
            suffix,
        } => {
            let prefix_length = prefix.len();
            let fields = prefix
                .iter()
                .chain(suffix.iter())
                .cloned()
                .collect::<Vec<_>>();
            expand_cover_product(&fields)
                .into_iter()
                .map(|fields| CoverPattern::Slice {
                    prefix: fields[..prefix_length].to_vec().into_boxed_slice(),
                    has_rest: *has_rest,
                    suffix: fields[prefix_length..].to_vec().into_boxed_slice(),
                })
                .collect()
        }
        other => vec![other.clone()],
    }
}

fn expand_cover_product(fields: &[CoverPattern]) -> Vec<Vec<CoverPattern>> {
    let mut products = vec![Vec::new()];
    for field in fields {
        let alternatives = expand_cover(field);
        let mut next = Vec::new();
        for prefix in products {
            for alternative in &alternatives {
                let mut product = prefix.clone();
                product.push(alternative.clone());
                next.push(product);
            }
        }
        products = next;
    }
    products
}

#[derive(Clone)]
struct ConstructorSpec {
    tag: ConstructorTag,
    fields: Vec<PatternType>,
}

fn useful(matrix: &[Vec<CoverPattern>], candidate: &[CoverPattern], types: &[PatternType]) -> bool {
    if candidate.is_empty() {
        return matrix.is_empty();
    }
    let constructors = constructor_partition(&types[0], matrix, &candidate[0]);
    for constructor in constructors {
        let Some(mut specialized_candidate) = specialize(&candidate[0], &constructor) else {
            continue;
        };
        specialized_candidate.extend_from_slice(&candidate[1..]);
        let mut specialized_matrix = Vec::new();
        for row in matrix {
            if let Some(mut specialized_row) = specialize(&row[0], &constructor) {
                specialized_row.extend_from_slice(&row[1..]);
                specialized_matrix.push(specialized_row);
            }
        }
        let mut specialized_types = constructor.fields.clone();
        specialized_types.extend_from_slice(&types[1..]);
        if useful(
            &specialized_matrix,
            &specialized_candidate,
            &specialized_types,
        ) {
            return true;
        }
    }
    false
}

fn specialize(pattern: &CoverPattern, constructor: &ConstructorSpec) -> Option<Vec<CoverPattern>> {
    match pattern {
        CoverPattern::Wildcard => Some(vec![CoverPattern::Wildcard; constructor.fields.len()]),
        CoverPattern::Constructor(tag, fields) if tag == &constructor.tag => Some(fields.to_vec()),
        CoverPattern::Signed(start, end) => match &constructor.tag {
            ConstructorTag::Signed(atom_start, atom_end)
                if *start <= *atom_start && *atom_end <= *end =>
            {
                Some(Vec::new())
            }
            _ => None,
        },
        CoverPattern::Unsigned(start, end) => match &constructor.tag {
            ConstructorTag::Unsigned(atom_start, atom_end)
                if *start <= *atom_start && *atom_end <= *end =>
            {
                Some(Vec::new())
            }
            _ => None,
        },
        CoverPattern::Char(start, end) => match &constructor.tag {
            ConstructorTag::Char(atom_start, atom_end)
                if *start <= *atom_start && *atom_end <= *end =>
            {
                Some(Vec::new())
            }
            _ => None,
        },
        CoverPattern::String(value) => match &constructor.tag {
            ConstructorTag::String(atom) if atom == value => Some(Vec::new()),
            _ => None,
        },
        CoverPattern::Slice {
            prefix,
            has_rest,
            suffix,
        } => specialize_slice(prefix, *has_rest, suffix, constructor),
        CoverPattern::Empty
        | CoverPattern::Pending
        | CoverPattern::Or(_)
        | CoverPattern::Constructor(_, _) => None,
    }
}

fn specialize_slice(
    prefix: &[CoverPattern],
    has_rest: bool,
    suffix: &[CoverPattern],
    constructor: &ConstructorSpec,
) -> Option<Vec<CoverPattern>> {
    let ConstructorTag::SliceLength { minimum, maximum } = &constructor.tag else {
        return None;
    };
    let explicit = prefix.len().checked_add(suffix.len())?;
    let matches_atom = if has_rest {
        *minimum >= explicit
    } else {
        *minimum == explicit && maximum.is_some_and(|maximum| maximum == explicit)
    };
    if !matches_atom || constructor.fields.len() != *minimum {
        return None;
    }

    let mut fields = vec![CoverPattern::Wildcard; *minimum];
    fields[..prefix.len()].clone_from_slice(prefix);
    let suffix_start = minimum.checked_sub(suffix.len())?;
    fields[suffix_start..].clone_from_slice(suffix);
    Some(fields)
}

fn constructor_partition(
    ty: &PatternType,
    matrix: &[Vec<CoverPattern>],
    candidate: &CoverPattern,
) -> Vec<ConstructorSpec> {
    match ty {
        PatternType::Unit => vec![ConstructorSpec {
            tag: ConstructorTag::Unit,
            fields: Vec::new(),
        }],
        PatternType::Bool => [false, true]
            .into_iter()
            .map(|value| ConstructorSpec {
                tag: ConstructorTag::Bool(value),
                fields: Vec::new(),
            })
            .collect(),
        PatternType::Tuple(fields) => vec![ConstructorSpec {
            tag: ConstructorTag::Tuple,
            fields: fields.to_vec(),
        }],
        PatternType::Array { element, length } => vec![ConstructorSpec {
            tag: ConstructorTag::Array,
            fields: vec![element.as_ref().clone(); *length],
        }],
        PatternType::SymbolicArray { .. } => vec![ConstructorSpec {
            tag: ConstructorTag::Array,
            fields: Vec::new(),
        }],
        PatternType::Slice(element) => slice_partition(element, matrix, candidate),
        PatternType::Record(record) => vec![ConstructorSpec {
            tag: ConstructorTag::Record,
            fields: record.fields.iter().map(|field| field.ty.clone()).collect(),
        }],
        PatternType::Enum(enum_ty) => enum_ty
            .variants
            .iter()
            .enumerate()
            .map(|(index, variant)| ConstructorSpec {
                tag: ConstructorTag::Enum(index),
                fields: variant.fields.to_vec(),
            })
            .collect(),
        PatternType::Reference {
            mutability,
            referent,
        } => vec![ConstructorSpec {
            tag: ConstructorTag::Reference(*mutability),
            fields: vec![referent.as_ref().clone()],
        }],
        PatternType::Integer(IntegerType::Signed(bits)) => {
            let integer = IntegerType::Signed(*bits);
            let (minimum, maximum) = integer.signed_bounds().expect("validated signed type");
            signed_partition(minimum, maximum, matrix, candidate)
        }
        PatternType::Integer(IntegerType::Unsigned(bits)) => {
            let integer = IntegerType::Unsigned(*bits);
            unsigned_partition(
                0,
                integer.unsigned_max().expect("validated unsigned type"),
                matrix,
                candidate,
            )
        }
        PatternType::Char => char_partition(matrix, candidate),
        PatternType::String | PatternType::Str => string_partition(matrix, candidate),
        PatternType::Float(_) | PatternType::Opaque(_) => vec![ConstructorSpec {
            tag: ConstructorTag::Opaque,
            fields: Vec::new(),
        }],
        PatternType::Unsupported(_) => Vec::new(),
    }
}

fn string_partition(
    matrix: &[Vec<CoverPattern>],
    candidate: &CoverPattern,
) -> Vec<ConstructorSpec> {
    let mut literals = BTreeSet::new();
    for pattern in matrix
        .iter()
        .map(|row| &row[0])
        .chain(std::iter::once(candidate))
    {
        if let CoverPattern::String(value) = pattern {
            literals.insert(value.clone());
        }
    }
    literals
        .into_iter()
        .map(|value| ConstructorSpec {
            tag: ConstructorTag::String(value),
            fields: Vec::new(),
        })
        .chain(std::iter::once(ConstructorSpec {
            tag: ConstructorTag::StringOther,
            fields: Vec::new(),
        }))
        .collect()
}

fn slice_partition(
    element: &PatternType,
    matrix: &[Vec<CoverPattern>],
    candidate: &CoverPattern,
) -> Vec<ConstructorSpec> {
    let mut maximum_prefix = 0_usize;
    let mut maximum_suffix = 0_usize;
    let mut exact_lengths = Vec::new();
    for pattern in matrix
        .iter()
        .map(|row| &row[0])
        .chain(std::iter::once(candidate))
    {
        if let CoverPattern::Slice {
            prefix,
            has_rest,
            suffix,
        } = pattern
        {
            maximum_prefix = maximum_prefix.max(prefix.len());
            maximum_suffix = maximum_suffix.max(suffix.len());
            if !has_rest {
                exact_lengths.push(
                    prefix
                        .len()
                        .checked_add(suffix.len())
                        .expect("two resident pattern field counts fit usize"),
                );
            }
        }
    }
    // Below this threshold, a from-start projection and a from-end projection
    // can alias at a different index for every concrete length. At and above it
    // all source-observable prefix/suffix slots are disjoint, so one residual
    // constructor is sufficient. Exact-length patterns additionally isolate
    // their one accepted length from the following residual interval.
    let stabilization = maximum_prefix
        .checked_add(maximum_suffix)
        .expect("two resident pattern field counts fit usize");
    let mut boundaries = (0..=stabilization).collect::<BTreeSet<_>>();
    for exact in exact_lengths {
        if let Some(after) = exact.checked_add(1) {
            boundaries.insert(after);
        }
    }
    let boundaries = boundaries.into_iter().collect::<Vec<_>>();
    boundaries
        .iter()
        .enumerate()
        .map(|(index, minimum)| ConstructorSpec {
            tag: ConstructorTag::SliceLength {
                minimum: *minimum,
                maximum: boundaries.get(index + 1).map(|next| next - 1),
            },
            fields: vec![element.clone(); *minimum],
        })
        .collect()
}

fn signed_partition(
    minimum: i128,
    maximum: i128,
    matrix: &[Vec<CoverPattern>],
    candidate: &CoverPattern,
) -> Vec<ConstructorSpec> {
    let mut boundaries = vec![minimum];
    for pattern in matrix
        .iter()
        .map(|row| &row[0])
        .chain(std::iter::once(candidate))
    {
        if let CoverPattern::Signed(start, end) = pattern {
            boundaries.push(*start);
            if *end < maximum {
                boundaries.push(*end + 1);
            }
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
        .iter()
        .enumerate()
        .map(|(index, start)| {
            let end = boundaries.get(index + 1).map_or(maximum, |next| *next - 1);
            ConstructorSpec {
                tag: ConstructorTag::Signed(*start, end),
                fields: Vec::new(),
            }
        })
        .collect()
}

fn unsigned_partition(
    minimum: u128,
    maximum: u128,
    matrix: &[Vec<CoverPattern>],
    candidate: &CoverPattern,
) -> Vec<ConstructorSpec> {
    let mut boundaries = vec![minimum];
    for pattern in matrix
        .iter()
        .map(|row| &row[0])
        .chain(std::iter::once(candidate))
    {
        if let CoverPattern::Unsigned(start, end) = pattern {
            boundaries.push(*start);
            if *end < maximum {
                boundaries.push(*end + 1);
            }
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
        .iter()
        .enumerate()
        .map(|(index, start)| {
            let end = boundaries.get(index + 1).map_or(maximum, |next| *next - 1);
            ConstructorSpec {
                tag: ConstructorTag::Unsigned(*start, end),
                fields: Vec::new(),
            }
        })
        .collect()
}

fn char_partition(matrix: &[Vec<CoverPattern>], candidate: &CoverPattern) -> Vec<ConstructorSpec> {
    let mut output = Vec::new();
    for (minimum, maximum) in [(0_u32, 0xD7FF_u32), (0xE000_u32, 0x10_FFFF_u32)] {
        let mut boundaries = vec![minimum];
        for pattern in matrix
            .iter()
            .map(|row| &row[0])
            .chain(std::iter::once(candidate))
        {
            if let CoverPattern::Char(start, end) = pattern {
                let start = (*start).max(minimum);
                let end = (*end).min(maximum);
                if start <= end {
                    boundaries.push(start);
                    if end < maximum {
                        boundaries.push(end + 1);
                    }
                }
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        output.extend(boundaries.iter().enumerate().map(|(index, start)| {
            let end = boundaries.get(index + 1).map_or(maximum, |next| *next - 1);
            ConstructorSpec {
                tag: ConstructorTag::Char(*start, end),
                fields: Vec::new(),
            }
        }));
    }
    output
}

fn build_decision_tree(arms: &[TypedPatternArm]) -> DecisionTree {
    let mut tree = DecisionTree::Fail;
    for (arm_index, arm) in arms.iter().enumerate().rev() {
        let arm_fallback = tree.clone();
        let mut arm_tree = arm_fallback.clone();
        let alternatives = expand_typed(&arm.pattern);
        for alternative in alternatives.into_iter().rev() {
            let bindings = binding_facts(&alternative);
            let success = if arm.has_guard {
                DecisionTree::Guard {
                    arm_index,
                    bindings: bindings.clone().into_boxed_slice(),
                    on_true: Box::new(DecisionTree::Leaf {
                        arm_index,
                        bindings: bindings.into_boxed_slice(),
                    }),
                    on_false: Box::new(arm_fallback.clone()),
                }
            } else {
                DecisionTree::Leaf {
                    arm_index,
                    bindings: bindings.into_boxed_slice(),
                }
            };
            arm_tree = compile_tests(&alternative, Vec::new(), success, arm_tree);
        }
        tree = arm_tree;
    }
    tree
}

fn expand_typed(pattern: &TypedPattern) -> Vec<TypedPattern> {
    match &pattern.kind {
        TypedPatternKind::Or(alternatives) => alternatives.iter().flat_map(expand_typed).collect(),
        TypedPatternKind::Dereference {
            mutability,
            inserted,
            pattern: nested,
        } => expand_typed(nested)
            .into_iter()
            .map(|nested| TypedPattern {
                ty: pattern.ty.clone(),
                kind: TypedPatternKind::Dereference {
                    mutability: *mutability,
                    inserted: *inserted,
                    pattern: Box::new(nested),
                },
            })
            .collect(),
        TypedPatternKind::At {
            binding,
            pattern: nested,
        } => expand_typed(nested)
            .into_iter()
            .map(|nested| TypedPattern {
                ty: pattern.ty.clone(),
                kind: TypedPatternKind::At {
                    binding: binding.clone(),
                    pattern: Box::new(nested),
                },
            })
            .collect(),
        TypedPatternKind::Tuple(fields) => expand_typed_product(fields)
            .into_iter()
            .map(|fields| TypedPattern {
                ty: pattern.ty.clone(),
                kind: TypedPatternKind::Tuple(fields.into_boxed_slice()),
            })
            .collect(),
        TypedPatternKind::Slice {
            elements,
            prefix_length,
            suffix_length,
        } => expand_typed_product(elements)
            .into_iter()
            .map(|elements| TypedPattern {
                ty: pattern.ty.clone(),
                kind: TypedPatternKind::Slice {
                    elements: elements.into_boxed_slice(),
                    prefix_length: *prefix_length,
                    suffix_length: *suffix_length,
                },
            })
            .collect(),
        TypedPatternKind::DynamicSlice {
            prefix,
            has_rest,
            suffix,
        } => expand_typed_sequence(prefix, suffix)
            .into_iter()
            .map(|(prefix, suffix)| TypedPattern {
                ty: pattern.ty.clone(),
                kind: TypedPatternKind::DynamicSlice {
                    prefix: prefix.into_boxed_slice(),
                    has_rest: *has_rest,
                    suffix: suffix.into_boxed_slice(),
                },
            })
            .collect(),
        TypedPatternKind::SymbolicSlice {
            prefix,
            has_rest,
            suffix,
            length,
        } => expand_typed_sequence(prefix, suffix)
            .into_iter()
            .map(|(prefix, suffix)| TypedPattern {
                ty: pattern.ty.clone(),
                kind: TypedPatternKind::SymbolicSlice {
                    prefix: prefix.into_boxed_slice(),
                    has_rest: *has_rest,
                    suffix: suffix.into_boxed_slice(),
                    length: length.clone(),
                },
            })
            .collect(),
        TypedPatternKind::Record {
            record_name,
            field_names,
            fields,
        } => expand_typed_product(fields)
            .into_iter()
            .map(|fields| TypedPattern {
                ty: pattern.ty.clone(),
                kind: TypedPatternKind::Record {
                    record_name: record_name.clone(),
                    field_names: field_names.clone(),
                    fields: fields.into_boxed_slice(),
                },
            })
            .collect(),
        TypedPatternKind::Constructor {
            enum_name,
            variant_index,
            variant,
            fields,
        } => expand_typed_product(fields)
            .into_iter()
            .map(|fields| TypedPattern {
                ty: pattern.ty.clone(),
                kind: TypedPatternKind::Constructor {
                    enum_name: enum_name.clone(),
                    variant_index: *variant_index,
                    variant: variant.clone(),
                    fields: fields.into_boxed_slice(),
                },
            })
            .collect(),
        TypedPatternKind::RecordConstructor {
            enum_name,
            variant_index,
            variant,
            field_names,
            fields,
        } => expand_typed_product(fields)
            .into_iter()
            .map(|fields| TypedPattern {
                ty: pattern.ty.clone(),
                kind: TypedPatternKind::RecordConstructor {
                    enum_name: enum_name.clone(),
                    variant_index: *variant_index,
                    variant: variant.clone(),
                    field_names: field_names.clone(),
                    fields: fields.into_boxed_slice(),
                },
            })
            .collect(),
        _ => vec![pattern.clone()],
    }
}

fn expand_typed_sequence(
    prefix: &[TypedPattern],
    suffix: &[TypedPattern],
) -> Vec<(Vec<TypedPattern>, Vec<TypedPattern>)> {
    let prefix_length = prefix.len();
    let fields = prefix
        .iter()
        .chain(suffix.iter())
        .cloned()
        .collect::<Vec<_>>();
    expand_typed_product(&fields)
        .into_iter()
        .map(|fields| {
            (
                fields[..prefix_length].to_vec(),
                fields[prefix_length..].to_vec(),
            )
        })
        .collect()
}

fn expand_typed_product(fields: &[TypedPattern]) -> Vec<Vec<TypedPattern>> {
    let mut products = vec![Vec::new()];
    for field in fields {
        let alternatives = expand_typed(field);
        let mut next = Vec::new();
        for prefix in products {
            for alternative in &alternatives {
                let mut product = prefix.clone();
                product.push(alternative.clone());
                next.push(product);
            }
        }
        products = next;
    }
    products
}

fn binding_facts(pattern: &TypedPattern) -> Vec<PatternBindingFact> {
    let mut facts = Vec::new();
    collect_binding_facts(pattern, &mut Vec::new(), &mut facts);
    facts.sort();
    facts
}

fn collect_binding_facts(
    pattern: &TypedPattern,
    path: &mut Vec<PatternProjection>,
    facts: &mut Vec<PatternBindingFact>,
) {
    match &pattern.kind {
        TypedPatternKind::Binding(binding) => facts.push(make_binding_fact(binding, path)),
        TypedPatternKind::At { binding, pattern } => {
            facts.push(make_binding_fact(binding, path));
            collect_binding_facts(pattern, path, facts);
        }
        TypedPatternKind::Dereference {
            mutability,
            inserted,
            pattern,
        } => {
            path.push(if *inserted {
                PatternProjection::InsertedDeref(*mutability)
            } else {
                PatternProjection::ExplicitDeref(*mutability)
            });
            collect_binding_facts(pattern, path, facts);
            path.pop();
        }
        TypedPatternKind::Tuple(fields) => {
            for (index, field) in fields.iter().enumerate() {
                path.push(PatternProjection::TupleField(index));
                collect_binding_facts(field, path, facts);
                path.pop();
            }
        }
        TypedPatternKind::Slice { elements, .. } => {
            for (index, element) in elements.iter().enumerate() {
                path.push(PatternProjection::ArrayElement(index));
                collect_binding_facts(element, path, facts);
                path.pop();
            }
        }
        TypedPatternKind::DynamicSlice { prefix, suffix, .. }
        | TypedPatternKind::SymbolicSlice { prefix, suffix, .. } => {
            for (index, element) in prefix.iter().enumerate() {
                path.push(PatternProjection::SliceElementFromStart(index));
                collect_binding_facts(element, path, facts);
                path.pop();
            }
            for (index, element) in suffix.iter().enumerate() {
                path.push(PatternProjection::SliceElementFromEnd(
                    suffix.len() - index - 1,
                ));
                collect_binding_facts(element, path, facts);
                path.pop();
            }
        }
        TypedPatternKind::Record {
            record_name,
            field_names,
            fields,
        } => {
            for (field_index, (field, field_name)) in
                fields.iter().zip(field_names.iter()).enumerate()
            {
                path.push(PatternProjection::RecordField {
                    record_name: record_name.clone(),
                    field_index,
                    field: field_name.clone(),
                });
                collect_binding_facts(field, path, facts);
                path.pop();
            }
        }
        TypedPatternKind::Constructor {
            variant_index,
            fields,
            ..
        } => {
            for (field_index, field) in fields.iter().enumerate() {
                path.push(PatternProjection::EnumField {
                    variant_index: *variant_index,
                    field_index,
                });
                collect_binding_facts(field, path, facts);
                path.pop();
            }
        }
        TypedPatternKind::RecordConstructor {
            variant_index,
            fields,
            ..
        } => {
            for (field_index, field) in fields.iter().enumerate() {
                path.push(PatternProjection::EnumField {
                    variant_index: *variant_index,
                    field_index,
                });
                collect_binding_facts(field, path, facts);
                path.pop();
            }
        }
        TypedPatternKind::Or(alternatives) => {
            // Decision-tree construction expands every `Or` before this walk.
            debug_assert!(alternatives.is_empty());
        }
        _ => {}
    }
}

fn make_binding_fact(binding: &TypedBinding, path: &[PatternProjection]) -> PatternBindingFact {
    PatternBindingFact {
        binding: binding.clone(),
        path: path.to_vec().into_boxed_slice(),
        ownership: match binding.mode {
            BindingMode::Move => OwnershipFactKind::Move,
            BindingMode::Ref => OwnershipFactKind::Ref,
            BindingMode::RefMut => OwnershipFactKind::RefMut,
        },
    }
}

fn compile_tests(
    pattern: &TypedPattern,
    mut path: Vec<PatternProjection>,
    success: DecisionTree,
    failure: DecisionTree,
) -> DecisionTree {
    match &pattern.kind {
        TypedPatternKind::Wildcard | TypedPatternKind::Binding(_) | TypedPatternKind::Unit => {
            success
        }
        TypedPatternKind::Literal(literal) => {
            compile_literal_test(literal.clone(), path, success, failure)
        }
        TypedPatternKind::NeedsCtfe(constant) => DecisionTree::NeedsCtfe {
            path: path.into_boxed_slice(),
            test: PendingPatternTest::ConstEquals(constant.clone()),
            on_match: Box::new(success),
            on_mismatch: Box::new(failure),
        },
        TypedPatternKind::Dereference {
            mutability,
            inserted,
            pattern,
        } => {
            path.push(if *inserted {
                PatternProjection::InsertedDeref(*mutability)
            } else {
                PatternProjection::ExplicitDeref(*mutability)
            });
            compile_tests(pattern, path, success, failure)
        }
        TypedPatternKind::Tuple(fields) => compile_product_tests(
            fields,
            path,
            success,
            failure,
            PatternProjection::TupleField,
        ),
        TypedPatternKind::Slice { elements, .. } => compile_product_tests(
            elements,
            path,
            success,
            failure,
            PatternProjection::ArrayElement,
        ),
        TypedPatternKind::DynamicSlice {
            prefix,
            has_rest,
            suffix,
        } => compile_dynamic_slice_tests(prefix, *has_rest, suffix, path, success, failure),
        TypedPatternKind::SymbolicSlice {
            prefix,
            has_rest,
            suffix,
            length,
        } => {
            compile_symbolic_slice_tests(prefix, *has_rest, suffix, length, path, success, failure)
        }
        TypedPatternKind::Record {
            record_name,
            field_names,
            fields,
        } => compile_product_tests(fields, path, success, failure, |field_index| {
            PatternProjection::RecordField {
                record_name: record_name.clone(),
                field_index,
                field: field_names[field_index].clone(),
            }
        }),
        TypedPatternKind::Constructor {
            enum_name,
            variant_index,
            variant,
            fields,
        } => {
            let nested = compile_enum_fields(
                fields,
                *variant_index,
                path.clone(),
                success,
                failure.clone(),
            );
            DecisionTree::Test {
                path: path.into_boxed_slice(),
                test: PatternTest::EnumVariant {
                    enum_name: enum_name.clone(),
                    variant_index: *variant_index,
                    variant: variant.clone(),
                },
                on_match: Box::new(nested),
                on_mismatch: Box::new(failure),
            }
        }
        TypedPatternKind::RecordConstructor {
            enum_name,
            variant_index,
            variant,
            fields,
            ..
        } => {
            let nested = compile_enum_fields(
                fields,
                *variant_index,
                path.clone(),
                success,
                failure.clone(),
            );
            DecisionTree::Test {
                path: path.into_boxed_slice(),
                test: PatternTest::EnumVariant {
                    enum_name: enum_name.clone(),
                    variant_index: *variant_index,
                    variant: variant.clone(),
                },
                on_match: Box::new(nested),
                on_mismatch: Box::new(failure),
            }
        }
        TypedPatternKind::Range {
            start,
            end,
            inclusive,
        } => {
            if matches!(start, TypedRangeEndpoint::NeedsCtfe(_))
                || matches!(end, TypedRangeEndpoint::NeedsCtfe(_))
            {
                DecisionTree::NeedsCtfe {
                    path: path.into_boxed_slice(),
                    test: PendingPatternTest::Range {
                        ty: pattern.ty.clone(),
                        start: start.clone(),
                        end: end.clone(),
                        inclusive: *inclusive,
                    },
                    on_match: Box::new(success),
                    on_mismatch: Box::new(failure),
                }
            } else {
                compile_range_test(start, end, *inclusive, path, success, failure)
            }
        }
        TypedPatternKind::At { pattern, .. } => compile_tests(pattern, path, success, failure),
        TypedPatternKind::Or(_) => {
            unreachable!("or-patterns are expanded before tree construction")
        }
    }
}

fn sequence_length_constraint(
    prefix_length: usize,
    has_rest: bool,
    suffix_length: usize,
) -> SequenceLengthConstraint {
    let explicit = prefix_length
        .checked_add(suffix_length)
        .expect("two resident pattern field counts fit usize");
    if has_rest {
        SequenceLengthConstraint::AtLeast(explicit)
    } else {
        SequenceLengthConstraint::Exact(explicit)
    }
}

fn compile_dynamic_slice_tests(
    prefix: &[TypedPattern],
    has_rest: bool,
    suffix: &[TypedPattern],
    path: Vec<PatternProjection>,
    success: DecisionTree,
    failure: DecisionTree,
) -> DecisionTree {
    let constraint = sequence_length_constraint(prefix.len(), has_rest, suffix.len());
    let nested =
        compile_sequence_element_tests(prefix, suffix, path.clone(), success, failure.clone());
    if constraint == SequenceLengthConstraint::AtLeast(0) {
        nested
    } else {
        DecisionTree::Test {
            path: path.into_boxed_slice(),
            test: PatternTest::SliceLength(constraint),
            on_match: Box::new(nested),
            on_mismatch: Box::new(failure),
        }
    }
}

fn compile_symbolic_slice_tests(
    prefix: &[TypedPattern],
    has_rest: bool,
    suffix: &[TypedPattern],
    length: &PatternConst,
    path: Vec<PatternProjection>,
    success: DecisionTree,
    failure: DecisionTree,
) -> DecisionTree {
    let constraint = sequence_length_constraint(prefix.len(), has_rest, suffix.len());
    let nested =
        compile_sequence_element_tests(prefix, suffix, path.clone(), success, failure.clone());
    DecisionTree::NeedsCtfe {
        path: path.into_boxed_slice(),
        test: PendingPatternTest::ArrayLength {
            length: length.clone(),
            constraint,
        },
        on_match: Box::new(nested),
        on_mismatch: Box::new(failure),
    }
}

fn compile_sequence_element_tests(
    prefix: &[TypedPattern],
    suffix: &[TypedPattern],
    path: Vec<PatternProjection>,
    success: DecisionTree,
    failure: DecisionTree,
) -> DecisionTree {
    let mut tree = success;
    for (index, field) in suffix.iter().enumerate().rev() {
        let mut field_path = path.clone();
        field_path.push(PatternProjection::SliceElementFromEnd(
            suffix.len() - index - 1,
        ));
        tree = compile_tests(field, field_path, tree, failure.clone());
    }
    for (index, field) in prefix.iter().enumerate().rev() {
        let mut field_path = path.clone();
        field_path.push(PatternProjection::SliceElementFromStart(index));
        tree = compile_tests(field, field_path, tree, failure.clone());
    }
    tree
}

fn compile_product_tests(
    fields: &[TypedPattern],
    path: Vec<PatternProjection>,
    success: DecisionTree,
    failure: DecisionTree,
    projection: impl Fn(usize) -> PatternProjection,
) -> DecisionTree {
    let mut tree = success;
    for (index, field) in fields.iter().enumerate().rev() {
        let mut field_path = path.clone();
        field_path.push(projection(index));
        tree = compile_tests(field, field_path, tree, failure.clone());
    }
    tree
}

fn compile_enum_fields(
    fields: &[TypedPattern],
    variant_index: usize,
    path: Vec<PatternProjection>,
    success: DecisionTree,
    failure: DecisionTree,
) -> DecisionTree {
    let mut tree = success;
    for (field_index, field) in fields.iter().enumerate().rev() {
        let mut field_path = path.clone();
        field_path.push(PatternProjection::EnumField {
            variant_index,
            field_index,
        });
        tree = compile_tests(field, field_path, tree, failure.clone());
    }
    tree
}

fn compile_literal_test(
    literal: PatternLiteral,
    path: Vec<PatternProjection>,
    success: DecisionTree,
    failure: DecisionTree,
) -> DecisionTree {
    let test = match literal {
        PatternLiteral::Bool(value) => PatternTest::Bool(value),
        PatternLiteral::Signed(value) => PatternTest::SignedRange {
            start: value,
            end: value,
            inclusive: true,
        },
        PatternLiteral::Unsigned(value) => PatternTest::UnsignedRange {
            start: value,
            end: value,
            inclusive: true,
        },
        PatternLiteral::Char(value) => PatternTest::CharRange {
            start: value,
            end: value,
            inclusive: true,
        },
        PatternLiteral::String(value) => PatternTest::String(value),
    };
    DecisionTree::Test {
        path: path.into_boxed_slice(),
        test,
        on_match: Box::new(success),
        on_mismatch: Box::new(failure),
    }
}

fn compile_range_test(
    start: &TypedRangeEndpoint,
    end: &TypedRangeEndpoint,
    inclusive: bool,
    path: Vec<PatternProjection>,
    success: DecisionTree,
    failure: DecisionTree,
) -> DecisionTree {
    let (TypedRangeEndpoint::Literal(start), TypedRangeEndpoint::Literal(end)) = (start, end)
    else {
        unreachable!("pending range is compiled by the symbolic branch");
    };
    let test = match (start, end) {
        (PatternLiteral::Signed(start), PatternLiteral::Signed(end)) => PatternTest::SignedRange {
            start: *start,
            end: *end,
            inclusive,
        },
        (PatternLiteral::Unsigned(start), PatternLiteral::Unsigned(end)) => {
            PatternTest::UnsignedRange {
                start: *start,
                end: *end,
                inclusive,
            }
        }
        (PatternLiteral::Char(start), PatternLiteral::Char(end)) => PatternTest::CharRange {
            start: *start,
            end: *end,
            inclusive,
        },
        _ => unreachable!("checked range endpoints have one scalar type"),
    };
    DecisionTree::Test {
        path: path.into_boxed_slice(),
        test,
        on_match: Box::new(success),
        on_mismatch: Box::new(failure),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn immutable(ty: PatternType) -> PatternScrutinee {
        PatternScrutinee::new(ty, PlaceMutability::Immutable)
    }

    fn arm(pattern: Pattern) -> PatternArm {
        PatternArm::new(pattern, false)
    }

    fn wildcard() -> Pattern {
        Pattern::Wildcard
    }

    fn bool_literal(value: bool) -> Pattern {
        Pattern::Literal(PatternLiteral::Bool(value))
    }

    fn string_literal(value: &str) -> Pattern {
        Pattern::Literal(PatternLiteral::String(value.into()))
    }

    fn record_field(name: &str, pattern: Pattern) -> RecordPatternField {
        RecordPatternField::new(name, pattern)
    }

    fn option_bool() -> PatternType {
        PatternType::Enum(EnumType::new(
            "OptionBool",
            vec![
                EnumVariant::new("None", Vec::new()),
                EnumVariant::new("Some", vec![PatternType::Bool]),
            ],
        ))
    }

    #[test]
    fn bool_match_is_exhaustive() {
        let analysis = analyze_pattern_match(
            &immutable(PatternType::Bool),
            &[arm(bool_literal(false)), arm(bool_literal(true))],
        )
        .unwrap();
        let PatternMatchAnalysis::Complete(complete) = analysis else {
            panic!("literal-only bool match must be complete");
        };
        assert_eq!(complete.reachability().len(), 2);
        assert!(matches!(complete.tree(), DecisionTree::Test { .. }));
    }

    #[test]
    fn bool_match_reports_non_exhaustive() {
        let errors =
            analyze_pattern_match(&immutable(PatternType::Bool), &[arm(bool_literal(false))])
                .unwrap_err();
        assert_eq!(
            errors.as_slice()[0].kind(),
            &PatternErrorKind::NonExhaustiveMatch
        );
        assert_eq!(errors.as_slice()[0].code().as_str(), "PATTERN002");
    }

    #[test]
    fn enum_payload_and_nested_bool_are_exhaustive() {
        let analysis = analyze_pattern_match(
            &immutable(option_bool()),
            &[
                arm(Pattern::constructor("None", Vec::new())),
                arm(Pattern::constructor("Some", vec![bool_literal(false)])),
                arm(Pattern::constructor("Some", vec![bool_literal(true)])),
            ],
        )
        .unwrap();
        assert!(matches!(analysis, PatternMatchAnalysis::Complete(_)));
    }

    #[test]
    fn nested_tuple_constructor_space_finds_gap() {
        let ty = PatternType::tuple(vec![PatternType::Bool, option_bool()]);
        let errors = analyze_pattern_match(
            &immutable(ty),
            &[
                arm(Pattern::tuple(vec![bool_literal(false), wildcard()])),
                arm(Pattern::tuple(vec![
                    bool_literal(true),
                    Pattern::constructor("None", Vec::new()),
                ])),
            ],
        )
        .unwrap_err();
        assert!(errors
            .as_slice()
            .iter()
            .any(|error| error.kind() == &PatternErrorKind::NonExhaustiveMatch));
    }

    #[test]
    fn fixed_array_slice_patterns_are_type_driven() {
        let ty = PatternType::array(PatternType::Bool, 2);
        let analysis = analyze_pattern_match(
            &immutable(ty),
            &[
                arm(Pattern::slice(vec![bool_literal(false)], true, Vec::new())),
                arm(Pattern::slice(vec![bool_literal(true)], true, Vec::new())),
            ],
        )
        .unwrap();
        assert!(matches!(analysis, PatternMatchAnalysis::Complete(_)));
    }

    #[test]
    fn fixed_array_without_rest_requires_exact_length() {
        let errors = analyze_pattern_match(
            &immutable(PatternType::array(PatternType::Bool, 2)),
            &[arm(Pattern::slice(vec![wildcard()], false, Vec::new()))],
        )
        .unwrap_err();
        assert_eq!(errors.as_slice()[0].kind(), &PatternErrorKind::WrongArity);
    }

    #[test]
    fn disjoint_or_pattern_is_accepted() {
        let pattern = Pattern::or(vec![bool_literal(false), bool_literal(true)]);
        let analysis =
            analyze_pattern_match(&immutable(PatternType::Bool), &[arm(pattern)]).unwrap();
        assert!(matches!(analysis, PatternMatchAnalysis::Complete(_)));
    }

    #[test]
    fn duplicate_or_alternative_is_rejected() {
        let pattern = Pattern::or(vec![bool_literal(false), bool_literal(false)]);
        let errors = analyze_pattern_match(
            &immutable(PatternType::Bool),
            &[arm(pattern), arm(wildcard())],
        )
        .unwrap_err();
        assert_eq!(
            errors.as_slice()[0].kind(),
            &PatternErrorKind::DuplicateOrAlternative
        );
        assert_eq!(
            errors.as_slice()[0].code(),
            PatternDiagnosticCode::Pattern001
        );
    }

    #[test]
    fn or_bindings_require_same_names_and_modes() {
        let pattern = Pattern::or(vec![
            Pattern::Binding(PatternBinding::inferred("left")),
            Pattern::Binding(PatternBinding::new("right", BindingAnnotation::Ref, false)),
        ]);
        let errors =
            analyze_pattern_match(&immutable(PatternType::Bool), &[arm(pattern)]).unwrap_err();
        assert_eq!(
            errors.as_slice()[0].kind(),
            &PatternErrorKind::OrBindingMismatch
        );
    }

    #[test]
    fn fallback_after_wildcard_is_unreachable() {
        let errors = analyze_pattern_match(
            &immutable(PatternType::Bool),
            &[arm(wildcard()), arm(bool_literal(true))],
        )
        .unwrap_err();
        assert_eq!(
            errors.as_slice()[0].kind(),
            &PatternErrorKind::UnreachableArm
        );
        assert_eq!(errors.as_slice()[0].arm_index(), Some(1));
        assert_eq!(
            errors.as_slice()[0].code(),
            PatternDiagnosticCode::Pattern001
        );
    }

    #[test]
    fn guarded_arm_does_not_contribute_to_coverage() {
        let errors = analyze_pattern_match(
            &immutable(PatternType::Bool),
            &[PatternArm::new(wildcard(), true)],
        )
        .unwrap_err();
        assert_eq!(
            errors.as_slice()[0].kind(),
            &PatternErrorKind::NonExhaustiveMatch
        );
    }

    #[test]
    fn guarded_or_rejection_skips_the_rest_of_the_same_arm() {
        let guarded = PatternArm::new(
            Pattern::or(vec![bool_literal(false), bool_literal(true)]),
            true,
        );
        let analysis =
            analyze_pattern_match(&immutable(PatternType::Bool), &[guarded, arm(wildcard())])
                .unwrap();
        let PatternMatchAnalysis::Complete(complete) = analysis else {
            panic!("literal-only guarded match is concrete");
        };
        let DecisionTree::Test { on_match, .. } = complete.tree() else {
            panic!("the first alternative must begin with a test");
        };
        let DecisionTree::Guard { on_false, .. } = on_match.as_ref() else {
            panic!("a matching guarded alternative must evaluate its guard");
        };
        assert!(matches!(
            on_false.as_ref(),
            DecisionTree::Leaf { arm_index: 1, .. }
        ));
    }

    #[test]
    fn literal_ranges_partition_integer_space() {
        let i8_ty = PatternType::Integer(IntegerType::Signed(8));
        let range = |start, end| Pattern::Range {
            start: RangeEndpoint::Literal(PatternLiteral::Signed(start)),
            end: RangeEndpoint::Literal(PatternLiteral::Signed(end)),
            inclusive: true,
        };
        let analysis = analyze_pattern_match(
            &immutable(i8_ty),
            &[arm(range(-128, -1)), arm(range(0, 127))],
        )
        .unwrap();
        assert!(matches!(analysis, PatternMatchAnalysis::Complete(_)));
    }

    #[test]
    fn equal_exclusive_range_is_unreachable() {
        let pattern = Pattern::Range {
            start: RangeEndpoint::Literal(PatternLiteral::Unsigned(4)),
            end: RangeEndpoint::Literal(PatternLiteral::Unsigned(4)),
            inclusive: false,
        };
        let errors = analyze_pattern_match(
            &immutable(PatternType::Integer(IntegerType::Unsigned(8))),
            &[arm(pattern), arm(wildcard())],
        )
        .unwrap_err();
        assert_eq!(
            errors.as_slice()[0].kind(),
            &PatternErrorKind::UnreachableArm
        );
    }

    #[test]
    fn character_ranges_skip_surrogates_and_cover_the_domain() {
        let range = |start, end| Pattern::Range {
            start: RangeEndpoint::Literal(PatternLiteral::Char(start)),
            end: RangeEndpoint::Literal(PatternLiteral::Char(end)),
            inclusive: true,
        };
        let analysis = analyze_pattern_match(
            &immutable(PatternType::Char),
            &[
                arm(range('\0', '\u{D7FF}')),
                arm(range('\u{E000}', '\u{10FFFF}')),
            ],
        )
        .unwrap();
        assert!(matches!(analysis, PatternMatchAnalysis::Complete(_)));
    }

    #[test]
    fn exclusive_character_range_preserves_a_scalar_endpoint_in_the_tree() {
        let range = |start, end, inclusive| Pattern::Range {
            start: RangeEndpoint::Literal(PatternLiteral::Char(start)),
            end: RangeEndpoint::Literal(PatternLiteral::Char(end)),
            inclusive,
        };
        let analysis = analyze_pattern_match(
            &immutable(PatternType::Char),
            &[
                arm(range('\0', '\u{E000}', false)),
                arm(range('\u{E000}', '\u{10FFFF}', true)),
            ],
        )
        .unwrap();
        let PatternMatchAnalysis::Complete(complete) = analysis else {
            panic!("literal ranges are concrete");
        };
        let DecisionTree::Test { test, .. } = complete.tree() else {
            panic!("the first range must compile to a test");
        };
        assert_eq!(
            test,
            &PatternTest::CharRange {
                start: '\0',
                end: '\u{E000}',
                inclusive: false,
            }
        );
    }

    #[test]
    fn plain_binding_is_irrefutable() {
        let analysis = check_irrefutable_pattern(
            &immutable(option_bool()),
            &Pattern::Binding(PatternBinding::inferred("value")),
        )
        .unwrap();
        assert!(matches!(analysis, IrrefutablePatternAnalysis::Complete(_)));
    }

    #[test]
    fn enum_constructor_is_refutable_in_parameter_context() {
        let errors = check_irrefutable_pattern(
            &immutable(option_bool()),
            &Pattern::constructor("None", Vec::new()),
        )
        .unwrap_err();
        assert_eq!(
            errors.as_slice()[0].kind(),
            &PatternErrorKind::RefutablePattern
        );
    }

    #[test]
    fn match_ergonomics_inserts_deref_and_ref_binding() {
        let ty = PatternType::reference(ReferenceMutability::Shared, PatternType::Bool);
        let analysis = check_irrefutable_pattern(
            &immutable(ty),
            &Pattern::Binding(PatternBinding::inferred("value")),
        )
        .unwrap();
        let IrrefutablePatternAnalysis::Complete(pattern) = analysis else {
            panic!("binding is const-independent");
        };
        let TypedPatternKind::Dereference {
            inserted: true,
            pattern,
            ..
        } = pattern.kind()
        else {
            panic!("shared reference must insert a dereference");
        };
        let TypedPatternKind::Binding(binding) = pattern.kind() else {
            panic!("nested pattern must remain a binding");
        };
        assert_eq!(binding.mode(), BindingMode::Ref);
        assert_eq!(binding.matched_type(), &PatternType::Bool);
    }

    #[test]
    fn mutable_reference_ergonomics_records_ref_mut() {
        let ty = PatternType::reference(ReferenceMutability::Mutable, PatternType::Bool);
        let analysis = analyze_pattern_match(
            &immutable(ty),
            &[arm(Pattern::Binding(PatternBinding::inferred("value")))],
        )
        .unwrap();
        let PatternMatchAnalysis::Complete(complete) = analysis else {
            panic!("binding is concrete");
        };
        let DecisionTree::Leaf { bindings, .. } = complete.tree() else {
            panic!("irrefutable binding compiles directly to a leaf");
        };
        assert_eq!(bindings[0].ownership(), OwnershipFactKind::RefMut);
        assert_eq!(
            bindings[0].path(),
            &[PatternProjection::InsertedDeref(
                ReferenceMutability::Mutable
            )]
        );
    }

    #[test]
    fn explicit_ref_mut_requires_mutable_path() {
        let binding = Pattern::Binding(PatternBinding::new(
            "value",
            BindingAnnotation::RefMut,
            false,
        ));
        let errors =
            check_irrefutable_pattern(&immutable(PatternType::Bool), &binding).unwrap_err();
        assert_eq!(
            errors.as_slice()[0].kind(),
            &PatternErrorKind::MutableBorrowOfImmutablePath
        );
    }

    #[test]
    fn explicit_mutable_reference_pattern_consumes_the_reference_value() {
        let ty = PatternType::reference(ReferenceMutability::Mutable, PatternType::Bool);
        let pattern = Pattern::Reference {
            mutability: ReferenceMutability::Mutable,
            pattern: Box::new(Pattern::Binding(PatternBinding::inferred("value"))),
        };
        let analysis = check_irrefutable_pattern(&immutable(ty), &pattern).unwrap();
        let IrrefutablePatternAnalysis::Complete(pattern) = analysis else {
            panic!("explicit reference pattern is const-independent");
        };
        let TypedPatternKind::Dereference {
            inserted: false,
            pattern,
            ..
        } = pattern.kind()
        else {
            panic!("the explicit dereference must be retained");
        };
        let TypedPatternKind::Binding(binding) = pattern.kind() else {
            panic!("nested pattern must remain a binding");
        };
        assert_eq!(binding.mode(), BindingMode::Move);
    }

    #[test]
    fn duplicate_binding_through_nested_or_is_rejected() {
        let alternative = |value| Pattern::At {
            binding: PatternBinding::inferred("duplicate"),
            pattern: Box::new(bool_literal(value)),
        };
        let pattern = Pattern::tuple(vec![
            Pattern::or(vec![alternative(false), alternative(true)]),
            Pattern::Binding(PatternBinding::inferred("duplicate")),
        ]);
        let errors = check_irrefutable_pattern(
            &immutable(PatternType::tuple(vec![
                PatternType::Bool,
                PatternType::Bool,
            ])),
            &pattern,
        )
        .unwrap_err();
        assert_eq!(
            errors.as_slice()[0].kind(),
            &PatternErrorKind::DuplicateBinding
        );
    }

    #[test]
    fn at_pattern_records_whole_and_nested_binding_facts() {
        let pattern = Pattern::At {
            binding: PatternBinding::inferred("whole"),
            pattern: Box::new(Pattern::tuple(vec![Pattern::Binding(
                PatternBinding::inferred("field"),
            )])),
        };
        let analysis = analyze_pattern_match(
            &immutable(PatternType::tuple(vec![PatternType::Bool])),
            &[arm(pattern)],
        )
        .unwrap();
        let PatternMatchAnalysis::Complete(complete) = analysis else {
            panic!("at pattern is concrete");
        };
        let DecisionTree::Leaf { bindings, .. } = complete.tree() else {
            panic!("irrefutable at pattern compiles to leaf");
        };
        assert_eq!(bindings.len(), 2);
    }

    #[test]
    fn pending_const_produces_typed_needs_ctfe_tree() {
        let constant = PatternConst::new("example/pkg::LIMIT", PatternType::Bool);
        let analysis = analyze_pattern_match(
            &immutable(PatternType::Bool),
            &[arm(Pattern::Const(constant.clone())), arm(wildcard())],
        )
        .unwrap();
        let PatternMatchAnalysis::NeedsCtfe(pending) = analysis else {
            panic!("const pattern must remain pending");
        };
        assert_eq!(pending.dependencies(), &[constant]);
        assert!(matches!(pending.tree(), DecisionTree::NeedsCtfe { .. }));
    }

    #[test]
    fn pending_or_alternatives_do_not_claim_to_be_duplicates() {
        let first = PatternConst::new("example/pkg::FIRST", PatternType::Bool);
        let second = PatternConst::new("example/pkg::SECOND", PatternType::Bool);
        let analysis = analyze_pattern_match(
            &immutable(PatternType::Bool),
            &[
                arm(Pattern::or(vec![
                    Pattern::Const(first.clone()),
                    Pattern::Const(second.clone()),
                ])),
                arm(wildcard()),
            ],
        )
        .unwrap();
        let PatternMatchAnalysis::NeedsCtfe(pending) = analysis else {
            panic!("const alternatives remain pending rather than equal");
        };
        assert_eq!(pending.dependencies(), &[first, second]);
    }

    #[test]
    fn pending_range_endpoint_suppresses_reachability_claims() {
        let ty = PatternType::Integer(IntegerType::Unsigned(8));
        let constant = PatternConst::new("example/pkg::END", ty.clone());
        let pattern = Pattern::Range {
            start: RangeEndpoint::Literal(PatternLiteral::Unsigned(0)),
            end: RangeEndpoint::Const(constant.clone()),
            inclusive: true,
        };
        let analysis =
            analyze_pattern_match(&immutable(ty), &[arm(pattern), arm(wildcard())]).unwrap();
        let PatternMatchAnalysis::NeedsCtfe(pending) = analysis else {
            panic!("const range must remain pending");
        };
        assert_eq!(pending.dependencies(), &[constant]);
        assert!(matches!(pending.tree(), DecisionTree::NeedsCtfe { .. }));
    }

    #[test]
    fn pending_payload_cannot_hide_a_structural_exhaustiveness_gap() {
        let constant = PatternConst::new("example/pkg::FLAG", PatternType::Bool);
        let errors = analyze_pattern_match(
            &immutable(option_bool()),
            &[arm(Pattern::constructor(
                "Some",
                vec![Pattern::Const(constant)],
            ))],
        )
        .unwrap_err();
        assert_eq!(
            errors.as_slice()[0].kind(),
            &PatternErrorKind::NonExhaustiveMatch
        );
        assert_eq!(
            errors.as_slice()[0].message(),
            "match is structurally non-exhaustive before CTFE"
        );
    }

    #[test]
    fn nominal_record_fields_normalize_to_declaration_order() {
        let ty = PatternType::Record(RecordType::new(
            "Pair",
            vec![
                RecordField::new("left", PatternType::Bool),
                RecordField::new("right", PatternType::Bool),
            ],
        ));
        let pattern = Pattern::record(
            "Pair",
            vec![
                record_field(
                    "right",
                    Pattern::Binding(PatternBinding::inferred("right_value")),
                ),
                record_field(
                    "left",
                    Pattern::Binding(PatternBinding::inferred("left_value")),
                ),
            ],
        );
        let analysis = analyze_pattern_match(&immutable(ty), &[arm(pattern)]).unwrap();
        let PatternMatchAnalysis::Complete(complete) = analysis else {
            panic!("record binding pattern is const-independent");
        };
        let TypedPatternKind::Record {
            field_names,
            fields,
            ..
        } = complete.arms()[0].pattern().kind()
        else {
            panic!("record pattern must retain its nominal product form");
        };
        assert_eq!(
            field_names.iter().map(Box::as_ref).collect::<Vec<_>>(),
            ["left", "right"]
        );
        let TypedPatternKind::Binding(left) = fields[0].kind() else {
            panic!("left field must be first after normalization");
        };
        let TypedPatternKind::Binding(right) = fields[1].kind() else {
            panic!("right field must be second after normalization");
        };
        assert_eq!(left.name(), "left_value");
        assert_eq!(right.name(), "right_value");
    }

    #[test]
    fn nominal_record_reports_exact_missing_unknown_and_duplicate_fields() {
        let ty = PatternType::Record(RecordType::new(
            "Pair",
            vec![
                RecordField::new("left", PatternType::Bool),
                RecordField::new("right", PatternType::Bool),
            ],
        ));
        let missing = analyze_pattern_match(
            &immutable(ty.clone()),
            &[
                arm(Pattern::record(
                    "Pair",
                    vec![record_field("left", wildcard())],
                )),
                arm(wildcard()),
            ],
        )
        .unwrap_err();
        assert_eq!(
            missing.as_slice()[0].kind(),
            &PatternErrorKind::MissingField
        );
        assert_eq!(
            missing.as_slice()[0].message(),
            "record pattern for `Pair` is missing field `right`"
        );

        let unknown = analyze_pattern_match(
            &immutable(ty.clone()),
            &[arm(Pattern::record(
                "Pair",
                vec![
                    record_field("left", wildcard()),
                    record_field("other", wildcard()),
                ],
            ))],
        )
        .unwrap_err();
        assert_eq!(
            unknown.as_slice()[0].kind(),
            &PatternErrorKind::UnknownField
        );

        let duplicate = analyze_pattern_match(
            &immutable(ty),
            &[arm(Pattern::record(
                "Pair",
                vec![
                    record_field("left", wildcard()),
                    record_field("left", wildcard()),
                    record_field("right", wildcard()),
                ],
            ))],
        )
        .unwrap_err();
        assert_eq!(
            duplicate.as_slice()[0].kind(),
            &PatternErrorKind::DuplicateField
        );
    }

    #[test]
    fn record_enum_variant_participates_in_exhaustiveness() {
        let ty = PatternType::Enum(EnumType::new(
            "Choice",
            vec![
                EnumVariant::new("None", Vec::new()),
                EnumVariant::record(
                    "Pair",
                    vec![
                        RecordField::new("left", PatternType::Bool),
                        RecordField::new("right", PatternType::Bool),
                    ],
                ),
            ],
        ));
        let analysis = analyze_pattern_match(
            &immutable(ty),
            &[
                arm(Pattern::constructor("None", Vec::new())),
                arm(Pattern::record(
                    "Pair",
                    vec![
                        record_field("right", wildcard()),
                        record_field("left", wildcard()),
                    ],
                )),
            ],
        )
        .unwrap();
        assert!(matches!(analysis, PatternMatchAnalysis::Complete(_)));
    }

    #[test]
    fn string_and_str_literals_have_infinite_domain_coverage() {
        for ty in [PatternType::String, PatternType::Str] {
            let non_exhaustive =
                analyze_pattern_match(&immutable(ty.clone()), &[arm(string_literal("ready"))])
                    .unwrap_err();
            assert_eq!(
                non_exhaustive.as_slice()[0].kind(),
                &PatternErrorKind::NonExhaustiveMatch
            );

            let analysis = analyze_pattern_match(
                &immutable(ty),
                &[arm(string_literal("ready")), arm(wildcard())],
            )
            .unwrap();
            let PatternMatchAnalysis::Complete(complete) = analysis else {
                panic!("string literal and fallback are const-independent");
            };
            assert!(matches!(
                complete.tree(),
                DecisionTree::Test {
                    test: PatternTest::String(value),
                    ..
                } if value.as_ref() == "ready"
            ));
        }
    }

    #[test]
    fn dynamic_slice_length_partition_is_exhaustive() {
        let analysis = analyze_pattern_match(
            &immutable(PatternType::slice(PatternType::Bool)),
            &[
                arm(Pattern::slice(Vec::new(), false, Vec::new())),
                arm(Pattern::slice(vec![wildcard()], false, Vec::new())),
                arm(Pattern::slice(vec![wildcard()], true, vec![wildcard()])),
            ],
        )
        .unwrap();
        let PatternMatchAnalysis::Complete(complete) = analysis else {
            panic!("dynamic slice lengths are a concrete runtime domain");
        };
        assert!(matches!(
            complete.tree(),
            DecisionTree::Test {
                test: PatternTest::SliceLength(SequenceLengthConstraint::Exact(0)),
                ..
            }
        ));
    }

    #[test]
    fn dynamic_slice_partition_detects_a_missing_length() {
        let errors = analyze_pattern_match(
            &immutable(PatternType::slice(PatternType::Bool)),
            &[
                arm(Pattern::slice(Vec::new(), false, Vec::new())),
                arm(Pattern::slice(vec![wildcard()], true, vec![wildcard()])),
            ],
        )
        .unwrap_err();
        assert_eq!(
            errors.as_slice()[0].kind(),
            &PatternErrorKind::NonExhaustiveMatch
        );
    }

    #[test]
    fn dynamic_slice_partition_preserves_prefix_suffix_alias_changes() {
        let errors = analyze_pattern_match(
            &immutable(PatternType::slice(PatternType::Bool)),
            &[
                arm(Pattern::slice(Vec::new(), false, Vec::new())),
                arm(Pattern::slice(vec![bool_literal(false)], true, Vec::new())),
                arm(Pattern::slice(Vec::new(), true, vec![bool_literal(true)])),
            ],
        )
        .unwrap_err();
        assert_eq!(
            errors.as_slice()[0].kind(),
            &PatternErrorKind::NonExhaustiveMatch
        );

        let complete = analyze_pattern_match(
            &immutable(PatternType::slice(PatternType::Bool)),
            &[
                arm(Pattern::slice(Vec::new(), false, Vec::new())),
                arm(Pattern::slice(vec![bool_literal(false)], true, Vec::new())),
                arm(Pattern::slice(Vec::new(), true, vec![bool_literal(true)])),
                arm(Pattern::slice(
                    vec![bool_literal(true)],
                    true,
                    vec![bool_literal(false)],
                )),
            ],
        )
        .unwrap();
        assert!(matches!(complete, PatternMatchAnalysis::Complete(_)));
    }

    #[test]
    fn dynamic_slice_suffix_bindings_use_end_relative_projection() {
        let pattern = Pattern::slice(
            vec![wildcard()],
            true,
            vec![Pattern::Binding(PatternBinding::inferred("last"))],
        );
        let analysis = analyze_pattern_match(
            &immutable(PatternType::slice(PatternType::Bool)),
            &[arm(pattern), arm(wildcard())],
        )
        .unwrap();
        let PatternMatchAnalysis::Complete(complete) = analysis else {
            panic!("dynamic slice pattern is const-independent");
        };
        let DecisionTree::Test { on_match, .. } = complete.tree() else {
            panic!("nonempty slice arm must test its minimum length");
        };
        let DecisionTree::Leaf { bindings, .. } = on_match.as_ref() else {
            panic!("wildcard elements require no additional tests");
        };
        assert_eq!(
            bindings[0].path(),
            &[PatternProjection::SliceElementFromEnd(0)]
        );
    }

    #[test]
    fn symbolic_array_length_remains_a_checked_pending_test() {
        let length = PatternConst::new(
            "example/pkg::N",
            PatternType::Integer(IntegerType::Unsigned(64)),
        );
        let ty = PatternType::symbolic_array(PatternType::Bool, length.clone());
        let analysis = analyze_pattern_match(
            &immutable(ty),
            &[
                arm(Pattern::slice(vec![bool_literal(true)], true, Vec::new())),
                arm(wildcard()),
            ],
        )
        .unwrap();
        let PatternMatchAnalysis::NeedsCtfe(pending) = analysis else {
            panic!("symbolic array length must not become a host usize");
        };
        assert_eq!(pending.dependencies(), std::slice::from_ref(&length));
        assert!(matches!(
            pending.tree(),
            DecisionTree::NeedsCtfe {
                test: PendingPatternTest::ArrayLength {
                    length: retained,
                    constraint: SequenceLengthConstraint::AtLeast(1),
                },
                ..
            } if retained == &length
        ));
    }

    #[test]
    fn vacuous_rest_pattern_on_symbolic_array_is_complete_without_ctfe() {
        let length = PatternConst::new(
            "example/pkg::N",
            PatternType::Integer(IntegerType::Unsigned(64)),
        );
        let analysis = analyze_pattern_match(
            &immutable(PatternType::symbolic_array(PatternType::Bool, length)),
            &[arm(Pattern::slice(Vec::new(), true, Vec::new()))],
        )
        .unwrap();
        let PatternMatchAnalysis::Complete(complete) = analysis else {
            panic!("`[..]` imposes no symbolic length predicate");
        };
        assert!(matches!(complete.tree(), DecisionTree::Leaf { .. }));
    }

    #[test]
    fn unsupported_type_fails_closed() {
        let errors = analyze_pattern_match(
            &immutable(PatternType::Unsupported("Map<K,V>".into())),
            &[arm(wildcard())],
        )
        .unwrap_err();
        assert_eq!(
            errors.as_slice()[0].kind(),
            &PatternErrorKind::UnsupportedType
        );
        assert_eq!(
            errors.as_slice()[0].code(),
            PatternDiagnosticCode::Pattern001
        );
    }

    #[test]
    fn float_scrutinees_bind_and_require_a_wildcard() {
        let analysis = analyze_pattern_match(
            &immutable(PatternType::Float(FloatType::F32)),
            &[arm(wildcard())],
        )
        .unwrap();
        let PatternMatchAnalysis::Complete(complete) = analysis else {
            panic!("a wildcard over a float domain needs no CTFE");
        };
        assert!(matches!(complete.tree(), DecisionTree::Leaf { .. }));

        let errors =
            analyze_pattern_match(&immutable(PatternType::Float(FloatType::F64)), &[]).unwrap_err();
        assert_eq!(
            errors.as_slice()[0].code(),
            PatternDiagnosticCode::Pattern002
        );
    }
}
