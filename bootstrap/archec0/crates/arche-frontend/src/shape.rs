//! Symbolic, pre-stable-identity shapes for M27-C1.
//!
//! Nominal leaves retain canonical declaration paths (type-tree tag 29), not
//! provisional `DefinitionId` values. These trees are sufficient for C1
//! goldens and for later C2-C5 checking, but they are not typed-MIR evidence and
//! cannot construct a stable identity by themselves.

use std::collections::BTreeSet;
use std::fmt;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum TargetRoot {
    Library,
    Binary(String),
    Environment(String),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DeclarationKind {
    World,
    Component,
    Resource,
    Tag,
    System,
    Schedule,
    Function,
    Generator,
    Struct,
    Enum,
    Trait,
    Impl,
    TypeAlias,
    Const,
    Static,
    Query,
}

impl DeclarationKind {
    pub const fn tag(self) -> u8 {
        match self {
            Self::World => 1,
            Self::Component => 2,
            Self::Resource => 3,
            Self::Tag => 4,
            Self::System => 5,
            Self::Schedule => 6,
            Self::Function => 7,
            Self::Generator => 8,
            Self::Struct => 9,
            Self::Enum => 10,
            Self::Trait => 11,
            Self::Impl => 12,
            Self::TypeAlias => 13,
            Self::Const => 14,
            Self::Static => 15,
            Self::Query => 16,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SemanticDeclarationPath {
    pub registry_origin: String,
    pub package_name: String,
    pub target: TargetRoot,
    pub modules: Vec<String>,
    pub kind: DeclarationKind,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum IntegerType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Isize,
    Usize,
}

impl IntegerType {
    pub const fn tag(self) -> u8 {
        match self {
            Self::I8 => 1,
            Self::I16 => 2,
            Self::I32 => 3,
            Self::I64 => 4,
            Self::U8 => 5,
            Self::U16 => 6,
            Self::U32 => 7,
            Self::U64 => 8,
            Self::Isize => 9,
            Self::Usize => 10,
        }
    }

    pub const fn byte_width(self) -> usize {
        match self {
            Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 => 4,
            Self::I64 | Self::U64 | Self::Isize | Self::Usize => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Mutability {
    Shared,
    Mutable,
}

impl Mutability {
    const fn tag(self) -> u8 {
        match self {
            Self::Shared => 1,
            Self::Mutable => 2,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicLifetime {
    Static,
    Bound { depth: u64, index: u64 },
    ErasedLocal,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GenericParameterKind {
    Type,
    Lifetime,
    IntegerConst(IntegerType),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GenericParameterShape {
    pub index: u64,
    pub name: String,
    pub kind: GenericParameterKind,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicPredicate {
    Trait {
        trait_path: SemanticDeclarationPath,
        self_type: SymbolicType,
        arguments: Vec<GenericArgumentShape>,
    },
    LifetimeOutlives {
        longer: SymbolicLifetime,
        shorter: SymbolicLifetime,
    },
    TypeOutlives {
        ty: SymbolicType,
        lifetime: SymbolicLifetime,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GenericArgumentShape {
    Type(SymbolicType),
    Lifetime(SymbolicLifetime),
    IntegerConst(SymbolicConstExpression),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicConstNode {
    IntegerLiteral(Vec<u8>),
    Bound { depth: u64, index: u64 },
    ConstDefinitionPath(SemanticDeclarationPath),
    WrappingNeg(Box<SymbolicConstExpression>),
    BitNot(Box<SymbolicConstExpression>),
    WrappingMul(Box<SymbolicConstExpression>, Box<SymbolicConstExpression>),
    IntegerDivide(Box<SymbolicConstExpression>, Box<SymbolicConstExpression>),
    IntegerRemainder(Box<SymbolicConstExpression>, Box<SymbolicConstExpression>),
    WrappingAdd(Box<SymbolicConstExpression>, Box<SymbolicConstExpression>),
    WrappingSub(Box<SymbolicConstExpression>, Box<SymbolicConstExpression>),
    MaskedShiftLeft(Box<SymbolicConstExpression>, Box<SymbolicConstExpression>),
    MaskedShiftRight(Box<SymbolicConstExpression>, Box<SymbolicConstExpression>),
    BitAnd(Box<SymbolicConstExpression>, Box<SymbolicConstExpression>),
    BitXor(Box<SymbolicConstExpression>, Box<SymbolicConstExpression>),
    BitOr(Box<SymbolicConstExpression>, Box<SymbolicConstExpression>),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicConstExpression {
    pub integer_type: IntegerType,
    pub node: SymbolicConstNode,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CaptureMode {
    Shared,
    Mutable,
    Move,
}

impl CaptureMode {
    const fn tag(self) -> u8 {
        match self {
            Self::Shared => 1,
            Self::Mutable => 2,
            Self::Move => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CallTrait {
    Fn,
    FnMut,
    FnOnce,
}

impl CallTrait {
    const fn tag(self) -> u8 {
        match self {
            Self::Fn => 1,
            Self::FnMut => 2,
            Self::FnOnce => 3,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicCapture {
    pub ordinal: u64,
    pub mode: CaptureMode,
    pub ty: SymbolicType,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GeneratorTarget {
    Named {
        declaration: SemanticDeclarationPath,
        arguments: Vec<GenericArgumentShape>,
        hidden_lifetime_binders: Vec<u64>,
    },
    Anonymous {
        owner: SemanticDeclarationPath,
        expression_ordinal: u64,
        arguments: Vec<GenericArgumentShape>,
    },
}

/// A nested semantic-type effect set with an explicit C1/C4 readiness boundary.
///
/// C1 uses [`Self::pending_c4`] for nonempty source lists so their order and
/// duplicates remain inspectable. C4 replaces that wrapper with
/// [`Self::resolved`]; only then may the canonical type encoder sort the
/// members and reject duplicates. Empty source sets are already canonical.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicTypeEffectSet {
    members: Vec<SymbolicType>,
    readiness: SymbolicShapeReadiness,
}

impl SymbolicTypeEffectSet {
    pub fn pending_c4(members: Vec<SymbolicType>) -> Self {
        if members.is_empty() {
            return Self::default();
        }
        Self {
            members,
            readiness: SymbolicShapeReadiness::PendingC4,
        }
    }

    pub fn resolved(members: Vec<SymbolicType>) -> Self {
        let readiness = symbolic_type_list_readiness(&members);
        Self { members, readiness }
    }

    pub fn members(&self) -> &[SymbolicType] {
        &self.members
    }

    pub const fn readiness(&self) -> SymbolicShapeReadiness {
        self.readiness
    }

    pub fn into_members(self) -> Vec<SymbolicType> {
        self.members
    }
}

impl Default for SymbolicTypeEffectSet {
    fn default() -> Self {
        Self {
            members: Vec::new(),
            readiness: SymbolicShapeReadiness::ConstIndependent,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Isize,
    Usize,
    F32,
    F64,
    Bool,
    Char,
    Entity,
    Unit,
    Never,
    Str,
    Slice(Box<SymbolicType>),
    Array {
        element: Box<SymbolicType>,
        length: SymbolicConstExpression,
    },
    Tuple(Vec<SymbolicType>),
    Reference {
        mutability: Mutability,
        lifetime: SymbolicLifetime,
        pointee: Box<SymbolicType>,
    },
    RawPointer {
        mutability: Mutability,
        pointee: Box<SymbolicType>,
    },
    /// Identity-only nominal-path form (tag 29). C1 never fabricates tag 24.
    NominalPath {
        declaration: SemanticDeclarationPath,
        arguments: Vec<GenericArgumentShape>,
    },
    FunctionPointer {
        unsafe_: bool,
        parameters: Vec<SymbolicType>,
        result: Box<SymbolicType>,
        requires: SymbolicTypeEffectSet,
        throws: SymbolicTypeEffectSet,
    },
    BoundType {
        depth: u64,
        index: u64,
    },
    Closure {
        owner: Box<SemanticDeclarationPath>,
        expression_ordinal: u64,
        captures: Vec<SymbolicCapture>,
        parameters: Vec<SymbolicType>,
        result: Box<SymbolicType>,
        requires: SymbolicTypeEffectSet,
        throws: SymbolicTypeEffectSet,
        arguments: Vec<GenericArgumentShape>,
    },
    Generator {
        target: Box<GeneratorTarget>,
        captures: Vec<SymbolicCapture>,
        parameters: Vec<SymbolicType>,
        factory_unsafe: bool,
        resume: Box<SymbolicType>,
        yields: Box<SymbolicType>,
        result: Box<SymbolicType>,
        requires: SymbolicTypeEffectSet,
        throws: SymbolicTypeEffectSet,
    },
    JoinHandle {
        result: Box<SymbolicType>,
        throws: SymbolicTypeEffectSet,
    },
    GeneratorFactory {
        target: Box<GeneratorTarget>,
        captures: Vec<SymbolicCapture>,
        call_trait: CallTrait,
        parameters: Vec<SymbolicType>,
        factory_unsafe: bool,
        produced_generator: Box<SymbolicType>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EffectKind {
    Requires,
    Throws,
}

impl EffectKind {
    const fn tag(self) -> u8 {
        match self {
            Self::Requires => 1,
            Self::Throws => 2,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicEffectAtom {
    pub kind: EffectKind,
    pub ty: SymbolicType,
}

/// Earliest milestone that can finish a C1 symbolic declaration-shape leaf.
///
/// `NeedsCtfe` is already a canonical pre-result tree: C4 may seal its bytes,
/// but C5 must replace const-definition-path nodes with their contextual bits
/// before a final `DefinitionId` can be minted. Pending C2/C4 leaves have no
/// canonical pre-result byte representation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicShapeReadiness {
    ConstIndependent,
    NeedsCtfe,
    PendingC2,
    PendingC4,
}

impl SymbolicShapeReadiness {
    pub const fn final_identity_ready(self) -> bool {
        matches!(self, Self::ConstIndependent)
    }

    pub const fn pre_result_ready(self) -> bool {
        matches!(self, Self::ConstIndependent | Self::NeedsCtfe)
    }

    const fn combine(self, other: Self) -> Self {
        if self as u8 >= other as u8 {
            self
        } else {
            other
        }
    }

    const fn tag(self) -> u8 {
        match self {
            Self::ConstIndependent => 1,
            Self::NeedsCtfe => 2,
            Self::PendingC2 => 3,
            Self::PendingC4 => 4,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicPendingShape {
    pub readiness: SymbolicShapeReadiness,
    pub source_span: SymbolicSourceSpan,
    pub kind: PendingShapeKind,
    /// Diagnostic/debug spelling only. Canonical pre-result encoders never
    /// consume this field.
    pub debug_spelling: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PendingShapeKind {
    PathUse,
    ContextualSelf,
    GenericFormation,
    EffectMember,
    Predicate,
}

impl PendingShapeKind {
    const fn tag(self) -> u8 {
        match self {
            Self::PathUse => 1,
            Self::ContextualSelf => 2,
            Self::GenericFormation => 3,
            Self::EffectMember => 4,
            Self::Predicate => 5,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicTypeShapeSkeleton {
    Resolved {
        value: SymbolicType,
        readiness: SymbolicShapeReadiness,
    },
    Pending(SymbolicPendingShape),
}

/// One member of either the `requires` or `throws` set. The containing set
/// supplies the effect kind, so the resolved payload is exactly its type tree.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicEffectShapeSkeleton {
    Resolved {
        value: SymbolicType,
        readiness: SymbolicShapeReadiness,
    },
    Pending(SymbolicPendingShape),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicPredicateShapeSkeleton {
    Resolved {
        value: Box<SymbolicPredicate>,
        readiness: SymbolicShapeReadiness,
    },
    Pending(SymbolicPendingShape),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicRecordForm {
    Unit,
    Tuple,
    Record,
}

impl SymbolicRecordForm {
    const fn tag(self) -> u8 {
        match self {
            Self::Unit => 1,
            Self::Tuple => 2,
            Self::Record => 3,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicFieldShapeSkeleton {
    /// Present only for record-form fields. Tuple fields carry no source name.
    pub name: Option<String>,
    pub ty: SymbolicTypeShapeSkeleton,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicRecordShapeSkeleton {
    pub form: SymbolicRecordForm,
    pub fields: Vec<SymbolicFieldShapeSkeleton>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicVariantShapeSkeleton {
    pub name: String,
    pub form: SymbolicRecordForm,
    pub fields: Vec<SymbolicFieldShapeSkeleton>,
}

/// Exact callable-parameter mode tags. An ordinary value parameter and a
/// receiver passed by value are intentionally distinct identity inputs.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicCallableParameterMode {
    Value,
    ReceiverValue,
    ReceiverShared,
    ReceiverMutable,
}

impl SymbolicCallableParameterMode {
    const fn tag(self) -> u8 {
        match self {
            Self::Value => 1,
            Self::ReceiverValue => 2,
            Self::ReceiverShared => 3,
            Self::ReceiverMutable => 4,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicCallableParameterSkeleton {
    pub mode: SymbolicCallableParameterMode,
    pub ty: SymbolicTypeShapeSkeleton,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicCallableKind {
    Function,
    Generator,
}

/// C1 retains these vectors as source lists, including order and duplicates.
/// C4 replaces their `PendingC4` leaves before treating either vector as a
/// canonical mathematical set.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicEffectSetsSkeleton {
    pub requires: Vec<SymbolicEffectShapeSkeleton>,
    pub throws: Vec<SymbolicEffectShapeSkeleton>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicCallableShapeSkeleton {
    pub kind: SymbolicCallableKind,
    pub parameters: Vec<SymbolicCallableParameterSkeleton>,
    pub result: SymbolicTypeShapeSkeleton,
    pub unsafe_: bool,
    pub resume: Option<SymbolicTypeShapeSkeleton>,
    pub yields: Option<SymbolicTypeShapeSkeleton>,
    pub effects: SymbolicEffectSetsSkeleton,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicMethodShapeSkeleton {
    pub name: String,
    /// Reused verbatim by the child method definition and its parent
    /// trait/implementation entry.
    pub shape: Box<SymbolicDeclarationShapeSkeleton>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicQueryTermKind {
    Read,
    Write,
    Exclude,
}

impl SymbolicQueryTermKind {
    const fn tag(self) -> u8 {
        match self {
            Self::Read => 1,
            Self::Write => 2,
            Self::Exclude => 3,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicQueryTermShapeSkeleton {
    pub kind: SymbolicQueryTermKind,
    pub ty: SymbolicTypeShapeSkeleton,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicSystemAccessShapeSkeleton {
    CapabilityShared(SymbolicTypeShapeSkeleton),
    CapabilityMutable(SymbolicTypeShapeSkeleton),
    ResourceRead(SymbolicTypeShapeSkeleton),
    ResourceWrite(SymbolicTypeShapeSkeleton),
    Query(Vec<SymbolicQueryTermShapeSkeleton>),
    Commands,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicCapabilityAccessMode {
    Shared,
    Mutable,
}

impl SymbolicCapabilityAccessMode {
    const fn tag(self) -> u8 {
        match self {
            Self::Shared => 1,
            Self::Mutable => 2,
        }
    }
}

/// Host-path-free retained source coordinate used only by the C1 debug
/// skeleton. Stable declaration preimages never consume this provenance.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicSourceSpan {
    pub file: u64,
    pub start_byte: u64,
    pub end_byte: u64,
    pub start_line: u64,
    pub start_column: u64,
    pub end_line: u64,
    pub end_column: u64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicImpliedCapabilityRequirementSkeleton {
    pub parameter_ordinal: u64,
    pub parameter_span: SymbolicSourceSpan,
    pub access: SymbolicCapabilityAccessMode,
    pub referent: SymbolicTypeShapeSkeleton,
    /// Always PendingC4 in C1. C4 validates the sealed capability, unions it
    /// into `effects.requires`, and removes this provenance row before any
    /// declaration preimage can be encoded.
    pub readiness: SymbolicShapeReadiness,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicDeclarationPayloadSkeleton {
    World,
    Record(SymbolicRecordShapeSkeleton),
    Enum(Vec<SymbolicVariantShapeSkeleton>),
    Tag,
    Callable(Box<SymbolicCallableShapeSkeleton>),
    System {
        accesses: Vec<SymbolicSystemAccessShapeSkeleton>,
        implied_requires: Vec<SymbolicImpliedCapabilityRequirementSkeleton>,
        result: SymbolicTypeShapeSkeleton,
        effects: SymbolicEffectSetsSkeleton,
    },
    Trait {
        methods: Vec<SymbolicMethodShapeSkeleton>,
    },
    Impl {
        trait_ref: Option<SymbolicTypeShapeSkeleton>,
        target: SymbolicTypeShapeSkeleton,
        is_default: bool,
        methods: Vec<SymbolicMethodShapeSkeleton>,
    },
    Alias {
        target: SymbolicTypeShapeSkeleton,
    },
    Const {
        ty: SymbolicTypeShapeSkeleton,
    },
    Static {
        mutable: bool,
        ty: SymbolicTypeShapeSkeleton,
    },
    Query {
        terms: Vec<SymbolicQueryTermShapeSkeleton>,
    },
    Schedule {
        effects: SymbolicEffectSetsSkeleton,
        readiness: SymbolicShapeReadiness,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SymbolicDeclarationShapeSkeleton {
    /// Alpha-normalized source generic kinds followed by compiler-synthesized
    /// hidden lifetime binder kinds.
    pub generic_parameters: Vec<GenericParameterKind>,
    pub predicates: Vec<SymbolicPredicateShapeSkeleton>,
    pub payload: SymbolicDeclarationPayloadSkeleton,
}

impl Default for SymbolicDeclarationShapeSkeleton {
    fn default() -> Self {
        Self {
            generic_parameters: Vec::new(),
            predicates: Vec::new(),
            payload: SymbolicDeclarationPayloadSkeleton::Schedule {
                effects: SymbolicEffectSetsSkeleton::default(),
                readiness: SymbolicShapeReadiness::PendingC4,
            },
        }
    }
}

#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolicDefinitionOwnerSkeleton {
    #[default]
    TopLevel,
    Trait {
        path: SemanticDeclarationPath,
        shape: Box<SymbolicDeclarationShapeSkeleton>,
    },
    InherentImpl {
        target: SymbolicTypeShapeSkeleton,
        generic_parameters: Vec<GenericParameterKind>,
        predicates: Vec<SymbolicPredicateShapeSkeleton>,
    },
    TraitImpl {
        trait_ref: SymbolicTypeShapeSkeleton,
        target: SymbolicTypeShapeSkeleton,
        generic_parameters: Vec<GenericParameterKind>,
        predicates: Vec<SymbolicPredicateShapeSkeleton>,
        is_default: bool,
    },
    SystemQuery {
        path: SemanticDeclarationPath,
        shape: Box<SymbolicDeclarationShapeSkeleton>,
    },
}

/// Ready-only wrapper. Construction validates every leaf and canonical set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalDeclarationShape {
    skeleton: SymbolicDeclarationShapeSkeleton,
    readiness: SymbolicShapeReadiness,
}

impl CanonicalDeclarationShape {
    pub const fn readiness(&self) -> SymbolicShapeReadiness {
        self.readiness
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalDefinitionOwner {
    skeleton: SymbolicDefinitionOwnerSkeleton,
    readiness: SymbolicShapeReadiness,
}

impl CanonicalDefinitionOwner {
    pub const fn readiness(&self) -> SymbolicShapeReadiness {
        self.readiness
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShapeEncodingError {
    LengthOverflow(&'static str),
    InvalidFixedWidthBits {
        integer_type: IntegerType,
        actual: usize,
    },
    DuplicateEffect,
    DuplicatePredicate,
    ZeroExpressionOrdinal,
    NonCanonicalGenericParameterIndex {
        expected: u64,
        actual: u64,
    },
    NonCanonicalCaptureOrdinal {
        expected: u64,
        actual: u64,
    },
    NamedGeneratorCaptures,
    AnonymousGeneratorUnsafe,
    InconsistentShapeReadiness,
    InvalidDeclarationShape(&'static str),
    FinalIdentityNeedsCtfe,
}

impl fmt::Display for ShapeEncodingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthOverflow(label) => write!(formatter, "{label} length exceeds u64"),
            Self::InvalidFixedWidthBits {
                integer_type,
                actual,
            } => write!(
                formatter,
                "{integer_type:?} literal requires {} bytes, found {actual}",
                integer_type.byte_width()
            ),
            Self::DuplicateEffect => formatter.write_str("duplicate symbolic effect atom"),
            Self::DuplicatePredicate => formatter.write_str("duplicate symbolic predicate"),
            Self::ZeroExpressionOrdinal => {
                formatter.write_str("anonymous expression ordinal must be nonzero")
            }
            Self::NonCanonicalGenericParameterIndex { expected, actual } => write!(
                formatter,
                "generic parameter index must be dense source order {expected}, found {actual}"
            ),
            Self::NonCanonicalCaptureOrdinal { expected, actual } => write!(
                formatter,
                "capture ordinal must be dense one-based preorder {expected}, found {actual}"
            ),
            Self::NamedGeneratorCaptures => {
                formatter.write_str("named generator shapes cannot carry transferred captures")
            }
            Self::AnonymousGeneratorUnsafe => {
                formatter.write_str("anonymous generator factory_unsafe must be false")
            }
            Self::InconsistentShapeReadiness => {
                formatter.write_str("symbolic shape leaf readiness does not match its payload")
            }
            Self::InvalidDeclarationShape(message) => formatter.write_str(message),
            Self::FinalIdentityNeedsCtfe => formatter.write_str(
                "final identity shape still contains const-definition-path dependencies",
            ),
        }
    }
}

impl std::error::Error for ShapeEncodingError {}

impl SymbolicTypeShapeSkeleton {
    pub fn resolved(value: SymbolicType) -> Self {
        let readiness = symbolic_type_readiness(&value);
        Self::Resolved { value, readiness }
    }

    pub fn pending(
        readiness: SymbolicShapeReadiness,
        source_span: SymbolicSourceSpan,
        kind: PendingShapeKind,
        debug_spelling: impl Into<String>,
    ) -> Self {
        Self::Pending(SymbolicPendingShape {
            readiness,
            source_span,
            kind,
            debug_spelling: debug_spelling.into(),
        })
    }

    pub const fn readiness(&self) -> SymbolicShapeReadiness {
        match self {
            Self::Resolved { readiness, .. } => *readiness,
            Self::Pending(pending) => pending.readiness,
        }
    }
}

impl SymbolicEffectShapeSkeleton {
    pub fn resolved(value: SymbolicType) -> Self {
        let readiness = symbolic_type_readiness(&value);
        Self::Resolved { value, readiness }
    }

    /// Retains a C1-resolved symbolic atom while leaving effect-set
    /// canonicalization, union, and duplicate rejection to C4.
    pub fn resolved_pending_c4(value: SymbolicType) -> Self {
        Self::Resolved {
            value,
            readiness: SymbolicShapeReadiness::PendingC4,
        }
    }

    pub fn pending(
        readiness: SymbolicShapeReadiness,
        source_span: SymbolicSourceSpan,
        kind: PendingShapeKind,
        debug_spelling: impl Into<String>,
    ) -> Self {
        Self::Pending(SymbolicPendingShape {
            readiness,
            source_span,
            kind,
            debug_spelling: debug_spelling.into(),
        })
    }

    pub const fn readiness(&self) -> SymbolicShapeReadiness {
        match self {
            Self::Resolved { readiness, .. } => *readiness,
            Self::Pending(pending) => pending.readiness,
        }
    }
}

impl SymbolicPredicateShapeSkeleton {
    pub fn resolved(value: SymbolicPredicate) -> Self {
        let readiness = symbolic_predicate_readiness(&value);
        Self::Resolved {
            value: Box::new(value),
            readiness,
        }
    }

    pub fn pending(
        readiness: SymbolicShapeReadiness,
        source_span: SymbolicSourceSpan,
        kind: PendingShapeKind,
        debug_spelling: impl Into<String>,
    ) -> Self {
        Self::Pending(SymbolicPendingShape {
            readiness,
            source_span,
            kind,
            debug_spelling: debug_spelling.into(),
        })
    }

    pub const fn readiness(&self) -> SymbolicShapeReadiness {
        match self {
            Self::Resolved { readiness, .. } => *readiness,
            Self::Pending(pending) => pending.readiness,
        }
    }
}

pub fn symbolic_const_readiness(expression: &SymbolicConstExpression) -> SymbolicShapeReadiness {
    match &expression.node {
        SymbolicConstNode::ConstDefinitionPath(_) => SymbolicShapeReadiness::NeedsCtfe,
        SymbolicConstNode::WrappingNeg(child) | SymbolicConstNode::BitNot(child) => {
            symbolic_const_readiness(child)
        }
        SymbolicConstNode::WrappingMul(left, right)
        | SymbolicConstNode::IntegerDivide(left, right)
        | SymbolicConstNode::IntegerRemainder(left, right)
        | SymbolicConstNode::WrappingAdd(left, right)
        | SymbolicConstNode::WrappingSub(left, right)
        | SymbolicConstNode::MaskedShiftLeft(left, right)
        | SymbolicConstNode::MaskedShiftRight(left, right)
        | SymbolicConstNode::BitAnd(left, right)
        | SymbolicConstNode::BitXor(left, right)
        | SymbolicConstNode::BitOr(left, right) => {
            symbolic_const_readiness(left).combine(symbolic_const_readiness(right))
        }
        SymbolicConstNode::IntegerLiteral(_) | SymbolicConstNode::Bound { .. } => {
            SymbolicShapeReadiness::ConstIndependent
        }
    }
}

fn symbolic_type_list_readiness(types: &[SymbolicType]) -> SymbolicShapeReadiness {
    types
        .iter()
        .fold(SymbolicShapeReadiness::ConstIndependent, |readiness, ty| {
            readiness.combine(symbolic_type_readiness(ty))
        })
}

pub fn symbolic_type_readiness(ty: &SymbolicType) -> SymbolicShapeReadiness {
    let generic_list = |arguments: &[GenericArgumentShape]| {
        arguments.iter().fold(
            SymbolicShapeReadiness::ConstIndependent,
            |readiness, argument| {
                let argument = match argument {
                    GenericArgumentShape::Type(ty) => symbolic_type_readiness(ty),
                    GenericArgumentShape::IntegerConst(value) => symbolic_const_readiness(value),
                    GenericArgumentShape::Lifetime(_) => SymbolicShapeReadiness::ConstIndependent,
                };
                readiness.combine(argument)
            },
        )
    };
    match ty {
        SymbolicType::Slice(element)
        | SymbolicType::RawPointer {
            pointee: element, ..
        } => symbolic_type_readiness(element),
        SymbolicType::Array { element, length } => {
            symbolic_type_readiness(element).combine(symbolic_const_readiness(length))
        }
        SymbolicType::Tuple(types) => symbolic_type_list_readiness(types),
        SymbolicType::Reference { pointee, .. } => symbolic_type_readiness(pointee),
        SymbolicType::NominalPath { arguments, .. } => generic_list(arguments),
        SymbolicType::FunctionPointer {
            parameters,
            result,
            requires,
            throws,
            ..
        } => symbolic_type_list_readiness(parameters)
            .combine(symbolic_type_readiness(result))
            .combine(requires.readiness())
            .combine(throws.readiness()),
        SymbolicType::Closure {
            captures,
            parameters,
            result,
            requires,
            throws,
            arguments,
            ..
        } => captures
            .iter()
            .fold(
                SymbolicShapeReadiness::ConstIndependent,
                |state, capture| state.combine(symbolic_type_readiness(&capture.ty)),
            )
            .combine(symbolic_type_list_readiness(parameters))
            .combine(symbolic_type_readiness(result))
            .combine(requires.readiness())
            .combine(throws.readiness())
            .combine(generic_list(arguments)),
        SymbolicType::Generator {
            target,
            captures,
            parameters,
            resume,
            yields,
            result,
            requires,
            throws,
            ..
        } => generator_target_readiness(target)
            .combine(captures.iter().fold(
                SymbolicShapeReadiness::ConstIndependent,
                |state, capture| state.combine(symbolic_type_readiness(&capture.ty)),
            ))
            .combine(symbolic_type_list_readiness(parameters))
            .combine(symbolic_type_readiness(resume))
            .combine(symbolic_type_readiness(yields))
            .combine(symbolic_type_readiness(result))
            .combine(requires.readiness())
            .combine(throws.readiness()),
        SymbolicType::JoinHandle { result, throws } => {
            symbolic_type_readiness(result).combine(throws.readiness())
        }
        SymbolicType::GeneratorFactory {
            target,
            captures,
            parameters,
            produced_generator,
            ..
        } => generator_target_readiness(target)
            .combine(captures.iter().fold(
                SymbolicShapeReadiness::ConstIndependent,
                |state, capture| state.combine(symbolic_type_readiness(&capture.ty)),
            ))
            .combine(symbolic_type_list_readiness(parameters))
            .combine(symbolic_type_readiness(produced_generator)),
        SymbolicType::I8
        | SymbolicType::I16
        | SymbolicType::I32
        | SymbolicType::I64
        | SymbolicType::U8
        | SymbolicType::U16
        | SymbolicType::U32
        | SymbolicType::U64
        | SymbolicType::Isize
        | SymbolicType::Usize
        | SymbolicType::F32
        | SymbolicType::F64
        | SymbolicType::Bool
        | SymbolicType::Char
        | SymbolicType::Entity
        | SymbolicType::Unit
        | SymbolicType::Never
        | SymbolicType::Str
        | SymbolicType::BoundType { .. } => SymbolicShapeReadiness::ConstIndependent,
    }
}

fn generator_target_readiness(target: &GeneratorTarget) -> SymbolicShapeReadiness {
    let arguments = match target {
        GeneratorTarget::Named { arguments, .. } | GeneratorTarget::Anonymous { arguments, .. } => {
            arguments
        }
    };
    arguments.iter().fold(
        SymbolicShapeReadiness::ConstIndependent,
        |state, argument| {
            state.combine(match argument {
                GenericArgumentShape::Type(ty) => symbolic_type_readiness(ty),
                GenericArgumentShape::IntegerConst(value) => symbolic_const_readiness(value),
                GenericArgumentShape::Lifetime(_) => SymbolicShapeReadiness::ConstIndependent,
            })
        },
    )
}

pub fn symbolic_predicate_readiness(predicate: &SymbolicPredicate) -> SymbolicShapeReadiness {
    match predicate {
        SymbolicPredicate::Trait {
            self_type,
            arguments,
            ..
        } => arguments
            .iter()
            .fold(symbolic_type_readiness(self_type), |state, argument| {
                state.combine(match argument {
                    GenericArgumentShape::Type(ty) => symbolic_type_readiness(ty),
                    GenericArgumentShape::IntegerConst(value) => symbolic_const_readiness(value),
                    GenericArgumentShape::Lifetime(_) => SymbolicShapeReadiness::ConstIndependent,
                })
            }),
        SymbolicPredicate::TypeOutlives { ty, .. } => symbolic_type_readiness(ty),
        SymbolicPredicate::LifetimeOutlives { .. } => SymbolicShapeReadiness::ConstIndependent,
    }
}

pub fn declaration_shape_readiness(
    shape: &SymbolicDeclarationShapeSkeleton,
) -> Result<SymbolicShapeReadiness, ShapeEncodingError> {
    let mut readiness = SymbolicShapeReadiness::ConstIndependent;
    validate_predicate_set(&shape.predicates, &mut readiness)?;
    validate_payload(&shape.payload, &mut readiness)?;
    Ok(readiness)
}

pub fn try_canonicalize_declaration_shape(
    shape: &SymbolicDeclarationShapeSkeleton,
) -> Result<Option<CanonicalDeclarationShape>, ShapeEncodingError> {
    let readiness = declaration_shape_readiness(shape)?;
    if !readiness.pre_result_ready() {
        return Ok(None);
    }
    Ok(Some(CanonicalDeclarationShape {
        skeleton: shape.clone(),
        readiness,
    }))
}

pub fn owner_shape_readiness(
    owner: &SymbolicDefinitionOwnerSkeleton,
) -> Result<SymbolicShapeReadiness, ShapeEncodingError> {
    let mut readiness = SymbolicShapeReadiness::ConstIndependent;
    match owner {
        SymbolicDefinitionOwnerSkeleton::TopLevel => {}
        SymbolicDefinitionOwnerSkeleton::Trait { shape, .. }
        | SymbolicDefinitionOwnerSkeleton::SystemQuery { shape, .. } => {
            readiness = readiness.combine(declaration_shape_readiness(shape)?);
        }
        SymbolicDefinitionOwnerSkeleton::InherentImpl {
            target, predicates, ..
        } => {
            readiness = readiness.combine(validate_type_leaf(target)?);
            validate_predicate_set(predicates, &mut readiness)?;
        }
        SymbolicDefinitionOwnerSkeleton::TraitImpl {
            trait_ref,
            target,
            predicates,
            ..
        } => {
            readiness = readiness.combine(validate_type_leaf(trait_ref)?);
            readiness = readiness.combine(validate_type_leaf(target)?);
            validate_predicate_set(predicates, &mut readiness)?;
        }
    }
    Ok(readiness)
}

pub fn try_canonicalize_definition_owner(
    owner: &SymbolicDefinitionOwnerSkeleton,
) -> Result<Option<CanonicalDefinitionOwner>, ShapeEncodingError> {
    let readiness = owner_shape_readiness(owner)?;
    if !readiness.pre_result_ready() {
        return Ok(None);
    }
    Ok(Some(CanonicalDefinitionOwner {
        skeleton: owner.clone(),
        readiness,
    }))
}

fn validate_type_leaf(
    value: &SymbolicTypeShapeSkeleton,
) -> Result<SymbolicShapeReadiness, ShapeEncodingError> {
    match value {
        SymbolicTypeShapeSkeleton::Resolved { value, readiness } => {
            let expected = symbolic_type_readiness(value);
            if *readiness != expected {
                return Err(ShapeEncodingError::InconsistentShapeReadiness);
            }
            if expected.pre_result_ready() {
                encode_symbolic_type(value)?;
            }
            Ok(expected)
        }
        SymbolicTypeShapeSkeleton::Pending(pending) => {
            if pending.readiness.pre_result_ready() {
                return Err(ShapeEncodingError::InconsistentShapeReadiness);
            }
            Ok(pending.readiness)
        }
    }
}

fn validate_effect_leaf(
    value: &SymbolicEffectShapeSkeleton,
) -> Result<SymbolicShapeReadiness, ShapeEncodingError> {
    match value {
        SymbolicEffectShapeSkeleton::Resolved { value, readiness } => {
            let expected = symbolic_type_readiness(value);
            if *readiness != expected && *readiness != SymbolicShapeReadiness::PendingC4 {
                return Err(ShapeEncodingError::InconsistentShapeReadiness);
            }
            Ok(*readiness)
        }
        SymbolicEffectShapeSkeleton::Pending(pending) => {
            if pending.readiness.pre_result_ready() {
                return Err(ShapeEncodingError::InconsistentShapeReadiness);
            }
            Ok(pending.readiness)
        }
    }
}

fn validate_predicate_set(
    predicates: &[SymbolicPredicateShapeSkeleton],
    readiness: &mut SymbolicShapeReadiness,
) -> Result<(), ShapeEncodingError> {
    let mut resolved = Vec::new();
    let mut canonical_ready = true;
    for predicate in predicates {
        match predicate {
            SymbolicPredicateShapeSkeleton::Resolved {
                value,
                readiness: stored,
            } => {
                let expected = symbolic_predicate_readiness(value);
                if *stored != expected {
                    return Err(ShapeEncodingError::InconsistentShapeReadiness);
                }
                *readiness = readiness.combine(expected);
                canonical_ready &= expected.pre_result_ready();
                resolved.push(value.as_ref().clone());
            }
            SymbolicPredicateShapeSkeleton::Pending(pending) => {
                if pending.readiness.pre_result_ready() {
                    return Err(ShapeEncodingError::InconsistentShapeReadiness);
                }
                *readiness = readiness.combine(pending.readiness);
                canonical_ready = false;
            }
        }
    }
    if canonical_ready {
        encode_symbolic_predicate_set(&resolved)?;
    }
    Ok(())
}

fn validate_effect_sets(
    effects: &SymbolicEffectSetsSkeleton,
    readiness: &mut SymbolicShapeReadiness,
) -> Result<(), ShapeEncodingError> {
    for set in [&effects.requires, &effects.throws] {
        let mut resolved = Vec::new();
        let mut canonical_ready = true;
        for effect in set {
            let effect_readiness = validate_effect_leaf(effect)?;
            *readiness = readiness.combine(effect_readiness);
            canonical_ready &= effect_readiness.pre_result_ready();
            if let SymbolicEffectShapeSkeleton::Resolved { value, .. } = effect {
                resolved.push(value.clone());
            }
        }
        if canonical_ready {
            let mut rows = resolved
                .iter()
                .map(encode_symbolic_type)
                .collect::<Result<Vec<_>, _>>()?;
            rows.sort();
            if rows.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(ShapeEncodingError::DuplicateEffect);
            }
        }
    }
    Ok(())
}

fn validate_record(
    record: &SymbolicRecordShapeSkeleton,
    readiness: &mut SymbolicShapeReadiness,
) -> Result<(), ShapeEncodingError> {
    match record.form {
        SymbolicRecordForm::Unit if !record.fields.is_empty() => {
            return Err(ShapeEncodingError::InvalidDeclarationShape(
                "unit record form cannot carry fields",
            ));
        }
        SymbolicRecordForm::Tuple if record.fields.iter().any(|field| field.name.is_some()) => {
            return Err(ShapeEncodingError::InvalidDeclarationShape(
                "tuple record fields cannot carry names",
            ));
        }
        SymbolicRecordForm::Record if record.fields.iter().any(|field| field.name.is_none()) => {
            return Err(ShapeEncodingError::InvalidDeclarationShape(
                "record fields require names",
            ));
        }
        SymbolicRecordForm::Unit | SymbolicRecordForm::Tuple | SymbolicRecordForm::Record => {}
    }
    if record.form == SymbolicRecordForm::Record {
        validate_unique_member_names(
            record
                .fields
                .iter()
                .filter_map(|field| field.name.as_deref()),
            "record declaration repeats a field name",
        )?;
    }
    for field in &record.fields {
        *readiness = readiness.combine(validate_type_leaf(&field.ty)?);
    }
    Ok(())
}

fn validate_unique_member_names<'a>(
    names: impl IntoIterator<Item = &'a str>,
    message: &'static str,
) -> Result<(), ShapeEncodingError> {
    let mut unique = BTreeSet::new();
    if names.into_iter().any(|name| !unique.insert(name)) {
        return Err(ShapeEncodingError::InvalidDeclarationShape(message));
    }
    Ok(())
}

fn validate_callable(
    callable: &SymbolicCallableShapeSkeleton,
    readiness: &mut SymbolicShapeReadiness,
) -> Result<(), ShapeEncodingError> {
    match callable.kind {
        SymbolicCallableKind::Function
            if callable.resume.is_some() || callable.yields.is_some() =>
        {
            return Err(ShapeEncodingError::InvalidDeclarationShape(
                "function callable cannot carry generator resume/yield types",
            ));
        }
        SymbolicCallableKind::Generator
            if callable.resume.is_none() || callable.yields.is_none() =>
        {
            return Err(ShapeEncodingError::InvalidDeclarationShape(
                "generator callable requires resume and yield types",
            ));
        }
        SymbolicCallableKind::Function | SymbolicCallableKind::Generator => {}
    }
    for parameter in &callable.parameters {
        *readiness = readiness.combine(validate_type_leaf(&parameter.ty)?);
    }
    *readiness = readiness.combine(validate_type_leaf(&callable.result)?);
    if let Some(resume) = &callable.resume {
        *readiness = readiness.combine(validate_type_leaf(resume)?);
    }
    if let Some(yields) = &callable.yields {
        *readiness = readiness.combine(validate_type_leaf(yields)?);
    }
    validate_effect_sets(&callable.effects, readiness)
}

fn validate_query_terms(
    terms: &[SymbolicQueryTermShapeSkeleton],
    readiness: &mut SymbolicShapeReadiness,
) -> Result<(), ShapeEncodingError> {
    for term in terms {
        *readiness = readiness.combine(validate_type_leaf(&term.ty)?);
    }
    Ok(())
}

fn validate_payload(
    payload: &SymbolicDeclarationPayloadSkeleton,
    readiness: &mut SymbolicShapeReadiness,
) -> Result<(), ShapeEncodingError> {
    match payload {
        SymbolicDeclarationPayloadSkeleton::World | SymbolicDeclarationPayloadSkeleton::Tag => {}
        SymbolicDeclarationPayloadSkeleton::Record(record) => {
            validate_record(record, readiness)?;
        }
        SymbolicDeclarationPayloadSkeleton::Enum(variants) => {
            validate_unique_member_names(
                variants.iter().map(|variant| variant.name.as_str()),
                "enum declaration repeats a variant name",
            )?;
            for variant in variants {
                validate_record(
                    &SymbolicRecordShapeSkeleton {
                        form: variant.form,
                        fields: variant.fields.clone(),
                    },
                    readiness,
                )?;
            }
        }
        SymbolicDeclarationPayloadSkeleton::Callable(callable) => {
            validate_callable(callable, readiness)?;
        }
        SymbolicDeclarationPayloadSkeleton::System {
            accesses,
            implied_requires,
            result,
            effects,
        } => {
            for access in accesses {
                match access {
                    SymbolicSystemAccessShapeSkeleton::CapabilityShared(ty)
                    | SymbolicSystemAccessShapeSkeleton::CapabilityMutable(ty)
                    | SymbolicSystemAccessShapeSkeleton::ResourceRead(ty)
                    | SymbolicSystemAccessShapeSkeleton::ResourceWrite(ty) => {
                        *readiness = readiness.combine(validate_type_leaf(ty)?);
                    }
                    SymbolicSystemAccessShapeSkeleton::Query(terms) => {
                        validate_query_terms(terms, readiness)?;
                    }
                    SymbolicSystemAccessShapeSkeleton::Commands => {}
                }
            }
            for implied in implied_requires {
                if implied.readiness != SymbolicShapeReadiness::PendingC4 {
                    return Err(ShapeEncodingError::InconsistentShapeReadiness);
                }
                *readiness = readiness
                    .combine(validate_type_leaf(&implied.referent)?)
                    .combine(implied.readiness);
            }
            *readiness = readiness.combine(validate_type_leaf(result)?);
            validate_effect_sets(effects, readiness)?;
        }
        SymbolicDeclarationPayloadSkeleton::Trait { methods } => {
            validate_method_list(methods, readiness)?;
        }
        SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref,
            target,
            methods,
            ..
        } => {
            if let Some(trait_ref) = trait_ref {
                *readiness = readiness.combine(validate_type_leaf(trait_ref)?);
            }
            *readiness = readiness.combine(validate_type_leaf(target)?);
            validate_method_list(methods, readiness)?;
        }
        SymbolicDeclarationPayloadSkeleton::Alias { target } => {
            *readiness = readiness.combine(validate_type_leaf(target)?);
        }
        SymbolicDeclarationPayloadSkeleton::Const { ty }
        | SymbolicDeclarationPayloadSkeleton::Static { ty, .. } => {
            *readiness = readiness.combine(validate_type_leaf(ty)?);
        }
        SymbolicDeclarationPayloadSkeleton::Query { terms } => {
            validate_query_terms(terms, readiness)?;
        }
        SymbolicDeclarationPayloadSkeleton::Schedule {
            effects,
            readiness: schedule_readiness,
        } => {
            *readiness = readiness.combine(*schedule_readiness);
            validate_effect_sets(effects, readiness)?;
        }
    }
    Ok(())
}

fn validate_method_list(
    methods: &[SymbolicMethodShapeSkeleton],
    readiness: &mut SymbolicShapeReadiness,
) -> Result<(), ShapeEncodingError> {
    validate_unique_member_names(
        methods.iter().map(|method| method.name.as_str()),
        "trait or impl declaration repeats a method name",
    )?;
    for method in methods {
        if !matches!(
            method.shape.payload,
            SymbolicDeclarationPayloadSkeleton::Callable(_)
        ) {
            return Err(ShapeEncodingError::InvalidDeclarationShape(
                "trait/impl method entry must reuse a callable child declaration shape",
            ));
        }
        *readiness = readiness.combine(declaration_shape_readiness(&method.shape)?);
    }
    Ok(())
}

pub fn encode_declaration_shape_preimage(
    shape: &CanonicalDeclarationShape,
) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = Vec::new();
    encode_alpha_generic_kinds(&shape.skeleton.generic_parameters, &mut output)?;
    encode_predicate_skeleton_set(&shape.skeleton.predicates, &mut output)?;
    encode_declaration_payload(&shape.skeleton.payload, &mut output)?;
    Ok(output)
}

/// Final `DefinitionId` declaration-shape encoding. Unlike the pre-result
/// encoder, this operation fails closed while any C5 CTFE normalization is
/// outstanding.
pub fn encode_final_declaration_shape_identity(
    shape: &CanonicalDeclarationShape,
) -> Result<Vec<u8>, ShapeEncodingError> {
    if !shape.readiness.final_identity_ready() {
        return Err(ShapeEncodingError::FinalIdentityNeedsCtfe);
    }
    encode_declaration_shape_preimage(shape)
}

pub fn encode_definition_owner_entry(
    owner: &CanonicalDefinitionOwner,
) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = Vec::new();
    match &owner.skeleton {
        SymbolicDefinitionOwnerSkeleton::TopLevel => {}
        SymbolicDefinitionOwnerSkeleton::Trait { path, shape } => {
            output.push(1);
            encode_declaration_path(path, &mut output)?;
            let shape = try_canonicalize_declaration_shape(shape)?.ok_or(
                ShapeEncodingError::InvalidDeclarationShape(
                    "canonical trait owner contains a pending parent shape",
                ),
            )?;
            bytes(
                &mut output,
                &encode_declaration_shape_preimage(&shape)?,
                "trait owner shape",
            )?;
        }
        SymbolicDefinitionOwnerSkeleton::InherentImpl {
            target,
            generic_parameters,
            predicates,
        } => {
            output.push(2);
            encode_type_leaf(target, &mut output)?;
            encode_alpha_generic_kinds(generic_parameters, &mut output)?;
            encode_predicate_skeleton_set(predicates, &mut output)?;
        }
        SymbolicDefinitionOwnerSkeleton::TraitImpl {
            trait_ref,
            target,
            generic_parameters,
            predicates,
            is_default,
        } => {
            output.push(3);
            encode_type_leaf(trait_ref, &mut output)?;
            encode_type_leaf(target, &mut output)?;
            encode_alpha_generic_kinds(generic_parameters, &mut output)?;
            encode_predicate_skeleton_set(predicates, &mut output)?;
            output.push(u8::from(*is_default));
        }
        SymbolicDefinitionOwnerSkeleton::SystemQuery { path, shape } => {
            output.push(4);
            encode_declaration_path(path, &mut output)?;
            let shape = try_canonicalize_declaration_shape(shape)?.ok_or(
                ShapeEncodingError::InvalidDeclarationShape(
                    "canonical query owner contains a pending parent shape",
                ),
            )?;
            bytes(
                &mut output,
                &encode_declaration_shape_preimage(&shape)?,
                "system query owner shape",
            )?;
        }
    }
    Ok(output)
}

/// Final owner-entry encoding for `DefinitionId`. Pre-result owner entries may
/// carry const-definition paths; this operation rejects them until C5 has
/// normalized every dependency.
pub fn encode_final_definition_owner_identity(
    owner: &CanonicalDefinitionOwner,
) -> Result<Vec<u8>, ShapeEncodingError> {
    if !owner.readiness.final_identity_ready() {
        return Err(ShapeEncodingError::FinalIdentityNeedsCtfe);
    }
    encode_definition_owner_entry(owner)
}

fn encode_alpha_generic_kinds(
    parameters: &[GenericParameterKind],
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    count(output, parameters.len(), "alpha generic parameter list")?;
    for parameter in parameters {
        match parameter {
            GenericParameterKind::Type => output.push(1),
            GenericParameterKind::Lifetime => output.push(2),
            GenericParameterKind::IntegerConst(integer_type) => {
                output.push(3);
                output.push(integer_type.tag());
            }
        }
    }
    Ok(())
}

fn encode_predicate_skeleton_set(
    predicates: &[SymbolicPredicateShapeSkeleton],
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    let values = predicates
        .iter()
        .map(|predicate| match predicate {
            SymbolicPredicateShapeSkeleton::Resolved { value, .. } => Ok(value.as_ref().clone()),
            SymbolicPredicateShapeSkeleton::Pending(_) => {
                Err(ShapeEncodingError::InvalidDeclarationShape(
                    "canonical predicate set contains a pending member",
                ))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    output.extend_from_slice(&encode_symbolic_predicate_set(&values)?);
    Ok(())
}

fn encode_type_leaf(
    ty: &SymbolicTypeShapeSkeleton,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    let SymbolicTypeShapeSkeleton::Resolved { value, .. } = ty else {
        return Err(ShapeEncodingError::InvalidDeclarationShape(
            "canonical declaration shape contains a pending type",
        ));
    };
    nested_type(output, value)
}

fn encode_effect_skeleton_set(
    effects: &[SymbolicEffectShapeSkeleton],
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    let values = effects
        .iter()
        .map(|effect| match effect {
            SymbolicEffectShapeSkeleton::Resolved { value, .. } => Ok(value.clone()),
            SymbolicEffectShapeSkeleton::Pending(_) => {
                Err(ShapeEncodingError::InvalidDeclarationShape(
                    "canonical effect set contains a pending member",
                ))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    type_set(output, &values, "declaration effect set")
}

fn encode_effect_sets(
    effects: &SymbolicEffectSetsSkeleton,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    encode_effect_skeleton_set(&effects.requires, output)?;
    encode_effect_skeleton_set(&effects.throws, output)
}

fn encode_record_shape(
    record: &SymbolicRecordShapeSkeleton,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    output.push(record.form.tag());
    count(output, record.fields.len(), "record field list")?;
    for field in &record.fields {
        if let Some(name) = &field.name {
            string(output, name, "record field name")?;
        }
        encode_type_leaf(&field.ty, output)?;
    }
    Ok(())
}

fn encode_callable_shape(
    callable: &SymbolicCallableShapeSkeleton,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    count(output, callable.parameters.len(), "callable parameter list")?;
    for parameter in &callable.parameters {
        output.push(parameter.mode.tag());
        encode_type_leaf(&parameter.ty, output)?;
    }
    encode_type_leaf(&callable.result, output)?;
    output.push(u8::from(callable.unsafe_));
    if matches!(callable.kind, SymbolicCallableKind::Generator) {
        encode_type_leaf(
            callable
                .resume
                .as_ref()
                .ok_or(ShapeEncodingError::InvalidDeclarationShape(
                    "generator callable is missing its resume type",
                ))?,
            output,
        )?;
        encode_type_leaf(
            callable
                .yields
                .as_ref()
                .ok_or(ShapeEncodingError::InvalidDeclarationShape(
                    "generator callable is missing its yield type",
                ))?,
            output,
        )?;
    }
    encode_effect_sets(&callable.effects, output)
}

pub fn encode_method_entry(
    method: &SymbolicMethodShapeSkeleton,
) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = Vec::new();
    string(&mut output, &method.name, "method name")?;
    let callable = try_canonicalize_declaration_shape(&method.shape)?.ok_or(
        ShapeEncodingError::InvalidDeclarationShape(
            "canonical method entry contains a pending callable shape",
        ),
    )?;
    bytes(
        &mut output,
        &encode_declaration_shape_preimage(&callable)?,
        "method callable shape",
    )?;
    Ok(output)
}

fn encode_method_list(
    methods: &[SymbolicMethodShapeSkeleton],
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    count(output, methods.len(), "method list")?;
    for method in methods {
        output.extend_from_slice(&encode_method_entry(method)?);
    }
    Ok(())
}

fn encode_query_terms(
    terms: &[SymbolicQueryTermShapeSkeleton],
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    count(output, terms.len(), "query term list")?;
    for term in terms {
        output.push(term.kind.tag());
        encode_type_leaf(&term.ty, output)?;
    }
    Ok(())
}

fn encode_declaration_payload(
    payload: &SymbolicDeclarationPayloadSkeleton,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    match payload {
        SymbolicDeclarationPayloadSkeleton::World | SymbolicDeclarationPayloadSkeleton::Tag => {}
        SymbolicDeclarationPayloadSkeleton::Record(record) => {
            encode_record_shape(record, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Enum(variants) => {
            count(output, variants.len(), "enum variant list")?;
            for variant in variants {
                string(output, &variant.name, "variant name")?;
                encode_record_shape(
                    &SymbolicRecordShapeSkeleton {
                        form: variant.form,
                        fields: variant.fields.clone(),
                    },
                    output,
                )?;
            }
        }
        SymbolicDeclarationPayloadSkeleton::Callable(callable) => {
            encode_callable_shape(callable, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::System {
            accesses,
            implied_requires: _,
            result,
            effects,
        } => {
            count(output, accesses.len(), "system access list")?;
            for access in accesses {
                match access {
                    SymbolicSystemAccessShapeSkeleton::CapabilityShared(ty) => {
                        output.push(1);
                        encode_type_leaf(ty, output)?;
                    }
                    SymbolicSystemAccessShapeSkeleton::CapabilityMutable(ty) => {
                        output.push(2);
                        encode_type_leaf(ty, output)?;
                    }
                    SymbolicSystemAccessShapeSkeleton::ResourceRead(ty) => {
                        output.push(3);
                        encode_type_leaf(ty, output)?;
                    }
                    SymbolicSystemAccessShapeSkeleton::ResourceWrite(ty) => {
                        output.push(4);
                        encode_type_leaf(ty, output)?;
                    }
                    SymbolicSystemAccessShapeSkeleton::Query(terms) => {
                        output.push(5);
                        encode_query_terms(terms, output)?;
                    }
                    SymbolicSystemAccessShapeSkeleton::Commands => output.push(6),
                }
            }
            encode_type_leaf(result, output)?;
            output.push(0);
            encode_effect_sets(effects, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Trait { methods } => {
            encode_method_list(methods, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref,
            target,
            is_default,
            methods,
        } => {
            match trait_ref {
                Some(trait_ref) => {
                    output.push(1);
                    encode_type_leaf(trait_ref, output)?;
                }
                None => output.push(0),
            }
            encode_type_leaf(target, output)?;
            output.push(u8::from(*is_default));
            encode_method_list(methods, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Alias { target } => {
            encode_type_leaf(target, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Const { ty } => {
            encode_type_leaf(ty, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Static { mutable, ty } => {
            encode_type_leaf(ty, output)?;
            output.push(u8::from(*mutable));
        }
        SymbolicDeclarationPayloadSkeleton::Query { terms } => {
            encode_query_terms(terms, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Schedule { effects, .. } => {
            encode_effect_sets(effects, output)?;
        }
    }
    Ok(())
}

/// Deterministic C1 inventory/debug projection of the typed skeleton. Unlike
/// `encode_declaration_shape_preimage`, this encoding is not an identity
/// authority: it includes readiness tags and pending diagnostic spelling.
pub fn encode_symbolic_declaration_shape_skeleton_c1(
    shape: &SymbolicDeclarationShapeSkeleton,
) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = vec![declaration_shape_readiness(shape)?.tag()];
    encode_alpha_generic_kinds(&shape.generic_parameters, &mut output)?;
    encode_debug_predicate_set(&shape.predicates, &mut output)?;
    encode_debug_payload(&shape.payload, &mut output)?;
    Ok(output)
}

/// Deterministic C1 inventory/debug projection of the typed owner skeleton.
pub fn encode_symbolic_definition_owner_skeleton_c1(
    owner: &SymbolicDefinitionOwnerSkeleton,
) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = vec![owner_shape_readiness(owner)?.tag()];
    match owner {
        SymbolicDefinitionOwnerSkeleton::TopLevel => output.push(0),
        SymbolicDefinitionOwnerSkeleton::Trait { path, shape } => {
            output.push(1);
            encode_declaration_path(path, &mut output)?;
            bytes(
                &mut output,
                &encode_symbolic_declaration_shape_skeleton_c1(shape)?,
                "C1 trait owner skeleton",
            )?;
        }
        SymbolicDefinitionOwnerSkeleton::InherentImpl {
            target,
            generic_parameters,
            predicates,
        } => {
            output.push(2);
            encode_debug_type_leaf(target, &mut output)?;
            encode_alpha_generic_kinds(generic_parameters, &mut output)?;
            encode_debug_predicate_set(predicates, &mut output)?;
        }
        SymbolicDefinitionOwnerSkeleton::TraitImpl {
            trait_ref,
            target,
            generic_parameters,
            predicates,
            is_default,
        } => {
            output.push(3);
            encode_debug_type_leaf(trait_ref, &mut output)?;
            encode_debug_type_leaf(target, &mut output)?;
            encode_alpha_generic_kinds(generic_parameters, &mut output)?;
            encode_debug_predicate_set(predicates, &mut output)?;
            output.push(u8::from(*is_default));
        }
        SymbolicDefinitionOwnerSkeleton::SystemQuery { path, shape } => {
            output.push(4);
            encode_declaration_path(path, &mut output)?;
            bytes(
                &mut output,
                &encode_symbolic_declaration_shape_skeleton_c1(shape)?,
                "C1 query owner skeleton",
            )?;
        }
    }
    Ok(output)
}

fn encode_debug_pending(
    pending: &SymbolicPendingShape,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    output.push(0);
    output.push(pending.readiness.tag());
    encode_debug_source_span(output, pending.source_span);
    output.push(pending.kind.tag());
    string(
        output,
        &pending.debug_spelling,
        "pending symbolic shape diagnostic spelling",
    )
}

fn encode_debug_type_leaf(
    ty: &SymbolicTypeShapeSkeleton,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    match ty {
        SymbolicTypeShapeSkeleton::Resolved { value, readiness } => {
            output.push(1);
            output.push(readiness.tag());
            bytes(
                output,
                &encode_symbolic_type_skeleton_c1(value)?,
                "resolved C1 symbolic type",
            )
        }
        SymbolicTypeShapeSkeleton::Pending(pending) => encode_debug_pending(pending, output),
    }
}

fn encode_debug_effect_leaf(
    effect: &SymbolicEffectShapeSkeleton,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    match effect {
        SymbolicEffectShapeSkeleton::Resolved { value, readiness } => {
            output.push(1);
            output.push(readiness.tag());
            bytes(
                output,
                &encode_symbolic_type_skeleton_c1(value)?,
                "resolved C1 symbolic effect type",
            )
        }
        SymbolicEffectShapeSkeleton::Pending(pending) => encode_debug_pending(pending, output),
    }
}

fn encode_debug_predicate_set(
    predicates: &[SymbolicPredicateShapeSkeleton],
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    let mut rows = predicates
        .iter()
        .map(|predicate| {
            let mut row = Vec::new();
            match predicate {
                SymbolicPredicateShapeSkeleton::Resolved { value, readiness } => {
                    row.push(1);
                    row.push(readiness.tag());
                    bytes(
                        &mut row,
                        &encode_symbolic_predicate_skeleton_c1(value)?,
                        "resolved C1 symbolic predicate",
                    )?;
                }
                SymbolicPredicateShapeSkeleton::Pending(pending) => {
                    encode_debug_pending(pending, &mut row)?;
                }
            }
            Ok(row)
        })
        .collect::<Result<Vec<_>, ShapeEncodingError>>()?;
    rows.sort();
    if rows.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ShapeEncodingError::DuplicatePredicate);
    }
    count(output, rows.len(), "C1 predicate skeleton set")?;
    for row in rows {
        bytes(output, &row, "C1 predicate skeleton")?;
    }
    Ok(())
}

fn encode_debug_effect_set(
    effects: &[SymbolicEffectShapeSkeleton],
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    count(output, effects.len(), "C1 effect skeleton source list")?;
    for effect in effects {
        let mut row = Vec::new();
        encode_debug_effect_leaf(effect, &mut row)?;
        bytes(output, &row, "C1 effect skeleton")?;
    }
    Ok(())
}

fn encode_debug_effect_sets(
    effects: &SymbolicEffectSetsSkeleton,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    encode_debug_effect_set(&effects.requires, output)?;
    encode_debug_effect_set(&effects.throws, output)
}

fn encode_debug_source_span(output: &mut Vec<u8>, span: SymbolicSourceSpan) {
    for field in [
        span.file,
        span.start_byte,
        span.end_byte,
        span.start_line,
        span.start_column,
        span.end_line,
        span.end_column,
    ] {
        output.extend_from_slice(&field.to_le_bytes());
    }
}

fn encode_debug_record(
    record: &SymbolicRecordShapeSkeleton,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    output.push(record.form.tag());
    count(output, record.fields.len(), "C1 record field list")?;
    for field in &record.fields {
        if let Some(name) = &field.name {
            string(output, name, "C1 record field name")?;
        }
        encode_debug_type_leaf(&field.ty, output)?;
    }
    Ok(())
}

fn encode_debug_callable(
    callable: &SymbolicCallableShapeSkeleton,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    output.push(match callable.kind {
        SymbolicCallableKind::Function => 1,
        SymbolicCallableKind::Generator => 2,
    });
    count(
        output,
        callable.parameters.len(),
        "C1 callable parameter list",
    )?;
    for parameter in &callable.parameters {
        output.push(parameter.mode.tag());
        encode_debug_type_leaf(&parameter.ty, output)?;
    }
    encode_debug_type_leaf(&callable.result, output)?;
    output.push(u8::from(callable.unsafe_));
    match &callable.resume {
        Some(resume) => {
            output.push(1);
            encode_debug_type_leaf(resume, output)?;
        }
        None => output.push(0),
    }
    match &callable.yields {
        Some(yields) => {
            output.push(1);
            encode_debug_type_leaf(yields, output)?;
        }
        None => output.push(0),
    }
    encode_debug_effect_sets(&callable.effects, output)
}

fn encode_debug_method_list(
    methods: &[SymbolicMethodShapeSkeleton],
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    count(output, methods.len(), "C1 method list")?;
    for method in methods {
        string(output, &method.name, "C1 method name")?;
        bytes(
            output,
            &encode_symbolic_declaration_shape_skeleton_c1(&method.shape)?,
            "C1 child callable skeleton",
        )?;
    }
    Ok(())
}

fn encode_debug_query_terms(
    terms: &[SymbolicQueryTermShapeSkeleton],
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    count(output, terms.len(), "C1 query term list")?;
    for term in terms {
        output.push(term.kind.tag());
        encode_debug_type_leaf(&term.ty, output)?;
    }
    Ok(())
}

fn encode_debug_payload(
    payload: &SymbolicDeclarationPayloadSkeleton,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    match payload {
        SymbolicDeclarationPayloadSkeleton::World => output.push(1),
        SymbolicDeclarationPayloadSkeleton::Record(record) => {
            output.push(2);
            encode_debug_record(record, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Enum(variants) => {
            output.push(3);
            count(output, variants.len(), "C1 enum variant list")?;
            for variant in variants {
                string(output, &variant.name, "C1 variant name")?;
                encode_debug_record(
                    &SymbolicRecordShapeSkeleton {
                        form: variant.form,
                        fields: variant.fields.clone(),
                    },
                    output,
                )?;
            }
        }
        SymbolicDeclarationPayloadSkeleton::Tag => output.push(4),
        SymbolicDeclarationPayloadSkeleton::Callable(callable) => {
            output.push(5);
            encode_debug_callable(callable, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::System {
            accesses,
            implied_requires,
            result,
            effects,
        } => {
            output.push(6);
            count(output, accesses.len(), "C1 system access list")?;
            for access in accesses {
                match access {
                    SymbolicSystemAccessShapeSkeleton::CapabilityShared(ty) => {
                        output.push(1);
                        encode_debug_type_leaf(ty, output)?;
                    }
                    SymbolicSystemAccessShapeSkeleton::CapabilityMutable(ty) => {
                        output.push(2);
                        encode_debug_type_leaf(ty, output)?;
                    }
                    SymbolicSystemAccessShapeSkeleton::ResourceRead(ty) => {
                        output.push(3);
                        encode_debug_type_leaf(ty, output)?;
                    }
                    SymbolicSystemAccessShapeSkeleton::ResourceWrite(ty) => {
                        output.push(4);
                        encode_debug_type_leaf(ty, output)?;
                    }
                    SymbolicSystemAccessShapeSkeleton::Query(terms) => {
                        output.push(5);
                        encode_debug_query_terms(terms, output)?;
                    }
                    SymbolicSystemAccessShapeSkeleton::Commands => output.push(6),
                }
            }
            count(
                output,
                implied_requires.len(),
                "C1 implied capability requirement list",
            )?;
            for implied in implied_requires {
                output.extend_from_slice(&implied.parameter_ordinal.to_le_bytes());
                encode_debug_source_span(output, implied.parameter_span);
                output.push(implied.access.tag());
                encode_debug_type_leaf(&implied.referent, output)?;
                output.push(implied.readiness.tag());
            }
            encode_debug_type_leaf(result, output)?;
            encode_debug_effect_sets(effects, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Trait { methods } => {
            output.push(7);
            encode_debug_method_list(methods, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref,
            target,
            is_default,
            methods,
        } => {
            output.push(8);
            match trait_ref {
                Some(trait_ref) => {
                    output.push(1);
                    encode_debug_type_leaf(trait_ref, output)?;
                }
                None => output.push(0),
            }
            encode_debug_type_leaf(target, output)?;
            output.push(u8::from(*is_default));
            encode_debug_method_list(methods, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Alias { target } => {
            output.push(9);
            encode_debug_type_leaf(target, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Const { ty } => {
            output.push(10);
            encode_debug_type_leaf(ty, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Static { mutable, ty } => {
            output.push(11);
            encode_debug_type_leaf(ty, output)?;
            output.push(u8::from(*mutable));
        }
        SymbolicDeclarationPayloadSkeleton::Query { terms } => {
            output.push(12);
            encode_debug_query_terms(terms, output)?;
        }
        SymbolicDeclarationPayloadSkeleton::Schedule { effects, readiness } => {
            output.push(13);
            output.push(readiness.tag());
            encode_debug_effect_sets(effects, output)?;
        }
    }
    Ok(())
}

pub fn encode_symbolic_type(ty: &SymbolicType) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = Vec::new();
    encode_type_into(ty, &mut output)?;
    Ok(output)
}

pub(crate) fn encode_symbolic_type_skeleton_c1(
    ty: &SymbolicType,
) -> Result<Vec<u8>, ShapeEncodingError> {
    if symbolic_type_readiness(ty).pre_result_ready() {
        return encode_symbolic_type(ty);
    }
    let mut output = Vec::new();
    encode_type_skeleton_c1_into(ty, &mut output)?;
    Ok(output)
}

fn encode_type_skeleton_c1_into(
    ty: &SymbolicType,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    match ty {
        SymbolicType::I8
        | SymbolicType::I16
        | SymbolicType::I32
        | SymbolicType::I64
        | SymbolicType::U8
        | SymbolicType::U16
        | SymbolicType::U32
        | SymbolicType::U64
        | SymbolicType::Isize
        | SymbolicType::Usize
        | SymbolicType::F32
        | SymbolicType::F64
        | SymbolicType::Bool
        | SymbolicType::Char
        | SymbolicType::Entity
        | SymbolicType::Unit
        | SymbolicType::Never
        | SymbolicType::Str
        | SymbolicType::BoundType { .. } => encode_type_into(ty, output)?,
        SymbolicType::Slice(element) => {
            output.push(19);
            nested_type_skeleton_c1(output, element)?;
        }
        SymbolicType::Array { element, length } => {
            output.push(20);
            nested_type_skeleton_c1(output, element)?;
            bytes(output, &encode_symbolic_const(length)?, "array length")?;
        }
        SymbolicType::Tuple(elements) => {
            output.push(21);
            types_skeleton_c1(output, elements, "C1 tuple type")?;
        }
        SymbolicType::Reference {
            mutability,
            lifetime,
            pointee,
        } => {
            output.push(22);
            output.push(mutability.tag());
            encode_lifetime(lifetime, output);
            nested_type_skeleton_c1(output, pointee)?;
        }
        SymbolicType::RawPointer {
            mutability,
            pointee,
        } => {
            output.push(23);
            output.push(mutability.tag());
            nested_type_skeleton_c1(output, pointee)?;
        }
        SymbolicType::NominalPath {
            declaration,
            arguments,
        } => {
            output.push(29);
            encode_declaration_path(declaration, output)?;
            generic_arguments_skeleton_c1(output, arguments)?;
        }
        SymbolicType::FunctionPointer {
            unsafe_,
            parameters,
            result,
            requires,
            throws,
        } => {
            output.push(25);
            output.push(u8::from(*unsafe_));
            types_skeleton_c1(output, parameters, "C1 function parameters")?;
            nested_type_skeleton_c1(output, result)?;
            nested_effect_set_skeleton_c1(output, requires, "C1 requires source list")?;
            nested_effect_set_skeleton_c1(output, throws, "C1 throws source list")?;
        }
        SymbolicType::Closure {
            owner,
            expression_ordinal,
            captures,
            parameters,
            result,
            requires,
            throws,
            arguments,
        } => {
            nonzero_ordinal(*expression_ordinal)?;
            output.push(27);
            encode_declaration_path(owner, output)?;
            output.extend_from_slice(&expression_ordinal.to_le_bytes());
            captures_skeleton_c1(captures, output)?;
            types_skeleton_c1(output, parameters, "C1 closure parameters")?;
            nested_type_skeleton_c1(output, result)?;
            nested_effect_set_skeleton_c1(output, requires, "C1 closure requires source list")?;
            nested_effect_set_skeleton_c1(output, throws, "C1 closure throws source list")?;
            generic_arguments_skeleton_c1(output, arguments)?;
        }
        SymbolicType::Generator {
            target,
            captures,
            parameters,
            factory_unsafe,
            resume,
            yields,
            result,
            requires,
            throws,
        } => {
            validate_generator_shape(target, captures, *factory_unsafe)?;
            output.push(28);
            generator_target_skeleton_c1(target, output)?;
            captures_skeleton_c1(captures, output)?;
            types_skeleton_c1(output, parameters, "C1 generator parameters")?;
            output.push(u8::from(*factory_unsafe));
            nested_type_skeleton_c1(output, resume)?;
            nested_type_skeleton_c1(output, yields)?;
            nested_type_skeleton_c1(output, result)?;
            nested_effect_set_skeleton_c1(output, requires, "C1 generator requires source list")?;
            nested_effect_set_skeleton_c1(output, throws, "C1 generator throws source list")?;
        }
        SymbolicType::JoinHandle { result, throws } => {
            output.push(30);
            nested_type_skeleton_c1(output, result)?;
            nested_effect_set_skeleton_c1(output, throws, "C1 join handle throws source list")?;
        }
        SymbolicType::GeneratorFactory {
            target,
            captures,
            call_trait,
            parameters,
            factory_unsafe,
            produced_generator,
        } => {
            validate_generator_shape(target, captures, *factory_unsafe)?;
            output.push(31);
            generator_target_skeleton_c1(target, output)?;
            captures_skeleton_c1(captures, output)?;
            output.push(call_trait.tag());
            types_skeleton_c1(output, parameters, "C1 generator factory parameters")?;
            output.push(u8::from(*factory_unsafe));
            nested_type_skeleton_c1(output, produced_generator)?;
        }
    }
    Ok(())
}

fn nested_type_skeleton_c1(
    output: &mut Vec<u8>,
    ty: &SymbolicType,
) -> Result<(), ShapeEncodingError> {
    bytes(
        output,
        &encode_symbolic_type_skeleton_c1(ty)?,
        "C1 semantic type skeleton",
    )
}

fn types_skeleton_c1(
    output: &mut Vec<u8>,
    values: &[SymbolicType],
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    count(output, values.len(), label)?;
    for value in values {
        nested_type_skeleton_c1(output, value)?;
    }
    Ok(())
}

fn nested_effect_set_skeleton_c1(
    output: &mut Vec<u8>,
    effects: &SymbolicTypeEffectSet,
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    output.push(effects.readiness().tag());
    types_skeleton_c1(output, effects.members(), label)
}

fn generic_arguments_skeleton_c1(
    output: &mut Vec<u8>,
    arguments: &[GenericArgumentShape],
) -> Result<(), ShapeEncodingError> {
    count(output, arguments.len(), "C1 generic argument list")?;
    for argument in arguments {
        match argument {
            GenericArgumentShape::Type(ty) => {
                output.push(1);
                nested_type_skeleton_c1(output, ty)?;
            }
            GenericArgumentShape::Lifetime(lifetime) => {
                output.push(2);
                encode_lifetime(lifetime, output);
            }
            GenericArgumentShape::IntegerConst(expression) => {
                output.push(3);
                bytes(
                    output,
                    &encode_symbolic_const(expression)?,
                    "C1 integer const argument",
                )?;
            }
        }
    }
    Ok(())
}

fn generator_target_skeleton_c1(
    target: &GeneratorTarget,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    match target {
        GeneratorTarget::Named {
            declaration,
            arguments,
            hidden_lifetime_binders,
        } => {
            output.push(1);
            encode_declaration_path(declaration, output)?;
            generic_arguments_skeleton_c1(output, arguments)?;
            count(
                output,
                hidden_lifetime_binders.len(),
                "C1 generator hidden lifetime binders",
            )?;
            for position in hidden_lifetime_binders {
                output.extend_from_slice(&position.to_le_bytes());
            }
        }
        GeneratorTarget::Anonymous {
            owner,
            expression_ordinal,
            arguments,
        } => {
            nonzero_ordinal(*expression_ordinal)?;
            output.push(2);
            encode_declaration_path(owner, output)?;
            output.extend_from_slice(&expression_ordinal.to_le_bytes());
            generic_arguments_skeleton_c1(output, arguments)?;
        }
    }
    Ok(())
}

fn captures_skeleton_c1(
    captures: &[SymbolicCapture],
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    count(output, captures.len(), "C1 capture list")?;
    for (index, capture) in captures.iter().enumerate() {
        let expected = u64::try_from(index)
            .map_err(|_| ShapeEncodingError::LengthOverflow("C1 capture list"))?
            .checked_add(1)
            .ok_or(ShapeEncodingError::LengthOverflow("C1 capture list"))?;
        if capture.ordinal != expected {
            return Err(ShapeEncodingError::NonCanonicalCaptureOrdinal {
                expected,
                actual: capture.ordinal,
            });
        }
        output.extend_from_slice(&capture.ordinal.to_le_bytes());
        output.push(capture.mode.tag());
        nested_type_skeleton_c1(output, &capture.ty)?;
    }
    Ok(())
}

fn encode_symbolic_predicate_skeleton_c1(
    predicate: &SymbolicPredicate,
) -> Result<Vec<u8>, ShapeEncodingError> {
    if symbolic_predicate_readiness(predicate).pre_result_ready() {
        return encode_symbolic_predicate(predicate);
    }
    let mut output = Vec::new();
    match predicate {
        SymbolicPredicate::Trait {
            trait_path,
            self_type,
            arguments,
        } => {
            output.push(1);
            encode_declaration_path(trait_path, &mut output)?;
            nested_type_skeleton_c1(&mut output, self_type)?;
            generic_arguments_skeleton_c1(&mut output, arguments)?;
        }
        SymbolicPredicate::LifetimeOutlives { longer, shorter } => {
            output.push(2);
            encode_lifetime(longer, &mut output);
            encode_lifetime(shorter, &mut output);
        }
        SymbolicPredicate::TypeOutlives { ty, lifetime } => {
            output.push(3);
            nested_type_skeleton_c1(&mut output, ty)?;
            encode_lifetime(lifetime, &mut output);
        }
    }
    Ok(output)
}

pub fn encode_symbolic_const(
    expression: &SymbolicConstExpression,
) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = Vec::new();
    encode_const_into(expression, &mut output)?;
    Ok(output)
}

pub fn encode_symbolic_effect(atom: &SymbolicEffectAtom) -> Result<Vec<u8>, ShapeEncodingError> {
    let tree = encode_symbolic_type(&atom.ty)?;
    let mut output = vec![atom.kind.tag()];
    bytes(&mut output, &tree, "symbolic effect type")?;
    Ok(output)
}

pub(crate) fn encode_symbolic_effect_skeleton_c1(
    atom: &SymbolicEffectAtom,
) -> Result<Vec<u8>, ShapeEncodingError> {
    let tree = encode_symbolic_type_skeleton_c1(&atom.ty)?;
    let mut output = vec![atom.kind.tag()];
    bytes(&mut output, &tree, "C1 symbolic effect type skeleton")?;
    Ok(output)
}

pub fn encode_symbolic_effect_set(
    atoms: &[SymbolicEffectAtom],
) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut rows = atoms
        .iter()
        .map(encode_symbolic_effect)
        .collect::<Result<Vec<_>, _>>()?;
    rows.sort();
    if rows.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ShapeEncodingError::DuplicateEffect);
    }
    let mut output = Vec::new();
    count(&mut output, rows.len(), "symbolic effect set")?;
    for row in rows {
        bytes(&mut output, &row, "symbolic effect atom")?;
    }
    Ok(output)
}

pub fn encode_generic_parameters(
    parameters: &[GenericParameterShape],
) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = Vec::new();
    count(&mut output, parameters.len(), "generic parameter list")?;
    for (expected, parameter) in parameters.iter().enumerate() {
        let expected = u64::try_from(expected)
            .map_err(|_| ShapeEncodingError::LengthOverflow("generic parameter list"))?;
        if parameter.index != expected {
            return Err(ShapeEncodingError::NonCanonicalGenericParameterIndex {
                expected,
                actual: parameter.index,
            });
        }
        output.extend_from_slice(&parameter.index.to_le_bytes());
        string(&mut output, &parameter.name, "generic parameter name")?;
        match parameter.kind {
            GenericParameterKind::Type => output.push(1),
            GenericParameterKind::Lifetime => output.push(2),
            GenericParameterKind::IntegerConst(integer_type) => {
                output.push(3);
                output.push(integer_type.tag());
            }
        }
    }
    Ok(output)
}

pub fn encode_generic_arguments(
    arguments: &[GenericArgumentShape],
) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = Vec::new();
    generic_arguments(&mut output, arguments)?;
    Ok(output)
}

pub fn encode_symbolic_predicate(
    predicate: &SymbolicPredicate,
) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut output = Vec::new();
    match predicate {
        SymbolicPredicate::Trait {
            trait_path,
            self_type,
            arguments,
        } => {
            output.push(1);
            encode_declaration_path(trait_path, &mut output)?;
            nested_type(&mut output, self_type)?;
            generic_arguments(&mut output, arguments)?;
        }
        SymbolicPredicate::LifetimeOutlives { longer, shorter } => {
            output.push(2);
            encode_lifetime(longer, &mut output);
            encode_lifetime(shorter, &mut output);
        }
        SymbolicPredicate::TypeOutlives { ty, lifetime } => {
            output.push(3);
            nested_type(&mut output, ty)?;
            encode_lifetime(lifetime, &mut output);
        }
    }
    Ok(output)
}

pub fn encode_symbolic_predicate_set(
    predicates: &[SymbolicPredicate],
) -> Result<Vec<u8>, ShapeEncodingError> {
    let mut rows = predicates
        .iter()
        .map(encode_symbolic_predicate)
        .collect::<Result<Vec<_>, _>>()?;
    rows.sort();
    if rows.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ShapeEncodingError::DuplicatePredicate);
    }
    let mut output = Vec::new();
    count(&mut output, rows.len(), "symbolic predicate set")?;
    for row in rows {
        bytes(&mut output, &row, "symbolic predicate")?;
    }
    Ok(output)
}

pub(crate) fn encode_target_root(
    target: &TargetRoot,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    match target {
        TargetRoot::Library => output.push(1),
        TargetRoot::Binary(name) => {
            output.push(2);
            string(output, name, "binary target name")?;
        }
        TargetRoot::Environment(name) => {
            output.push(3);
            string(output, name, "environment target name")?;
        }
    }
    Ok(())
}

pub(crate) fn encode_declaration_path(
    path: &SemanticDeclarationPath,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    string(output, &path.registry_origin, "registry origin")?;
    string(output, &path.package_name, "package name")?;
    encode_target_root(&path.target, output)?;
    strings(output, &path.modules, "module path")?;
    output.push(path.kind.tag());
    string(output, &path.name, "declaration name")
}

fn encode_type_into(ty: &SymbolicType, output: &mut Vec<u8>) -> Result<(), ShapeEncodingError> {
    match ty {
        SymbolicType::I8 => output.push(1),
        SymbolicType::I16 => output.push(2),
        SymbolicType::I32 => output.push(3),
        SymbolicType::I64 => output.push(4),
        SymbolicType::U8 => output.push(5),
        SymbolicType::U16 => output.push(6),
        SymbolicType::U32 => output.push(7),
        SymbolicType::U64 => output.push(8),
        SymbolicType::Isize => output.push(9),
        SymbolicType::Usize => output.push(10),
        SymbolicType::F32 => output.push(11),
        SymbolicType::F64 => output.push(12),
        SymbolicType::Bool => output.push(13),
        SymbolicType::Char => output.push(14),
        SymbolicType::Entity => output.push(15),
        SymbolicType::Unit => output.push(16),
        SymbolicType::Never => output.push(17),
        SymbolicType::Str => output.push(18),
        SymbolicType::Slice(element) => {
            output.push(19);
            nested_type(output, element)?;
        }
        SymbolicType::Array { element, length } => {
            output.push(20);
            nested_type(output, element)?;
            let encoded = encode_symbolic_const(length)?;
            bytes(output, &encoded, "array length")?;
        }
        SymbolicType::Tuple(elements) => {
            output.push(21);
            types(output, elements, "tuple type")?;
        }
        SymbolicType::Reference {
            mutability,
            lifetime,
            pointee,
        } => {
            output.push(22);
            output.push(mutability.tag());
            encode_lifetime(lifetime, output);
            nested_type(output, pointee)?;
        }
        SymbolicType::RawPointer {
            mutability,
            pointee,
        } => {
            output.push(23);
            output.push(mutability.tag());
            nested_type(output, pointee)?;
        }
        SymbolicType::NominalPath {
            declaration,
            arguments,
        } => {
            output.push(29);
            encode_declaration_path(declaration, output)?;
            generic_arguments(output, arguments)?;
        }
        SymbolicType::FunctionPointer {
            unsafe_,
            parameters,
            result,
            requires,
            throws,
        } => {
            output.push(25);
            output.push(u8::from(*unsafe_));
            types(output, parameters, "function parameters")?;
            nested_type(output, result)?;
            nested_effect_set(output, requires, "requires set")?;
            nested_effect_set(output, throws, "throws set")?;
        }
        SymbolicType::BoundType { depth, index } => {
            output.push(26);
            output.extend_from_slice(&depth.to_le_bytes());
            output.extend_from_slice(&index.to_le_bytes());
        }
        SymbolicType::Closure {
            owner,
            expression_ordinal,
            captures,
            parameters,
            result,
            requires,
            throws,
            arguments,
        } => {
            nonzero_ordinal(*expression_ordinal)?;
            output.push(27);
            encode_declaration_path(owner, output)?;
            output.extend_from_slice(&expression_ordinal.to_le_bytes());
            encode_captures(captures, output)?;
            types(output, parameters, "closure parameters")?;
            nested_type(output, result)?;
            nested_effect_set(output, requires, "closure requires")?;
            nested_effect_set(output, throws, "closure throws")?;
            generic_arguments(output, arguments)?;
        }
        SymbolicType::Generator {
            target,
            captures,
            parameters,
            factory_unsafe,
            resume,
            yields,
            result,
            requires,
            throws,
        } => {
            validate_generator_shape(target, captures, *factory_unsafe)?;
            output.push(28);
            encode_generator_target(target, output)?;
            encode_captures(captures, output)?;
            types(output, parameters, "generator parameters")?;
            output.push(u8::from(*factory_unsafe));
            nested_type(output, resume)?;
            nested_type(output, yields)?;
            nested_type(output, result)?;
            nested_effect_set(output, requires, "generator requires")?;
            nested_effect_set(output, throws, "generator throws")?;
        }
        SymbolicType::JoinHandle { result, throws } => {
            output.push(30);
            nested_type(output, result)?;
            nested_effect_set(output, throws, "join handle throws")?;
        }
        SymbolicType::GeneratorFactory {
            target,
            captures,
            call_trait,
            parameters,
            factory_unsafe,
            produced_generator,
        } => {
            validate_generator_shape(target, captures, *factory_unsafe)?;
            output.push(31);
            encode_generator_target(target, output)?;
            encode_captures(captures, output)?;
            output.push(call_trait.tag());
            types(output, parameters, "generator factory parameters")?;
            output.push(u8::from(*factory_unsafe));
            nested_type(output, produced_generator)?;
        }
    }
    Ok(())
}

fn encode_const_into(
    expression: &SymbolicConstExpression,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    match &expression.node {
        SymbolicConstNode::IntegerLiteral(bits) => {
            if bits.len() != expression.integer_type.byte_width() {
                return Err(ShapeEncodingError::InvalidFixedWidthBits {
                    integer_type: expression.integer_type,
                    actual: bits.len(),
                });
            }
            output.push(1);
            output.push(expression.integer_type.tag());
            output.extend_from_slice(bits);
            Ok(())
        }
        SymbolicConstNode::Bound { depth, index } => {
            output.push(2);
            output.extend_from_slice(&depth.to_le_bytes());
            output.extend_from_slice(&index.to_le_bytes());
            Ok(())
        }
        _ => {
            output.push(3);
            let mut tree = Vec::new();
            encode_const_expression_node(expression, &mut tree)?;
            bytes(output, &tree, "hermetic const-expression tree")?;
            Ok(())
        }
    }
}

fn encode_const_expression_node(
    expression: &SymbolicConstExpression,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    match &expression.node {
        SymbolicConstNode::IntegerLiteral(bits) => {
            if bits.len() != expression.integer_type.byte_width() {
                return Err(ShapeEncodingError::InvalidFixedWidthBits {
                    integer_type: expression.integer_type,
                    actual: bits.len(),
                });
            }
            output.push(1);
            output.push(expression.integer_type.tag());
            output.extend_from_slice(bits);
            Ok(())
        }
        SymbolicConstNode::Bound { depth, index } => {
            output.push(2);
            output.push(expression.integer_type.tag());
            output.extend_from_slice(&depth.to_le_bytes());
            output.extend_from_slice(&index.to_le_bytes());
            Ok(())
        }
        SymbolicConstNode::ConstDefinitionPath(path) => {
            output.push(3);
            output.push(expression.integer_type.tag());
            encode_declaration_path(path, output)
        }
        SymbolicConstNode::WrappingNeg(child) => {
            encode_unary_const_node(4, expression.integer_type, child, output)
        }
        SymbolicConstNode::BitNot(child) => {
            encode_unary_const_node(5, expression.integer_type, child, output)
        }
        SymbolicConstNode::WrappingMul(left, right) => {
            encode_binary_const_node(6, expression.integer_type, left, right, output)
        }
        SymbolicConstNode::IntegerDivide(left, right) => {
            encode_binary_const_node(7, expression.integer_type, left, right, output)
        }
        SymbolicConstNode::IntegerRemainder(left, right) => {
            encode_binary_const_node(8, expression.integer_type, left, right, output)
        }
        SymbolicConstNode::WrappingAdd(left, right) => {
            encode_binary_const_node(9, expression.integer_type, left, right, output)
        }
        SymbolicConstNode::WrappingSub(left, right) => {
            encode_binary_const_node(10, expression.integer_type, left, right, output)
        }
        SymbolicConstNode::MaskedShiftLeft(left, right) => {
            encode_binary_const_node(11, expression.integer_type, left, right, output)
        }
        SymbolicConstNode::MaskedShiftRight(left, right) => {
            encode_binary_const_node(12, expression.integer_type, left, right, output)
        }
        SymbolicConstNode::BitAnd(left, right) => {
            encode_binary_const_node(13, expression.integer_type, left, right, output)
        }
        SymbolicConstNode::BitXor(left, right) => {
            encode_binary_const_node(14, expression.integer_type, left, right, output)
        }
        SymbolicConstNode::BitOr(left, right) => {
            encode_binary_const_node(15, expression.integer_type, left, right, output)
        }
    }
}

fn encode_unary_const_node(
    tag: u8,
    integer_type: IntegerType,
    child: &SymbolicConstExpression,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    output.push(tag);
    output.push(integer_type.tag());
    let mut encoded = Vec::new();
    encode_const_expression_node(child, &mut encoded)?;
    bytes(output, &encoded, "const-expression child")?;
    Ok(())
}

fn encode_binary_const_node(
    tag: u8,
    integer_type: IntegerType,
    left: &SymbolicConstExpression,
    right: &SymbolicConstExpression,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    output.push(tag);
    output.push(integer_type.tag());
    for child in [left, right] {
        let mut encoded = Vec::new();
        encode_const_expression_node(child, &mut encoded)?;
        bytes(output, &encoded, "const-expression child")?;
    }
    Ok(())
}

fn encode_lifetime(lifetime: &SymbolicLifetime, output: &mut Vec<u8>) {
    match lifetime {
        SymbolicLifetime::Static => output.push(1),
        SymbolicLifetime::Bound { depth, index } => {
            output.push(2);
            output.extend_from_slice(&depth.to_le_bytes());
            output.extend_from_slice(&index.to_le_bytes());
        }
        SymbolicLifetime::ErasedLocal => output.push(3),
    }
}

fn generic_arguments(
    output: &mut Vec<u8>,
    arguments: &[GenericArgumentShape],
) -> Result<(), ShapeEncodingError> {
    count(output, arguments.len(), "generic argument list")?;
    for argument in arguments {
        match argument {
            GenericArgumentShape::Type(ty) => {
                output.push(1);
                nested_type(output, ty)?;
            }
            GenericArgumentShape::Lifetime(lifetime) => {
                output.push(2);
                encode_lifetime(lifetime, output);
            }
            GenericArgumentShape::IntegerConst(expression) => {
                output.push(3);
                let encoded = encode_symbolic_const(expression)?;
                bytes(output, &encoded, "integer const argument")?;
            }
        }
    }
    Ok(())
}

fn encode_generator_target(
    target: &GeneratorTarget,
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    match target {
        GeneratorTarget::Named {
            declaration,
            arguments,
            hidden_lifetime_binders,
        } => {
            output.push(1);
            encode_declaration_path(declaration, output)?;
            generic_arguments(output, arguments)?;
            count(
                output,
                hidden_lifetime_binders.len(),
                "generator hidden lifetime binders",
            )?;
            for position in hidden_lifetime_binders {
                output.extend_from_slice(&position.to_le_bytes());
            }
        }
        GeneratorTarget::Anonymous {
            owner,
            expression_ordinal,
            arguments,
        } => {
            nonzero_ordinal(*expression_ordinal)?;
            output.push(2);
            encode_declaration_path(owner, output)?;
            output.extend_from_slice(&expression_ordinal.to_le_bytes());
            generic_arguments(output, arguments)?;
        }
    }
    Ok(())
}

fn encode_captures(
    captures: &[SymbolicCapture],
    output: &mut Vec<u8>,
) -> Result<(), ShapeEncodingError> {
    count(output, captures.len(), "capture list")?;
    for (index, capture) in captures.iter().enumerate() {
        let expected = u64::try_from(index)
            .map_err(|_| ShapeEncodingError::LengthOverflow("capture list"))?
            .checked_add(1)
            .ok_or(ShapeEncodingError::LengthOverflow("capture list"))?;
        if capture.ordinal != expected {
            return Err(ShapeEncodingError::NonCanonicalCaptureOrdinal {
                expected,
                actual: capture.ordinal,
            });
        }
        output.extend_from_slice(&capture.ordinal.to_le_bytes());
        output.push(capture.mode.tag());
        nested_type(output, &capture.ty)?;
    }
    Ok(())
}

fn validate_generator_shape(
    target: &GeneratorTarget,
    captures: &[SymbolicCapture],
    factory_unsafe: bool,
) -> Result<(), ShapeEncodingError> {
    match target {
        GeneratorTarget::Named { .. } if !captures.is_empty() => {
            Err(ShapeEncodingError::NamedGeneratorCaptures)
        }
        GeneratorTarget::Anonymous { .. } if factory_unsafe => {
            Err(ShapeEncodingError::AnonymousGeneratorUnsafe)
        }
        _ => Ok(()),
    }
}

fn nested_type(output: &mut Vec<u8>, ty: &SymbolicType) -> Result<(), ShapeEncodingError> {
    let encoded = encode_symbolic_type(ty)?;
    bytes(output, &encoded, "semantic type")
}

fn types(
    output: &mut Vec<u8>,
    values: &[SymbolicType],
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    count(output, values.len(), label)?;
    for value in values {
        nested_type(output, value)?;
    }
    Ok(())
}

fn type_set(
    output: &mut Vec<u8>,
    values: &[SymbolicType],
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    let mut encoded = values
        .iter()
        .map(encode_symbolic_type)
        .collect::<Result<Vec<_>, _>>()?;
    encoded.sort();
    if encoded.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ShapeEncodingError::DuplicateEffect);
    }
    count(output, encoded.len(), label)?;
    for value in encoded {
        bytes(output, &value, label)?;
    }
    Ok(())
}

fn nested_effect_set(
    output: &mut Vec<u8>,
    effects: &SymbolicTypeEffectSet,
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    let expected = symbolic_type_list_readiness(effects.members());
    if effects.readiness() != expected {
        if effects.readiness() == SymbolicShapeReadiness::PendingC4 {
            return Err(ShapeEncodingError::InvalidDeclarationShape(
                "semantic type effect set is pending C4 canonicalization",
            ));
        }
        return Err(ShapeEncodingError::InconsistentShapeReadiness);
    }
    if !expected.pre_result_ready() {
        return Err(ShapeEncodingError::InvalidDeclarationShape(
            "semantic type effect set contains a pending nested type",
        ));
    }
    type_set(output, effects.members(), label)
}

fn strings(
    output: &mut Vec<u8>,
    values: &[String],
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    count(output, values.len(), label)?;
    for value in values {
        string(output, value, label)?;
    }
    Ok(())
}

fn string(
    output: &mut Vec<u8>,
    value: &str,
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    bytes(output, value.as_bytes(), label)
}

fn bytes(
    output: &mut Vec<u8>,
    value: &[u8],
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    count(output, value.len(), label)?;
    output.extend_from_slice(value);
    Ok(())
}

fn count(
    output: &mut Vec<u8>,
    value: usize,
    label: &'static str,
) -> Result<(), ShapeEncodingError> {
    let value = u64::try_from(value).map_err(|_| ShapeEncodingError::LengthOverflow(label))?;
    output.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn nonzero_ordinal(value: u64) -> Result<(), ShapeEncodingError> {
    if value == 0 {
        Err(ShapeEncodingError::ZeroExpressionOrdinal)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declaration(name: &str) -> SemanticDeclarationPath {
        SemanticDeclarationPath {
            registry_origin: "registry+https://packages.arche-lang.org".to_owned(),
            package_name: "example/shapes".to_owned(),
            target: TargetRoot::Library,
            modules: vec!["model".to_owned()],
            kind: DeclarationKind::Struct,
            name: name.to_owned(),
        }
    }

    fn literal(value: u32) -> SymbolicConstExpression {
        SymbolicConstExpression {
            integer_type: IntegerType::U32,
            node: SymbolicConstNode::IntegerLiteral(value.to_le_bytes().to_vec()),
        }
    }

    fn type_leaf(ty: SymbolicType) -> SymbolicTypeShapeSkeleton {
        SymbolicTypeShapeSkeleton::resolved(ty)
    }

    fn declaration_shape(
        payload: SymbolicDeclarationPayloadSkeleton,
    ) -> SymbolicDeclarationShapeSkeleton {
        SymbolicDeclarationShapeSkeleton {
            generic_parameters: Vec::new(),
            predicates: Vec::new(),
            payload,
        }
    }

    fn canonical_declaration_bytes(shape: &SymbolicDeclarationShapeSkeleton) -> Vec<u8> {
        let canonical = try_canonicalize_declaration_shape(shape)
            .unwrap()
            .expect("test shape is pre-result ready");
        encode_declaration_shape_preimage(&canonical).unwrap()
    }

    fn manual_count(output: &mut Vec<u8>, value: usize) {
        output.extend_from_slice(&u64::try_from(value).unwrap().to_le_bytes());
    }

    fn manual_blob(output: &mut Vec<u8>, value: &[u8]) {
        manual_count(output, value.len());
        output.extend_from_slice(value);
    }

    fn manual_string(output: &mut Vec<u8>, value: &str) {
        manual_blob(output, value.as_bytes());
    }

    fn manual_empty_declaration_prefix() -> Vec<u8> {
        let mut output = Vec::new();
        manual_count(&mut output, 0);
        manual_count(&mut output, 0);
        output
    }

    #[test]
    fn scalar_and_nominal_path_tags_are_exact() {
        assert_eq!(encode_symbolic_type(&SymbolicType::I8).unwrap(), [1]);
        assert_eq!(encode_symbolic_type(&SymbolicType::I32).unwrap(), [3]);
        assert_eq!(encode_symbolic_type(&SymbolicType::Never).unwrap(), [17]);
        let nominal = encode_symbolic_type(&SymbolicType::NominalPath {
            declaration: declaration("Point"),
            arguments: Vec::new(),
        })
        .unwrap();
        assert_eq!(nominal[0], 29);
    }

    #[test]
    fn every_c1_symbolic_type_variant_has_its_frozen_tag() {
        let owner = declaration("Owner");
        let anonymous = GeneratorTarget::Anonymous {
            owner: owner.clone(),
            expression_ordinal: 1,
            arguments: Vec::new(),
        };
        let named = GeneratorTarget::Named {
            declaration: owner.clone(),
            arguments: Vec::new(),
            hidden_lifetime_binders: Vec::new(),
        };
        let cases = vec![
            (SymbolicType::I8, 1),
            (SymbolicType::I16, 2),
            (SymbolicType::I32, 3),
            (SymbolicType::I64, 4),
            (SymbolicType::U8, 5),
            (SymbolicType::U16, 6),
            (SymbolicType::U32, 7),
            (SymbolicType::U64, 8),
            (SymbolicType::Isize, 9),
            (SymbolicType::Usize, 10),
            (SymbolicType::F32, 11),
            (SymbolicType::F64, 12),
            (SymbolicType::Bool, 13),
            (SymbolicType::Char, 14),
            (SymbolicType::Entity, 15),
            (SymbolicType::Unit, 16),
            (SymbolicType::Never, 17),
            (SymbolicType::Str, 18),
            (SymbolicType::Slice(Box::new(SymbolicType::U8)), 19),
            (
                SymbolicType::Array {
                    element: Box::new(SymbolicType::U8),
                    length: literal(2),
                },
                20,
            ),
            (SymbolicType::Tuple(vec![SymbolicType::I32]), 21),
            (
                SymbolicType::Reference {
                    mutability: Mutability::Shared,
                    lifetime: SymbolicLifetime::Static,
                    pointee: Box::new(SymbolicType::I32),
                },
                22,
            ),
            (
                SymbolicType::RawPointer {
                    mutability: Mutability::Mutable,
                    pointee: Box::new(SymbolicType::I32),
                },
                23,
            ),
            (
                SymbolicType::FunctionPointer {
                    unsafe_: false,
                    parameters: Vec::new(),
                    result: Box::new(SymbolicType::Unit),
                    requires: SymbolicTypeEffectSet::default(),
                    throws: SymbolicTypeEffectSet::default(),
                },
                25,
            ),
            (SymbolicType::BoundType { depth: 0, index: 0 }, 26),
            (
                SymbolicType::Closure {
                    owner: Box::new(owner.clone()),
                    expression_ordinal: 1,
                    captures: Vec::new(),
                    parameters: Vec::new(),
                    result: Box::new(SymbolicType::Unit),
                    requires: SymbolicTypeEffectSet::default(),
                    throws: SymbolicTypeEffectSet::default(),
                    arguments: Vec::new(),
                },
                27,
            ),
            (
                SymbolicType::Generator {
                    target: Box::new(anonymous),
                    captures: Vec::new(),
                    parameters: Vec::new(),
                    factory_unsafe: false,
                    resume: Box::new(SymbolicType::Unit),
                    yields: Box::new(SymbolicType::Unit),
                    result: Box::new(SymbolicType::Unit),
                    requires: SymbolicTypeEffectSet::default(),
                    throws: SymbolicTypeEffectSet::default(),
                },
                28,
            ),
            (
                SymbolicType::NominalPath {
                    declaration: owner,
                    arguments: Vec::new(),
                },
                29,
            ),
            (
                SymbolicType::JoinHandle {
                    result: Box::new(SymbolicType::Unit),
                    throws: SymbolicTypeEffectSet::default(),
                },
                30,
            ),
            (
                SymbolicType::GeneratorFactory {
                    target: Box::new(named),
                    captures: Vec::new(),
                    call_trait: CallTrait::Fn,
                    parameters: Vec::new(),
                    factory_unsafe: false,
                    produced_generator: Box::new(SymbolicType::Unit),
                },
                31,
            ),
        ];
        for (ty, expected) in cases {
            assert_eq!(encode_symbolic_type(&ty).unwrap()[0], expected);
        }
    }

    #[test]
    fn const_operator_tags_and_contextual_width_are_exact() {
        let sum = SymbolicConstExpression {
            integer_type: IntegerType::U32,
            node: SymbolicConstNode::WrappingAdd(Box::new(literal(1)), Box::new(literal(2))),
        };
        let encoded = encode_symbolic_const(&sum).unwrap();
        assert_eq!(encoded[0], 3);
        let tree_length = u64::from_le_bytes(encoded[1..9].try_into().unwrap());
        assert_eq!(usize::try_from(tree_length).unwrap(), encoded.len() - 9);
        assert_eq!(&encoded[9..11], &[9, IntegerType::U32.tag()]);
        assert!(matches!(
            encode_symbolic_const(&SymbolicConstExpression {
                integer_type: IntegerType::U32,
                node: SymbolicConstNode::IntegerLiteral(vec![0; 3]),
            }),
            Err(ShapeEncodingError::InvalidFixedWidthBits { .. })
        ));
    }

    #[test]
    fn every_symbolic_const_node_has_its_frozen_tag() {
        let child = || Box::new(literal(1));
        let binary = |node| SymbolicConstExpression {
            integer_type: IntegerType::U32,
            node,
        };
        let cases = vec![
            (literal(1), 1),
            (binary(SymbolicConstNode::Bound { depth: 0, index: 0 }), 2),
            (
                binary(SymbolicConstNode::ConstDefinitionPath(declaration("N"))),
                3,
            ),
            (binary(SymbolicConstNode::WrappingNeg(child())), 4),
            (binary(SymbolicConstNode::BitNot(child())), 5),
            (binary(SymbolicConstNode::WrappingMul(child(), child())), 6),
            (
                binary(SymbolicConstNode::IntegerDivide(child(), child())),
                7,
            ),
            (
                binary(SymbolicConstNode::IntegerRemainder(child(), child())),
                8,
            ),
            (binary(SymbolicConstNode::WrappingAdd(child(), child())), 9),
            (binary(SymbolicConstNode::WrappingSub(child(), child())), 10),
            (
                binary(SymbolicConstNode::MaskedShiftLeft(child(), child())),
                11,
            ),
            (
                binary(SymbolicConstNode::MaskedShiftRight(child(), child())),
                12,
            ),
            (binary(SymbolicConstNode::BitAnd(child(), child())), 13),
            (binary(SymbolicConstNode::BitXor(child(), child())), 14),
            (binary(SymbolicConstNode::BitOr(child(), child())), 15),
        ];
        for (expression, expected) in cases {
            let encoded = encode_symbolic_const(&expression).unwrap();
            match expected {
                1 | 2 => assert_eq!(encoded[0], expected),
                _ => {
                    assert_eq!(encoded[0], 3);
                    assert_eq!(encoded[9], expected);
                }
            }
        }
    }

    #[test]
    fn outer_const_tree_tags_do_not_leak_inner_operator_tags() {
        let bound = SymbolicConstExpression {
            integer_type: IntegerType::U64,
            node: SymbolicConstNode::Bound { depth: 2, index: 3 },
        };
        assert_eq!(
            encode_symbolic_const(&bound).unwrap(),
            [
                vec![2],
                2_u64.to_le_bytes().to_vec(),
                3_u64.to_le_bytes().to_vec()
            ]
            .concat()
        );

        let path = SymbolicConstExpression {
            integer_type: IntegerType::U64,
            node: SymbolicConstNode::ConstDefinitionPath(declaration("N")),
        };
        let encoded = encode_symbolic_const(&path).unwrap();
        assert_eq!(encoded[0], 3);
        assert_eq!(&encoded[9..11], &[3, IntegerType::U64.tag()]);
    }

    #[test]
    fn effects_sort_by_complete_bytes_and_reject_duplicates() {
        let first = SymbolicEffectAtom {
            kind: EffectKind::Throws,
            ty: SymbolicType::I32,
        };
        let second = SymbolicEffectAtom {
            kind: EffectKind::Requires,
            ty: SymbolicType::NominalPath {
                declaration: declaration("Files"),
                arguments: Vec::new(),
            },
        };
        assert_eq!(
            encode_symbolic_effect_set(&[first.clone(), second.clone()]).unwrap(),
            encode_symbolic_effect_set(&[second, first.clone()]).unwrap()
        );
        assert_eq!(
            encode_symbolic_effect_set(&[first.clone(), first]),
            Err(ShapeEncodingError::DuplicateEffect)
        );
    }

    #[test]
    fn nested_effect_sets_preserve_c1_source_lists_until_c4_resolution() {
        let pending = SymbolicType::FunctionPointer {
            unsafe_: false,
            parameters: vec![SymbolicType::Usize],
            result: Box::new(SymbolicType::Unit),
            requires: SymbolicTypeEffectSet::pending_c4(vec![
                SymbolicType::U8,
                SymbolicType::I32,
                SymbolicType::U8,
            ]),
            throws: SymbolicTypeEffectSet::pending_c4(vec![SymbolicType::I64, SymbolicType::I8]),
        };
        assert_eq!(
            symbolic_type_readiness(&pending),
            SymbolicShapeReadiness::PendingC4
        );
        assert_eq!(
            encode_symbolic_type(&pending),
            Err(ShapeEncodingError::InvalidDeclarationShape(
                "semantic type effect set is pending C4 canonicalization"
            ))
        );

        let source = encode_symbolic_type_skeleton_c1(&pending).unwrap();
        let reordered = SymbolicType::FunctionPointer {
            unsafe_: false,
            parameters: vec![SymbolicType::Usize],
            result: Box::new(SymbolicType::Unit),
            requires: SymbolicTypeEffectSet::pending_c4(vec![
                SymbolicType::I32,
                SymbolicType::U8,
                SymbolicType::U8,
            ]),
            throws: SymbolicTypeEffectSet::pending_c4(vec![SymbolicType::I8, SymbolicType::I64]),
        };
        assert_ne!(
            source,
            encode_symbolic_type_skeleton_c1(&reordered).unwrap()
        );

        let owner = declaration("EffectOwner");
        let source_members = vec![SymbolicType::U8, SymbolicType::I32, SymbolicType::U8];
        let reordered_members = vec![SymbolicType::I32, SymbolicType::U8, SymbolicType::U8];
        let source_order_cases = vec![
            (
                SymbolicType::Closure {
                    owner: Box::new(owner.clone()),
                    expression_ordinal: 1,
                    captures: Vec::new(),
                    parameters: Vec::new(),
                    result: Box::new(SymbolicType::Unit),
                    requires: SymbolicTypeEffectSet::pending_c4(source_members.clone()),
                    throws: SymbolicTypeEffectSet::default(),
                    arguments: Vec::new(),
                },
                SymbolicType::Closure {
                    owner: Box::new(owner.clone()),
                    expression_ordinal: 1,
                    captures: Vec::new(),
                    parameters: Vec::new(),
                    result: Box::new(SymbolicType::Unit),
                    requires: SymbolicTypeEffectSet::pending_c4(reordered_members.clone()),
                    throws: SymbolicTypeEffectSet::default(),
                    arguments: Vec::new(),
                },
            ),
            (
                SymbolicType::Generator {
                    target: Box::new(GeneratorTarget::Named {
                        declaration: owner.clone(),
                        arguments: Vec::new(),
                        hidden_lifetime_binders: Vec::new(),
                    }),
                    captures: Vec::new(),
                    parameters: Vec::new(),
                    factory_unsafe: false,
                    resume: Box::new(SymbolicType::Unit),
                    yields: Box::new(SymbolicType::Unit),
                    result: Box::new(SymbolicType::Unit),
                    requires: SymbolicTypeEffectSet::pending_c4(source_members.clone()),
                    throws: SymbolicTypeEffectSet::default(),
                },
                SymbolicType::Generator {
                    target: Box::new(GeneratorTarget::Named {
                        declaration: owner,
                        arguments: Vec::new(),
                        hidden_lifetime_binders: Vec::new(),
                    }),
                    captures: Vec::new(),
                    parameters: Vec::new(),
                    factory_unsafe: false,
                    resume: Box::new(SymbolicType::Unit),
                    yields: Box::new(SymbolicType::Unit),
                    result: Box::new(SymbolicType::Unit),
                    requires: SymbolicTypeEffectSet::pending_c4(reordered_members.clone()),
                    throws: SymbolicTypeEffectSet::default(),
                },
            ),
            (
                SymbolicType::JoinHandle {
                    result: Box::new(SymbolicType::Unit),
                    throws: SymbolicTypeEffectSet::pending_c4(source_members),
                },
                SymbolicType::JoinHandle {
                    result: Box::new(SymbolicType::Unit),
                    throws: SymbolicTypeEffectSet::pending_c4(reordered_members),
                },
            ),
        ];
        for (pending, reordered) in source_order_cases {
            assert_eq!(
                symbolic_type_readiness(&pending),
                SymbolicShapeReadiness::PendingC4
            );
            assert_ne!(
                encode_symbolic_type_skeleton_c1(&pending).unwrap(),
                encode_symbolic_type_skeleton_c1(&reordered).unwrap()
            );
            assert!(encode_symbolic_type(&pending).is_err());
        }

        let canonical = |requires, throws| SymbolicType::FunctionPointer {
            unsafe_: false,
            parameters: vec![SymbolicType::Usize],
            result: Box::new(SymbolicType::Unit),
            requires: SymbolicTypeEffectSet::resolved(requires),
            throws: SymbolicTypeEffectSet::resolved(throws),
        };
        let first = canonical(
            vec![SymbolicType::I32, SymbolicType::U8],
            vec![SymbolicType::I64, SymbolicType::I8],
        );
        let second = canonical(
            vec![SymbolicType::U8, SymbolicType::I32],
            vec![SymbolicType::I8, SymbolicType::I64],
        );
        assert_eq!(
            symbolic_type_readiness(&first),
            SymbolicShapeReadiness::ConstIndependent
        );
        assert_eq!(
            encode_symbolic_type(&first).unwrap(),
            encode_symbolic_type(&second).unwrap()
        );
        let duplicate = canonical(vec![SymbolicType::U8, SymbolicType::U8], Vec::new());
        assert_eq!(
            encode_symbolic_type(&duplicate),
            Err(ShapeEncodingError::DuplicateEffect)
        );
        let duplicate_shape = declaration_shape(SymbolicDeclarationPayloadSkeleton::Alias {
            target: type_leaf(duplicate),
        });
        assert_eq!(
            try_canonicalize_declaration_shape(&duplicate_shape),
            Err(ShapeEncodingError::DuplicateEffect)
        );

        let nested = SymbolicType::JoinHandle {
            result: Box::new(SymbolicType::Tuple(vec![pending])),
            throws: SymbolicTypeEffectSet::default(),
        };
        assert_eq!(
            symbolic_type_readiness(&nested),
            SymbolicShapeReadiness::PendingC4
        );
        assert!(encode_symbolic_type_skeleton_c1(&nested).is_ok());
        assert!(encode_symbolic_type(&nested).is_err());
    }

    #[test]
    fn generic_parameter_kind_and_source_order_are_pinned() {
        let encoded = encode_generic_parameters(&[
            GenericParameterShape {
                index: 0,
                name: "T".to_owned(),
                kind: GenericParameterKind::Type,
            },
            GenericParameterShape {
                index: 1,
                name: "a".to_owned(),
                kind: GenericParameterKind::Lifetime,
            },
            GenericParameterShape {
                index: 2,
                name: "N".to_owned(),
                kind: GenericParameterKind::IntegerConst(IntegerType::Usize),
            },
        ])
        .unwrap();
        assert_eq!(u64::from_le_bytes(encoded[..8].try_into().unwrap()), 3);
        assert!(encoded.windows(2).any(|bytes| bytes == [3, 10]));
    }

    #[test]
    fn predicate_tags_are_exact_and_sets_are_canonical() {
        let trait_bound = SymbolicPredicate::Trait {
            trait_path: declaration("Trait"),
            self_type: SymbolicType::BoundType { depth: 0, index: 0 },
            arguments: Vec::new(),
        };
        let lifetime = SymbolicPredicate::LifetimeOutlives {
            longer: SymbolicLifetime::Static,
            shorter: SymbolicLifetime::Bound { depth: 0, index: 0 },
        };
        let type_outlives = SymbolicPredicate::TypeOutlives {
            ty: SymbolicType::I32,
            lifetime: SymbolicLifetime::Static,
        };
        assert_eq!(encode_symbolic_predicate(&trait_bound).unwrap()[0], 1);
        assert_eq!(encode_symbolic_predicate(&lifetime).unwrap()[0], 2);
        assert_eq!(encode_symbolic_predicate(&type_outlives).unwrap()[0], 3);
        assert_eq!(
            encode_symbolic_predicate_set(&[
                trait_bound.clone(),
                lifetime.clone(),
                type_outlives.clone(),
            ])
            .unwrap(),
            encode_symbolic_predicate_set(&[type_outlives, trait_bound.clone(), lifetime]).unwrap()
        );
        assert_eq!(
            encode_symbolic_predicate_set(&[trait_bound.clone(), trait_bound]),
            Err(ShapeEncodingError::DuplicatePredicate)
        );
    }

    #[test]
    fn anonymous_state_shapes_reject_provisional_zero_ordinals() {
        let ty = SymbolicType::GeneratorFactory {
            target: Box::new(GeneratorTarget::Anonymous {
                owner: declaration("Owner"),
                expression_ordinal: 0,
                arguments: Vec::new(),
            }),
            captures: Vec::new(),
            call_trait: CallTrait::FnOnce,
            parameters: Vec::new(),
            factory_unsafe: false,
            produced_generator: Box::new(SymbolicType::Unit),
        };
        assert_eq!(
            encode_symbolic_type(&ty),
            Err(ShapeEncodingError::ZeroExpressionOrdinal)
        );
    }

    #[test]
    fn generic_and_capture_ordinals_are_canonical() {
        assert_eq!(
            encode_generic_parameters(&[GenericParameterShape {
                index: 1,
                name: "T".to_owned(),
                kind: GenericParameterKind::Type,
            }]),
            Err(ShapeEncodingError::NonCanonicalGenericParameterIndex {
                expected: 0,
                actual: 1,
            })
        );

        let closure = SymbolicType::Closure {
            owner: Box::new(declaration("Owner")),
            expression_ordinal: 1,
            captures: vec![SymbolicCapture {
                ordinal: 0,
                mode: CaptureMode::Move,
                ty: SymbolicType::I32,
            }],
            parameters: Vec::new(),
            result: Box::new(SymbolicType::Unit),
            requires: SymbolicTypeEffectSet::default(),
            throws: SymbolicTypeEffectSet::default(),
            arguments: Vec::new(),
        };
        assert_eq!(
            encode_symbolic_type(&closure),
            Err(ShapeEncodingError::NonCanonicalCaptureOrdinal {
                expected: 1,
                actual: 0,
            })
        );
    }

    #[test]
    fn named_and_anonymous_generator_invariants_are_checked() {
        let named = SymbolicType::Generator {
            target: Box::new(GeneratorTarget::Named {
                declaration: declaration("Generator"),
                arguments: Vec::new(),
                hidden_lifetime_binders: Vec::new(),
            }),
            captures: vec![SymbolicCapture {
                ordinal: 1,
                mode: CaptureMode::Move,
                ty: SymbolicType::I32,
            }],
            parameters: Vec::new(),
            factory_unsafe: false,
            resume: Box::new(SymbolicType::Unit),
            yields: Box::new(SymbolicType::Unit),
            result: Box::new(SymbolicType::Unit),
            requires: SymbolicTypeEffectSet::default(),
            throws: SymbolicTypeEffectSet::default(),
        };
        assert_eq!(
            encode_symbolic_type(&named),
            Err(ShapeEncodingError::NamedGeneratorCaptures)
        );

        let anonymous = SymbolicType::GeneratorFactory {
            target: Box::new(GeneratorTarget::Anonymous {
                owner: declaration("Owner"),
                expression_ordinal: 1,
                arguments: Vec::new(),
            }),
            captures: Vec::new(),
            call_trait: CallTrait::FnOnce,
            parameters: Vec::new(),
            factory_unsafe: true,
            produced_generator: Box::new(SymbolicType::Unit),
        };
        assert_eq!(
            encode_symbolic_type(&anonymous),
            Err(ShapeEncodingError::AnonymousGeneratorUnsafe)
        );
    }

    #[test]
    fn declaration_record_forms_match_manual_interleaved_vectors() {
        assert_eq!(
            canonical_declaration_bytes(&declaration_shape(
                SymbolicDeclarationPayloadSkeleton::World
            )),
            manual_empty_declaration_prefix()
        );
        assert_eq!(
            canonical_declaration_bytes(&declaration_shape(
                SymbolicDeclarationPayloadSkeleton::Tag
            )),
            manual_empty_declaration_prefix()
        );

        let unit = declaration_shape(SymbolicDeclarationPayloadSkeleton::Record(
            SymbolicRecordShapeSkeleton {
                form: SymbolicRecordForm::Unit,
                fields: Vec::new(),
            },
        ));
        let mut unit_expected = manual_empty_declaration_prefix();
        unit_expected.push(1);
        manual_count(&mut unit_expected, 0);
        assert_eq!(canonical_declaration_bytes(&unit), unit_expected);

        let tuple = declaration_shape(SymbolicDeclarationPayloadSkeleton::Record(
            SymbolicRecordShapeSkeleton {
                form: SymbolicRecordForm::Tuple,
                fields: vec![
                    SymbolicFieldShapeSkeleton {
                        name: None,
                        ty: type_leaf(SymbolicType::I32),
                    },
                    SymbolicFieldShapeSkeleton {
                        name: None,
                        ty: type_leaf(SymbolicType::U8),
                    },
                ],
            },
        ));
        let mut tuple_expected = manual_empty_declaration_prefix();
        tuple_expected.push(2);
        manual_count(&mut tuple_expected, 2);
        manual_blob(&mut tuple_expected, &[3]);
        manual_blob(&mut tuple_expected, &[5]);
        assert_eq!(canonical_declaration_bytes(&tuple), tuple_expected);

        let record = declaration_shape(SymbolicDeclarationPayloadSkeleton::Record(
            SymbolicRecordShapeSkeleton {
                form: SymbolicRecordForm::Record,
                fields: vec![
                    SymbolicFieldShapeSkeleton {
                        name: Some("left".to_owned()),
                        ty: type_leaf(SymbolicType::I32),
                    },
                    SymbolicFieldShapeSkeleton {
                        name: Some("right".to_owned()),
                        ty: type_leaf(SymbolicType::U8),
                    },
                ],
            },
        ));
        let mut record_expected = manual_empty_declaration_prefix();
        record_expected.push(3);
        manual_count(&mut record_expected, 2);
        manual_string(&mut record_expected, "left");
        manual_blob(&mut record_expected, &[3]);
        manual_string(&mut record_expected, "right");
        manual_blob(&mut record_expected, &[5]);
        assert_eq!(canonical_declaration_bytes(&record), record_expected);

        let enumeration = declaration_shape(SymbolicDeclarationPayloadSkeleton::Enum(vec![
            SymbolicVariantShapeSkeleton {
                name: "Unit".to_owned(),
                form: SymbolicRecordForm::Unit,
                fields: Vec::new(),
            },
            SymbolicVariantShapeSkeleton {
                name: "Tuple".to_owned(),
                form: SymbolicRecordForm::Tuple,
                fields: vec![SymbolicFieldShapeSkeleton {
                    name: None,
                    ty: type_leaf(SymbolicType::I32),
                }],
            },
            SymbolicVariantShapeSkeleton {
                name: "Record".to_owned(),
                form: SymbolicRecordForm::Record,
                fields: vec![SymbolicFieldShapeSkeleton {
                    name: Some("value".to_owned()),
                    ty: type_leaf(SymbolicType::U8),
                }],
            },
        ]));
        let mut enum_expected = manual_empty_declaration_prefix();
        manual_count(&mut enum_expected, 3);
        manual_string(&mut enum_expected, "Unit");
        enum_expected.push(1);
        manual_count(&mut enum_expected, 0);
        manual_string(&mut enum_expected, "Tuple");
        enum_expected.push(2);
        manual_count(&mut enum_expected, 1);
        manual_blob(&mut enum_expected, &[3]);
        manual_string(&mut enum_expected, "Record");
        enum_expected.push(3);
        manual_count(&mut enum_expected, 1);
        manual_string(&mut enum_expected, "value");
        manual_blob(&mut enum_expected, &[5]);
        assert_eq!(canonical_declaration_bytes(&enumeration), enum_expected);
    }

    #[test]
    fn callable_modes_and_effect_sets_match_manual_vector() {
        let receiver = SymbolicType::Reference {
            mutability: Mutability::Shared,
            lifetime: SymbolicLifetime::Static,
            pointee: Box::new(SymbolicType::I32),
        };
        let shape = SymbolicDeclarationShapeSkeleton {
            generic_parameters: vec![
                GenericParameterKind::Type,
                GenericParameterKind::Lifetime,
                GenericParameterKind::IntegerConst(IntegerType::U8),
            ],
            predicates: Vec::new(),
            payload: SymbolicDeclarationPayloadSkeleton::Callable(Box::new(
                SymbolicCallableShapeSkeleton {
                    kind: SymbolicCallableKind::Function,
                    parameters: vec![
                        SymbolicCallableParameterSkeleton {
                            mode: SymbolicCallableParameterMode::Value,
                            ty: type_leaf(SymbolicType::I32),
                        },
                        SymbolicCallableParameterSkeleton {
                            mode: SymbolicCallableParameterMode::ReceiverShared,
                            ty: type_leaf(receiver.clone()),
                        },
                    ],
                    result: type_leaf(SymbolicType::Unit),
                    unsafe_: true,
                    resume: None,
                    yields: None,
                    effects: SymbolicEffectSetsSkeleton {
                        requires: vec![SymbolicEffectShapeSkeleton::resolved(SymbolicType::U8)],
                        throws: vec![SymbolicEffectShapeSkeleton::resolved(SymbolicType::I32)],
                    },
                },
            )),
        };
        let mut expected = Vec::new();
        manual_count(&mut expected, 3);
        expected.extend_from_slice(&[1, 2, 3, IntegerType::U8.tag()]);
        manual_count(&mut expected, 0);
        manual_count(&mut expected, 2);
        expected.push(1);
        manual_blob(&mut expected, &[3]);
        expected.push(3);
        manual_blob(&mut expected, &encode_symbolic_type(&receiver).unwrap());
        manual_blob(&mut expected, &[16]);
        expected.push(1);
        manual_count(&mut expected, 1);
        manual_blob(&mut expected, &[5]);
        manual_count(&mut expected, 1);
        manual_blob(&mut expected, &[3]);
        assert_eq!(canonical_declaration_bytes(&shape), expected);

        let mut receiver_value = shape.clone();
        let SymbolicDeclarationPayloadSkeleton::Callable(callable) = &mut receiver_value.payload
        else {
            unreachable!();
        };
        callable.parameters[0].mode = SymbolicCallableParameterMode::ReceiverValue;
        assert_ne!(
            canonical_declaration_bytes(&shape),
            canonical_declaration_bytes(&receiver_value),
            "ordinary value and by-value receiver mode tags are distinct"
        );
    }

    #[test]
    fn generator_declaration_callable_matches_manual_vector_and_function_differs() {
        let shape = SymbolicDeclarationShapeSkeleton {
            generic_parameters: vec![GenericParameterKind::Lifetime],
            predicates: Vec::new(),
            payload: SymbolicDeclarationPayloadSkeleton::Callable(Box::new(
                SymbolicCallableShapeSkeleton {
                    kind: SymbolicCallableKind::Generator,
                    parameters: vec![SymbolicCallableParameterSkeleton {
                        mode: SymbolicCallableParameterMode::Value,
                        ty: type_leaf(SymbolicType::I32),
                    }],
                    result: type_leaf(SymbolicType::Bool),
                    unsafe_: true,
                    resume: Some(type_leaf(SymbolicType::U8)),
                    yields: Some(type_leaf(SymbolicType::I32)),
                    effects: SymbolicEffectSetsSkeleton {
                        requires: vec![SymbolicEffectShapeSkeleton::resolved(SymbolicType::U8)],
                        throws: vec![SymbolicEffectShapeSkeleton::resolved(SymbolicType::I32)],
                    },
                },
            )),
        };
        let mut expected = Vec::new();
        manual_count(&mut expected, 1);
        expected.push(2);
        manual_count(&mut expected, 0);
        manual_count(&mut expected, 1);
        expected.push(1);
        manual_blob(&mut expected, &[3]);
        manual_blob(&mut expected, &[13]);
        expected.push(1);
        manual_blob(&mut expected, &[5]);
        manual_blob(&mut expected, &[3]);
        manual_count(&mut expected, 1);
        manual_blob(&mut expected, &[5]);
        manual_count(&mut expected, 1);
        manual_blob(&mut expected, &[3]);
        assert_eq!(canonical_declaration_bytes(&shape), expected);

        let mut function = shape.clone();
        let SymbolicDeclarationPayloadSkeleton::Callable(callable) = &mut function.payload else {
            unreachable!();
        };
        callable.kind = SymbolicCallableKind::Function;
        callable.resume = None;
        callable.yields = None;
        assert_ne!(
            canonical_declaration_bytes(&shape),
            canonical_declaration_bytes(&function)
        );

        let mut missing_yield = shape;
        let SymbolicDeclarationPayloadSkeleton::Callable(callable) = &mut missing_yield.payload
        else {
            unreachable!();
        };
        callable.yields = None;
        assert_eq!(
            try_canonicalize_declaration_shape(&missing_yield),
            Err(ShapeEncodingError::InvalidDeclarationShape(
                "generator callable requires resume and yield types"
            ))
        );
    }

    #[test]
    fn system_access_rows_match_manual_six_tag_vector() {
        let shared = SymbolicType::Reference {
            mutability: Mutability::Shared,
            lifetime: SymbolicLifetime::Static,
            pointee: Box::new(SymbolicType::I32),
        };
        let mutable = SymbolicType::Reference {
            mutability: Mutability::Mutable,
            lifetime: SymbolicLifetime::Static,
            pointee: Box::new(SymbolicType::I32),
        };
        let query = vec![
            SymbolicQueryTermShapeSkeleton {
                kind: SymbolicQueryTermKind::Read,
                ty: type_leaf(SymbolicType::I32),
            },
            SymbolicQueryTermShapeSkeleton {
                kind: SymbolicQueryTermKind::Write,
                ty: type_leaf(SymbolicType::U8),
            },
            SymbolicQueryTermShapeSkeleton {
                kind: SymbolicQueryTermKind::Exclude,
                ty: type_leaf(SymbolicType::Bool),
            },
        ];
        let shape = declaration_shape(SymbolicDeclarationPayloadSkeleton::System {
            accesses: vec![
                SymbolicSystemAccessShapeSkeleton::CapabilityShared(type_leaf(shared.clone())),
                SymbolicSystemAccessShapeSkeleton::CapabilityMutable(type_leaf(mutable.clone())),
                SymbolicSystemAccessShapeSkeleton::ResourceRead(type_leaf(SymbolicType::I32)),
                SymbolicSystemAccessShapeSkeleton::ResourceWrite(type_leaf(SymbolicType::U8)),
                SymbolicSystemAccessShapeSkeleton::Query(query),
                SymbolicSystemAccessShapeSkeleton::Commands,
            ],
            implied_requires: Vec::new(),
            result: type_leaf(SymbolicType::Unit),
            effects: SymbolicEffectSetsSkeleton::default(),
        });
        let mut expected = manual_empty_declaration_prefix();
        manual_count(&mut expected, 6);
        expected.push(1);
        manual_blob(&mut expected, &encode_symbolic_type(&shared).unwrap());
        expected.push(2);
        manual_blob(&mut expected, &encode_symbolic_type(&mutable).unwrap());
        expected.push(3);
        manual_blob(&mut expected, &[3]);
        expected.push(4);
        manual_blob(&mut expected, &[5]);
        expected.push(5);
        manual_count(&mut expected, 3);
        for (tag, ty) in [(1, 3), (2, 5), (3, 13)] {
            expected.push(tag);
            manual_blob(&mut expected, &[ty]);
        }
        expected.push(6);
        manual_blob(&mut expected, &[16]);
        expected.push(0);
        manual_count(&mut expected, 0);
        manual_count(&mut expected, 0);
        assert_eq!(canonical_declaration_bytes(&shape), expected);
    }

    #[test]
    fn implied_capability_rows_block_preimage_and_are_debug_only_provenance() {
        let span = SymbolicSourceSpan {
            file: 7,
            start_byte: 11,
            end_byte: 19,
            start_line: 2,
            start_column: 3,
            end_line: 2,
            end_column: 11,
        };
        let shape = declaration_shape(SymbolicDeclarationPayloadSkeleton::System {
            accesses: vec![SymbolicSystemAccessShapeSkeleton::CapabilityShared(
                type_leaf(SymbolicType::Reference {
                    mutability: Mutability::Shared,
                    lifetime: SymbolicLifetime::Static,
                    pointee: Box::new(SymbolicType::U8),
                }),
            )],
            implied_requires: vec![SymbolicImpliedCapabilityRequirementSkeleton {
                parameter_ordinal: 0,
                parameter_span: span,
                access: SymbolicCapabilityAccessMode::Shared,
                referent: type_leaf(SymbolicType::U8),
                readiness: SymbolicShapeReadiness::PendingC4,
            }],
            result: type_leaf(SymbolicType::Unit),
            effects: SymbolicEffectSetsSkeleton::default(),
        });
        assert_eq!(
            declaration_shape_readiness(&shape).unwrap(),
            SymbolicShapeReadiness::PendingC4
        );
        assert!(try_canonicalize_declaration_shape(&shape)
            .unwrap()
            .is_none());

        let debug = encode_symbolic_declaration_shape_skeleton_c1(&shape).unwrap();
        let mut changed = shape.clone();
        {
            let SymbolicDeclarationPayloadSkeleton::System {
                implied_requires, ..
            } = &mut changed.payload
            else {
                unreachable!();
            };
            implied_requires[0].access = SymbolicCapabilityAccessMode::Mutable;
        }
        assert_ne!(
            encode_symbolic_declaration_shape_skeleton_c1(&changed).unwrap(),
            debug
        );

        let SymbolicDeclarationPayloadSkeleton::System {
            implied_requires, ..
        } = &mut changed.payload
        else {
            unreachable!();
        };
        implied_requires[0].readiness = SymbolicShapeReadiness::ConstIndependent;
        assert_eq!(
            try_canonicalize_declaration_shape(&changed),
            Err(ShapeEncodingError::InconsistentShapeReadiness)
        );
    }

    #[test]
    fn trait_and_impl_entries_reuse_exact_child_callable_shape() {
        let child = declaration_shape(SymbolicDeclarationPayloadSkeleton::Callable(Box::new(
            SymbolicCallableShapeSkeleton {
                kind: SymbolicCallableKind::Function,
                parameters: vec![SymbolicCallableParameterSkeleton {
                    mode: SymbolicCallableParameterMode::ReceiverValue,
                    ty: type_leaf(SymbolicType::I32),
                }],
                result: type_leaf(SymbolicType::Unit),
                unsafe_: false,
                resume: None,
                yields: None,
                effects: SymbolicEffectSetsSkeleton::default(),
            },
        )));
        let child_bytes = canonical_declaration_bytes(&child);
        let method = SymbolicMethodShapeSkeleton {
            name: "run".to_owned(),
            shape: Box::new(child),
        };
        let mut entry = Vec::new();
        manual_string(&mut entry, "run");
        manual_blob(&mut entry, &child_bytes);
        assert_eq!(encode_method_entry(&method).unwrap(), entry);

        let trait_shape = declaration_shape(SymbolicDeclarationPayloadSkeleton::Trait {
            methods: vec![method.clone()],
        });
        let mut trait_expected = manual_empty_declaration_prefix();
        manual_count(&mut trait_expected, 1);
        trait_expected.extend_from_slice(&entry);
        assert_eq!(canonical_declaration_bytes(&trait_shape), trait_expected);

        let implementation = declaration_shape(SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref: None,
            target: type_leaf(SymbolicType::I32),
            is_default: false,
            methods: vec![method],
        });
        let mut impl_expected = manual_empty_declaration_prefix();
        impl_expected.push(0);
        manual_blob(&mut impl_expected, &[3]);
        impl_expected.push(0);
        manual_count(&mut impl_expected, 1);
        impl_expected.extend_from_slice(&entry);
        assert_eq!(canonical_declaration_bytes(&implementation), impl_expected);
    }

    #[test]
    fn ready_wrapper_rejects_duplicate_structural_members() {
        let field = |name: &str| SymbolicFieldShapeSkeleton {
            name: Some(name.to_owned()),
            ty: type_leaf(SymbolicType::I32),
        };
        let duplicate_fields = declaration_shape(SymbolicDeclarationPayloadSkeleton::Record(
            SymbolicRecordShapeSkeleton {
                form: SymbolicRecordForm::Record,
                fields: vec![field("same"), field("same")],
            },
        ));
        assert_eq!(
            try_canonicalize_declaration_shape(&duplicate_fields),
            Err(ShapeEncodingError::InvalidDeclarationShape(
                "record declaration repeats a field name"
            ))
        );

        let duplicate_variants = declaration_shape(SymbolicDeclarationPayloadSkeleton::Enum(vec![
            SymbolicVariantShapeSkeleton {
                name: "Same".to_owned(),
                form: SymbolicRecordForm::Unit,
                fields: Vec::new(),
            },
            SymbolicVariantShapeSkeleton {
                name: "Same".to_owned(),
                form: SymbolicRecordForm::Unit,
                fields: Vec::new(),
            },
        ]));
        assert_eq!(
            try_canonicalize_declaration_shape(&duplicate_variants),
            Err(ShapeEncodingError::InvalidDeclarationShape(
                "enum declaration repeats a variant name"
            ))
        );

        let duplicate_variant_fields =
            declaration_shape(SymbolicDeclarationPayloadSkeleton::Enum(vec![
                SymbolicVariantShapeSkeleton {
                    name: "Record".to_owned(),
                    form: SymbolicRecordForm::Record,
                    fields: vec![field("same"), field("same")],
                },
            ]));
        assert_eq!(
            try_canonicalize_declaration_shape(&duplicate_variant_fields),
            Err(ShapeEncodingError::InvalidDeclarationShape(
                "record declaration repeats a field name"
            ))
        );

        let callable = || {
            declaration_shape(SymbolicDeclarationPayloadSkeleton::Callable(Box::new(
                SymbolicCallableShapeSkeleton {
                    kind: SymbolicCallableKind::Function,
                    parameters: Vec::new(),
                    result: type_leaf(SymbolicType::Unit),
                    unsafe_: false,
                    resume: None,
                    yields: None,
                    effects: SymbolicEffectSetsSkeleton::default(),
                },
            )))
        };
        let duplicate_methods = declaration_shape(SymbolicDeclarationPayloadSkeleton::Trait {
            methods: vec![
                SymbolicMethodShapeSkeleton {
                    name: "same".to_owned(),
                    shape: Box::new(callable()),
                },
                SymbolicMethodShapeSkeleton {
                    name: "same".to_owned(),
                    shape: Box::new(callable()),
                },
            ],
        });
        assert_eq!(
            try_canonicalize_declaration_shape(&duplicate_methods),
            Err(ShapeEncodingError::InvalidDeclarationShape(
                "trait or impl declaration repeats a method name"
            ))
        );
    }

    #[test]
    fn pending_and_ctfe_readiness_fail_closed_at_the_correct_gate() {
        let pending = declaration_shape(SymbolicDeclarationPayloadSkeleton::Alias {
            target: SymbolicTypeShapeSkeleton::pending(
                SymbolicShapeReadiness::PendingC2,
                SymbolicSourceSpan {
                    file: 1,
                    start_byte: 2,
                    end_byte: 3,
                    start_line: 1,
                    start_column: 3,
                    end_line: 1,
                    end_column: 4,
                },
                PendingShapeKind::PathUse,
                "Self::Item",
            ),
        });
        assert_eq!(
            declaration_shape_readiness(&pending).unwrap(),
            SymbolicShapeReadiness::PendingC2
        );
        assert!(try_canonicalize_declaration_shape(&pending)
            .unwrap()
            .is_none());

        let schedule = declaration_shape(SymbolicDeclarationPayloadSkeleton::Schedule {
            effects: SymbolicEffectSetsSkeleton::default(),
            readiness: SymbolicShapeReadiness::PendingC4,
        });
        assert!(try_canonicalize_declaration_shape(&schedule)
            .unwrap()
            .is_none());

        let width = SymbolicConstExpression {
            integer_type: IntegerType::Usize,
            node: SymbolicConstNode::ConstDefinitionPath(declaration("WIDTH")),
        };
        let needs_ctfe_target = type_leaf(SymbolicType::Array {
            element: Box::new(SymbolicType::U8),
            length: width,
        });
        let needs_ctfe = declaration_shape(SymbolicDeclarationPayloadSkeleton::Alias {
            target: needs_ctfe_target.clone(),
        });
        let canonical = try_canonicalize_declaration_shape(&needs_ctfe)
            .unwrap()
            .expect("const-definition path is a valid pre-result tree");
        assert_eq!(canonical.readiness(), SymbolicShapeReadiness::NeedsCtfe);
        assert!(encode_declaration_shape_preimage(&canonical).is_ok());
        assert_eq!(
            encode_final_declaration_shape_identity(&canonical),
            Err(ShapeEncodingError::FinalIdentityNeedsCtfe)
        );

        let owner = SymbolicDefinitionOwnerSkeleton::InherentImpl {
            target: needs_ctfe_target,
            generic_parameters: Vec::new(),
            predicates: Vec::new(),
        };
        let owner = try_canonicalize_definition_owner(&owner)
            .unwrap()
            .expect("const-dependent owner is pre-result ready");
        assert!(encode_definition_owner_entry(&owner).is_ok());
        assert_eq!(
            encode_final_definition_owner_identity(&owner),
            Err(ShapeEncodingError::FinalIdentityNeedsCtfe)
        );
    }

    #[test]
    fn declaration_predicate_and_effect_sets_sort_and_reject_duplicates() {
        let first = SymbolicPredicateShapeSkeleton::resolved(SymbolicPredicate::LifetimeOutlives {
            longer: SymbolicLifetime::Static,
            shorter: SymbolicLifetime::Bound { depth: 0, index: 0 },
        });
        let second = SymbolicPredicateShapeSkeleton::resolved(SymbolicPredicate::TypeOutlives {
            ty: SymbolicType::I32,
            lifetime: SymbolicLifetime::Static,
        });
        let shape = |predicates| SymbolicDeclarationShapeSkeleton {
            generic_parameters: vec![GenericParameterKind::Lifetime],
            predicates,
            payload: SymbolicDeclarationPayloadSkeleton::World,
        };
        assert_eq!(
            canonical_declaration_bytes(&shape(vec![first.clone(), second.clone()])),
            canonical_declaration_bytes(&shape(vec![second.clone(), first.clone()]))
        );
        assert_eq!(
            try_canonicalize_declaration_shape(&shape(vec![first.clone(), first])),
            Err(ShapeEncodingError::DuplicatePredicate)
        );

        let effect_shape = |requires| {
            declaration_shape(SymbolicDeclarationPayloadSkeleton::Callable(Box::new(
                SymbolicCallableShapeSkeleton {
                    kind: SymbolicCallableKind::Function,
                    parameters: Vec::new(),
                    result: type_leaf(SymbolicType::Unit),
                    unsafe_: false,
                    resume: None,
                    yields: None,
                    effects: SymbolicEffectSetsSkeleton {
                        requires,
                        throws: Vec::new(),
                    },
                },
            )))
        };
        let i32_effect = SymbolicEffectShapeSkeleton::resolved(SymbolicType::I32);
        let u8_effect = SymbolicEffectShapeSkeleton::resolved(SymbolicType::U8);
        assert_eq!(
            canonical_declaration_bytes(&effect_shape(vec![i32_effect.clone(), u8_effect.clone()])),
            canonical_declaration_bytes(&effect_shape(vec![u8_effect, i32_effect.clone()]))
        );
        assert_eq!(
            try_canonicalize_declaration_shape(&effect_shape(vec![i32_effect.clone(), i32_effect])),
            Err(ShapeEncodingError::DuplicateEffect)
        );
    }

    #[test]
    fn c1_effect_projection_retains_pending_duplicates_and_source_order() {
        let effect_shape = |requires| {
            declaration_shape(SymbolicDeclarationPayloadSkeleton::Callable(Box::new(
                SymbolicCallableShapeSkeleton {
                    kind: SymbolicCallableKind::Function,
                    parameters: Vec::new(),
                    result: type_leaf(SymbolicType::Unit),
                    unsafe_: false,
                    resume: None,
                    yields: None,
                    effects: SymbolicEffectSetsSkeleton {
                        requires,
                        throws: Vec::new(),
                    },
                },
            )))
        };
        let i32_effect = SymbolicEffectShapeSkeleton::resolved_pending_c4(SymbolicType::I32);
        let u8_effect = SymbolicEffectShapeSkeleton::resolved_pending_c4(SymbolicType::U8);
        let source = effect_shape(vec![
            i32_effect.clone(),
            u8_effect.clone(),
            i32_effect.clone(),
        ]);
        let reordered = effect_shape(vec![u8_effect, i32_effect.clone(), i32_effect]);

        assert_eq!(
            declaration_shape_readiness(&source).unwrap(),
            SymbolicShapeReadiness::PendingC4
        );
        assert!(try_canonicalize_declaration_shape(&source)
            .unwrap()
            .is_none());
        assert_ne!(
            encode_symbolic_declaration_shape_skeleton_c1(&source).unwrap(),
            encode_symbolic_declaration_shape_skeleton_c1(&reordered).unwrap()
        );
    }
}
