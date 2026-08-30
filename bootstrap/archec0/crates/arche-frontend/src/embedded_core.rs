//! Branded C1 authority for the compiler-owned virtual `arche/core` interface.
//!
//! The authority in this module is intentionally narrower than the
//! `VerifiedGenericCore` brand specified for M27-C5. C1 has no Generic Core
//! schema or verifier yet, so it cannot truthfully manufacture that later
//! brand. It does, however, need a non-forgeable, immutable authority for name
//! resolution and semantic-inventory construction. The release projection
//! below therefore commits the fixed package identity, synthetic source, every
//! virtual definition/type/variant/constructible-record/trait/method/prelude
//! row, and the complete symbolic `panic` body that C5 must lower and verify.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr as _;
use std::sync::{Arc, OnceLock};

use arche_foundation::identity::{InterfaceHash, PackageId};
use arche_package::{canonical_package_id, PackageName, OFFICIAL_REGISTRY_IDENTITY};
use sha2::{Digest as _, Sha256};

use crate::{FileId, SourcePosition, Span, EMBEDDED_CORE_FILE_ID};

/// The selected embedded interface release.
pub const EMBEDDED_CORE_INTERFACE_VERSION: u32 = 1;
/// The embedded package is always the official registry identity.
pub const EMBEDDED_CORE_SCOPED_NAME: &str = "arche/core";
/// The selected package version recorded by the compiler release manifest.
pub const EMBEDDED_CORE_PACKAGE_VERSION: &str = "0.1.0";
/// Package-relative diagnostic path of the hostless synthetic snapshot.
pub const EMBEDDED_CORE_PACKAGE_PATH: &str = "src/lib.arc";

const RELEASE_PROJECTION_FORMAT_VERSION: u32 = 3;
const RELEASE_PACKAGE_ID: [u8; 16] = [
    0xf2, 0x0c, 0x23, 0xd1, 0x72, 0x7e, 0xaa, 0xa1, 0x78, 0xdb, 0x3e, 0xee, 0x1d, 0x54, 0x61, 0x0e,
];
const RELEASE_SOURCE_DIGEST: [u8; 32] = [
    0xd7, 0xaf, 0x6e, 0x7c, 0x0c, 0x08, 0x14, 0x82, 0x41, 0x48, 0xba, 0xb3, 0xc9, 0x06, 0x89, 0x4d,
    0x59, 0x10, 0xfa, 0xde, 0x5a, 0x6c, 0xe6, 0x2f, 0x60, 0x55, 0x0f, 0xf0, 0xde, 0xd4, 0xed, 0xc7,
];
const RELEASE_INTERFACE_DIGEST: [u8; 32] = [
    0xa9, 0x6f, 0x3c, 0xfb, 0x62, 0x60, 0xa4, 0x17, 0x9d, 0xbb, 0xb2, 0x04, 0x3a, 0xff, 0xa1, 0xe0,
    0x3d, 0x9c, 0x5b, 0x81, 0xd9, 0x04, 0x5e, 0x4c, 0xc4, 0xcb, 0x3d, 0x26, 0xb6, 0xda, 0xd8, 0x8a,
];
const RELEASE_INTERFACE_HASH: [u8; 16] = [
    0x95, 0xfa, 0x41, 0x30, 0x4e, 0xff, 0x64, 0x70, 0xa1, 0x16, 0xe4, 0x09, 0x7d, 0x7b, 0xe2, 0x2d,
];

/// Namespace used by a virtual prelude binding.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum VirtualNamespace {
    Type = 1,
    Value = 2,
}

/// A sealed row key. There is deliberately no public raw constructor.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VirtualDefinitionId(u16);

impl VirtualDefinitionId {
    pub const fn ordinal(self) -> u16 {
        self.0
    }
}

/// A sealed method row key. There is deliberately no public raw constructor.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VirtualMethodId(u16);

impl VirtualMethodId {
    pub const fn ordinal(self) -> u16 {
        self.0
    }
}

/// A sealed embedded enum-variant row key. There is deliberately no public raw
/// constructor; only the verified release projection can create one.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VirtualEnumVariantId(u16);

impl VirtualEnumVariantId {
    pub const fn ordinal(self) -> u16 {
        self.0
    }
}

/// A sealed semantic type-tree constructor that deliberately has no nominal
/// definition identity. `JoinHandle` is the sole 0.1 row and lowers as tag 30.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VirtualSemanticTypeId(u16);

impl VirtualSemanticTypeId {
    pub const fn ordinal(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum VirtualDefinitionKind {
    PrimitiveType = 1,
    NominalType = 2,
    CapabilityType = 3,
    OpaqueType = 4,
    Trait = 5,
    Function = 6,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum VirtualDeclarationKind {
    Primitive = 1,
    Struct = 2,
    Enum = 3,
    Trait = 4,
    Function = 5,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum VirtualTypeFlavor {
    Primitive = 1,
    Transparent = 2,
    Managed = 3,
    Capability = 4,
    Opaque = 5,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum UserImplPolicy {
    AllowedAndValidated = 1,
    CompilerDerivedOnly = 2,
    Forbidden = 3,
}

/// Semantic identity of a compiler-known trait.
///
/// Unlike [`VirtualDefinitionId`], this identity is independent of the C1
/// release projection's row order and is therefore suitable for C2 semantic
/// decisions.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CompilerTraitKind {
    Add,
    BitAnd,
    BitNot,
    BitOr,
    BitXor,
    Clone,
    Copy,
    Div,
    Drop,
    EcsKey,
    EcsValue,
    Eq,
    Fn,
    FnMut,
    FnOnce,
    From,
    IntoIterator,
    Iterator,
    LogicalNot,
    Mul,
    Neg,
    Ord,
    Rem,
    Send,
    ShiftLeft,
    ShiftRight,
    Sub,
    Sync,
    TryFrom,
    Unpin,
    UnwindPayload,
}

/// Semantic identity of an embedded nominal type. These values, rather than
/// virtual row ordinals, are the C2 identity bridge for compiler-owned
/// nominals.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CompilerNominalKind {
    AllocError,
    App,
    Arc,
    ArcWeak,
    AtomicRmw,
    Box,
    Caps,
    ChannelClosed,
    Commands,
    GeneratorState,
    IoError,
    Map,
    MapIter,
    MaybeUninit,
    OpenOptions,
    Option,
    Ordering,
    Pin,
    ProcessError,
    ProcessOutput,
    ProcessSpec,
    Query,
    Rc,
    RcWeak,
    Result,
    SocketAddress,
    String,
    ThreadError,
    Vec,
}

/// A coordinate in the compiler trait's explicit type-generic list. The raw
/// coordinate has no public constructor; coordinates are obtained only from a
/// verified typed authority.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompilerTraitGenericParameter(u8);

impl CompilerTraitGenericParameter {
    pub const fn index(self) -> u8 {
        self.0
    }
}

/// The required equality between implicit `Self` and the compiler trait's
/// semantic operands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerTraitSelfRelation {
    OperatedType,
    CallableType,
    Target(CompilerTraitGenericParameter),
    LeftHandSide(CompilerTraitGenericParameter),
    Input(CompilerTraitGenericParameter),
    Source(CompilerTraitGenericParameter),
    Iterator(CompilerTraitGenericParameter),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerTraitReceiverMode {
    None,
    Value,
    Shared,
    Mutable,
}

/// Semantic identity of the single method supplied by a compiler-known trait.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CompilerTraitMethodKind {
    Add,
    BitAnd,
    BitNot,
    BitOr,
    BitXor,
    Clone,
    Div,
    Drop,
    Eq,
    FnCall,
    FnMutCall,
    FnOnceCall,
    From,
    IntoIterator,
    IteratorNext,
    LogicalNot,
    Mul,
    Neg,
    OrdCompare,
    Rem,
    ShiftLeft,
    ShiftRight,
    Sub,
    TryFrom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerPrimitiveTypePattern {
    Never,
    Unit,
    Bool,
    Char,
    Entity,
    F32,
    F64,
    I8,
    I16,
    I32,
    I64,
    Isize,
    Str,
    U8,
    U16,
    U32,
    U64,
    Usize,
}

/// A type pattern in a compiler-trait callable. All nominal and generic
/// references are typed; no consumer needs to interpret the C1 signature
/// string.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerTraitTypePattern {
    SelfType,
    ExplicitGeneric(CompilerTraitGenericParameter),
    SharedReference(Box<CompilerTraitTypePattern>),
    MutableReference(Box<CompilerTraitTypePattern>),
    Primitive(CompilerPrimitiveTypePattern),
    Nominal {
        kind: CompilerNominalKind,
        arguments: Box<[CompilerTraitTypePattern]>,
    },
}

/// Complete callable shape required by the compiler-known trait method.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerTraitCallablePattern {
    Fixed {
        parameters: Box<[CompilerTraitTypePattern]>,
        result: CompilerTraitTypePattern,
    },
    /// `Fn*::call` adopts the parameter/result/effect shape encoded by its
    /// `Signature` generic argument exactly.
    ExactSignatureAndEffects {
        signature: CompilerTraitGenericParameter,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerTraitEffectPattern {
    Empty,
    ExactSignature,
}

/// Coordinate in one intrinsic nominal method's declared generic list.
/// Coordinates are obtained only from the verified typed authority.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompilerMethodGenericParameter(u8);

impl CompilerMethodGenericParameter {
    pub const fn index(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerMethodGenericParameterKind {
    Type,
    Lifetime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerMethodGenericBoundPattern {
    CompilerTrait(CompilerTraitKind),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerMethodGenericParameterAuthority {
    coordinate: CompilerMethodGenericParameter,
    source_name: String,
    kind: CompilerMethodGenericParameterKind,
    bounds: Box<[CompilerMethodGenericBoundPattern]>,
}

impl CompilerMethodGenericParameterAuthority {
    pub const fn coordinate(&self) -> CompilerMethodGenericParameter {
        self.coordinate
    }

    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    pub const fn kind(&self) -> CompilerMethodGenericParameterKind {
        self.kind
    }

    pub fn bounds(&self) -> &[CompilerMethodGenericBoundPattern] {
        &self.bounds
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerMethodLifetimePattern {
    Elided,
    Generic(CompilerMethodGenericParameter),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerMethodGenericArgumentPattern {
    Type(CompilerMethodTypePattern),
    Lifetime(CompilerMethodGenericParameter),
}

/// Closed type-pattern algebra for intrinsic methods owned by embedded
/// nominals. Definition leaves are verified C1 bridge IDs, never invented
/// stable semantic identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerMethodTypePattern {
    Generic(CompilerMethodGenericParameter),
    Definition {
        definition: VirtualDefinitionId,
        arguments: Box<[CompilerMethodGenericArgumentPattern]>,
    },
    SharedReference {
        lifetime: CompilerMethodLifetimePattern,
        referent: Box<CompilerMethodTypePattern>,
    },
    MutableReference {
        lifetime: CompilerMethodLifetimePattern,
        referent: Box<CompilerMethodTypePattern>,
    },
    Slice(Box<CompilerMethodTypePattern>),
    Tuple(Box<[CompilerMethodTypePattern]>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerNominalMethodReceiverMode {
    None,
    Value,
    Shared,
    Mutable,
}

/// Coordinate in the compile-time selector list following `;` in an
/// intrinsic signature.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompilerMethodSelector(u8);

impl CompilerMethodSelector {
    pub const fn index(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerMethodSelectorKind {
    DefinitionId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerMethodSelectorAuthority {
    coordinate: CompilerMethodSelector,
    source_name: String,
    kind: CompilerMethodSelectorKind,
}

impl CompilerMethodSelectorAuthority {
    pub const fn coordinate(&self) -> CompilerMethodSelector {
        self.coordinate
    }

    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    pub const fn kind(&self) -> CompilerMethodSelectorKind {
        self.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerNominalMethodEffectPattern {
    Drop(CompilerMethodTypePattern),
    Selector(CompilerMethodSelector),
}

/// Verified typed authority for one non-trait method owned by a compiler-known
/// embedded nominal.
#[derive(Debug, Eq, PartialEq)]
pub struct CompilerNominalMethodAuthority {
    owner: CompilerNominalKind,
    c1_method: VirtualMethodId,
    source_name: String,
    stable_name: String,
    lowering: VirtualMethodLowering,
    is_unsafe: bool,
    generics: Box<[CompilerMethodGenericParameterAuthority]>,
    receiver: CompilerNominalMethodReceiverMode,
    receiver_type: Option<CompilerMethodTypePattern>,
    parameters: Box<[CompilerMethodTypePattern]>,
    selectors: Box<[CompilerMethodSelectorAuthority]>,
    result: CompilerMethodTypePattern,
    requires: Box<[CompilerNominalMethodEffectPattern]>,
    throws: Box<[CompilerNominalMethodEffectPattern]>,
}

impl CompilerNominalMethodAuthority {
    pub const fn owner(&self) -> CompilerNominalKind {
        self.owner
    }

    pub const fn c1_method(&self) -> VirtualMethodId {
        self.c1_method
    }

    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    pub fn stable_name(&self) -> &str {
        &self.stable_name
    }

    pub const fn lowering(&self) -> VirtualMethodLowering {
        self.lowering
    }

    pub const fn is_unsafe(&self) -> bool {
        self.is_unsafe
    }

    pub fn generics(&self) -> &[CompilerMethodGenericParameterAuthority] {
        &self.generics
    }

    pub const fn receiver(&self) -> CompilerNominalMethodReceiverMode {
        self.receiver
    }

    /// Complete source-level receiver type. This includes the outer reference
    /// for borrowed receivers and the owner instantiation for value receivers.
    pub const fn receiver_type(&self) -> Option<&CompilerMethodTypePattern> {
        self.receiver_type.as_ref()
    }

    /// Runtime parameters after the receiver, if any, has been removed.
    pub fn parameters(&self) -> &[CompilerMethodTypePattern] {
        &self.parameters
    }

    pub fn selectors(&self) -> &[CompilerMethodSelectorAuthority] {
        &self.selectors
    }

    pub const fn result(&self) -> &CompilerMethodTypePattern {
        &self.result
    }

    pub fn requires(&self) -> &[CompilerNominalMethodEffectPattern] {
        &self.requires
    }

    pub fn throws(&self) -> &[CompilerNominalMethodEffectPattern] {
        &self.throws
    }
}

/// Verified typed authority for one embedded nominal. `definition` is exposed
/// only as a bridge back to C1 HIR; [`CompilerNominalKind`] is its semantic
/// identity.
#[derive(Debug)]
pub struct CompilerNominalAuthority {
    kind: CompilerNominalKind,
    definition: VirtualDefinitionId,
    flavor: VirtualTypeFlavor,
    declaration_kind: VirtualDeclarationKind,
}

impl CompilerNominalAuthority {
    pub const fn kind(&self) -> CompilerNominalKind {
        self.kind
    }

    pub const fn c1_definition(&self) -> VirtualDefinitionId {
        self.definition
    }

    pub const fn flavor(&self) -> VirtualTypeFlavor {
        self.flavor
    }

    pub const fn declaration_kind(&self) -> VirtualDeclarationKind {
        self.declaration_kind
    }
}

/// Verified typed authority for one compiler-trait method.
#[derive(Debug)]
pub struct CompilerTraitMethodAuthority {
    kind: CompilerTraitMethodKind,
    c1_method: VirtualMethodId,
    source_name: String,
    receiver: CompilerTraitReceiverMode,
    callable: CompilerTraitCallablePattern,
    effects: CompilerTraitEffectPattern,
}

impl CompilerTraitMethodAuthority {
    pub const fn kind(&self) -> CompilerTraitMethodKind {
        self.kind
    }

    pub const fn c1_method(&self) -> VirtualMethodId {
        self.c1_method
    }

    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    pub const fn receiver(&self) -> CompilerTraitReceiverMode {
        self.receiver
    }

    pub const fn callable(&self) -> &CompilerTraitCallablePattern {
        &self.callable
    }

    pub const fn effects(&self) -> CompilerTraitEffectPattern {
        self.effects
    }
}

/// Verified typed authority for one compiler-known trait.
#[derive(Debug)]
pub struct CompilerTraitAuthority {
    kind: CompilerTraitKind,
    c1_definition: VirtualDefinitionId,
    explicit_generic_arity: u8,
    designated_self: CompilerTraitSelfRelation,
    user_impl_policy: UserImplPolicy,
    method: Option<CompilerTraitMethodAuthority>,
}

impl CompilerTraitAuthority {
    pub const fn kind(&self) -> CompilerTraitKind {
        self.kind
    }

    pub const fn c1_definition(&self) -> VirtualDefinitionId {
        self.c1_definition
    }

    pub const fn explicit_generic_arity(&self) -> u8 {
        self.explicit_generic_arity
    }

    pub const fn designated_self(&self) -> CompilerTraitSelfRelation {
        self.designated_self
    }

    pub const fn user_impl_policy(&self) -> UserImplPolicy {
        self.user_impl_policy
    }

    pub const fn method(&self) -> Option<&CompilerTraitMethodAuthority> {
        self.method.as_ref()
    }
}

/// C2-only typed projection derived and checked at the branded Embedded Core
/// construction boundary. It deliberately does not participate in the frozen
/// C1 canonical byte stream.
#[derive(Debug)]
pub struct EmbeddedCoreC2TypedProjection {
    compiler_traits: Box<[CompilerTraitAuthority]>,
    primitive_definitions: Box<[(VirtualDefinitionId, CompilerPrimitiveTypePattern)]>,
    nominals: Box<[CompilerNominalAuthority]>,
    nominal_methods: Box<[CompilerNominalMethodAuthority]>,
    _seal: private::TypedSeal,
}

impl EmbeddedCoreC2TypedProjection {
    pub fn compiler_traits(&self) -> &[CompilerTraitAuthority] {
        &self.compiler_traits
    }

    pub fn nominals(&self) -> &[CompilerNominalAuthority] {
        &self.nominals
    }

    pub fn nominal_methods(&self) -> &[CompilerNominalMethodAuthority] {
        &self.nominal_methods
    }

    pub fn compiler_trait(&self, kind: CompilerTraitKind) -> &CompilerTraitAuthority {
        self.compiler_traits
            .iter()
            .find(|row| row.kind == kind)
            .expect("verified typed Core contains every compiler trait")
    }

    pub fn compiler_trait_for_c1_definition(
        &self,
        definition: VirtualDefinitionId,
    ) -> Option<&CompilerTraitAuthority> {
        self.compiler_traits
            .iter()
            .find(|row| row.c1_definition == definition)
    }

    /// Returns the verified primitive semantic kind for one C1 definition
    /// bridge ID. Consumers never interpret the C1 row name or ordinal.
    pub fn primitive_for_c1_definition(
        &self,
        definition: VirtualDefinitionId,
    ) -> Option<CompilerPrimitiveTypePattern> {
        self.primitive_definitions
            .iter()
            .find_map(|&(candidate, primitive)| (candidate == definition).then_some(primitive))
    }

    pub fn compiler_trait_method(
        &self,
        kind: CompilerTraitMethodKind,
    ) -> Option<&CompilerTraitMethodAuthority> {
        self.compiler_traits
            .iter()
            .filter_map(|row| row.method.as_ref())
            .find(|method| method.kind == kind)
    }

    pub fn compiler_trait_method_for_c1_method(
        &self,
        method: VirtualMethodId,
    ) -> Option<&CompilerTraitMethodAuthority> {
        self.compiler_traits
            .iter()
            .filter_map(|row| row.method.as_ref())
            .find(|row| row.c1_method == method)
    }

    pub fn nominal(&self, kind: CompilerNominalKind) -> &CompilerNominalAuthority {
        self.nominals
            .iter()
            .find(|row| row.kind == kind)
            .expect("verified typed Core contains every embedded nominal")
    }

    pub fn nominal_for_c1_definition(
        &self,
        definition: VirtualDefinitionId,
    ) -> Option<&CompilerNominalAuthority> {
        self.nominals
            .iter()
            .find(|row| row.definition == definition)
    }

    pub fn nominal_method_for_c1_method(
        &self,
        method: VirtualMethodId,
    ) -> Option<&CompilerNominalMethodAuthority> {
        self.nominal_methods
            .binary_search_by_key(&method, |row| row.c1_method)
            .ok()
            .map(|index| &self.nominal_methods[index])
    }

    pub fn nominal_method(
        &self,
        owner: CompilerNominalKind,
        source_name: &str,
    ) -> Option<&CompilerNominalMethodAuthority> {
        let mut rows = self
            .nominal_methods
            .iter()
            .filter(|row| row.owner == owner && row.source_name == source_name);
        let row = rows.next()?;
        rows.next().is_none().then_some(row)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum IntrinsicCtfeSupport {
    Hermetic = 1,
    IncludeAuthority = 2,
    NotExecutable = 3,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum CompilerOperation {
    PinNewChecked = 1,
    PinNewUnchecked = 2,
    MaybeUninitUninit = 3,
    MaybeUninitNew = 4,
    MaybeUninitAssumeInit = 5,
    RawOffset = 6,
    RawWithAddress = 7,
    RawExposeAddress = 8,
    CapsTake = 9,
    PointerCast = 10,
}

const COMPILER_OPERATIONS: &[CompilerOperation] = &[
    CompilerOperation::PinNewChecked,
    CompilerOperation::PinNewUnchecked,
    CompilerOperation::MaybeUninitUninit,
    CompilerOperation::MaybeUninitNew,
    CompilerOperation::MaybeUninitAssumeInit,
    CompilerOperation::RawOffset,
    CompilerOperation::RawWithAddress,
    CompilerOperation::RawExposeAddress,
    CompilerOperation::CapsTake,
    CompilerOperation::PointerCast,
];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum VirtualMethodLowering {
    TraitDispatch,
    Intrinsic { id: u16, ctfe: IntrinsicCtfeSupport },
    CompilerOperation(CompilerOperation),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum VirtualFunctionLowering {
    Intrinsic { id: u16, ctfe: IntrinsicCtfeSupport },
    CompilerOwnedBody,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualDefinitionRow {
    id: VirtualDefinitionId,
    namespace: VirtualNamespace,
    kind: VirtualDefinitionKind,
    declaration_kind: VirtualDeclarationKind,
    name: String,
    semantic_shape: String,
    span: Span,
}

impl VirtualDefinitionRow {
    pub const fn id(&self) -> VirtualDefinitionId {
        self.id
    }

    pub const fn namespace(&self) -> VirtualNamespace {
        self.namespace
    }

    pub const fn kind(&self) -> VirtualDefinitionKind {
        self.kind
    }

    pub const fn declaration_kind(&self) -> VirtualDeclarationKind {
        self.declaration_kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn semantic_shape(&self) -> &str {
        &self.semantic_shape
    }

    pub const fn span(&self) -> Span {
        self.span
    }
}

/// Exact value-namespace authority for a constructible embedded enum variant.
///
/// `ordinal` is the variant's declaration ordinal within `owner`; `id` is the
/// canonical release-row key ordered by `(owner, ordinal)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualEnumVariantRow {
    id: VirtualEnumVariantId,
    owner: VirtualDefinitionId,
    ordinal: u64,
    name: String,
    span: Span,
}

impl VirtualEnumVariantRow {
    pub const fn id(&self) -> VirtualEnumVariantId {
        self.id
    }

    pub const fn owner(&self) -> VirtualDefinitionId {
        self.owner
    }

    pub const fn ordinal(&self) -> u64 {
        self.ordinal
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn span(&self) -> Span {
        self.span
    }
}

/// Exact value-namespace authority for one of the six constructible embedded
/// record types. Constructor identity is the owning definition identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualRecordConstructorRow {
    owner: VirtualDefinitionId,
}

impl VirtualRecordConstructorRow {
    pub const fn owner(&self) -> VirtualDefinitionId {
        self.owner
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualTypeRow {
    definition: VirtualDefinitionId,
    flavor: VirtualTypeFlavor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualSemanticTypeRow {
    id: VirtualSemanticTypeId,
    spelling: String,
    semantic_tag: u8,
    shape: String,
    span: Span,
}

impl VirtualSemanticTypeRow {
    pub const fn id(&self) -> VirtualSemanticTypeId {
        self.id
    }

    pub fn spelling(&self) -> &str {
        &self.spelling
    }

    pub const fn semantic_tag(&self) -> u8 {
        self.semantic_tag
    }

    pub fn shape(&self) -> &str {
        &self.shape
    }

    pub const fn span(&self) -> Span {
        self.span
    }
}

impl VirtualTypeRow {
    pub const fn definition(&self) -> VirtualDefinitionId {
        self.definition
    }

    pub const fn flavor(&self) -> VirtualTypeFlavor {
        self.flavor
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualTraitRow {
    definition: VirtualDefinitionId,
    methods: Box<[VirtualMethodId]>,
    user_impl_policy: UserImplPolicy,
}

impl VirtualTraitRow {
    pub const fn definition(&self) -> VirtualDefinitionId {
        self.definition
    }

    pub fn methods(&self) -> &[VirtualMethodId] {
        &self.methods
    }

    pub const fn user_impl_policy(&self) -> UserImplPolicy {
        self.user_impl_policy
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualMethodRow {
    id: VirtualMethodId,
    owner: Option<VirtualDefinitionId>,
    source_name: String,
    stable_name: String,
    signature: String,
    requires: String,
    throws: String,
    lowering: VirtualMethodLowering,
    span: Span,
}

impl VirtualMethodRow {
    pub const fn id(&self) -> VirtualMethodId {
        self.id
    }

    pub const fn owner(&self) -> Option<VirtualDefinitionId> {
        self.owner
    }

    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    pub fn stable_name(&self) -> &str {
        &self.stable_name
    }

    pub fn signature(&self) -> &str {
        &self.signature
    }

    pub fn requires(&self) -> &str {
        &self.requires
    }

    pub fn throws(&self) -> &str {
        &self.throws
    }

    pub const fn lowering(&self) -> VirtualMethodLowering {
        self.lowering
    }

    pub const fn span(&self) -> Span {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualFunctionRow {
    definition: VirtualDefinitionId,
    lowering: VirtualFunctionLowering,
}

impl VirtualFunctionRow {
    pub const fn definition(&self) -> VirtualDefinitionId {
        self.definition
    }

    pub const fn lowering(&self) -> VirtualFunctionLowering {
        self.lowering
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum VirtualPreludeTarget {
    Definition(VirtualDefinitionId),
    SemanticType(VirtualSemanticTypeId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualPreludeBindingRow {
    spelling: String,
    namespace: VirtualNamespace,
    target: VirtualPreludeTarget,
}

impl VirtualPreludeBindingRow {
    pub fn spelling(&self) -> &str {
        &self.spelling
    }

    pub const fn namespace(&self) -> VirtualNamespace {
        self.namespace
    }

    pub const fn target(&self) -> VirtualPreludeTarget {
        self.target
    }
}

/// Immutable hostless source retained by every embedded-core projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddedCoreSyntheticSource {
    package_path: String,
    bytes: Arc<[u8]>,
    digest: [u8; 32],
}

/// The embedded package's sole module is a public library root. It is not a
/// manifest target row and its path is always empty.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddedCoreRootModuleRow {
    package: PackageId,
    file: FileId,
    path: Box<[String]>,
}

impl EmbeddedCoreRootModuleRow {
    pub const fn package(&self) -> PackageId {
        self.package
    }

    pub const fn file(&self) -> FileId {
        self.file
    }

    pub fn path(&self) -> &[String] {
        &self.path
    }

    pub const fn is_public_library_root(&self) -> bool {
        true
    }
}

impl EmbeddedCoreSyntheticSource {
    pub const fn file_id(&self) -> FileId {
        EMBEDDED_CORE_FILE_ID
    }

    pub fn package_path(&self) -> &str {
        &self.package_path
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    pub fn len(&self) -> u64 {
        u64::try_from(self.bytes.len()).expect("verified embedded source length fits u64")
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanicTerminal {
    User,
}

/// C1's complete symbolic authority for the compiler-owned `panic` body.
///
/// This row is deliberately not called `GenericCoreBody`: C5 must lower it and
/// pass the Generic Core verifier before any `VerifiedGenericCore` value exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerOwnedPanicBodyAuthority {
    owner: VirtualDefinitionId,
    generic_parameter_shape: String,
    parameter_type: String,
    result_type: String,
    declared_requires: Box<[VirtualDefinitionId]>,
    declared_throws: Box<[VirtualDefinitionId]>,
    terminal: PanicTerminal,
    required_ctfe_failure: String,
    symbolic_body_bytes: Arc<[u8]>,
    span: Span,
}

impl CompilerOwnedPanicBodyAuthority {
    pub const fn owner(&self) -> VirtualDefinitionId {
        self.owner
    }

    pub fn generic_parameter_shape(&self) -> &str {
        &self.generic_parameter_shape
    }

    pub fn parameter_type(&self) -> &str {
        &self.parameter_type
    }

    pub fn result_type(&self) -> &str {
        &self.result_type
    }

    pub fn declared_requires(&self) -> &[VirtualDefinitionId] {
        &self.declared_requires
    }

    pub fn declared_throws(&self) -> &[VirtualDefinitionId] {
        &self.declared_throws
    }

    pub const fn terminal(&self) -> PanicTerminal {
        self.terminal
    }

    pub fn required_ctfe_failure(&self) -> &str {
        &self.required_ctfe_failure
    }

    pub fn symbolic_body_bytes(&self) -> &[u8] {
        &self.symbolic_body_bytes
    }

    pub const fn span(&self) -> Span {
        self.span
    }
}

/// Exact C1 release-manifest projection consumed by HIR and inventory building.
///
/// `canonical_bytes` are a C1 projection, not a serialized or branded Generic
/// Core package. C5 must reproduce these rows in its full package and verify the
/// later canonical printer before Generic Core branding.
#[derive(Clone, Debug)]
pub struct EmbeddedCoreC1PackageProjection {
    package: PackageId,
    registry_origin: String,
    scoped_name: String,
    version: String,
    interface_version: u32,
    interface_digest: [u8; 32],
    public_interface_hash: InterfaceHash,
    source: EmbeddedCoreSyntheticSource,
    root_module: EmbeddedCoreRootModuleRow,
    definitions: Box<[VirtualDefinitionRow]>,
    types: Box<[VirtualTypeRow]>,
    enum_variants: Box<[VirtualEnumVariantRow]>,
    record_constructors: Box<[VirtualRecordConstructorRow]>,
    semantic_types: Box<[VirtualSemanticTypeRow]>,
    traits: Box<[VirtualTraitRow]>,
    methods: Box<[VirtualMethodRow]>,
    functions: Box<[VirtualFunctionRow]>,
    prelude: Box<[VirtualPreludeBindingRow]>,
    panic_body: CompilerOwnedPanicBodyAuthority,
    canonical_bytes: Arc<[u8]>,
}

impl EmbeddedCoreC1PackageProjection {
    pub const fn package(&self) -> PackageId {
        self.package
    }

    pub fn registry_origin(&self) -> &str {
        &self.registry_origin
    }

    pub fn scoped_name(&self) -> &str {
        &self.scoped_name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub const fn interface_version(&self) -> u32 {
        self.interface_version
    }

    pub const fn interface_digest(&self) -> &[u8; 32] {
        &self.interface_digest
    }

    pub const fn public_interface_hash(&self) -> InterfaceHash {
        self.public_interface_hash
    }

    pub const fn source(&self) -> &EmbeddedCoreSyntheticSource {
        &self.source
    }

    pub const fn root_module(&self) -> &EmbeddedCoreRootModuleRow {
        &self.root_module
    }

    pub const fn ctfe_budgets(&self) -> (u64, u64, u64) {
        (0, 0, 0)
    }

    pub const fn dependency_count(&self) -> usize {
        0
    }

    pub const fn target_count(&self) -> usize {
        0
    }

    pub const fn has_initializer_root(&self) -> bool {
        false
    }

    pub fn definitions(&self) -> &[VirtualDefinitionRow] {
        &self.definitions
    }

    pub fn types(&self) -> &[VirtualTypeRow] {
        &self.types
    }

    pub fn enum_variants(&self) -> &[VirtualEnumVariantRow] {
        &self.enum_variants
    }

    pub fn record_constructors(&self) -> &[VirtualRecordConstructorRow] {
        &self.record_constructors
    }

    pub fn semantic_types(&self) -> &[VirtualSemanticTypeRow] {
        &self.semantic_types
    }

    pub fn traits(&self) -> &[VirtualTraitRow] {
        &self.traits
    }

    pub fn methods(&self) -> &[VirtualMethodRow] {
        &self.methods
    }

    pub fn functions(&self) -> &[VirtualFunctionRow] {
        &self.functions
    }

    pub fn prelude(&self) -> &[VirtualPreludeBindingRow] {
        &self.prelude
    }

    pub const fn panic_body(&self) -> &CompilerOwnedPanicBodyAuthority {
        &self.panic_body
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }
}

/// The sole branded embedded-core authority accepted by later frontend stages.
#[derive(Debug)]
pub struct VerifiedEmbeddedCoreAuthority {
    projection: EmbeddedCoreC1PackageProjection,
    typed_c2: EmbeddedCoreC2TypedProjection,
    _seal: private::Seal,
}

impl VerifiedEmbeddedCoreAuthority {
    pub const fn interface_version(&self) -> u32 {
        self.projection.interface_version
    }

    pub const fn interface_digest(&self) -> &[u8; 32] {
        &self.projection.interface_digest
    }

    pub const fn package_id(&self) -> PackageId {
        self.projection.package
    }

    pub const fn projection(&self) -> &EmbeddedCoreC1PackageProjection {
        &self.projection
    }

    /// Returns the strictly validated C2 semantic view. Its trait and nominal
    /// enums, not C1 virtual row ordinals, are semantic identities.
    pub const fn typed_c2(&self) -> &EmbeddedCoreC2TypedProjection {
        &self.typed_c2
    }

    pub fn compiler_trait(&self, kind: CompilerTraitKind) -> &CompilerTraitAuthority {
        self.typed_c2.compiler_trait(kind)
    }

    /// Resolves a verified C1 definition bridge ID to its primitive semantic
    /// kind without exposing C1 row-order or spelling as authority.
    pub fn compiler_primitive_for_c1_definition(
        &self,
        definition: VirtualDefinitionId,
    ) -> Option<CompilerPrimitiveTypePattern> {
        self.typed_c2.primitive_for_c1_definition(definition)
    }

    pub fn compiler_nominal(&self, kind: CompilerNominalKind) -> &CompilerNominalAuthority {
        self.typed_c2.nominal(kind)
    }

    pub fn compiler_nominal_method_for_c1_method(
        &self,
        method: VirtualMethodId,
    ) -> Option<&CompilerNominalMethodAuthority> {
        self.typed_c2.nominal_method_for_c1_method(method)
    }

    pub fn compiler_nominal_method(
        &self,
        owner: CompilerNominalKind,
        source_name: &str,
    ) -> Option<&CompilerNominalMethodAuthority> {
        self.typed_c2.nominal_method(owner, source_name)
    }

    pub fn lookup_prelude(
        &self,
        spelling: &str,
        namespace: VirtualNamespace,
    ) -> Option<VirtualPreludeTarget> {
        self.projection
            .prelude
            .binary_search_by(|row| {
                (row.namespace, row.spelling.as_str()).cmp(&(namespace, spelling))
            })
            .ok()
            .map(|index| self.projection.prelude[index].target)
    }

    pub fn lookup_prelude_definition(
        &self,
        spelling: &str,
        namespace: VirtualNamespace,
    ) -> Option<VirtualDefinitionId> {
        match self.lookup_prelude(spelling, namespace)? {
            VirtualPreludeTarget::Definition(definition) => Some(definition),
            VirtualPreludeTarget::SemanticType(_) => None,
        }
    }

    pub fn definition(&self, id: VirtualDefinitionId) -> Option<&VirtualDefinitionRow> {
        self.projection.definitions.get(usize::from(id.0))
    }

    pub fn semantic_type(&self, id: VirtualSemanticTypeId) -> Option<&VirtualSemanticTypeRow> {
        self.projection.semantic_types.get(usize::from(id.0))
    }

    pub fn method(&self, id: VirtualMethodId) -> Option<&VirtualMethodRow> {
        self.projection.methods.get(usize::from(id.0))
    }

    pub fn enum_variant(&self, id: VirtualEnumVariantId) -> Option<&VirtualEnumVariantRow> {
        self.projection.enum_variants.get(usize::from(id.0))
    }

    pub fn lookup_enum_variant(
        &self,
        owner: VirtualDefinitionId,
        name: &str,
    ) -> Option<VirtualEnumVariantId> {
        self.projection
            .enum_variants
            .iter()
            .find(|row| row.owner == owner && row.name == name)
            .map(|row| row.id)
    }

    pub fn record_constructor(
        &self,
        owner: VirtualDefinitionId,
    ) -> Option<&VirtualRecordConstructorRow> {
        self.projection
            .record_constructors
            .binary_search_by_key(&owner, |row| row.owner)
            .ok()
            .map(|index| &self.projection.record_constructors[index])
    }

    pub fn lookup_record_constructor(&self, type_spelling: &str) -> Option<VirtualDefinitionId> {
        let owner = self.lookup_prelude_definition(type_spelling, VirtualNamespace::Type)?;
        self.record_constructor(owner)
            .map(VirtualRecordConstructorRow::owner)
    }

    pub fn lookup_method(
        &self,
        owner: Option<VirtualDefinitionId>,
        source_name: &str,
    ) -> Option<VirtualMethodId> {
        let mut matches = self
            .projection
            .methods
            .iter()
            .filter(|row| row.owner == owner && row.source_name == source_name);
        let first = matches.next()?;
        matches.next().is_none().then_some(first.id)
    }

    /// Resolves compiler syntax only after typed lowering has selected its
    /// sealed operation. In particular, raw `as` has distinct expose-address
    /// and pointer-cast rows and is intentionally ambiguous by source spelling.
    pub fn lookup_compiler_operation(
        &self,
        operation: CompilerOperation,
    ) -> Option<VirtualMethodId> {
        self.projection
            .methods
            .iter()
            .find(|row| row.lowering == VirtualMethodLowering::CompilerOperation(operation))
            .map(|row| row.id)
    }
}

/// Deterministic verification failure for the compiler's compiled-in release
/// manifest. No caller can supply or replace the manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmbeddedCoreVerificationError {
    InvalidCompiledPackageName,
    PackageIdentityMismatch,
    SyntheticSourceDigestMismatch,
    InterfaceDigestMismatch,
    InterfaceHashMismatch,
    NonCanonicalOrder(&'static str),
    DuplicateRow(&'static str),
    InvalidReference(&'static str),
    InvalidTypedTraitAuthority,
    InvalidTypedMethodAuthority,
    InvalidTypedPrimitiveAuthority,
    InvalidTypedNominalAuthority,
    InvalidTypedNominalMethodAuthority,
    InvalidTypedSourceAgreement,
    InvalidSpan,
    InvalidPanicBody,
}

impl fmt::Display for EmbeddedCoreVerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCompiledPackageName => {
                formatter.write_str("compiled embedded-core package name is invalid")
            }
            Self::PackageIdentityMismatch => {
                formatter.write_str("embedded-core PackageId differs from the release manifest")
            }
            Self::SyntheticSourceDigestMismatch => formatter
                .write_str("embedded-core synthetic source differs from the release manifest"),
            Self::InterfaceDigestMismatch => formatter
                .write_str("embedded-core interface projection differs from the release manifest"),
            Self::InterfaceHashMismatch => formatter
                .write_str("embedded-core public interface differs from the release manifest"),
            Self::NonCanonicalOrder(rows) => {
                write!(
                    formatter,
                    "embedded-core {rows} rows are not in canonical order"
                )
            }
            Self::DuplicateRow(rows) => {
                write!(formatter, "embedded-core {rows} rows are duplicated")
            }
            Self::InvalidReference(rows) => {
                write!(
                    formatter,
                    "embedded-core {rows} row has an invalid reference"
                )
            }
            Self::InvalidTypedTraitAuthority => {
                formatter.write_str("embedded-core typed compiler-trait authority is inconsistent")
            }
            Self::InvalidTypedMethodAuthority => formatter
                .write_str("embedded-core typed compiler-trait method authority is inconsistent"),
            Self::InvalidTypedPrimitiveAuthority => {
                formatter.write_str("embedded-core typed primitive authority is inconsistent")
            }
            Self::InvalidTypedNominalAuthority => {
                formatter.write_str("embedded-core typed nominal authority is inconsistent")
            }
            Self::InvalidTypedNominalMethodAuthority => {
                formatter.write_str("embedded-core typed nominal method authority is inconsistent")
            }
            Self::InvalidTypedSourceAgreement => formatter
                .write_str("embedded-core typed authority disagrees with its synthetic source"),
            Self::InvalidSpan => formatter.write_str("embedded-core source span is invalid"),
            Self::InvalidPanicBody => {
                formatter.write_str("embedded-core panic symbolic body is invalid")
            }
        }
    }
}

impl std::error::Error for EmbeddedCoreVerificationError {}

static VERIFIED_AUTHORITY: OnceLock<
    Result<Arc<VerifiedEmbeddedCoreAuthority>, EmbeddedCoreVerificationError>,
> = OnceLock::new();

/// Loads the compiler-selected release manifest and returns its single sealed
/// authority Arc. This is the only public construction boundary.
pub fn verified_embedded_core_authority(
) -> Result<Arc<VerifiedEmbeddedCoreAuthority>, EmbeddedCoreVerificationError> {
    VERIFIED_AUTHORITY
        .get_or_init(|| {
            let projection = build_release_projection()?;
            verify_projection(&projection)?;
            let typed_c2 = build_typed_c2_projection(&projection)?;
            verify_typed_c2_projection(&projection, &typed_c2)?;
            Ok(Arc::new(VerifiedEmbeddedCoreAuthority {
                projection,
                typed_c2,
                _seal: private::Seal,
            }))
        })
        .clone()
}

#[derive(Clone, Copy)]
struct DefinitionSpec {
    name: &'static str,
    namespace: VirtualNamespace,
    kind: VirtualDefinitionKind,
    declaration_kind: VirtualDeclarationKind,
    shape: &'static str,
    flavor: Option<VirtualTypeFlavor>,
    trait_policy: Option<UserImplPolicy>,
    prelude: bool,
}

#[derive(Clone, Copy)]
struct CompilerTraitMethodSpec {
    kind: CompilerTraitMethodKind,
    source_name: &'static str,
    stable_name: &'static str,
    signature: &'static str,
    receiver: CompilerTraitReceiverMode,
}

#[derive(Clone, Copy)]
struct CompilerTraitSpec {
    kind: CompilerTraitKind,
    name: &'static str,
    shape: &'static str,
    generic_names: &'static [&'static str],
    designated_self: CompilerTraitSelfRelation,
    user_impl_policy: UserImplPolicy,
    method: Option<CompilerTraitMethodSpec>,
}

#[derive(Clone, Copy)]
struct EnumVariantSpec {
    owner: &'static str,
    ordinal: u64,
    name: &'static str,
}

const ENUM_VARIANTS: &[EnumVariantSpec] = &[
    EnumVariantSpec {
        owner: "AllocError",
        ordinal: 0,
        name: "OutOfMemory",
    },
    EnumVariantSpec {
        owner: "AtomicRmw",
        ordinal: 0,
        name: "Add",
    },
    EnumVariantSpec {
        owner: "AtomicRmw",
        ordinal: 1,
        name: "Sub",
    },
    EnumVariantSpec {
        owner: "AtomicRmw",
        ordinal: 2,
        name: "And",
    },
    EnumVariantSpec {
        owner: "AtomicRmw",
        ordinal: 3,
        name: "Or",
    },
    EnumVariantSpec {
        owner: "AtomicRmw",
        ordinal: 4,
        name: "Xor",
    },
    EnumVariantSpec {
        owner: "AtomicRmw",
        ordinal: 5,
        name: "Exchange",
    },
    EnumVariantSpec {
        owner: "AtomicRmw",
        ordinal: 6,
        name: "Min",
    },
    EnumVariantSpec {
        owner: "AtomicRmw",
        ordinal: 7,
        name: "Max",
    },
    EnumVariantSpec {
        owner: "ChannelClosed",
        ordinal: 0,
        name: "Unit",
    },
    EnumVariantSpec {
        owner: "GeneratorState",
        ordinal: 0,
        name: "Yielded",
    },
    EnumVariantSpec {
        owner: "GeneratorState",
        ordinal: 1,
        name: "Complete",
    },
    EnumVariantSpec {
        owner: "Option",
        ordinal: 0,
        name: "None",
    },
    EnumVariantSpec {
        owner: "Option",
        ordinal: 1,
        name: "Some",
    },
    EnumVariantSpec {
        owner: "Ordering",
        ordinal: 0,
        name: "Relaxed",
    },
    EnumVariantSpec {
        owner: "Ordering",
        ordinal: 1,
        name: "Acquire",
    },
    EnumVariantSpec {
        owner: "Ordering",
        ordinal: 2,
        name: "Release",
    },
    EnumVariantSpec {
        owner: "Ordering",
        ordinal: 3,
        name: "AcqRel",
    },
    EnumVariantSpec {
        owner: "Ordering",
        ordinal: 4,
        name: "SeqCst",
    },
    EnumVariantSpec {
        owner: "Result",
        ordinal: 0,
        name: "Ok",
    },
    EnumVariantSpec {
        owner: "Result",
        ordinal: 1,
        name: "Err",
    },
    EnumVariantSpec {
        owner: "SocketAddress",
        ordinal: 0,
        name: "V4",
    },
    EnumVariantSpec {
        owner: "SocketAddress",
        ordinal: 1,
        name: "V6",
    },
];

const CONSTRUCTIBLE_RECORDS: &[&str] = &[
    "IoError",
    "OpenOptions",
    "ProcessError",
    "ProcessOutput",
    "ProcessSpec",
    "ThreadError",
];

#[derive(Clone, Copy)]
struct MethodSpec {
    owner: Option<&'static str>,
    source_name: &'static str,
    stable_name: &'static str,
    signature: &'static str,
    requires: &'static str,
    throws: &'static str,
    lowering: VirtualMethodLowering,
}

type SourceBackedRows = (
    Box<[VirtualDefinitionRow]>,
    Box<[VirtualEnumVariantRow]>,
    Box<[VirtualSemanticTypeRow]>,
    Box<[VirtualMethodRow]>,
    EmbeddedCoreSyntheticSource,
);

fn build_release_projection(
) -> Result<EmbeddedCoreC1PackageProjection, EmbeddedCoreVerificationError> {
    let package_name = PackageName::from_str(EMBEDDED_CORE_SCOPED_NAME)
        .map_err(|_| EmbeddedCoreVerificationError::InvalidCompiledPackageName)?;
    let package = canonical_package_id(&package_name);
    let mut definition_specs = definition_specs();
    definition_specs.sort_by(|left, right| {
        (left.namespace, left.name, left.kind).cmp(&(right.namespace, right.name, right.kind))
    });

    let mut method_specs = method_specs();
    method_specs.sort_by_key(|row| row.stable_name);

    let (definitions, enum_variants, semantic_types, methods, source) =
        build_source_backed_rows(&definition_specs, &method_specs)?;

    let types = definition_specs
        .iter()
        .enumerate()
        .filter_map(|(index, spec)| {
            let flavor = spec.flavor?;
            Some(VirtualTypeRow {
                definition: VirtualDefinitionId(u16::try_from(index).ok()?),
                flavor,
            })
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();

    let traits = build_trait_rows(&definition_specs, &methods)?;
    let record_constructors = build_record_constructor_rows(&definitions)?;
    let prelude = build_prelude_rows(&definition_specs, &semantic_types)?;
    let functions = build_function_rows(&definitions)?;
    let panic_definition = find_definition_id(&definitions, "panic", VirtualNamespace::Value)
        .ok_or(EmbeddedCoreVerificationError::InvalidPanicBody)?;
    let panic_span = definitions[usize::from(panic_definition.0)].span;
    let panic_body = CompilerOwnedPanicBodyAuthority {
        owner: panic_definition,
        generic_parameter_shape: "type T: UnwindPayload".to_owned(),
        parameter_type: "T".to_owned(),
        result_type: "!".to_owned(),
        declared_requires: Box::new([]),
        declared_throws: Box::new([]),
        terminal: PanicTerminal::User,
        required_ctfe_failure: "CTFE006".to_owned(),
        symbolic_body_bytes: Arc::from(
            b"ARCHE-PANIC-SYMBOLIC 1\0owner=panic;terminal=PanicKind::User;payload=T".as_slice(),
        ),
        span: panic_span,
    };

    let public_bytes = encode_public_interface(
        &definitions,
        &enum_variants,
        &record_constructors,
        &semantic_types,
        &traits,
        &methods,
        &prelude,
    );
    let public_interface_hash = InterfaceHash::from_canonical_preimage(&public_bytes);
    let mut projection = EmbeddedCoreC1PackageProjection {
        package,
        registry_origin: OFFICIAL_REGISTRY_IDENTITY.to_owned(),
        scoped_name: EMBEDDED_CORE_SCOPED_NAME.to_owned(),
        version: EMBEDDED_CORE_PACKAGE_VERSION.to_owned(),
        interface_version: EMBEDDED_CORE_INTERFACE_VERSION,
        interface_digest: [0; 32],
        public_interface_hash,
        root_module: EmbeddedCoreRootModuleRow {
            package,
            file: EMBEDDED_CORE_FILE_ID,
            path: Box::new([]),
        },
        source,
        definitions,
        types,
        enum_variants,
        record_constructors,
        semantic_types,
        traits,
        methods,
        functions,
        prelude,
        panic_body,
        canonical_bytes: Arc::from([]),
    };
    let canonical_bytes = encode_projection(&projection);
    projection.interface_digest = digest_interface_projection(&canonical_bytes);
    projection.canonical_bytes = Arc::from(canonical_bytes);
    Ok(projection)
}

fn build_source_backed_rows(
    definition_specs: &[DefinitionSpec],
    method_specs: &[MethodSpec],
) -> Result<SourceBackedRows, EmbeddedCoreVerificationError> {
    let mut bytes = b"ARCHE-EMBEDDED-CORE-SOURCE 1\n".to_vec();
    let mut line = 2_u64;
    let mut definitions = Vec::with_capacity(definition_specs.len());
    for (index, spec) in definition_specs.iter().enumerate() {
        let id = VirtualDefinitionId(
            u16::try_from(index)
                .map_err(|_| EmbeddedCoreVerificationError::InvalidReference("definition"))?,
        );
        let prefix = format!(
            "definition\t{}\t{}\t{}\t",
            id.0,
            namespace_name(spec.namespace),
            definition_kind_name(spec.kind)
        );
        bytes.extend_from_slice(prefix.as_bytes());
        let start = position(
            &bytes,
            line,
            u64::try_from(prefix.len()).unwrap_or(u64::MAX) + 1,
        )?;
        bytes.extend_from_slice(spec.name.as_bytes());
        let end = position(
            &bytes,
            line,
            u64::try_from(prefix.len() + spec.name.len()).unwrap_or(u64::MAX) + 1,
        )?;
        bytes.push(b'\t');
        bytes.extend_from_slice(spec.shape.as_bytes());
        bytes.push(b'\n');
        definitions.push(VirtualDefinitionRow {
            id,
            namespace: spec.namespace,
            kind: spec.kind,
            declaration_kind: spec.declaration_kind,
            name: spec.name.to_owned(),
            semantic_shape: spec.shape.to_owned(),
            span: Span {
                file: EMBEDDED_CORE_FILE_ID,
                start,
                end,
            },
        });
        line = line
            .checked_add(1)
            .ok_or(EmbeddedCoreVerificationError::InvalidSpan)?;
    }

    let mut variant_specs = ENUM_VARIANTS
        .iter()
        .map(|spec| {
            let owner = find_definition_id(&definitions, spec.owner, VirtualNamespace::Type)
                .ok_or(EmbeddedCoreVerificationError::InvalidReference(
                    "enum variant owner",
                ))?;
            Ok((owner, *spec))
        })
        .collect::<Result<Vec<_>, EmbeddedCoreVerificationError>>()?;
    variant_specs.sort_by_key(|(owner, spec)| (*owner, spec.ordinal));
    let mut enum_variants = Vec::with_capacity(variant_specs.len());
    for (index, (owner, spec)) in variant_specs.into_iter().enumerate() {
        let id = VirtualEnumVariantId(
            u16::try_from(index)
                .map_err(|_| EmbeddedCoreVerificationError::InvalidReference("enum variant"))?,
        );
        let prefix = format!("enum-variant\t{}\t{}\t{}\t", id.0, owner.0, spec.ordinal);
        bytes.extend_from_slice(prefix.as_bytes());
        let start = position(
            &bytes,
            line,
            u64::try_from(prefix.len()).unwrap_or(u64::MAX) + 1,
        )?;
        bytes.extend_from_slice(spec.name.as_bytes());
        let end = position(
            &bytes,
            line,
            u64::try_from(prefix.len() + spec.name.len()).unwrap_or(u64::MAX) + 1,
        )?;
        bytes.push(b'\n');
        enum_variants.push(VirtualEnumVariantRow {
            id,
            owner,
            ordinal: spec.ordinal,
            name: spec.name.to_owned(),
            span: Span {
                file: EMBEDDED_CORE_FILE_ID,
                start,
                end,
            },
        });
        line = line
            .checked_add(1)
            .ok_or(EmbeddedCoreVerificationError::InvalidSpan)?;
    }

    let mut semantic_types = Vec::with_capacity(SEMANTIC_TYPES.len());
    for (index, &(spelling, semantic_tag, shape)) in SEMANTIC_TYPES.iter().enumerate() {
        let id = VirtualSemanticTypeId(
            u16::try_from(index)
                .map_err(|_| EmbeddedCoreVerificationError::InvalidReference("semantic type"))?,
        );
        let prefix = format!("semantic-type\t{}\t{}\t", id.0, semantic_tag);
        bytes.extend_from_slice(prefix.as_bytes());
        let start = position(
            &bytes,
            line,
            u64::try_from(prefix.len()).unwrap_or(u64::MAX) + 1,
        )?;
        bytes.extend_from_slice(spelling.as_bytes());
        let end = position(
            &bytes,
            line,
            u64::try_from(prefix.len() + spelling.len()).unwrap_or(u64::MAX) + 1,
        )?;
        bytes.push(b'\t');
        bytes.extend_from_slice(shape.as_bytes());
        bytes.push(b'\n');
        semantic_types.push(VirtualSemanticTypeRow {
            id,
            spelling: spelling.to_owned(),
            semantic_tag,
            shape: shape.to_owned(),
            span: Span {
                file: EMBEDDED_CORE_FILE_ID,
                start,
                end,
            },
        });
        line = line
            .checked_add(1)
            .ok_or(EmbeddedCoreVerificationError::InvalidSpan)?;
    }

    let mut methods = Vec::with_capacity(method_specs.len());
    for (index, spec) in method_specs.iter().enumerate() {
        let id = VirtualMethodId(
            u16::try_from(index)
                .map_err(|_| EmbeddedCoreVerificationError::InvalidReference("method"))?,
        );
        let owner = spec
            .owner
            .map(|name| {
                find_definition_id(&definitions, name, VirtualNamespace::Type).ok_or(
                    EmbeddedCoreVerificationError::InvalidReference("method owner"),
                )
            })
            .transpose()?;
        let prefix = format!("method\t{}\t{}\t", id.0, spec.owner.unwrap_or("<compiler>"));
        bytes.extend_from_slice(prefix.as_bytes());
        let start = position(
            &bytes,
            line,
            u64::try_from(prefix.len()).unwrap_or(u64::MAX) + 1,
        )?;
        bytes.extend_from_slice(spec.source_name.as_bytes());
        let end = position(
            &bytes,
            line,
            u64::try_from(prefix.len() + spec.source_name.len()).unwrap_or(u64::MAX) + 1,
        )?;
        bytes.push(b'\t');
        bytes.extend_from_slice(spec.stable_name.as_bytes());
        bytes.push(b'\t');
        bytes.extend_from_slice(spec.signature.as_bytes());
        bytes.push(b'\t');
        bytes.extend_from_slice(spec.requires.as_bytes());
        bytes.push(b'\t');
        bytes.extend_from_slice(spec.throws.as_bytes());
        bytes.push(b'\n');
        methods.push(VirtualMethodRow {
            id,
            owner,
            source_name: spec.source_name.to_owned(),
            stable_name: spec.stable_name.to_owned(),
            signature: spec.signature.to_owned(),
            requires: spec.requires.to_owned(),
            throws: spec.throws.to_owned(),
            lowering: spec.lowering,
            span: Span {
                file: EMBEDDED_CORE_FILE_ID,
                start,
                end,
            },
        });
        line = line
            .checked_add(1)
            .ok_or(EmbeddedCoreVerificationError::InvalidSpan)?;
    }

    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    Ok((
        definitions.into_boxed_slice(),
        enum_variants.into_boxed_slice(),
        semantic_types.into_boxed_slice(),
        methods.into_boxed_slice(),
        EmbeddedCoreSyntheticSource {
            package_path: EMBEDDED_CORE_PACKAGE_PATH.to_owned(),
            bytes: Arc::from(bytes),
            digest,
        },
    ))
}

fn build_record_constructor_rows(
    definitions: &[VirtualDefinitionRow],
) -> Result<Box<[VirtualRecordConstructorRow]>, EmbeddedCoreVerificationError> {
    let mut rows = CONSTRUCTIBLE_RECORDS
        .iter()
        .map(|name| {
            let owner = find_definition_id(definitions, name, VirtualNamespace::Type).ok_or(
                EmbeddedCoreVerificationError::InvalidReference("record constructor"),
            )?;
            Ok(VirtualRecordConstructorRow { owner })
        })
        .collect::<Result<Vec<_>, EmbeddedCoreVerificationError>>()?;
    rows.sort_by_key(|row| row.owner);
    Ok(rows.into_boxed_slice())
}

fn position(
    bytes: &[u8],
    line: u64,
    column: u64,
) -> Result<SourcePosition, EmbeddedCoreVerificationError> {
    Ok(SourcePosition {
        byte: u64::try_from(bytes.len()).map_err(|_| EmbeddedCoreVerificationError::InvalidSpan)?,
        line,
        column,
    })
}

fn build_trait_rows(
    specs: &[DefinitionSpec],
    methods: &[VirtualMethodRow],
) -> Result<Box<[VirtualTraitRow]>, EmbeddedCoreVerificationError> {
    specs
        .iter()
        .enumerate()
        .filter_map(|(index, spec)| spec.trait_policy.map(|policy| (index, policy)))
        .map(|(index, policy)| {
            let definition = VirtualDefinitionId(
                u16::try_from(index)
                    .map_err(|_| EmbeddedCoreVerificationError::InvalidReference("trait"))?,
            );
            let owned_methods = methods
                .iter()
                .filter(|method| {
                    method.owner == Some(definition)
                        && method.lowering == VirtualMethodLowering::TraitDispatch
                })
                .map(|method| method.id)
                .collect::<Vec<_>>()
                .into_boxed_slice();
            Ok(VirtualTraitRow {
                definition,
                methods: owned_methods,
                user_impl_policy: policy,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn build_prelude_rows(
    specs: &[DefinitionSpec],
    semantic_types: &[VirtualSemanticTypeRow],
) -> Result<Box<[VirtualPreludeBindingRow]>, EmbeddedCoreVerificationError> {
    let mut rows = specs
        .iter()
        .enumerate()
        .filter(|(_, spec)| spec.prelude)
        .map(|(index, spec)| {
            Ok(VirtualPreludeBindingRow {
                spelling: spec.name.to_owned(),
                namespace: spec.namespace,
                target: VirtualPreludeTarget::Definition(VirtualDefinitionId(
                    u16::try_from(index)
                        .map_err(|_| EmbeddedCoreVerificationError::InvalidReference("prelude"))?,
                )),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    rows.extend(semantic_types.iter().map(|row| VirtualPreludeBindingRow {
        spelling: row.spelling.clone(),
        namespace: VirtualNamespace::Type,
        target: VirtualPreludeTarget::SemanticType(row.id),
    }));
    rows.sort_by(|left, right| {
        (left.namespace, left.spelling.as_str()).cmp(&(right.namespace, right.spelling.as_str()))
    });
    Ok(rows.into_boxed_slice())
}

fn build_function_rows(
    definitions: &[VirtualDefinitionRow],
) -> Result<Box<[VirtualFunctionRow]>, EmbeddedCoreVerificationError> {
    let row = |name: &str, lowering| {
        Ok(VirtualFunctionRow {
            definition: find_definition_id(definitions, name, VirtualNamespace::Value)
                .ok_or(EmbeddedCoreVerificationError::InvalidReference("function"))?,
            lowering,
        })
    };
    Ok(vec![
        row(
            "include_bytes",
            VirtualFunctionLowering::Intrinsic {
                id: 70,
                ctfe: IntrinsicCtfeSupport::IncludeAuthority,
            },
        )?,
        row(
            "include_str",
            VirtualFunctionLowering::Intrinsic {
                id: 71,
                ctfe: IntrinsicCtfeSupport::IncludeAuthority,
            },
        )?,
        row("panic", VirtualFunctionLowering::CompilerOwnedBody)?,
    ]
    .into_boxed_slice())
}

fn find_definition_id(
    definitions: &[VirtualDefinitionRow],
    name: &str,
    namespace: VirtualNamespace,
) -> Option<VirtualDefinitionId> {
    definitions
        .iter()
        .find(|row| row.namespace == namespace && row.name == name)
        .map(|row| row.id)
}

fn build_typed_c2_projection(
    projection: &EmbeddedCoreC1PackageProjection,
) -> Result<EmbeddedCoreC2TypedProjection, EmbeddedCoreVerificationError> {
    let compiler_traits = COMPILER_TRAITS
        .iter()
        .map(|spec| build_typed_compiler_trait(projection, spec))
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();
    let primitive_definitions = PRIMITIVE_TYPES
        .iter()
        .map(|spec| {
            let definition =
                find_definition_id(&projection.definitions, spec.name, VirtualNamespace::Type)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedPrimitiveAuthority)?;
            let row = projection
                .definitions
                .get(usize::from(definition.0))
                .ok_or(EmbeddedCoreVerificationError::InvalidTypedPrimitiveAuthority)?;
            if row.kind != VirtualDefinitionKind::PrimitiveType
                || row.declaration_kind != VirtualDeclarationKind::Primitive
            {
                return Err(EmbeddedCoreVerificationError::InvalidTypedPrimitiveAuthority);
            }
            Ok((definition, spec.kind))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();
    let nominals = NOMINAL_TYPES
        .iter()
        .map(|&(kind, name, shape, flavor, declaration_kind)| {
            let definition =
                find_definition_id(&projection.definitions, name, VirtualNamespace::Type)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalAuthority)?;
            let definition_row = projection
                .definitions
                .get(usize::from(definition.0))
                .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalAuthority)?;
            let type_row = projection
                .types
                .iter()
                .find(|row| row.definition == definition)
                .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalAuthority)?;
            if definition_row.name != name
                || definition_row.semantic_shape != shape
                || definition_row.kind != VirtualDefinitionKind::NominalType
                || definition_row.declaration_kind != declaration_kind
                || type_row.flavor != flavor
            {
                return Err(EmbeddedCoreVerificationError::InvalidTypedNominalAuthority);
            }
            Ok(CompilerNominalAuthority {
                kind,
                definition,
                flavor,
                declaration_kind,
            })
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();
    let nominal_methods = build_typed_nominal_methods(projection)?;
    Ok(EmbeddedCoreC2TypedProjection {
        compiler_traits,
        primitive_definitions,
        nominals,
        nominal_methods,
        _seal: private::TypedSeal,
    })
}

#[derive(Debug, Eq, PartialEq)]
struct ParsedNominalMethodSignature {
    head_before_generics: String,
    head_after_generics: String,
    receiver_in_head: bool,
    is_unsafe: bool,
    generics: Box<[CompilerMethodGenericParameterAuthority]>,
    receiver: CompilerNominalMethodReceiverMode,
    receiver_type: Option<CompilerMethodTypePattern>,
    parameters: Box<[CompilerMethodTypePattern]>,
    selectors: Box<[CompilerMethodSelectorAuthority]>,
    result: CompilerMethodTypePattern,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NominalMethodEmptyEffectSyntax {
    Omitted,
    Braced,
}

#[derive(Debug, Eq, PartialEq)]
struct ParsedNominalMethodEffects {
    patterns: Box<[CompilerNominalMethodEffectPattern]>,
    empty_syntax: NominalMethodEmptyEffectSyntax,
}

fn build_typed_nominal_methods(
    projection: &EmbeddedCoreC1PackageProjection,
) -> Result<Box<[CompilerNominalMethodAuthority]>, EmbeddedCoreVerificationError> {
    let mut authorities = Vec::new();
    let mut seen_methods = BTreeSet::new();
    let mut seen_names = BTreeSet::new();

    for spec in METHOD_SPECS {
        let Some(owner_name) = spec.owner else {
            continue;
        };
        let Some(owner_kind) = compiler_nominal_kind_for_name(owner_name) else {
            continue;
        };
        if !is_intrinsic_nominal_method_owner(owner_kind) {
            continue;
        }

        let owner_definition =
            find_definition_id(&projection.definitions, owner_name, VirtualNamespace::Type)
                .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
        let owner_row = projection
            .definitions
            .get(usize::from(owner_definition.0))
            .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
        if owner_row.kind != VirtualDefinitionKind::NominalType {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        }

        let mut matching_rows = projection
            .methods
            .iter()
            .filter(|row| row.stable_name == spec.stable_name);
        let row = matching_rows
            .next()
            .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
        if matching_rows.next().is_some()
            || row.owner != Some(owner_definition)
            || row.source_name != spec.source_name
            || row.stable_name != spec.stable_name
            || row.signature != spec.signature
            || row.requires != spec.requires
            || row.throws != spec.throws
            || row.lowering != spec.lowering
        {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        }
        if !source_has_method_line(projection, row) {
            return Err(EmbeddedCoreVerificationError::InvalidTypedSourceAgreement);
        }
        if !seen_methods.insert(row.id) || !seen_names.insert((owner_kind, row.source_name.clone()))
        {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        }

        let parsed = parse_nominal_method_signature(
            projection,
            owner_name,
            owner_definition,
            spec.source_name,
            spec.signature,
        )?;
        let requires = parse_nominal_method_effects(
            projection,
            spec.requires,
            b'R',
            &parsed.generics,
            &parsed.selectors,
        )?;
        let throws = parse_nominal_method_effects(
            projection,
            spec.throws,
            b'T',
            &parsed.generics,
            &parsed.selectors,
        )?;
        if render_nominal_method_signature(projection, &parsed)? != spec.signature
            || render_nominal_method_effects(
                projection,
                &requires,
                b'R',
                &parsed.generics,
                &parsed.selectors,
            )? != spec.requires
            || render_nominal_method_effects(
                projection,
                &throws,
                b'T',
                &parsed.generics,
                &parsed.selectors,
            )? != spec.throws
        {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        }

        authorities.push(CompilerNominalMethodAuthority {
            owner: owner_kind,
            c1_method: row.id,
            source_name: row.source_name.clone(),
            stable_name: row.stable_name.clone(),
            lowering: row.lowering,
            is_unsafe: parsed.is_unsafe,
            generics: parsed.generics,
            receiver: parsed.receiver,
            receiver_type: parsed.receiver_type,
            parameters: parsed.parameters,
            selectors: parsed.selectors,
            result: parsed.result,
            requires: requires.patterns,
            throws: throws.patterns,
        });
    }

    for row in &projection.methods {
        let Some(owner) = row.owner else {
            continue;
        };
        let Some(definition) = projection.definitions.get(usize::from(owner.0)) else {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        };
        let Some(kind) = compiler_nominal_kind_for_name(&definition.name) else {
            continue;
        };
        if is_intrinsic_nominal_method_owner(kind) && !seen_methods.contains(&row.id) {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        }
    }

    authorities.sort_by_key(|row| row.c1_method);
    Ok(authorities.into_boxed_slice())
}

fn compiler_nominal_kind_for_name(name: &str) -> Option<CompilerNominalKind> {
    NOMINAL_TYPES
        .iter()
        .find(|(_, nominal_name, _, _, _)| *nominal_name == name)
        .map(|(kind, _, _, _, _)| *kind)
}

fn is_intrinsic_nominal_method_owner(kind: CompilerNominalKind) -> bool {
    // `Caps` is the affine capability projection, whose compiler operation is
    // selected through capability authority rather than nominal method calls.
    kind != CompilerNominalKind::Caps
}

fn parse_nominal_method_signature(
    projection: &EmbeddedCoreC1PackageProjection,
    owner_name: &str,
    owner_definition: VirtualDefinitionId,
    source_name: &str,
    signature: &str,
) -> Result<ParsedNominalMethodSignature, EmbeddedCoreVerificationError> {
    let (is_unsafe, signature) = match signature.strip_prefix("unsafe ") {
        Some(signature) => (true, signature),
        None => (false, signature),
    };
    let open = signature
        .find('(')
        .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
    let close = matching_signature_parenthesis(signature, open)?;
    let result_text = signature
        .get(close + 1..)
        .and_then(|suffix| suffix.strip_prefix(" -> "))
        .filter(|result| !result.is_empty())
        .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
    let head = &signature[..open];
    let parameters_text = &signature[open + 1..close];
    let (head_before_generics, generics_text, head_after_generics) =
        split_nominal_method_head(head)?;
    let generics = parse_nominal_method_generics(generics_text)?;

    let sections = split_nominal_top_level(parameters_text, "; ")?;
    if sections.len() > 2 {
        return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
    }
    let mut parameters = split_nominal_top_level(sections.first().copied().unwrap_or(""), ", ")?
        .into_iter()
        .map(|value| parse_nominal_method_type(projection, value, &generics))
        .collect::<Result<Vec<_>, _>>()?;
    let selectors = match sections.get(1) {
        Some(value) if !value.is_empty() => parse_nominal_method_selectors(value)?,
        Some(_) => return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority),
        None => Vec::new(),
    };
    let result = parse_nominal_method_type(projection, result_text, &generics)?;

    let mut receiver_in_head = false;
    let mut receiver = CompilerNominalMethodReceiverMode::None;
    let mut receiver_type = None;
    if !head_after_generics.is_empty() {
        let expected_suffix = format!(".{source_name}");
        if head_before_generics != owner_name
            || head_after_generics != expected_suffix
            || generics.is_empty()
        {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        }
        let arguments = generics
            .iter()
            .map(nominal_method_generic_as_argument)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        receiver_in_head = true;
        receiver = CompilerNominalMethodReceiverMode::Value;
        receiver_type = Some(CompilerMethodTypePattern::Definition {
            definition: owner_definition,
            arguments,
        });
    } else if let Some(mode) = parameters
        .first()
        .and_then(|parameter| nominal_receiver_mode(parameter, owner_definition))
    {
        receiver = mode;
        receiver_type = Some(parameters.remove(0));
    }

    Ok(ParsedNominalMethodSignature {
        head_before_generics: head_before_generics.to_owned(),
        head_after_generics: head_after_generics.to_owned(),
        receiver_in_head,
        is_unsafe,
        generics: generics.into_boxed_slice(),
        receiver,
        receiver_type,
        parameters: parameters.into_boxed_slice(),
        selectors: selectors.into_boxed_slice(),
        result,
    })
}

fn matching_signature_parenthesis(
    signature: &str,
    open: usize,
) -> Result<usize, EmbeddedCoreVerificationError> {
    let mut depth = 0_u32;
    for (offset, byte) in signature[open..].bytes().enumerate() {
        match byte {
            b'(' => {
                depth = depth
                    .checked_add(1)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
            }
            b')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
                if depth == 0 {
                    return Ok(open + offset);
                }
            }
            _ => {}
        }
    }
    Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)
}

fn split_nominal_method_head(
    head: &str,
) -> Result<(&str, Option<&str>, &str), EmbeddedCoreVerificationError> {
    if head.is_empty()
        || !head
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-<>+'-,".contains(&byte))
    {
        return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
    }
    let Some(open) = head.find('<') else {
        if head.contains('>') {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        }
        return Ok((head, None, ""));
    };
    let mut depth = 0_u32;
    let mut close = None;
    for (offset, byte) in head[open..].bytes().enumerate() {
        match byte {
            b'<' => {
                depth = depth
                    .checked_add(1)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
            }
            b'>' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
                if depth == 0 {
                    close = Some(open + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close.ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
    let before = &head[..open];
    let generics = &head[open + 1..close];
    let after = &head[close + 1..];
    if before.is_empty()
        || generics.is_empty()
        || after.contains(['<', '>'])
        || (!after.is_empty() && !after.starts_with('.'))
    {
        return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
    }
    Ok((before, Some(generics), after))
}

fn parse_nominal_method_generics(
    generics: Option<&str>,
) -> Result<Vec<CompilerMethodGenericParameterAuthority>, EmbeddedCoreVerificationError> {
    let Some(generics) = generics else {
        return Ok(Vec::new());
    };
    let mut names = BTreeSet::new();
    split_nominal_top_level(generics, ",")?
        .into_iter()
        .enumerate()
        .map(|(index, declaration)| {
            let coordinate = CompilerMethodGenericParameter(u8::try_from(index).map_err(|_| {
                EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority
            })?);
            let (source_name, kind, bounds) = if declaration.starts_with('\'') {
                if !valid_lifetime_name(declaration) {
                    return Err(
                        EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority,
                    );
                }
                (
                    declaration,
                    CompilerMethodGenericParameterKind::Lifetime,
                    Vec::new(),
                )
            } else {
                let mut pieces = declaration.split(':');
                let source_name = pieces.next().unwrap_or_default();
                let bounds_text = pieces.next();
                if pieces.next().is_some() || !valid_identifier(source_name) {
                    return Err(
                        EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority,
                    );
                }
                let mut seen_bounds = BTreeSet::new();
                let bounds = match bounds_text {
                    Some("") => {
                        return Err(
                            EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority,
                        )
                    }
                    Some(bounds) => bounds
                        .split('+')
                        .map(|bound| {
                            let kind = compiler_trait_kind_for_name(bound).ok_or(
                                EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority,
                            )?;
                            if !seen_bounds.insert(kind) {
                                return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
                            }
                            Ok(CompilerMethodGenericBoundPattern::CompilerTrait(kind))
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    None => Vec::new(),
                };
                (
                    source_name,
                    CompilerMethodGenericParameterKind::Type,
                    bounds,
                )
            };
            if !names.insert(source_name.to_owned()) {
                return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
            }
            Ok(CompilerMethodGenericParameterAuthority {
                coordinate,
                source_name: source_name.to_owned(),
                kind,
                bounds: bounds.into_boxed_slice(),
            })
        })
        .collect()
}

fn compiler_trait_kind_for_name(name: &str) -> Option<CompilerTraitKind> {
    COMPILER_TRAITS
        .iter()
        .find(|spec| spec.name == name)
        .map(|spec| spec.kind)
}

fn compiler_trait_name_for_kind(kind: CompilerTraitKind) -> Option<&'static str> {
    COMPILER_TRAITS
        .iter()
        .find(|spec| spec.kind == kind)
        .map(|spec| spec.name)
}

fn valid_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(byte) if byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn valid_lifetime_name(value: &str) -> bool {
    value.strip_prefix('\'').is_some_and(valid_identifier)
}

fn nominal_method_generic_as_argument(
    generic: &CompilerMethodGenericParameterAuthority,
) -> CompilerMethodGenericArgumentPattern {
    match generic.kind {
        CompilerMethodGenericParameterKind::Type => CompilerMethodGenericArgumentPattern::Type(
            CompilerMethodTypePattern::Generic(generic.coordinate),
        ),
        CompilerMethodGenericParameterKind::Lifetime => {
            CompilerMethodGenericArgumentPattern::Lifetime(generic.coordinate)
        }
    }
}

fn nominal_receiver_mode(
    pattern: &CompilerMethodTypePattern,
    owner: VirtualDefinitionId,
) -> Option<CompilerNominalMethodReceiverMode> {
    match pattern {
        CompilerMethodTypePattern::Definition { definition, .. } if *definition == owner => {
            Some(CompilerNominalMethodReceiverMode::Value)
        }
        CompilerMethodTypePattern::SharedReference { referent, .. } if matches!(referent.as_ref(), CompilerMethodTypePattern::Definition { definition, .. } if *definition == owner) => {
            Some(CompilerNominalMethodReceiverMode::Shared)
        }
        CompilerMethodTypePattern::MutableReference { referent, .. } if matches!(referent.as_ref(), CompilerMethodTypePattern::Definition { definition, .. } if *definition == owner) => {
            Some(CompilerNominalMethodReceiverMode::Mutable)
        }
        _ => None,
    }
}

fn split_nominal_top_level<'a>(
    value: &'a str,
    delimiter: &str,
) -> Result<Vec<&'a str>, EmbeddedCoreVerificationError> {
    if delimiter.is_empty() {
        return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
    }
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let bytes = value.as_bytes();
    let delimiter = delimiter.as_bytes();
    let mut angle_depth = 0_u32;
    let mut parenthesis_depth = 0_u32;
    let mut bracket_depth = 0_u32;
    let mut start = 0_usize;
    let mut index = 0_usize;
    let mut values = Vec::new();
    while index < bytes.len() {
        if angle_depth == 0
            && parenthesis_depth == 0
            && bracket_depth == 0
            && bytes[index..].starts_with(delimiter)
        {
            if start == index {
                return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
            }
            values.push(&value[start..index]);
            index += delimiter.len();
            start = index;
            continue;
        }
        match bytes[index] {
            b'<' => {
                angle_depth = angle_depth
                    .checked_add(1)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
            }
            b'>' => {
                angle_depth = angle_depth
                    .checked_sub(1)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
            }
            b'(' => {
                parenthesis_depth = parenthesis_depth
                    .checked_add(1)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
            }
            b')' => {
                parenthesis_depth = parenthesis_depth
                    .checked_sub(1)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
            }
            b'[' => {
                bracket_depth = bracket_depth
                    .checked_add(1)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
            }
            b']' => {
                bracket_depth = bracket_depth
                    .checked_sub(1)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
            }
            _ => {}
        }
        index += 1;
    }
    if angle_depth != 0 || parenthesis_depth != 0 || bracket_depth != 0 || start == value.len() {
        return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
    }
    values.push(&value[start..]);
    Ok(values)
}

fn parse_nominal_method_selectors(
    selectors: &str,
) -> Result<Vec<CompilerMethodSelectorAuthority>, EmbeddedCoreVerificationError> {
    let mut names = BTreeSet::new();
    split_nominal_top_level(selectors, ", ")?
        .into_iter()
        .enumerate()
        .map(|(index, selector)| {
            let (source_name, kind) = selector
                .split_once(':')
                .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
            if !valid_identifier(source_name)
                || kind != "DefinitionId"
                || selector.matches(':').count() != 1
                || !names.insert(source_name.to_owned())
            {
                return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
            }
            Ok(CompilerMethodSelectorAuthority {
                coordinate: CompilerMethodSelector(u8::try_from(index).map_err(|_| {
                    EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority
                })?),
                source_name: source_name.to_owned(),
                kind: CompilerMethodSelectorKind::DefinitionId,
            })
        })
        .collect()
}

fn parse_nominal_method_type(
    projection: &EmbeddedCoreC1PackageProjection,
    value: &str,
    generics: &[CompilerMethodGenericParameterAuthority],
) -> Result<CompilerMethodTypePattern, EmbeddedCoreVerificationError> {
    if value.is_empty() || value.trim() != value {
        return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
    }
    if let Some(reference) = value.strip_prefix('&') {
        let (lifetime, reference) = if reference.starts_with('\'') {
            let separator = reference
                .find(' ')
                .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
            let lifetime_name = &reference[..separator];
            let generic = nominal_method_generic_by_name(generics, lifetime_name)
                .filter(|generic| generic.kind == CompilerMethodGenericParameterKind::Lifetime)
                .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
            (
                CompilerMethodLifetimePattern::Generic(generic.coordinate),
                &reference[separator + 1..],
            )
        } else {
            (CompilerMethodLifetimePattern::Elided, reference)
        };
        let (mutable, referent) = match reference.strip_prefix("mut ") {
            Some(referent) => (true, referent),
            None => (false, reference),
        };
        if referent.is_empty() {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        }
        let referent = Box::new(parse_nominal_method_type(projection, referent, generics)?);
        return Ok(if mutable {
            CompilerMethodTypePattern::MutableReference { lifetime, referent }
        } else {
            CompilerMethodTypePattern::SharedReference { lifetime, referent }
        });
    }
    if value == "()" {
        return nominal_method_definition_type(projection, value, Box::new([]));
    }
    if let Some(inner) = value
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
    {
        if inner.is_empty() {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        }
        return Ok(CompilerMethodTypePattern::Slice(Box::new(
            parse_nominal_method_type(projection, inner, generics)?,
        )));
    }
    if let Some(inner) = value
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
    {
        if inner.is_empty() {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        }
        let elements = split_nominal_top_level(inner, ",")?
            .into_iter()
            .map(|element| parse_nominal_method_type(projection, element, generics))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(CompilerMethodTypePattern::Tuple(
            elements.into_boxed_slice(),
        ));
    }
    if let Some(generic) = nominal_method_generic_by_name(generics, value) {
        if generic.kind != CompilerMethodGenericParameterKind::Type {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        }
        return Ok(CompilerMethodTypePattern::Generic(generic.coordinate));
    }
    if value.starts_with('\'') {
        return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
    }

    if let Some(open) = value.find('<') {
        let name = &value[..open];
        let arguments = value
            .get(open + 1..)
            .and_then(|arguments| arguments.strip_suffix('>'))
            .filter(|arguments| !arguments.is_empty())
            .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
        if name.is_empty() || name.contains('>') {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        }
        let arguments = split_nominal_top_level(arguments, ",")?
            .into_iter()
            .map(|argument| {
                if argument.starts_with('\'') {
                    let generic = nominal_method_generic_by_name(generics, argument)
                        .filter(|generic| {
                            generic.kind == CompilerMethodGenericParameterKind::Lifetime
                        })
                        .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
                    Ok(CompilerMethodGenericArgumentPattern::Lifetime(
                        generic.coordinate,
                    ))
                } else {
                    Ok(CompilerMethodGenericArgumentPattern::Type(
                        parse_nominal_method_type(projection, argument, generics)?,
                    ))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        return nominal_method_definition_type(projection, name, arguments.into_boxed_slice());
    }
    if value.contains('>') {
        return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
    }
    nominal_method_definition_type(projection, value, Box::new([]))
}

fn nominal_method_generic_by_name<'a>(
    generics: &'a [CompilerMethodGenericParameterAuthority],
    name: &str,
) -> Option<&'a CompilerMethodGenericParameterAuthority> {
    let mut matches = generics
        .iter()
        .filter(|generic| generic.source_name == name);
    let generic = matches.next()?;
    matches.next().is_none().then_some(generic)
}

fn nominal_method_definition_type(
    projection: &EmbeddedCoreC1PackageProjection,
    name: &str,
    arguments: Box<[CompilerMethodGenericArgumentPattern]>,
) -> Result<CompilerMethodTypePattern, EmbeddedCoreVerificationError> {
    let definition = find_definition_id(&projection.definitions, name, VirtualNamespace::Type)
        .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
    Ok(CompilerMethodTypePattern::Definition {
        definition,
        arguments,
    })
}

fn parse_nominal_method_effects(
    projection: &EmbeddedCoreC1PackageProjection,
    value: &str,
    field: u8,
    generics: &[CompilerMethodGenericParameterAuthority],
    selectors: &[CompilerMethodSelectorAuthority],
) -> Result<ParsedNominalMethodEffects, EmbeddedCoreVerificationError> {
    if value.is_empty() {
        return Ok(ParsedNominalMethodEffects {
            patterns: Box::new([]),
            empty_syntax: NominalMethodEmptyEffectSyntax::Omitted,
        });
    }
    if value == "{}" {
        return Ok(ParsedNominalMethodEffects {
            patterns: Box::new([]),
            empty_syntax: NominalMethodEmptyEffectSyntax::Braced,
        });
    }
    let prefix = match field {
        b'R' => "R{",
        b'T' => "T{",
        _ => return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority),
    };
    let inner = value
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix('}'))
        .filter(|value| !value.is_empty())
        .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
    let mut seen = BTreeSet::new();
    let effects = split_nominal_top_level(inner, ",")?
        .into_iter()
        .map(|effect| {
            if !seen.insert(effect.to_owned()) {
                return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
            }
            if let Some(inner) = effect
                .strip_prefix("Drop(")
                .and_then(|effect| effect.strip_suffix(')'))
            {
                if inner.is_empty() {
                    return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
                }
                return Ok(CompilerNominalMethodEffectPattern::Drop(
                    parse_nominal_method_type(projection, inner, generics)?,
                ));
            }
            let selector = selectors
                .iter()
                .filter(|selector| selector.source_name == effect)
                .collect::<Vec<_>>();
            if selector.len() != 1 {
                return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
            }
            Ok(CompilerNominalMethodEffectPattern::Selector(
                selector[0].coordinate,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ParsedNominalMethodEffects {
        patterns: effects.into_boxed_slice(),
        empty_syntax: NominalMethodEmptyEffectSyntax::Omitted,
    })
}

fn render_nominal_method_signature(
    projection: &EmbeddedCoreC1PackageProjection,
    parsed: &ParsedNominalMethodSignature,
) -> Result<String, EmbeddedCoreVerificationError> {
    let mut signature = String::new();
    if parsed.is_unsafe {
        signature.push_str("unsafe ");
    }
    signature.push_str(&parsed.head_before_generics);
    if !parsed.generics.is_empty() {
        signature.push('<');
        for (index, generic) in parsed.generics.iter().enumerate() {
            if index != 0 {
                signature.push(',');
            }
            if usize::from(generic.coordinate.0) != index
                || !match generic.kind {
                    CompilerMethodGenericParameterKind::Type => {
                        valid_identifier(&generic.source_name)
                    }
                    CompilerMethodGenericParameterKind::Lifetime => {
                        valid_lifetime_name(&generic.source_name) && generic.bounds.is_empty()
                    }
                }
            {
                return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
            }
            signature.push_str(&generic.source_name);
            if !generic.bounds.is_empty() {
                signature.push(':');
                for (bound_index, bound) in generic.bounds.iter().enumerate() {
                    if bound_index != 0 {
                        signature.push('+');
                    }
                    let CompilerMethodGenericBoundPattern::CompilerTrait(kind) = bound;
                    signature.push_str(compiler_trait_name_for_kind(*kind).ok_or(
                        EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority,
                    )?);
                }
            }
        }
        signature.push('>');
    }
    signature.push_str(&parsed.head_after_generics);
    signature.push('(');
    let mut parameter_count = 0_usize;
    if !parsed.receiver_in_head {
        if let Some(receiver) = &parsed.receiver_type {
            signature.push_str(&render_nominal_method_type(
                projection,
                receiver,
                &parsed.generics,
            )?);
            parameter_count += 1;
        }
    }
    for parameter in &parsed.parameters {
        if parameter_count != 0 {
            signature.push_str(", ");
        }
        signature.push_str(&render_nominal_method_type(
            projection,
            parameter,
            &parsed.generics,
        )?);
        parameter_count += 1;
    }
    if !parsed.selectors.is_empty() {
        signature.push_str("; ");
        for (index, selector) in parsed.selectors.iter().enumerate() {
            if usize::from(selector.coordinate.0) != index
                || !valid_identifier(&selector.source_name)
            {
                return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
            }
            if index != 0 {
                signature.push_str(", ");
            }
            signature.push_str(&selector.source_name);
            signature.push(':');
            signature.push_str(match selector.kind {
                CompilerMethodSelectorKind::DefinitionId => "DefinitionId",
            });
        }
    }
    signature.push_str(") -> ");
    signature.push_str(&render_nominal_method_type(
        projection,
        &parsed.result,
        &parsed.generics,
    )?);
    Ok(signature)
}

fn render_nominal_method_type(
    projection: &EmbeddedCoreC1PackageProjection,
    pattern: &CompilerMethodTypePattern,
    generics: &[CompilerMethodGenericParameterAuthority],
) -> Result<String, EmbeddedCoreVerificationError> {
    match pattern {
        CompilerMethodTypePattern::Generic(coordinate) => {
            let generic = nominal_method_generic_at(generics, *coordinate)?;
            if generic.kind != CompilerMethodGenericParameterKind::Type {
                return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
            }
            Ok(generic.source_name.clone())
        }
        CompilerMethodTypePattern::Definition {
            definition,
            arguments,
        } => {
            let row = projection
                .definitions
                .get(usize::from(definition.0))
                .filter(|row| row.namespace == VirtualNamespace::Type)
                .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
            let mut rendered = row.name.clone();
            if !arguments.is_empty() {
                rendered.push('<');
                for (index, argument) in arguments.iter().enumerate() {
                    if index != 0 {
                        rendered.push(',');
                    }
                    match argument {
                        CompilerMethodGenericArgumentPattern::Type(pattern) => rendered
                            .push_str(&render_nominal_method_type(projection, pattern, generics)?),
                        CompilerMethodGenericArgumentPattern::Lifetime(coordinate) => {
                            let generic = nominal_method_generic_at(generics, *coordinate)?;
                            if generic.kind != CompilerMethodGenericParameterKind::Lifetime {
                                return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
                            }
                            rendered.push_str(&generic.source_name);
                        }
                    }
                }
                rendered.push('>');
            }
            Ok(rendered)
        }
        CompilerMethodTypePattern::SharedReference { lifetime, referent } => {
            render_nominal_method_reference(projection, false, *lifetime, referent, generics)
        }
        CompilerMethodTypePattern::MutableReference { lifetime, referent } => {
            render_nominal_method_reference(projection, true, *lifetime, referent, generics)
        }
        CompilerMethodTypePattern::Slice(element) => Ok(format!(
            "[{}]",
            render_nominal_method_type(projection, element, generics)?
        )),
        CompilerMethodTypePattern::Tuple(elements) => {
            if elements.is_empty() {
                return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
            }
            let elements = elements
                .iter()
                .map(|element| render_nominal_method_type(projection, element, generics))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("({})", elements.join(",")))
        }
    }
}

fn render_nominal_method_reference(
    projection: &EmbeddedCoreC1PackageProjection,
    mutable: bool,
    lifetime: CompilerMethodLifetimePattern,
    referent: &CompilerMethodTypePattern,
    generics: &[CompilerMethodGenericParameterAuthority],
) -> Result<String, EmbeddedCoreVerificationError> {
    let mut rendered = String::from("&");
    if let CompilerMethodLifetimePattern::Generic(coordinate) = lifetime {
        let generic = nominal_method_generic_at(generics, coordinate)?;
        if generic.kind != CompilerMethodGenericParameterKind::Lifetime {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
        }
        rendered.push_str(&generic.source_name);
        rendered.push(' ');
    }
    if mutable {
        rendered.push_str("mut ");
    }
    rendered.push_str(&render_nominal_method_type(projection, referent, generics)?);
    Ok(rendered)
}

fn nominal_method_generic_at(
    generics: &[CompilerMethodGenericParameterAuthority],
    coordinate: CompilerMethodGenericParameter,
) -> Result<&CompilerMethodGenericParameterAuthority, EmbeddedCoreVerificationError> {
    let generic = generics
        .get(usize::from(coordinate.0))
        .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
    if generic.coordinate != coordinate {
        return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
    }
    Ok(generic)
}

fn render_nominal_method_effects(
    projection: &EmbeddedCoreC1PackageProjection,
    effects: &ParsedNominalMethodEffects,
    field: u8,
    generics: &[CompilerMethodGenericParameterAuthority],
    selectors: &[CompilerMethodSelectorAuthority],
) -> Result<String, EmbeddedCoreVerificationError> {
    if effects.patterns.is_empty() {
        return Ok(match effects.empty_syntax {
            NominalMethodEmptyEffectSyntax::Omitted => String::new(),
            NominalMethodEmptyEffectSyntax::Braced => String::from("{}"),
        });
    }
    let mut rendered = match field {
        b'R' => String::from("R{"),
        b'T' => String::from("T{"),
        _ => return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority),
    };
    for (index, effect) in effects.patterns.iter().enumerate() {
        if index != 0 {
            rendered.push(',');
        }
        match effect {
            CompilerNominalMethodEffectPattern::Drop(pattern) => {
                rendered.push_str("Drop(");
                rendered.push_str(&render_nominal_method_type(projection, pattern, generics)?);
                rendered.push(')');
            }
            CompilerNominalMethodEffectPattern::Selector(coordinate) => {
                let selector = selectors
                    .get(usize::from(coordinate.0))
                    .filter(|selector| selector.coordinate == *coordinate)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)?;
                rendered.push_str(&selector.source_name);
            }
        }
    }
    rendered.push('}');
    Ok(rendered)
}

fn build_typed_compiler_trait(
    projection: &EmbeddedCoreC1PackageProjection,
    spec: &CompilerTraitSpec,
) -> Result<CompilerTraitAuthority, EmbeddedCoreVerificationError> {
    let definition = find_definition_id(&projection.definitions, spec.name, VirtualNamespace::Type)
        .ok_or(EmbeddedCoreVerificationError::InvalidTypedTraitAuthority)?;
    let definition_row = projection
        .definitions
        .get(usize::from(definition.0))
        .ok_or(EmbeddedCoreVerificationError::InvalidTypedTraitAuthority)?;
    let trait_row = projection
        .traits
        .iter()
        .find(|row| row.definition == definition)
        .ok_or(EmbeddedCoreVerificationError::InvalidTypedTraitAuthority)?;
    if definition_row.kind != VirtualDefinitionKind::Trait
        || definition_row.declaration_kind != VirtualDeclarationKind::Trait
        || definition_row.name != spec.name
        || definition_row.semantic_shape != spec.shape
        || trait_row.user_impl_policy != spec.user_impl_policy
    {
        return Err(EmbeddedCoreVerificationError::InvalidTypedTraitAuthority);
    }
    let explicit_generic_arity = u8::try_from(spec.generic_names.len())
        .map_err(|_| EmbeddedCoreVerificationError::InvalidTypedTraitAuthority)?;
    verify_self_relation_coordinate(spec.designated_self, explicit_generic_arity)?;
    let method = match spec.method {
        Some(method_spec) => {
            if trait_row.methods.len() != 1 {
                return Err(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority);
            }
            let row = projection
                .methods
                .get(usize::from(trait_row.methods[0].0))
                .ok_or(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority)?;
            if row.owner != Some(definition)
                || row.source_name != method_spec.source_name
                || row.stable_name != method_spec.stable_name
                || row.signature != method_spec.signature
                || row.requires != "{}"
                || row.throws != "{}"
                || row.lowering != VirtualMethodLowering::TraitDispatch
            {
                return Err(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority);
            }
            let (callable, effects) = parse_trait_callable(spec, &method_spec)?;
            Some(CompilerTraitMethodAuthority {
                kind: method_spec.kind,
                c1_method: row.id,
                source_name: row.source_name.clone(),
                receiver: method_spec.receiver,
                callable,
                effects,
            })
        }
        None => {
            if !trait_row.methods.is_empty() {
                return Err(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority);
            }
            None
        }
    };
    Ok(CompilerTraitAuthority {
        kind: spec.kind,
        c1_definition: definition,
        explicit_generic_arity,
        designated_self: spec.designated_self,
        user_impl_policy: spec.user_impl_policy,
        method,
    })
}

fn verify_self_relation_coordinate(
    relation: CompilerTraitSelfRelation,
    arity: u8,
) -> Result<(), EmbeddedCoreVerificationError> {
    let coordinate = match relation {
        CompilerTraitSelfRelation::OperatedType | CompilerTraitSelfRelation::CallableType => {
            return Ok(())
        }
        CompilerTraitSelfRelation::Target(parameter)
        | CompilerTraitSelfRelation::LeftHandSide(parameter)
        | CompilerTraitSelfRelation::Input(parameter)
        | CompilerTraitSelfRelation::Source(parameter)
        | CompilerTraitSelfRelation::Iterator(parameter) => parameter,
    };
    if coordinate.0 >= arity {
        return Err(EmbeddedCoreVerificationError::InvalidTypedTraitAuthority);
    }
    Ok(())
}

fn parse_trait_callable(
    trait_spec: &CompilerTraitSpec,
    method_spec: &CompilerTraitMethodSpec,
) -> Result<(CompilerTraitCallablePattern, CompilerTraitEffectPattern), EmbeddedCoreVerificationError>
{
    if method_spec.signature == "callable exact signature/effects" {
        if trait_spec.generic_names != ["Signature"] {
            return Err(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority);
        }
        return Ok((
            CompilerTraitCallablePattern::ExactSignatureAndEffects {
                signature: CompilerTraitGenericParameter(0),
            },
            CompilerTraitEffectPattern::ExactSignature,
        ));
    }
    let signature = method_spec
        .signature
        .strip_prefix("fn(")
        .ok_or(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority)?;
    let (parameters, result) = signature
        .split_once(")->")
        .ok_or(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority)?;
    let mut parameters = split_top_level(parameters)?
        .into_iter()
        .map(|value| parse_trait_type_pattern(value, trait_spec.generic_names))
        .collect::<Result<Vec<_>, _>>()?;
    let result = parse_trait_type_pattern(result, trait_spec.generic_names)?;
    if method_spec.receiver != CompilerTraitReceiverMode::None {
        let expected_base = designated_self_pattern(trait_spec.designated_self);
        let expected = match method_spec.receiver {
            CompilerTraitReceiverMode::None => unreachable!(),
            CompilerTraitReceiverMode::Value => expected_base,
            CompilerTraitReceiverMode::Shared => {
                CompilerTraitTypePattern::SharedReference(Box::new(expected_base))
            }
            CompilerTraitReceiverMode::Mutable => {
                CompilerTraitTypePattern::MutableReference(Box::new(expected_base))
            }
        };
        if parameters.first() != Some(&expected) {
            return Err(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority);
        }
        parameters.remove(0);
    }
    Ok((
        CompilerTraitCallablePattern::Fixed {
            parameters: parameters.into_boxed_slice(),
            result,
        },
        CompilerTraitEffectPattern::Empty,
    ))
}

fn designated_self_pattern(relation: CompilerTraitSelfRelation) -> CompilerTraitTypePattern {
    match relation {
        CompilerTraitSelfRelation::OperatedType | CompilerTraitSelfRelation::CallableType => {
            CompilerTraitTypePattern::SelfType
        }
        CompilerTraitSelfRelation::Target(parameter)
        | CompilerTraitSelfRelation::LeftHandSide(parameter)
        | CompilerTraitSelfRelation::Input(parameter)
        | CompilerTraitSelfRelation::Source(parameter)
        | CompilerTraitSelfRelation::Iterator(parameter) => {
            CompilerTraitTypePattern::ExplicitGeneric(parameter)
        }
    }
}

fn split_top_level(value: &str) -> Result<Vec<&str>, EmbeddedCoreVerificationError> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let mut depth = 0_u32;
    let mut start = 0_usize;
    let mut values = Vec::new();
    for (index, byte) in value.bytes().enumerate() {
        match byte {
            b'<' => {
                depth = depth
                    .checked_add(1)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority)?;
            }
            b'>' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority)?;
            }
            b',' if depth == 0 => {
                if start == index {
                    return Err(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority);
                }
                values.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if depth != 0 || start == value.len() {
        return Err(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority);
    }
    values.push(&value[start..]);
    Ok(values)
}

fn parse_trait_type_pattern(
    value: &str,
    generic_names: &[&str],
) -> Result<CompilerTraitTypePattern, EmbeddedCoreVerificationError> {
    if let Some(inner) = value.strip_prefix("&mut ") {
        return Ok(CompilerTraitTypePattern::MutableReference(Box::new(
            parse_trait_type_pattern(inner, generic_names)?,
        )));
    }
    if let Some(inner) = value.strip_prefix('&') {
        return Ok(CompilerTraitTypePattern::SharedReference(Box::new(
            parse_trait_type_pattern(inner, generic_names)?,
        )));
    }
    if value == "Self" {
        return Ok(CompilerTraitTypePattern::SelfType);
    }
    if let Some(index) = generic_names.iter().position(|name| *name == value) {
        return Ok(CompilerTraitTypePattern::ExplicitGeneric(
            CompilerTraitGenericParameter(
                u8::try_from(index)
                    .map_err(|_| EmbeddedCoreVerificationError::InvalidTypedMethodAuthority)?,
            ),
        ));
    }
    match value {
        "bool" => {
            return Ok(CompilerTraitTypePattern::Primitive(
                CompilerPrimitiveTypePattern::Bool,
            ))
        }
        "i32" => {
            return Ok(CompilerTraitTypePattern::Primitive(
                CompilerPrimitiveTypePattern::I32,
            ))
        }
        "()" => {
            return Ok(CompilerTraitTypePattern::Primitive(
                CompilerPrimitiveTypePattern::Unit,
            ))
        }
        _ => {}
    }
    let (name, arguments) = value
        .split_once('<')
        .ok_or(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority)?;
    let arguments = arguments
        .strip_suffix('>')
        .ok_or(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority)?;
    let kind = match name {
        "Option" => CompilerNominalKind::Option,
        "Result" => CompilerNominalKind::Result,
        _ => return Err(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority),
    };
    let arguments = split_top_level(arguments)?
        .into_iter()
        .map(|argument| parse_trait_type_pattern(argument, generic_names))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CompilerTraitTypePattern::Nominal {
        kind,
        arguments: arguments.into_boxed_slice(),
    })
}

fn verify_typed_c2_projection(
    projection: &EmbeddedCoreC1PackageProjection,
    typed: &EmbeddedCoreC2TypedProjection,
) -> Result<(), EmbeddedCoreVerificationError> {
    if typed.compiler_traits.len() != COMPILER_TRAITS.len() {
        return Err(EmbeddedCoreVerificationError::InvalidTypedTraitAuthority);
    }
    if typed.primitive_definitions.len() != PRIMITIVE_TYPES.len() {
        return Err(EmbeddedCoreVerificationError::InvalidTypedPrimitiveAuthority);
    }
    if typed.nominals.len() != NOMINAL_TYPES.len() {
        return Err(EmbeddedCoreVerificationError::InvalidTypedNominalAuthority);
    }
    let expected_nominal_methods = build_typed_nominal_methods(projection)?;
    if typed.nominal_methods.as_ref() != expected_nominal_methods.as_ref() {
        return Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority);
    }

    let mut seen_primitives = BTreeSet::new();
    for (spec, &(definition, primitive)) in PRIMITIVE_TYPES.iter().zip(&typed.primitive_definitions)
    {
        if primitive != spec.kind || !seen_primitives.insert(definition) {
            return Err(EmbeddedCoreVerificationError::InvalidTypedPrimitiveAuthority);
        }
        let row = projection
            .definitions
            .get(usize::from(definition.0))
            .ok_or(EmbeddedCoreVerificationError::InvalidTypedPrimitiveAuthority)?;
        if row.name != spec.name
            || row.semantic_shape != spec.shape
            || row.kind != VirtualDefinitionKind::PrimitiveType
            || row.declaration_kind != VirtualDeclarationKind::Primitive
            || !source_has_definition_line(projection, row)
        {
            return Err(EmbeddedCoreVerificationError::InvalidTypedPrimitiveAuthority);
        }
    }

    for (spec, authority) in COMPILER_TRAITS.iter().zip(&typed.compiler_traits) {
        if authority.kind != spec.kind
            || authority.explicit_generic_arity
                != u8::try_from(spec.generic_names.len())
                    .map_err(|_| EmbeddedCoreVerificationError::InvalidTypedTraitAuthority)?
            || authority.designated_self != spec.designated_self
            || authority.user_impl_policy != spec.user_impl_policy
        {
            return Err(EmbeddedCoreVerificationError::InvalidTypedTraitAuthority);
        }
        let definition = projection
            .definitions
            .get(usize::from(authority.c1_definition.0))
            .ok_or(EmbeddedCoreVerificationError::InvalidTypedTraitAuthority)?;
        if definition.name != spec.name
            || definition.semantic_shape != spec.shape
            || !source_has_definition_line(projection, definition)
        {
            return Err(EmbeddedCoreVerificationError::InvalidTypedSourceAgreement);
        }
        match (spec.method, authority.method.as_ref()) {
            (Some(method_spec), Some(method)) => {
                let row = projection
                    .methods
                    .get(usize::from(method.c1_method.0))
                    .ok_or(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority)?;
                let (expected_callable, expected_effects) =
                    parse_trait_callable(spec, &method_spec)?;
                if method.kind != method_spec.kind
                    || method.source_name != method_spec.source_name
                    || method.receiver != method_spec.receiver
                    || method.callable != expected_callable
                    || method.effects != expected_effects
                    || row.owner != Some(authority.c1_definition)
                    || row.source_name != method_spec.source_name
                    || row.stable_name != method_spec.stable_name
                    || row.signature != method_spec.signature
                    || !source_has_method_line(projection, row)
                {
                    return Err(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority);
                }
            }
            (None, None) => {}
            _ => return Err(EmbeddedCoreVerificationError::InvalidTypedMethodAuthority),
        }
    }

    for (&(kind, name, shape, flavor, declaration_kind), authority) in
        NOMINAL_TYPES.iter().zip(&typed.nominals)
    {
        let definition = projection
            .definitions
            .get(usize::from(authority.definition.0))
            .ok_or(EmbeddedCoreVerificationError::InvalidTypedNominalAuthority)?;
        if authority.kind != kind
            || authority.flavor != flavor
            || authority.declaration_kind != declaration_kind
            || definition.name != name
            || definition.semantic_shape != shape
            || !source_has_definition_line(projection, definition)
        {
            return Err(EmbeddedCoreVerificationError::InvalidTypedNominalAuthority);
        }
    }
    Ok(())
}

fn source_has_definition_line(
    projection: &EmbeddedCoreC1PackageProjection,
    row: &VirtualDefinitionRow,
) -> bool {
    let line = format!(
        "definition\t{}\t{}\t{}\t{}\t{}",
        row.id.0,
        namespace_name(row.namespace),
        definition_kind_name(row.kind),
        row.name,
        row.semantic_shape
    );
    source_has_exact_line(projection.source.bytes(), line.as_bytes())
}

fn source_has_method_line(
    projection: &EmbeddedCoreC1PackageProjection,
    row: &VirtualMethodRow,
) -> bool {
    let owner = row
        .owner
        .and_then(|owner| projection.definitions.get(usize::from(owner.0)))
        .map_or("<compiler>", |definition| definition.name.as_str());
    let line = format!(
        "method\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        row.id.0, owner, row.source_name, row.stable_name, row.signature, row.requires, row.throws
    );
    source_has_exact_line(projection.source.bytes(), line.as_bytes())
}

fn source_has_exact_line(source: &[u8], expected: &[u8]) -> bool {
    source
        .split(|byte| *byte == b'\n')
        .any(|line| line == expected)
}

fn namespace_name(namespace: VirtualNamespace) -> &'static str {
    match namespace {
        VirtualNamespace::Type => "type",
        VirtualNamespace::Value => "value",
    }
}

fn definition_kind_name(kind: VirtualDefinitionKind) -> &'static str {
    match kind {
        VirtualDefinitionKind::PrimitiveType => "primitive",
        VirtualDefinitionKind::NominalType => "nominal",
        VirtualDefinitionKind::CapabilityType => "capability",
        VirtualDefinitionKind::OpaqueType => "opaque",
        VirtualDefinitionKind::Trait => "trait",
        VirtualDefinitionKind::Function => "function",
    }
}

fn definition_specs() -> Vec<DefinitionSpec> {
    let mut rows = Vec::new();

    for spec in PRIMITIVE_TYPES {
        rows.push(DefinitionSpec {
            name: spec.name,
            namespace: VirtualNamespace::Type,
            kind: VirtualDefinitionKind::PrimitiveType,
            declaration_kind: VirtualDeclarationKind::Primitive,
            shape: spec.shape,
            flavor: Some(VirtualTypeFlavor::Primitive),
            trait_policy: None,
            prelude: true,
        });
    }
    for &(_, name, shape, flavor, declaration_kind) in NOMINAL_TYPES {
        rows.push(DefinitionSpec {
            name,
            namespace: VirtualNamespace::Type,
            kind: VirtualDefinitionKind::NominalType,
            declaration_kind,
            shape,
            flavor: Some(flavor),
            trait_policy: None,
            prelude: true,
        });
    }
    for &(name, shape) in CAPABILITY_TYPES {
        rows.push(DefinitionSpec {
            name,
            namespace: VirtualNamespace::Type,
            kind: VirtualDefinitionKind::CapabilityType,
            declaration_kind: VirtualDeclarationKind::Struct,
            shape,
            flavor: Some(VirtualTypeFlavor::Capability),
            trait_policy: None,
            prelude: true,
        });
    }
    for &(name, shape) in OPAQUE_TYPES {
        rows.push(DefinitionSpec {
            name,
            namespace: VirtualNamespace::Type,
            kind: VirtualDefinitionKind::OpaqueType,
            declaration_kind: VirtualDeclarationKind::Struct,
            shape,
            flavor: Some(VirtualTypeFlavor::Opaque),
            trait_policy: None,
            prelude: true,
        });
    }
    for spec in COMPILER_TRAITS {
        rows.push(DefinitionSpec {
            name: spec.name,
            namespace: VirtualNamespace::Type,
            kind: VirtualDefinitionKind::Trait,
            declaration_kind: VirtualDeclarationKind::Trait,
            shape: spec.shape,
            flavor: None,
            trait_policy: Some(spec.user_impl_policy),
            prelude: true,
        });
    }
    for &(name, shape) in FUNCTIONS {
        rows.push(DefinitionSpec {
            name,
            namespace: VirtualNamespace::Value,
            kind: VirtualDefinitionKind::Function,
            declaration_kind: VirtualDeclarationKind::Function,
            shape,
            flavor: None,
            trait_policy: None,
            prelude: true,
        });
    }

    rows
}

#[derive(Clone, Copy)]
struct PrimitiveTypeSpec {
    kind: CompilerPrimitiveTypePattern,
    name: &'static str,
    shape: &'static str,
}

const PRIMITIVE_TYPES: &[PrimitiveTypeSpec] = &[
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::Never,
        name: "!",
        shape: "never; uninhabited",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::Unit,
        name: "()",
        shape: "unit; zero fields",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::Bool,
        name: "bool",
        shape: "scalar bool; width=1",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::Char,
        name: "char",
        shape: "scalar Unicode; width=4",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::Entity,
        name: "entity",
        shape: "scalar entity; width=8",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::F32,
        name: "f32",
        shape: "scalar IEEE754 binary32",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::F64,
        name: "f64",
        shape: "scalar IEEE754 binary64",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::I16,
        name: "i16",
        shape: "scalar signed; width=2",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::I32,
        name: "i32",
        shape: "scalar signed; width=4",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::I64,
        name: "i64",
        shape: "scalar signed; width=8",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::I8,
        name: "i8",
        shape: "scalar signed; width=1",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::Isize,
        name: "isize",
        shape: "scalar signed; width=8",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::Str,
        name: "str",
        shape: "dynamically sized UTF-8 string slice",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::U16,
        name: "u16",
        shape: "scalar unsigned; width=2",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::U32,
        name: "u32",
        shape: "scalar unsigned; width=4",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::U64,
        name: "u64",
        shape: "scalar unsigned; width=8",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::U8,
        name: "u8",
        shape: "scalar unsigned; width=1",
    },
    PrimitiveTypeSpec {
        kind: CompilerPrimitiveTypePattern::Usize,
        name: "usize",
        shape: "scalar unsigned; width=8",
    },
];

const NOMINAL_TYPES: &[(
    CompilerNominalKind,
    &str,
    &str,
    VirtualTypeFlavor,
    VirtualDeclarationKind,
)] = &[
    (
        CompilerNominalKind::AllocError,
        "AllocError",
        "pub enum AllocError { OutOfMemory }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        CompilerNominalKind::App,
        "App",
        "sealed compiler-owned App<W>; !Send + !Sync",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::Arc,
        "Arc",
        "owned atomic shared allocation Arc<T>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::ArcWeak,
        "ArcWeak",
        "nonowning atomic weak allocation ArcWeak<T>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::AtomicRmw,
        "AtomicRmw",
        "pub enum AtomicRmw { Add, Sub, And, Or, Xor, Exchange, Min, Max }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        CompilerNominalKind::Box,
        "Box",
        "unique owned allocation Box<T>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::Caps,
        "Caps",
        "sealed affine capability projection Caps<C...>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::ChannelClosed,
        "ChannelClosed",
        "pub enum ChannelClosed { Unit }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        CompilerNominalKind::Commands,
        "Commands",
        "sealed command buffer handle Commands<W>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::GeneratorState,
        "GeneratorState",
        "pub enum GeneratorState<Y,R> { Yielded(Y), Complete(R) }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        CompilerNominalKind::IoError,
        "IoError",
        "pub struct IoError { pub code:i32, pub message:String }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::Map,
        "Map",
        "owned ordered Map<K:Eq+Ord,V>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::MapIter,
        "MapIter",
        "shared-borrow iterator MapIter<'a,K,V>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::MaybeUninit,
        "MaybeUninit",
        "compiler-checked maybe-initialized storage MaybeUninit<T>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::OpenOptions,
        "OpenOptions",
        "pub struct OpenOptions { pub read:bool, pub write:bool, pub append:bool, pub truncate:bool, pub create:bool, pub create_new:bool }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::Option,
        "Option",
        "pub enum Option<T> { None, Some(T) }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        CompilerNominalKind::Ordering,
        "Ordering",
        "pub enum Ordering { Relaxed, Acquire, Release, AcqRel, SeqCst }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        CompilerNominalKind::Pin,
        "Pin",
        "pinning invariant wrapper Pin<P>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::ProcessError,
        "ProcessError",
        "pub struct ProcessError { pub code:i32, pub message:String }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::ProcessOutput,
        "ProcessOutput",
        "pub struct ProcessOutput { pub status:i32, pub stdout:Vec<u8>, pub stderr:Vec<u8> }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::ProcessSpec,
        "ProcessSpec",
        "pub struct ProcessSpec { pub program:String, pub arguments:Vec<String>, pub environment:Map<String,String>, pub current_directory:Option<String>, pub stdin:Vec<u8> }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::Query,
        "Query",
        "sealed query marker Query<Q>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::Rc,
        "Rc",
        "owned non-atomic shared allocation Rc<T>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::RcWeak,
        "RcWeak",
        "nonowning weak allocation RcWeak<T>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::Result,
        "Result",
        "pub enum Result<T,E> { Ok(T), Err(E) }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        CompilerNominalKind::SocketAddress,
        "SocketAddress",
        "pub enum SocketAddress { V4 { octets:[u8;4], port:u16 }, V6 { octets:[u8;16], port:u16, flow_info:u32, scope_id:u32 } }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        CompilerNominalKind::String,
        "String",
        "owned UTF-8 String",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::ThreadError,
        "ThreadError",
        "pub struct ThreadError { pub code:i32, pub message:String }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Struct,
    ),
    (
        CompilerNominalKind::Vec,
        "Vec",
        "owned contiguous Vec<T>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
];

const CAPABILITY_TYPES: &[(&str, &str)] = &[
    ("Args", "sealed affine Args capability"),
    ("Atomics", "sealed affine Atomics capability"),
    ("Environment", "sealed affine Environment capability"),
    ("Files", "sealed affine Files capability"),
    ("MonotonicClock", "sealed affine MonotonicClock capability"),
    ("Stdio", "sealed affine Stdio capability"),
    ("Subprocess", "sealed affine Subprocess capability"),
    (
        "Synchronization",
        "sealed affine Synchronization capability",
    ),
    ("Tcp", "sealed affine Tcp capability"),
    ("Threads", "sealed affine Threads capability"),
    ("Udp", "sealed affine Udp capability"),
    ("WallClock", "sealed affine WallClock capability"),
];

const OPAQUE_TYPES: &[(&str, &str)] = &[
    ("Atomic", "sealed atomic storage Atomic<T:AtomicScalar>"),
    ("Condvar", "sealed condition variable handle"),
    ("File", "sealed operating-system file handle"),
    ("Mutex", "sealed shared storage Mutex<T>"),
    ("MutexGuard", "sealed borrowed MutexGuard<'a,T>"),
    (
        "QueryBindings",
        "sealed descriptor-selected query result QueryBindings<Q>",
    ),
    ("QueryCursor", "sealed world-borrow cursor QueryCursor<Q>"),
    ("ReadGuard", "sealed borrowed ReadGuard<'a,T>"),
    ("Receiver", "sealed channel receiver Receiver<T>"),
    ("RwLock", "sealed shared storage RwLock<T>"),
    ("Sender", "sealed channel sender Sender<T>"),
    ("TcpListener", "sealed TCP listener handle"),
    ("TcpStream", "sealed TCP stream handle"),
    ("UdpSocket", "sealed UDP socket handle"),
    ("WriteGuard", "sealed borrowed WriteGuard<'a,T>"),
];

const SEMANTIC_TYPES: &[(&str, u8, &str)] = &[(
    "JoinHandle",
    30,
    "sealed semantic type tag 30 JoinHandle<T,E...>; result T; canonical variadic throws E; no nominal DefinitionId",
)];

const fn compiler_trait_method(
    kind: CompilerTraitMethodKind,
    source_name: &'static str,
    stable_name: &'static str,
    signature: &'static str,
    receiver: CompilerTraitReceiverMode,
) -> Option<CompilerTraitMethodSpec> {
    Some(CompilerTraitMethodSpec {
        kind,
        source_name,
        stable_name,
        signature,
        receiver,
    })
}

const COMPILER_TRAITS: &[CompilerTraitSpec] = &[
    CompilerTraitSpec {
        kind: CompilerTraitKind::Add,
        name: "Add",
        shape: "trait Add<Lhs,Rhs,Output> { fn add(Lhs,Rhs)->Output throws {} }",
        generic_names: &["Lhs", "Rhs", "Output"],
        designated_self: CompilerTraitSelfRelation::LeftHandSide(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::Add, "add", "trait.add.add", "fn(Lhs,Rhs)->Output", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::BitAnd,
        name: "BitAnd",
        shape: "trait BitAnd<Lhs,Rhs,Output> { fn bit_and(Lhs,Rhs)->Output throws {} }",
        generic_names: &["Lhs", "Rhs", "Output"],
        designated_self: CompilerTraitSelfRelation::LeftHandSide(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::BitAnd, "bit_and", "trait.bit-and.bit-and", "fn(Lhs,Rhs)->Output", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::BitNot,
        name: "BitNot",
        shape: "trait BitNot<Input,Output> { fn bit_not(Input)->Output throws {} }",
        generic_names: &["Input", "Output"],
        designated_self: CompilerTraitSelfRelation::Input(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::BitNot, "bit_not", "trait.bit-not.bit-not", "fn(Input)->Output", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::BitOr,
        name: "BitOr",
        shape: "trait BitOr<Lhs,Rhs,Output> { fn bit_or(Lhs,Rhs)->Output throws {} }",
        generic_names: &["Lhs", "Rhs", "Output"],
        designated_self: CompilerTraitSelfRelation::LeftHandSide(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::BitOr, "bit_or", "trait.bit-or.bit-or", "fn(Lhs,Rhs)->Output", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::BitXor,
        name: "BitXor",
        shape: "trait BitXor<Lhs,Rhs,Output> { fn bit_xor(Lhs,Rhs)->Output throws {} }",
        generic_names: &["Lhs", "Rhs", "Output"],
        designated_self: CompilerTraitSelfRelation::LeftHandSide(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::BitXor, "bit_xor", "trait.bit-xor.bit-xor", "fn(Lhs,Rhs)->Output", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Clone,
        name: "Clone",
        shape: "trait Clone { fn clone(&Self)->Self requires {} throws {} }",
        generic_names: &[],
        designated_self: CompilerTraitSelfRelation::OperatedType,
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::Clone, "clone", "trait.clone.clone", "fn(&Self)->Self", CompilerTraitReceiverMode::Shared),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Copy,
        name: "Copy",
        shape: "trait Copy { structural; no method; no Drop }",
        generic_names: &[],
        designated_self: CompilerTraitSelfRelation::OperatedType,
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: None,
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Div,
        name: "Div",
        shape: "trait Div<Lhs,Rhs,Output> { fn div(Lhs,Rhs)->Output throws {} }",
        generic_names: &["Lhs", "Rhs", "Output"],
        designated_self: CompilerTraitSelfRelation::LeftHandSide(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::Div, "div", "trait.div.div", "fn(Lhs,Rhs)->Output", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Drop,
        name: "Drop",
        shape: "trait Drop { fn drop(&mut Self)->() throws {}; one impl maximum }",
        generic_names: &[],
        designated_self: CompilerTraitSelfRelation::OperatedType,
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::Drop, "drop", "trait.drop.drop", "fn(&mut Self)->()", CompilerTraitReceiverMode::Mutable),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::EcsKey,
        name: "EcsKey",
        shape: "sealed structural EcsKey evidence; no method",
        generic_names: &[],
        designated_self: CompilerTraitSelfRelation::OperatedType,
        user_impl_policy: UserImplPolicy::Forbidden,
        method: None,
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::EcsValue,
        name: "EcsValue",
        shape: "sealed structural EcsValue evidence; no method",
        generic_names: &[],
        designated_self: CompilerTraitSelfRelation::OperatedType,
        user_impl_policy: UserImplPolicy::Forbidden,
        method: None,
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Eq,
        name: "Eq",
        shape: "trait Eq<Lhs,Rhs> { fn eq(&Lhs,&Rhs)->bool requires {} throws {} }",
        generic_names: &["Lhs", "Rhs"],
        designated_self: CompilerTraitSelfRelation::LeftHandSide(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::Eq, "eq", "trait.eq.eq", "fn(&Lhs,&Rhs)->bool", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Fn,
        name: "Fn",
        shape: "compiler-derived trait Fn<Signature> { fn call with exact signature/effects }",
        generic_names: &["Signature"],
        designated_self: CompilerTraitSelfRelation::CallableType,
        user_impl_policy: UserImplPolicy::CompilerDerivedOnly,
        method: compiler_trait_method(CompilerTraitMethodKind::FnCall, "call", "trait.fn.call", "callable exact signature/effects", CompilerTraitReceiverMode::Shared),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::FnMut,
        name: "FnMut",
        shape: "compiler-derived trait FnMut<Signature> { fn call with exact signature/effects }",
        generic_names: &["Signature"],
        designated_self: CompilerTraitSelfRelation::CallableType,
        user_impl_policy: UserImplPolicy::CompilerDerivedOnly,
        method: compiler_trait_method(CompilerTraitMethodKind::FnMutCall, "call", "trait.fn-mut.call", "callable exact signature/effects", CompilerTraitReceiverMode::Mutable),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::FnOnce,
        name: "FnOnce",
        shape: "compiler-derived trait FnOnce<Signature> { fn call with exact signature/effects }",
        generic_names: &["Signature"],
        designated_self: CompilerTraitSelfRelation::CallableType,
        user_impl_policy: UserImplPolicy::CompilerDerivedOnly,
        method: compiler_trait_method(CompilerTraitMethodKind::FnOnceCall, "call", "trait.fn-once.call", "callable exact signature/effects", CompilerTraitReceiverMode::Value),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::From,
        name: "From",
        shape: "trait From<Source,Target> { fn from(Source)->Target requires {} throws {} }",
        generic_names: &["Source", "Target"],
        designated_self: CompilerTraitSelfRelation::Target(CompilerTraitGenericParameter(1)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::From, "from", "trait.from.from", "fn(Source)->Target", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::IntoIterator,
        name: "IntoIterator",
        shape: "trait IntoIterator<Source,Iter> { fn into_iter(Source)->Iter requires {} throws {} }",
        generic_names: &["Source", "Iter"],
        designated_self: CompilerTraitSelfRelation::Source(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::IntoIterator, "into_iter", "trait.into-iterator.into-iter", "fn(Source)->Iter", CompilerTraitReceiverMode::Value),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Iterator,
        name: "Iterator",
        shape: "trait Iterator<Iter,Item> { fn next(&mut Iter)->Option<Item> requires {} throws {} }",
        generic_names: &["Iter", "Item"],
        designated_self: CompilerTraitSelfRelation::Iterator(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::IteratorNext, "next", "trait.iterator.next", "fn(&mut Iter)->Option<Item>", CompilerTraitReceiverMode::Mutable),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::LogicalNot,
        name: "LogicalNot",
        shape: "trait LogicalNot<Input,Output> { fn logical_not(Input)->Output throws {} }",
        generic_names: &["Input", "Output"],
        designated_self: CompilerTraitSelfRelation::Input(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::LogicalNot, "logical_not", "trait.logical-not.logical-not", "fn(Input)->Output", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Mul,
        name: "Mul",
        shape: "trait Mul<Lhs,Rhs,Output> { fn mul(Lhs,Rhs)->Output throws {} }",
        generic_names: &["Lhs", "Rhs", "Output"],
        designated_self: CompilerTraitSelfRelation::LeftHandSide(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::Mul, "mul", "trait.mul.mul", "fn(Lhs,Rhs)->Output", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Neg,
        name: "Neg",
        shape: "trait Neg<Input,Output> { fn neg(Input)->Output throws {} }",
        generic_names: &["Input", "Output"],
        designated_self: CompilerTraitSelfRelation::Input(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::Neg, "neg", "trait.neg.neg", "fn(Input)->Output", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Ord,
        name: "Ord",
        shape: "trait Ord<Lhs,Rhs> { fn compare(&Lhs,&Rhs)->i32 requires {} throws {} }",
        generic_names: &["Lhs", "Rhs"],
        designated_self: CompilerTraitSelfRelation::LeftHandSide(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::OrdCompare, "compare", "trait.ord.compare", "fn(&Lhs,&Rhs)->i32", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Rem,
        name: "Rem",
        shape: "trait Rem<Lhs,Rhs,Output> { fn rem(Lhs,Rhs)->Output throws {} }",
        generic_names: &["Lhs", "Rhs", "Output"],
        designated_self: CompilerTraitSelfRelation::LeftHandSide(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::Rem, "rem", "trait.rem.rem", "fn(Lhs,Rhs)->Output", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Send,
        name: "Send",
        shape: "compiler-derived structural Send judgment; no method",
        generic_names: &[],
        designated_self: CompilerTraitSelfRelation::OperatedType,
        user_impl_policy: UserImplPolicy::CompilerDerivedOnly,
        method: None,
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::ShiftLeft,
        name: "ShiftLeft",
        shape: "trait ShiftLeft<Lhs,Rhs,Output> { fn shift_left(Lhs,Rhs)->Output throws {} }",
        generic_names: &["Lhs", "Rhs", "Output"],
        designated_self: CompilerTraitSelfRelation::LeftHandSide(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::ShiftLeft, "shift_left", "trait.shift-left.shift-left", "fn(Lhs,Rhs)->Output", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::ShiftRight,
        name: "ShiftRight",
        shape: "trait ShiftRight<Lhs,Rhs,Output> { fn shift_right(Lhs,Rhs)->Output throws {} }",
        generic_names: &["Lhs", "Rhs", "Output"],
        designated_self: CompilerTraitSelfRelation::LeftHandSide(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::ShiftRight, "shift_right", "trait.shift-right.shift-right", "fn(Lhs,Rhs)->Output", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Sub,
        name: "Sub",
        shape: "trait Sub<Lhs,Rhs,Output> { fn sub(Lhs,Rhs)->Output throws {} }",
        generic_names: &["Lhs", "Rhs", "Output"],
        designated_self: CompilerTraitSelfRelation::LeftHandSide(CompilerTraitGenericParameter(0)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::Sub, "sub", "trait.sub.sub", "fn(Lhs,Rhs)->Output", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Sync,
        name: "Sync",
        shape: "compiler-derived structural Sync judgment; no method",
        generic_names: &[],
        designated_self: CompilerTraitSelfRelation::OperatedType,
        user_impl_policy: UserImplPolicy::CompilerDerivedOnly,
        method: None,
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::TryFrom,
        name: "TryFrom",
        shape: "trait TryFrom<Source,Target,Error> { fn try_from(Source)->Result<Target,Error> requires {} throws {} }",
        generic_names: &["Source", "Target", "Error"],
        designated_self: CompilerTraitSelfRelation::Target(CompilerTraitGenericParameter(1)),
        user_impl_policy: UserImplPolicy::AllowedAndValidated,
        method: compiler_trait_method(CompilerTraitMethodKind::TryFrom, "try_from", "trait.try-from.try-from", "fn(Source)->Result<Target,Error>", CompilerTraitReceiverMode::None),
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::Unpin,
        name: "Unpin",
        shape: "compiler-derived structural Unpin judgment; no method",
        generic_names: &[],
        designated_self: CompilerTraitSelfRelation::OperatedType,
        user_impl_policy: UserImplPolicy::CompilerDerivedOnly,
        method: None,
    },
    CompilerTraitSpec {
        kind: CompilerTraitKind::UnwindPayload,
        name: "UnwindPayload",
        shape: "sealed owned sized static unwind-payload judgment; no method",
        generic_names: &[],
        designated_self: CompilerTraitSelfRelation::OperatedType,
        user_impl_policy: UserImplPolicy::Forbidden,
        method: None,
    },
];

const FUNCTIONS: &[(&str, &str)] = &[
    (
        "include_bytes",
        "fn include_bytes<const N:usize>(literal-path)->&'static [u8;N] requires {} throws {}; intrinsic=70",
    ),
    (
        "include_str",
        "fn include_str(literal-path)->&'static str requires {} throws {}; intrinsic=71",
    ),
    (
        "panic",
        "safe fn panic<T:UnwindPayload>(T) requires {} throws {} -> !; compiler-owned body",
    ),
];

#[expect(
    clippy::too_many_arguments,
    reason = "the release-manifest table keeps each normative method field explicit"
)]
const fn intrinsic_method(
    owner: &'static str,
    source_name: &'static str,
    stable_name: &'static str,
    signature: &'static str,
    requires: &'static str,
    throws: &'static str,
    id: u16,
    ctfe: IntrinsicCtfeSupport,
) -> MethodSpec {
    MethodSpec {
        owner: Some(owner),
        source_name,
        stable_name,
        signature,
        requires,
        throws,
        lowering: VirtualMethodLowering::Intrinsic { id, ctfe },
    }
}

const fn compiler_method(
    owner: Option<&'static str>,
    source_name: &'static str,
    stable_name: &'static str,
    signature: &'static str,
    operation: CompilerOperation,
) -> MethodSpec {
    MethodSpec {
        owner,
        source_name,
        stable_name,
        signature,
        requires: "{}",
        throws: "{}",
        lowering: VirtualMethodLowering::CompilerOperation(operation),
    }
}

fn method_specs() -> Vec<MethodSpec> {
    COMPILER_TRAITS
        .iter()
        .filter_map(|trait_spec| {
            trait_spec.method.map(|method| MethodSpec {
                owner: Some(trait_spec.name),
                source_name: method.source_name,
                stable_name: method.stable_name,
                signature: method.signature,
                requires: "{}",
                throws: "{}",
                lowering: VirtualMethodLowering::TraitDispatch,
            })
        })
        .chain(METHOD_SPECS.iter().copied())
        .collect()
}

const H: IntrinsicCtfeSupport = IntrinsicCtfeSupport::Hermetic;
const N: IntrinsicCtfeSupport = IntrinsicCtfeSupport::NotExecutable;

const METHOD_SPECS: &[MethodSpec] = &[
    intrinsic_method(
        "String",
        "new",
        "string.new",
        "string.new() -> String",
        "",
        "",
        1,
        H,
    ),
    intrinsic_method(
        "String",
        "from_str",
        "string.from-str",
        "string.from-str(&str) -> String",
        "",
        "",
        2,
        H,
    ),
    intrinsic_method(
        "String",
        "len",
        "string.len",
        "string.len(&String) -> usize",
        "",
        "",
        3,
        H,
    ),
    intrinsic_method(
        "String",
        "push_str",
        "string.push-str",
        "string.push-str(&mut String, &str) -> ()",
        "",
        "",
        4,
        H,
    ),
    intrinsic_method(
        "String",
        "as_str",
        "string.as-str",
        "string.as-str(&String) -> &str",
        "",
        "",
        5,
        H,
    ),
    intrinsic_method(
        "Vec",
        "new",
        "vec.new",
        "vec.new<T>() -> Vec<T>",
        "",
        "",
        10,
        H,
    ),
    intrinsic_method(
        "Vec",
        "len",
        "vec.len",
        "vec.len<T>(&Vec<T>) -> usize",
        "",
        "",
        11,
        H,
    ),
    intrinsic_method(
        "Vec",
        "push",
        "vec.push",
        "vec.push<T>(&mut Vec<T>, T) -> ()",
        "",
        "",
        12,
        H,
    ),
    intrinsic_method(
        "Vec",
        "pop",
        "vec.pop",
        "vec.pop<T>(&mut Vec<T>) -> Option<T>",
        "",
        "",
        13,
        H,
    ),
    intrinsic_method(
        "Vec",
        "get",
        "vec.get",
        "vec.get<T>(&Vec<T>, usize) -> Option<&T>",
        "",
        "",
        14,
        H,
    ),
    intrinsic_method(
        "Map",
        "new",
        "map.new",
        "map.new<K:Eq+Ord,V>() -> Map<K,V>",
        "",
        "",
        20,
        H,
    ),
    intrinsic_method(
        "Map",
        "len",
        "map.len",
        "map.len<K,V>(&Map<K,V>) -> usize",
        "",
        "",
        21,
        H,
    ),
    intrinsic_method(
        "Map",
        "insert",
        "map.insert",
        "map.insert<K:Eq+Ord,V>(&mut Map<K,V>, K, V) -> Option<V>",
        "R{Drop(K)}",
        "",
        22,
        H,
    ),
    intrinsic_method(
        "Map",
        "get",
        "map.get",
        "map.get<K:Eq+Ord,V>(&Map<K,V>, &K) -> Option<&V>",
        "",
        "",
        23,
        H,
    ),
    intrinsic_method(
        "Map",
        "remove",
        "map.remove",
        "map.remove<K:Eq+Ord,V>(&mut Map<K,V>, &K) -> Option<V>",
        "R{Drop(K)}",
        "",
        24,
        H,
    ),
    intrinsic_method(
        "Map",
        "iter",
        "map.iter",
        "map.iter<'a,K:Eq+Ord,V>(&'a Map<K,V>) -> MapIter<'a,K,V>",
        "",
        "",
        25,
        H,
    ),
    intrinsic_method(
        "MapIter",
        "next",
        "map.next",
        "map.next<'a,'b,K,V>(&'b mut MapIter<'a,K,V>) -> Option<(&'a K,&'a V)>",
        "",
        "",
        26,
        H,
    ),
    intrinsic_method(
        "Box",
        "new",
        "box.new",
        "box.new<T>(T) -> Box<T>",
        "",
        "",
        30,
        H,
    ),
    intrinsic_method(
        "Box",
        "try_new",
        "box.try-new",
        "box.try-new<T>(T) -> Result<Box<T>,AllocError>",
        "",
        "",
        31,
        H,
    ),
    intrinsic_method(
        "Box",
        "as_ref",
        "box.as-ref",
        "box.as-ref<T>(&Box<T>) -> &T",
        "",
        "",
        32,
        H,
    ),
    intrinsic_method(
        "Box",
        "as_mut",
        "box.as-mut",
        "box.as-mut<T>(&mut Box<T>) -> &mut T",
        "",
        "",
        33,
        H,
    ),
    intrinsic_method(
        "Rc",
        "new",
        "rc.new",
        "rc.new<T>(T) -> Rc<T>",
        "",
        "",
        40,
        H,
    ),
    intrinsic_method(
        "Rc",
        "clone",
        "rc.clone",
        "rc.clone<T>(&Rc<T>) -> Rc<T>",
        "",
        "",
        41,
        H,
    ),
    intrinsic_method(
        "Rc",
        "downgrade",
        "rc.downgrade",
        "rc.downgrade<T>(&Rc<T>) -> RcWeak<T>",
        "",
        "",
        42,
        H,
    ),
    intrinsic_method(
        "RcWeak",
        "upgrade",
        "rc.upgrade",
        "rc.upgrade<T>(&RcWeak<T>) -> Option<Rc<T>>",
        "",
        "",
        43,
        H,
    ),
    intrinsic_method(
        "Rc",
        "as_ref",
        "rc.as-ref",
        "rc.as-ref<T>(&Rc<T>) -> &T",
        "",
        "",
        44,
        H,
    ),
    intrinsic_method(
        "Arc",
        "new",
        "arc.new",
        "arc.new<T>(T) -> Arc<T>",
        "",
        "",
        50,
        H,
    ),
    intrinsic_method(
        "Arc",
        "clone",
        "arc.clone",
        "arc.clone<T>(&Arc<T>) -> Arc<T>",
        "",
        "",
        51,
        H,
    ),
    intrinsic_method(
        "Arc",
        "downgrade",
        "arc.downgrade",
        "arc.downgrade<T>(&Arc<T>) -> ArcWeak<T>",
        "",
        "",
        52,
        H,
    ),
    intrinsic_method(
        "ArcWeak",
        "upgrade",
        "arc.upgrade",
        "arc.upgrade<T>(&ArcWeak<T>) -> Option<Arc<T>>",
        "",
        "",
        53,
        H,
    ),
    intrinsic_method(
        "Arc",
        "as_ref",
        "arc.as-ref",
        "arc.as-ref<T>(&Arc<T>) -> &T",
        "",
        "",
        54,
        H,
    ),
    intrinsic_method(
        "Box",
        "pin",
        "box.pin",
        "box.pin<T>(T) -> Pin<Box<T>>",
        "",
        "",
        60,
        H,
    ),
    intrinsic_method(
        "Pin",
        "as_ref",
        "pin.as-ref",
        "pin.as-ref<T>(&Pin<Box<T>>) -> Pin<&T>",
        "",
        "",
        61,
        H,
    ),
    intrinsic_method(
        "Pin",
        "as_mut",
        "pin.as-mut",
        "pin.as-mut<T>(&mut Pin<Box<T>>) -> Pin<&mut T>",
        "",
        "",
        62,
        H,
    ),
    compiler_method(
        Some("Pin"),
        "new",
        "pin.new",
        "Pin::new<T:Unpin>(&mut T) -> Pin<&mut T>",
        CompilerOperation::PinNewChecked,
    ),
    compiler_method(
        Some("Pin"),
        "new_unchecked",
        "pin.new-unchecked",
        "unsafe Pin::new_unchecked<T>(&mut T) -> Pin<&mut T>",
        CompilerOperation::PinNewUnchecked,
    ),
    compiler_method(
        Some("MaybeUninit"),
        "uninit",
        "maybe-uninit.uninit",
        "MaybeUninit::uninit<T>() -> MaybeUninit<T>",
        CompilerOperation::MaybeUninitUninit,
    ),
    compiler_method(
        Some("MaybeUninit"),
        "new",
        "maybe-uninit.new",
        "MaybeUninit::new<T>(T) -> MaybeUninit<T>",
        CompilerOperation::MaybeUninitNew,
    ),
    compiler_method(
        Some("MaybeUninit"),
        "assume_init",
        "maybe-uninit.assume-init",
        "unsafe MaybeUninit<T>.assume_init() -> T",
        CompilerOperation::MaybeUninitAssumeInit,
    ),
    compiler_method(
        None,
        "offset",
        "raw.offset",
        "unsafe pointer.offset(isize) -> pointer",
        CompilerOperation::RawOffset,
    ),
    compiler_method(
        None,
        "with_address",
        "raw.with-address",
        "unsafe pointer.with_address(usize) -> pointer",
        CompilerOperation::RawWithAddress,
    ),
    compiler_method(
        None,
        "as",
        "raw.expose-address",
        "unsafe pointer as usize",
        CompilerOperation::RawExposeAddress,
    ),
    compiler_method(
        None,
        "as",
        "pointer.cast",
        "unsafe pointer as pointer",
        CompilerOperation::PointerCast,
    ),
    compiler_method(
        Some("Caps"),
        "take",
        "caps.take",
        "Caps.take<Capability>() -> Capability",
        CompilerOperation::CapsTake,
    ),
    intrinsic_method(
        "Args",
        "all",
        "args.all",
        "args.all(&Args) -> Vec<String>",
        "R{Args}",
        "",
        100,
        N,
    ),
    intrinsic_method(
        "Environment",
        "get",
        "environment.get",
        "environment.get(&Environment, &str) -> Option<String>",
        "R{Environment}",
        "",
        101,
        N,
    ),
    intrinsic_method(
        "Stdio",
        "read",
        "stdio.read",
        "stdio.read(&mut Stdio, &mut [u8]) -> usize",
        "R{Stdio}",
        "T{IoError}",
        102,
        N,
    ),
    intrinsic_method(
        "Stdio",
        "write_out",
        "stdio.write-out",
        "stdio.write-out(&mut Stdio, &[u8]) -> ()",
        "R{Stdio}",
        "T{IoError}",
        103,
        N,
    ),
    intrinsic_method(
        "Stdio",
        "write_error",
        "stdio.write-error",
        "stdio.write-error(&mut Stdio, &[u8]) -> ()",
        "R{Stdio}",
        "T{IoError}",
        104,
        N,
    ),
    intrinsic_method(
        "Files",
        "open",
        "files.open",
        "files.open(&Files, &str, OpenOptions) -> File",
        "R{Files}",
        "T{IoError}",
        105,
        N,
    ),
    intrinsic_method(
        "Files",
        "read",
        "files.read",
        "files.read(&Files, &mut File, &mut [u8]) -> usize",
        "R{Files}",
        "T{IoError}",
        106,
        N,
    ),
    intrinsic_method(
        "Files",
        "write",
        "files.write",
        "files.write(&Files, &mut File, &[u8]) -> usize",
        "R{Files}",
        "T{IoError}",
        107,
        N,
    ),
    intrinsic_method(
        "Subprocess",
        "run",
        "subprocess.run",
        "subprocess.run(&Subprocess, ProcessSpec) -> ProcessOutput",
        "R{Subprocess}",
        "T{ProcessError}",
        108,
        N,
    ),
    intrinsic_method(
        "WallClock",
        "now",
        "clock.wall-now",
        "clock.wall-now(&WallClock) -> u64",
        "R{WallClock}",
        "",
        109,
        N,
    ),
    intrinsic_method(
        "MonotonicClock",
        "now",
        "clock.monotonic-now",
        "clock.monotonic-now(&MonotonicClock) -> u64",
        "R{MonotonicClock}",
        "",
        110,
        N,
    ),
    intrinsic_method(
        "Tcp",
        "bind",
        "tcp.bind",
        "tcp.bind(&Tcp, SocketAddress) -> TcpListener",
        "R{Tcp}",
        "T{IoError}",
        111,
        N,
    ),
    intrinsic_method(
        "Tcp",
        "connect",
        "tcp.connect",
        "tcp.connect(&Tcp, SocketAddress) -> TcpStream",
        "R{Tcp}",
        "T{IoError}",
        112,
        N,
    ),
    intrinsic_method(
        "Tcp",
        "accept",
        "tcp.accept",
        "tcp.accept(&Tcp, &mut TcpListener) -> (TcpStream,SocketAddress)",
        "R{Tcp}",
        "T{IoError}",
        113,
        N,
    ),
    intrinsic_method(
        "Tcp",
        "read",
        "tcp.read",
        "tcp.read(&Tcp, &mut TcpStream, &mut [u8]) -> usize",
        "R{Tcp}",
        "T{IoError}",
        114,
        N,
    ),
    intrinsic_method(
        "Tcp",
        "write",
        "tcp.write",
        "tcp.write(&Tcp, &mut TcpStream, &[u8]) -> usize",
        "R{Tcp}",
        "T{IoError}",
        115,
        N,
    ),
    intrinsic_method(
        "Udp",
        "bind",
        "udp.bind",
        "udp.bind(&Udp, SocketAddress) -> UdpSocket",
        "R{Udp}",
        "T{IoError}",
        116,
        N,
    ),
    intrinsic_method(
        "Udp",
        "receive",
        "udp.receive",
        "udp.receive(&Udp, &mut UdpSocket, &mut [u8]) -> (usize,SocketAddress)",
        "R{Udp}",
        "T{IoError}",
        117,
        N,
    ),
    intrinsic_method(
        "Udp",
        "send",
        "udp.send",
        "udp.send(&Udp, &mut UdpSocket, &[u8], SocketAddress) -> usize",
        "R{Udp}",
        "T{IoError}",
        118,
        N,
    ),
    intrinsic_method(
        "Threads",
        "spawn",
        "thread.spawn",
        "thread.spawn<F:FnOnce,T>(&Threads, F) -> JoinHandle<T,F.throws>",
        "R{Threads}+F.requires",
        "T{ThreadError}",
        120,
        N,
    ),
    intrinsic_method(
        "Threads",
        "scope",
        "thread.scope",
        "thread.scope<F:FnOnce,T>(&Threads, F) -> T",
        "R{Threads}+F.requires",
        "T{F.throws,ThreadError}",
        121,
        N,
    ),
    intrinsic_method(
        "Threads",
        "join",
        "thread.join",
        "thread.join<T,E...>(&Threads, JoinHandle<T,E...>) -> T",
        "R{Threads}",
        "T{E...,ThreadError}",
        122,
        N,
    ),
    intrinsic_method(
        "Atomics",
        "new",
        "atomic.new",
        "atomic.new<T:AtomicScalar>(&Atomics, T) -> Atomic<T>",
        "R{Atomics}",
        "",
        123,
        N,
    ),
    intrinsic_method(
        "Atomics",
        "load",
        "atomic.load",
        "atomic.load<T>(&Atomics, &Atomic<T>, Ordering) -> T",
        "R{Atomics}",
        "",
        124,
        N,
    ),
    intrinsic_method(
        "Atomics",
        "store",
        "atomic.store",
        "atomic.store<T>(&Atomics, &Atomic<T>, T, Ordering) -> ()",
        "R{Atomics}",
        "",
        125,
        N,
    ),
    intrinsic_method(
        "Atomics",
        "rmw",
        "atomic.rmw",
        "atomic.rmw<T>(&Atomics, &Atomic<T>, AtomicRmw, T, Ordering) -> T",
        "R{Atomics}",
        "",
        126,
        N,
    ),
    intrinsic_method(
        "Atomics",
        "compare_exchange",
        "atomic.compare-exchange",
        "atomic.compare-exchange<T>(&Atomics, &Atomic<T>, T, T, Ordering, Ordering) -> Result<T,T>",
        "R{Atomics}",
        "",
        127,
        N,
    ),
    intrinsic_method(
        "Synchronization",
        "mutex_new",
        "mutex.new",
        "mutex.new<T>(&Synchronization, T) -> Mutex<T>",
        "R{Synchronization}",
        "",
        128,
        N,
    ),
    intrinsic_method(
        "Synchronization",
        "mutex_lock",
        "mutex.lock",
        "mutex.lock<T>(&Synchronization, &Mutex<T>) -> MutexGuard<T>",
        "R{Synchronization}",
        "",
        129,
        N,
    ),
    intrinsic_method(
        "Synchronization",
        "rwlock_new",
        "rwlock.new",
        "rwlock.new<T>(&Synchronization, T) -> RwLock<T>",
        "R{Synchronization}",
        "",
        130,
        N,
    ),
    intrinsic_method(
        "Synchronization",
        "rwlock_read",
        "rwlock.read",
        "rwlock.read<T>(&Synchronization, &RwLock<T>) -> ReadGuard<T>",
        "R{Synchronization}",
        "",
        131,
        N,
    ),
    intrinsic_method(
        "Synchronization",
        "rwlock_write",
        "rwlock.write",
        "rwlock.write<T>(&Synchronization, &RwLock<T>) -> WriteGuard<T>",
        "R{Synchronization}",
        "",
        132,
        N,
    ),
    intrinsic_method(
        "Synchronization",
        "condvar_new",
        "condvar.new",
        "condvar.new(&Synchronization) -> Condvar",
        "R{Synchronization}",
        "",
        133,
        N,
    ),
    intrinsic_method(
        "Synchronization",
        "condvar_wait",
        "condvar.wait",
        "condvar.wait<T>(&Synchronization, &Condvar, MutexGuard<T>) -> MutexGuard<T>",
        "R{Synchronization}",
        "",
        134,
        N,
    ),
    intrinsic_method(
        "Synchronization",
        "condvar_notify_one",
        "condvar.notify-one",
        "condvar.notify-one(&Synchronization, &Condvar) -> ()",
        "R{Synchronization}",
        "",
        135,
        N,
    ),
    intrinsic_method(
        "Synchronization",
        "condvar_notify_all",
        "condvar.notify-all",
        "condvar.notify-all(&Synchronization, &Condvar) -> ()",
        "R{Synchronization}",
        "",
        136,
        N,
    ),
    intrinsic_method(
        "Synchronization",
        "channel_new",
        "channel.new",
        "channel.new<T>(&Synchronization) -> (Sender<T>,Receiver<T>)",
        "R{Synchronization}",
        "",
        140,
        N,
    ),
    intrinsic_method(
        "Synchronization",
        "channel_send",
        "channel.send",
        "channel.send<T>(&Synchronization, &Sender<T>, T) -> Result<(),ChannelClosed>",
        "R{Synchronization}",
        "",
        141,
        N,
    ),
    intrinsic_method(
        "Synchronization",
        "channel_receive",
        "channel.receive",
        "channel.receive<T>(&Synchronization, &Receiver<T>) -> Result<T,ChannelClosed>",
        "R{Synchronization}",
        "",
        142,
        N,
    ),
    intrinsic_method(
        "App",
        "run",
        "app.run",
        "app.run<W,C>(&mut App<W>, &mut Caps<C>; schedule:DefinitionId) -> ()",
        "R{schedule}",
        "T{schedule}",
        200,
        N,
    ),
    intrinsic_method(
        "App",
        "resource",
        "resource.read",
        "resource.read<W,T>(&App<W>) -> &T",
        "",
        "",
        201,
        N,
    ),
    intrinsic_method(
        "App",
        "resource_mut",
        "resource.write",
        "resource.write<W,T>(&mut App<W>) -> &mut T",
        "",
        "",
        202,
        N,
    ),
    intrinsic_method(
        "Query",
        "open",
        "query.open",
        "query.open<W,Q>(&mut App<W>) -> QueryCursor<Q>",
        "",
        "",
        203,
        N,
    ),
    intrinsic_method(
        "Query",
        "next",
        "query.next",
        "query.next<Q>(&mut QueryCursor<Q>) -> Option<QueryBindings<Q>>",
        "",
        "",
        204,
        N,
    ),
    intrinsic_method(
        "Query",
        "close",
        "query.close",
        "query.close<Q>(QueryCursor<Q>) -> ()",
        "",
        "",
        205,
        N,
    ),
    intrinsic_method(
        "Commands",
        "spawn",
        "commands.spawn",
        "commands.spawn<W,B>(&mut Commands<W>, B) -> entity",
        "",
        "",
        206,
        N,
    ),
    intrinsic_method(
        "Commands",
        "despawn",
        "commands.despawn",
        "commands.despawn<W>(&mut Commands<W>, entity) -> ()",
        "",
        "",
        207,
        N,
    ),
    intrinsic_method(
        "Commands",
        "add",
        "commands.add",
        "commands.add<W,T:EcsValue>(&mut Commands<W>, entity, T) -> ()",
        "",
        "",
        208,
        N,
    ),
    intrinsic_method(
        "Commands",
        "remove",
        "commands.remove",
        "commands.remove<W,T:EcsValue>(&mut Commands<W>, entity) -> ()",
        "",
        "",
        209,
        N,
    ),
    intrinsic_method(
        "App",
        "init_resource",
        "world.init-resource",
        "world.init-resource<W,T:EcsValue>(&mut App<W>, T) -> ()",
        "",
        "",
        210,
        N,
    ),
    intrinsic_method(
        "App",
        "init_spawn",
        "world.init-spawn",
        "world.init-spawn<W,B>(&mut App<W>, B) -> entity",
        "",
        "",
        211,
        N,
    ),
];

fn encode_public_interface(
    definitions: &[VirtualDefinitionRow],
    enum_variants: &[VirtualEnumVariantRow],
    record_constructors: &[VirtualRecordConstructorRow],
    semantic_types: &[VirtualSemanticTypeRow],
    traits: &[VirtualTraitRow],
    methods: &[VirtualMethodRow],
    prelude: &[VirtualPreludeBindingRow],
) -> Vec<u8> {
    let mut bytes = b"ARCHE-EMBEDDED-CORE-PUBLIC-C1\0".to_vec();
    push_u32(&mut bytes, RELEASE_PROJECTION_FORMAT_VERSION);
    push_rows(&mut bytes, definitions, encode_definition);
    push_rows(&mut bytes, enum_variants, encode_enum_variant);
    push_rows(&mut bytes, record_constructors, encode_record_constructor);
    push_rows(&mut bytes, semantic_types, encode_semantic_type);
    push_rows(&mut bytes, traits, encode_trait);
    push_rows(&mut bytes, methods, encode_method);
    push_rows(&mut bytes, prelude, encode_prelude);
    bytes
}

fn encode_projection(projection: &EmbeddedCoreC1PackageProjection) -> Vec<u8> {
    let mut bytes = b"ARCHE-EMBEDDED-CORE-C1-PROJECTION\0".to_vec();
    push_u32(&mut bytes, RELEASE_PROJECTION_FORMAT_VERSION);
    push_u32(&mut bytes, projection.interface_version);
    bytes.extend_from_slice(projection.package.as_bytes());
    push_string(&mut bytes, &projection.registry_origin);
    push_string(&mut bytes, &projection.scoped_name);
    push_string(&mut bytes, &projection.version);
    bytes.extend_from_slice(&[0; 32]);
    bytes.extend_from_slice(projection.public_interface_hash.as_bytes());
    push_u64(&mut bytes, projection.source.file_id().0);
    push_string(&mut bytes, projection.source.package_path());
    push_bytes(&mut bytes, projection.source.bytes());
    bytes.extend_from_slice(projection.source.digest());
    bytes.extend_from_slice(projection.root_module.package.as_bytes());
    push_u64(&mut bytes, projection.root_module.file.0);
    push_u64(
        &mut bytes,
        u64::try_from(projection.root_module.path.len()).unwrap_or(u64::MAX),
    );
    for segment in &projection.root_module.path {
        push_string(&mut bytes, segment);
    }
    push_u64(&mut bytes, 0); // dependencies
    push_u64(&mut bytes, 0); // CTFE step budget
    push_u64(&mut bytes, 0); // CTFE depth budget
    push_u64(&mut bytes, 0); // CTFE heap budget
    push_u64(&mut bytes, 0); // targets
    push_rows(&mut bytes, &projection.definitions, encode_definition);
    push_rows(&mut bytes, &projection.types, encode_type);
    push_rows(&mut bytes, &projection.enum_variants, encode_enum_variant);
    push_rows(
        &mut bytes,
        &projection.record_constructors,
        encode_record_constructor,
    );
    push_rows(&mut bytes, &projection.semantic_types, encode_semantic_type);
    push_rows(&mut bytes, &projection.traits, encode_trait);
    push_rows(&mut bytes, &projection.methods, encode_method);
    push_rows(&mut bytes, &projection.functions, encode_function);
    push_rows(&mut bytes, &projection.prelude, encode_prelude);
    encode_panic(&mut bytes, &projection.panic_body);
    bytes
}

fn digest_interface_projection(package_bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ARCHE-EMBEDDED-CORE\0");
    hasher.update(1_u32.to_le_bytes());
    hasher.update(EMBEDDED_CORE_INTERFACE_VERSION.to_le_bytes());
    hasher.update(
        u64::try_from(package_bytes.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    hasher.update(package_bytes);
    hasher.finalize().into()
}

fn push_rows<T>(bytes: &mut Vec<u8>, rows: &[T], encode: fn(&mut Vec<u8>, &T)) {
    push_u64(bytes, u64::try_from(rows.len()).unwrap_or(u64::MAX));
    for row in rows {
        encode(bytes, row);
    }
}

fn encode_definition(bytes: &mut Vec<u8>, row: &VirtualDefinitionRow) {
    push_u16(bytes, row.id.0);
    bytes.push(row.namespace as u8);
    bytes.push(row.kind as u8);
    bytes.push(row.declaration_kind as u8);
    push_string(bytes, &row.name);
    push_string(bytes, &row.semantic_shape);
    encode_span(bytes, row.span);
}

fn encode_type(bytes: &mut Vec<u8>, row: &VirtualTypeRow) {
    push_u16(bytes, row.definition.0);
    bytes.push(row.flavor as u8);
}

fn encode_enum_variant(bytes: &mut Vec<u8>, row: &VirtualEnumVariantRow) {
    push_u16(bytes, row.id.0);
    push_u16(bytes, row.owner.0);
    push_u64(bytes, row.ordinal);
    push_string(bytes, &row.name);
    encode_span(bytes, row.span);
}

fn encode_record_constructor(bytes: &mut Vec<u8>, row: &VirtualRecordConstructorRow) {
    push_u16(bytes, row.owner.0);
}

fn encode_semantic_type(bytes: &mut Vec<u8>, row: &VirtualSemanticTypeRow) {
    push_u16(bytes, row.id.0);
    push_string(bytes, &row.spelling);
    bytes.push(row.semantic_tag);
    push_string(bytes, &row.shape);
    encode_span(bytes, row.span);
}

fn encode_trait(bytes: &mut Vec<u8>, row: &VirtualTraitRow) {
    push_u16(bytes, row.definition.0);
    bytes.push(row.user_impl_policy as u8);
    push_u64(bytes, u64::try_from(row.methods.len()).unwrap_or(u64::MAX));
    for method in &row.methods {
        push_u16(bytes, method.0);
    }
}

fn encode_method(bytes: &mut Vec<u8>, row: &VirtualMethodRow) {
    push_u16(bytes, row.id.0);
    encode_optional_definition(bytes, row.owner);
    push_string(bytes, &row.source_name);
    push_string(bytes, &row.stable_name);
    push_string(bytes, &row.signature);
    push_string(bytes, &row.requires);
    push_string(bytes, &row.throws);
    match row.lowering {
        VirtualMethodLowering::TraitDispatch => bytes.push(1),
        VirtualMethodLowering::Intrinsic { id, ctfe } => {
            bytes.push(2);
            push_u16(bytes, id);
            bytes.push(ctfe as u8);
        }
        VirtualMethodLowering::CompilerOperation(operation) => {
            bytes.push(3);
            bytes.push(operation as u8);
        }
    }
    encode_span(bytes, row.span);
}

fn encode_function(bytes: &mut Vec<u8>, row: &VirtualFunctionRow) {
    push_u16(bytes, row.definition.0);
    match row.lowering {
        VirtualFunctionLowering::Intrinsic { id, ctfe } => {
            bytes.push(1);
            push_u16(bytes, id);
            bytes.push(ctfe as u8);
        }
        VirtualFunctionLowering::CompilerOwnedBody => bytes.push(2),
    }
}

fn encode_prelude(bytes: &mut Vec<u8>, row: &VirtualPreludeBindingRow) {
    bytes.push(row.namespace as u8);
    push_string(bytes, &row.spelling);
    match row.target {
        VirtualPreludeTarget::Definition(definition) => {
            bytes.push(1);
            push_u16(bytes, definition.0);
        }
        VirtualPreludeTarget::SemanticType(semantic_type) => {
            bytes.push(2);
            push_u16(bytes, semantic_type.0);
        }
    }
}

fn encode_panic(bytes: &mut Vec<u8>, row: &CompilerOwnedPanicBodyAuthority) {
    push_u16(bytes, row.owner.0);
    push_string(bytes, &row.generic_parameter_shape);
    push_string(bytes, &row.parameter_type);
    push_string(bytes, &row.result_type);
    push_u64(
        bytes,
        u64::try_from(row.declared_requires.len()).unwrap_or(u64::MAX),
    );
    for requirement in &row.declared_requires {
        push_u16(bytes, requirement.0);
    }
    push_u64(
        bytes,
        u64::try_from(row.declared_throws.len()).unwrap_or(u64::MAX),
    );
    for thrown in &row.declared_throws {
        push_u16(bytes, thrown.0);
    }
    bytes.push(match row.terminal {
        PanicTerminal::User => 1,
    });
    push_string(bytes, &row.required_ctfe_failure);
    push_bytes(bytes, &row.symbolic_body_bytes);
    encode_span(bytes, row.span);
}

fn encode_optional_definition(bytes: &mut Vec<u8>, value: Option<VirtualDefinitionId>) {
    match value {
        None => bytes.push(0),
        Some(value) => {
            bytes.push(1);
            push_u16(bytes, value.0);
        }
    }
}

fn encode_span(bytes: &mut Vec<u8>, span: Span) {
    push_u64(bytes, span.file.0);
    push_u64(bytes, span.start.byte);
    push_u64(bytes, span.end.byte);
    push_u64(bytes, span.start.line);
    push_u64(bytes, span.start.column);
    push_u64(bytes, span.end.line);
    push_u64(bytes, span.end.column);
}

fn push_string(bytes: &mut Vec<u8>, value: &str) {
    push_bytes(bytes, value.as_bytes());
}

fn push_bytes(bytes: &mut Vec<u8>, value: &[u8]) {
    push_u64(bytes, u64::try_from(value.len()).unwrap_or(u64::MAX));
    bytes.extend_from_slice(value);
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn verify_projection(
    projection: &EmbeddedCoreC1PackageProjection,
) -> Result<(), EmbeddedCoreVerificationError> {
    if projection.package.into_bytes() != RELEASE_PACKAGE_ID {
        return Err(EmbeddedCoreVerificationError::PackageIdentityMismatch);
    }
    if projection.interface_version != EMBEDDED_CORE_INTERFACE_VERSION
        || projection.registry_origin != OFFICIAL_REGISTRY_IDENTITY
        || projection.scoped_name != EMBEDDED_CORE_SCOPED_NAME
        || projection.version != EMBEDDED_CORE_PACKAGE_VERSION
    {
        return Err(EmbeddedCoreVerificationError::InterfaceDigestMismatch);
    }
    let actual_source_digest: [u8; 32] = Sha256::digest(projection.source.bytes()).into();
    if actual_source_digest != RELEASE_SOURCE_DIGEST
        || projection.source.digest != RELEASE_SOURCE_DIGEST
        || projection.source.file_id() != EMBEDDED_CORE_FILE_ID
        || projection.source.package_path != EMBEDDED_CORE_PACKAGE_PATH
    {
        return Err(EmbeddedCoreVerificationError::SyntheticSourceDigestMismatch);
    }
    if projection.root_module.package != projection.package
        || projection.root_module.file != EMBEDDED_CORE_FILE_ID
        || !projection.root_module.path.is_empty()
    {
        return Err(EmbeddedCoreVerificationError::InvalidReference(
            "root module",
        ));
    }
    let encoded = encode_projection(projection);
    if encoded.as_slice() != projection.canonical_bytes.as_ref()
        || digest_interface_projection(&encoded) != RELEASE_INTERFACE_DIGEST
        || projection.interface_digest != RELEASE_INTERFACE_DIGEST
    {
        return Err(EmbeddedCoreVerificationError::InterfaceDigestMismatch);
    }
    let public = encode_public_interface(
        &projection.definitions,
        &projection.enum_variants,
        &projection.record_constructors,
        &projection.semantic_types,
        &projection.traits,
        &projection.methods,
        &projection.prelude,
    );
    if InterfaceHash::from_canonical_preimage(&public).into_bytes() != RELEASE_INTERFACE_HASH
        || projection.public_interface_hash.into_bytes() != RELEASE_INTERFACE_HASH
    {
        return Err(EmbeddedCoreVerificationError::InterfaceHashMismatch);
    }
    verify_definition_rows(projection)?;
    verify_type_rows(projection)?;
    verify_enum_variant_rows(projection)?;
    verify_record_constructor_rows(projection)?;
    verify_semantic_type_rows(projection)?;
    verify_trait_rows(projection)?;
    verify_method_rows(projection)?;
    verify_function_rows(projection)?;
    verify_prelude_rows(projection)?;
    verify_panic_body(projection)?;
    Ok(())
}

fn verify_definition_rows(
    projection: &EmbeddedCoreC1PackageProjection,
) -> Result<(), EmbeddedCoreVerificationError> {
    let mut expected = definition_specs();
    expected.sort_by(|left, right| {
        (left.namespace, left.name, left.kind).cmp(&(right.namespace, right.name, right.kind))
    });
    if projection.definitions.len() != expected.len() {
        return Err(EmbeddedCoreVerificationError::InvalidReference(
            "definition declaration kind",
        ));
    }
    for (index, (row, expected)) in projection.definitions.iter().zip(expected).enumerate() {
        if usize::from(row.id.0) != index {
            return Err(EmbeddedCoreVerificationError::NonCanonicalOrder(
                "definition",
            ));
        }
        if index > 0 {
            let previous = &projection.definitions[index - 1];
            let previous_key = (previous.namespace, previous.name.as_str(), previous.kind);
            let current_key = (row.namespace, row.name.as_str(), row.kind);
            if previous_key >= current_key {
                return Err(if previous_key == current_key {
                    EmbeddedCoreVerificationError::DuplicateRow("definition")
                } else {
                    EmbeddedCoreVerificationError::NonCanonicalOrder("definition")
                });
            }
        }
        if row.namespace != expected.namespace
            || row.kind != expected.kind
            || row.name != expected.name
            || row.declaration_kind != expected.declaration_kind
        {
            return Err(EmbeddedCoreVerificationError::InvalidReference(
                "definition declaration kind",
            ));
        }
        verify_named_span(projection.source.bytes(), row.span, row.name.as_bytes())?;
    }
    Ok(())
}

fn verify_type_rows(
    projection: &EmbeddedCoreC1PackageProjection,
) -> Result<(), EmbeddedCoreVerificationError> {
    let mut previous = None;
    for row in &projection.types {
        let definition = projection
            .definitions
            .get(usize::from(row.definition.0))
            .ok_or(EmbeddedCoreVerificationError::InvalidReference("type"))?;
        if !matches!(
            definition.kind,
            VirtualDefinitionKind::PrimitiveType
                | VirtualDefinitionKind::NominalType
                | VirtualDefinitionKind::CapabilityType
                | VirtualDefinitionKind::OpaqueType
        ) {
            return Err(EmbeddedCoreVerificationError::InvalidReference("type"));
        }
        if previous.is_some_and(|value| value >= row.definition) {
            return Err(EmbeddedCoreVerificationError::NonCanonicalOrder("type"));
        }
        previous = Some(row.definition);
    }
    Ok(())
}

fn verify_enum_variant_rows(
    projection: &EmbeddedCoreC1PackageProjection,
) -> Result<(), EmbeddedCoreVerificationError> {
    let mut expected = ENUM_VARIANTS
        .iter()
        .map(|spec| {
            let owner =
                find_definition_id(&projection.definitions, spec.owner, VirtualNamespace::Type)
                    .ok_or(EmbeddedCoreVerificationError::InvalidReference(
                        "enum variant owner",
                    ))?;
            Ok((owner, *spec))
        })
        .collect::<Result<Vec<_>, EmbeddedCoreVerificationError>>()?;
    expected.sort_by_key(|(owner, spec)| (*owner, spec.ordinal));
    if projection.enum_variants.len() != expected.len() {
        return Err(EmbeddedCoreVerificationError::InvalidReference(
            "enum variant",
        ));
    }

    for (index, (row, (expected_owner, expected_spec))) in
        projection.enum_variants.iter().zip(expected).enumerate()
    {
        if usize::from(row.id.0) != index {
            return Err(EmbeddedCoreVerificationError::NonCanonicalOrder(
                "enum variant",
            ));
        }
        if index > 0 {
            let previous = &projection.enum_variants[index - 1];
            let previous_key = (previous.owner, previous.ordinal);
            let current_key = (row.owner, row.ordinal);
            if previous_key >= current_key {
                return Err(if previous_key == current_key {
                    EmbeddedCoreVerificationError::DuplicateRow("enum variant")
                } else {
                    EmbeddedCoreVerificationError::NonCanonicalOrder("enum variant")
                });
            }
        }
        let owner = projection.definitions.get(usize::from(row.owner.0)).ok_or(
            EmbeddedCoreVerificationError::InvalidReference("enum variant owner"),
        )?;
        if row.owner != expected_owner
            || row.ordinal != expected_spec.ordinal
            || row.name != expected_spec.name
            || owner.kind != VirtualDefinitionKind::NominalType
            || owner.declaration_kind != VirtualDeclarationKind::Enum
        {
            return Err(EmbeddedCoreVerificationError::InvalidReference(
                "enum variant",
            ));
        }
        verify_named_span(projection.source.bytes(), row.span, row.name.as_bytes())?;
    }
    Ok(())
}

fn verify_record_constructor_rows(
    projection: &EmbeddedCoreC1PackageProjection,
) -> Result<(), EmbeddedCoreVerificationError> {
    let mut expected = CONSTRUCTIBLE_RECORDS
        .iter()
        .map(|name| {
            find_definition_id(&projection.definitions, name, VirtualNamespace::Type).ok_or(
                EmbeddedCoreVerificationError::InvalidReference("record constructor"),
            )
        })
        .collect::<Result<Vec<_>, EmbeddedCoreVerificationError>>()?;
    expected.sort_unstable();
    if projection.record_constructors.len() != expected.len() {
        return Err(EmbeddedCoreVerificationError::InvalidReference(
            "record constructor",
        ));
    }

    for (index, (row, expected_owner)) in projection
        .record_constructors
        .iter()
        .zip(expected)
        .enumerate()
    {
        if index > 0 {
            let previous = projection.record_constructors[index - 1].owner;
            if previous >= row.owner {
                return Err(if previous == row.owner {
                    EmbeddedCoreVerificationError::DuplicateRow("record constructor")
                } else {
                    EmbeddedCoreVerificationError::NonCanonicalOrder("record constructor")
                });
            }
        }
        let owner = projection.definitions.get(usize::from(row.owner.0)).ok_or(
            EmbeddedCoreVerificationError::InvalidReference("record constructor"),
        )?;
        if row.owner != expected_owner
            || owner.kind != VirtualDefinitionKind::NominalType
            || owner.declaration_kind != VirtualDeclarationKind::Struct
        {
            return Err(EmbeddedCoreVerificationError::InvalidReference(
                "record constructor",
            ));
        }
    }
    Ok(())
}

fn verify_semantic_type_rows(
    projection: &EmbeddedCoreC1PackageProjection,
) -> Result<(), EmbeddedCoreVerificationError> {
    for (index, row) in projection.semantic_types.iter().enumerate() {
        if usize::from(row.id.0) != index {
            return Err(EmbeddedCoreVerificationError::NonCanonicalOrder(
                "semantic type",
            ));
        }
        if index > 0 && projection.semantic_types[index - 1].spelling >= row.spelling {
            return Err(EmbeddedCoreVerificationError::NonCanonicalOrder(
                "semantic type",
            ));
        }
        verify_named_span(projection.source.bytes(), row.span, row.spelling.as_bytes())?;
    }
    if projection.semantic_types.len() != 1
        || projection.semantic_types[0].spelling != "JoinHandle"
        || projection.semantic_types[0].semantic_tag != 30
    {
        return Err(EmbeddedCoreVerificationError::InvalidReference(
            "semantic type",
        ));
    }
    Ok(())
}

fn verify_trait_rows(
    projection: &EmbeddedCoreC1PackageProjection,
) -> Result<(), EmbeddedCoreVerificationError> {
    let mut previous = None;
    for row in &projection.traits {
        let definition = projection
            .definitions
            .get(usize::from(row.definition.0))
            .ok_or(EmbeddedCoreVerificationError::InvalidReference("trait"))?;
        if definition.kind != VirtualDefinitionKind::Trait {
            return Err(EmbeddedCoreVerificationError::InvalidReference("trait"));
        }
        if previous.is_some_and(|value| value >= row.definition) {
            return Err(EmbeddedCoreVerificationError::NonCanonicalOrder("trait"));
        }
        previous = Some(row.definition);
        let mut previous_method = None;
        for method in &row.methods {
            let method_row = projection.methods.get(usize::from(method.0)).ok_or(
                EmbeddedCoreVerificationError::InvalidReference("trait method"),
            )?;
            if method_row.owner != Some(row.definition)
                || method_row.lowering != VirtualMethodLowering::TraitDispatch
            {
                return Err(EmbeddedCoreVerificationError::InvalidReference(
                    "trait method",
                ));
            }
            if previous_method.is_some_and(|value| value >= *method) {
                return Err(EmbeddedCoreVerificationError::NonCanonicalOrder(
                    "trait method",
                ));
            }
            previous_method = Some(*method);
        }
    }
    Ok(())
}

fn verify_method_rows(
    projection: &EmbeddedCoreC1PackageProjection,
) -> Result<(), EmbeddedCoreVerificationError> {
    for (index, row) in projection.methods.iter().enumerate() {
        if usize::from(row.id.0) != index {
            return Err(EmbeddedCoreVerificationError::NonCanonicalOrder("method"));
        }
        if index > 0 && projection.methods[index - 1].stable_name >= row.stable_name {
            return Err(
                if projection.methods[index - 1].stable_name == row.stable_name {
                    EmbeddedCoreVerificationError::DuplicateRow("method")
                } else {
                    EmbeddedCoreVerificationError::NonCanonicalOrder("method")
                },
            );
        }
        if let Some(owner) = row.owner {
            projection.definitions.get(usize::from(owner.0)).ok_or(
                EmbeddedCoreVerificationError::InvalidReference("method owner"),
            )?;
        }
        verify_named_span(
            projection.source.bytes(),
            row.span,
            row.source_name.as_bytes(),
        )?;
    }
    let mut operations = projection
        .methods
        .iter()
        .filter_map(|row| match row.lowering {
            VirtualMethodLowering::CompilerOperation(operation) => Some(operation),
            VirtualMethodLowering::TraitDispatch | VirtualMethodLowering::Intrinsic { .. } => None,
        })
        .collect::<Vec<_>>();
    operations.sort_unstable();
    if operations.as_slice() != COMPILER_OPERATIONS {
        return Err(EmbeddedCoreVerificationError::InvalidReference(
            "compiler operation",
        ));
    }
    Ok(())
}

fn verify_function_rows(
    projection: &EmbeddedCoreC1PackageProjection,
) -> Result<(), EmbeddedCoreVerificationError> {
    let expected = [("include_bytes", 70_u16), ("include_str", 71_u16)];
    for (name, intrinsic) in expected {
        let definition = find_definition_id(&projection.definitions, name, VirtualNamespace::Value)
            .ok_or(EmbeddedCoreVerificationError::InvalidReference("function"))?;
        if !projection.functions.iter().any(|row| {
            row.definition == definition
                && row.lowering
                    == VirtualFunctionLowering::Intrinsic {
                        id: intrinsic,
                        ctfe: IntrinsicCtfeSupport::IncludeAuthority,
                    }
        }) {
            return Err(EmbeddedCoreVerificationError::InvalidReference("function"));
        }
    }
    let panic = find_definition_id(&projection.definitions, "panic", VirtualNamespace::Value)
        .ok_or(EmbeddedCoreVerificationError::InvalidPanicBody)?;
    if projection.functions.len() != 3
        || !projection.functions.iter().any(|row| {
            row.definition == panic && row.lowering == VirtualFunctionLowering::CompilerOwnedBody
        })
    {
        return Err(EmbeddedCoreVerificationError::InvalidReference("function"));
    }
    Ok(())
}

fn verify_prelude_rows(
    projection: &EmbeddedCoreC1PackageProjection,
) -> Result<(), EmbeddedCoreVerificationError> {
    for (index, row) in projection.prelude.iter().enumerate() {
        match row.target {
            VirtualPreludeTarget::Definition(definition) => {
                let definition = projection
                    .definitions
                    .get(usize::from(definition.0))
                    .ok_or(EmbeddedCoreVerificationError::InvalidReference("prelude"))?;
                if definition.name != row.spelling || definition.namespace != row.namespace {
                    return Err(EmbeddedCoreVerificationError::InvalidReference("prelude"));
                }
            }
            VirtualPreludeTarget::SemanticType(semantic_type) => {
                let semantic_type = projection
                    .semantic_types
                    .get(usize::from(semantic_type.0))
                    .ok_or(EmbeddedCoreVerificationError::InvalidReference("prelude"))?;
                if semantic_type.spelling != row.spelling || row.namespace != VirtualNamespace::Type
                {
                    return Err(EmbeddedCoreVerificationError::InvalidReference("prelude"));
                }
            }
        }
        if index > 0 {
            let previous = &projection.prelude[index - 1];
            let previous_key = (previous.namespace, previous.spelling.as_str());
            let current_key = (row.namespace, row.spelling.as_str());
            if previous_key >= current_key {
                return Err(if previous_key == current_key {
                    EmbeddedCoreVerificationError::DuplicateRow("prelude")
                } else {
                    EmbeddedCoreVerificationError::NonCanonicalOrder("prelude")
                });
            }
        }
    }
    if projection.prelude.len() != projection.definitions.len() + projection.semantic_types.len() {
        return Err(EmbeddedCoreVerificationError::InvalidReference("prelude"));
    }
    Ok(())
}

fn verify_panic_body(
    projection: &EmbeddedCoreC1PackageProjection,
) -> Result<(), EmbeddedCoreVerificationError> {
    let body = &projection.panic_body;
    let owner = projection
        .definitions
        .get(usize::from(body.owner.0))
        .ok_or(EmbeddedCoreVerificationError::InvalidPanicBody)?;
    if owner.name != "panic"
        || owner.namespace != VirtualNamespace::Value
        || body.generic_parameter_shape != "type T: UnwindPayload"
        || body.parameter_type != "T"
        || body.result_type != "!"
        || !body.declared_requires.is_empty()
        || !body.declared_throws.is_empty()
        || body.terminal != PanicTerminal::User
        || body.required_ctfe_failure != "CTFE006"
        || body.symbolic_body_bytes.as_ref()
            != b"ARCHE-PANIC-SYMBOLIC 1\0owner=panic;terminal=PanicKind::User;payload=T"
        || body.span != owner.span
    {
        return Err(EmbeddedCoreVerificationError::InvalidPanicBody);
    }
    Ok(())
}

fn verify_named_span(
    source: &[u8],
    span: Span,
    expected: &[u8],
) -> Result<(), EmbeddedCoreVerificationError> {
    if span.file != EMBEDDED_CORE_FILE_ID
        || span.start.line != span.end.line
        || span.start.column > span.end.column
    {
        return Err(EmbeddedCoreVerificationError::InvalidSpan);
    }
    let start =
        usize::try_from(span.start.byte).map_err(|_| EmbeddedCoreVerificationError::InvalidSpan)?;
    let end =
        usize::try_from(span.end.byte).map_err(|_| EmbeddedCoreVerificationError::InvalidSpan)?;
    if source.get(start..end) != Some(expected) {
        return Err(EmbeddedCoreVerificationError::InvalidSpan);
    }
    Ok(())
}

mod private {
    #[derive(Debug)]
    pub(super) struct Seal;

    #[derive(Debug)]
    pub(super) struct TypedSeal;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write as _;
            write!(&mut output, "{byte:02x}").unwrap();
        }
        output
    }

    fn recompute_local_commitments(projection: &mut EmbeddedCoreC1PackageProjection) {
        let public = encode_public_interface(
            &projection.definitions,
            &projection.enum_variants,
            &projection.record_constructors,
            &projection.semantic_types,
            &projection.traits,
            &projection.methods,
            &projection.prelude,
        );
        projection.public_interface_hash = InterfaceHash::from_canonical_preimage(&public);
        let canonical = encode_projection(projection);
        projection.interface_digest = digest_interface_projection(&canonical);
        projection.canonical_bytes = Arc::from(canonical);
    }

    #[test]
    fn reports_release_goldens() {
        let projection = build_release_projection().unwrap();
        assert_eq!(
            hex(projection.package.as_bytes()),
            "f20c23d1727eaaa178db3eee1d54610e"
        );
        assert_eq!(
            hex(projection.source.digest()),
            "d7af6e7c0c0814824148bab3c906894d5910fade5a6ce62f60550ff0ded4edc7"
        );
        assert_eq!(
            hex(projection.interface_digest()),
            "a96f3cfb6260a4179dbbb2043affa1e03d9c5b81d9045e4cc4cb3d26b6dad88a"
        );
        assert_eq!(
            hex(projection.public_interface_hash.as_bytes()),
            "95fa41304eff6470a116e4097d7be22d"
        );
    }

    #[test]
    fn verified_factory_returns_one_exact_arc() {
        let first = verified_embedded_core_authority().unwrap();
        let second = verified_embedded_core_authority().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.interface_version(), EMBEDDED_CORE_INTERFACE_VERSION);
        assert_eq!(first.package_id().into_bytes(), RELEASE_PACKAGE_ID);
        assert_eq!(first.interface_digest(), &RELEASE_INTERFACE_DIGEST);
        assert_eq!(first.projection().source().file_id(), FileId(u64::MAX));
        assert!(!first.projection().source().is_empty());
    }

    fn release_with_typed_projection() -> (
        EmbeddedCoreC1PackageProjection,
        EmbeddedCoreC2TypedProjection,
    ) {
        let projection = build_release_projection().unwrap();
        verify_projection(&projection).unwrap();
        let typed = build_typed_c2_projection(&projection).unwrap();
        verify_typed_c2_projection(&projection, &typed).unwrap();
        (projection, typed)
    }

    fn trait_definition(
        projection: &EmbeddedCoreC1PackageProjection,
        kind: CompilerTraitKind,
    ) -> VirtualDefinitionId {
        let spec = COMPILER_TRAITS
            .iter()
            .find(|spec| spec.kind == kind)
            .unwrap();
        find_definition_id(&projection.definitions, spec.name, VirtualNamespace::Type).unwrap()
    }

    #[test]
    fn typed_c2_authority_has_semantic_trait_nominal_and_callable_identities() {
        let authority = verified_embedded_core_authority().unwrap();
        let typed = authority.typed_c2();
        assert_eq!(typed.compiler_traits().len(), 31);
        assert_eq!(typed.primitive_definitions.len(), PRIMITIVE_TYPES.len());
        assert_eq!(typed.nominals().len(), 29);

        let unit = find_definition_id(
            authority.projection().definitions(),
            "()",
            VirtualNamespace::Type,
        )
        .unwrap();
        assert_eq!(
            typed.primitive_for_c1_definition(unit),
            Some(CompilerPrimitiveTypePattern::Unit)
        );
        assert_eq!(
            authority.compiler_primitive_for_c1_definition(unit),
            Some(CompilerPrimitiveTypePattern::Unit)
        );

        let clone = typed.compiler_trait(CompilerTraitKind::Clone);
        assert_eq!(clone.explicit_generic_arity(), 0);
        assert_eq!(
            clone.designated_self(),
            CompilerTraitSelfRelation::OperatedType
        );
        assert_eq!(
            clone.user_impl_policy(),
            UserImplPolicy::AllowedAndValidated
        );
        let clone_method = clone.method().unwrap();
        assert_eq!(clone_method.kind(), CompilerTraitMethodKind::Clone);
        assert_eq!(clone_method.receiver(), CompilerTraitReceiverMode::Shared);
        assert_eq!(
            clone_method.callable(),
            &CompilerTraitCallablePattern::Fixed {
                parameters: Box::new([]),
                result: CompilerTraitTypePattern::SelfType,
            }
        );

        let eq = typed.compiler_trait(CompilerTraitKind::Eq);
        assert_eq!(eq.explicit_generic_arity(), 2);
        assert_eq!(
            eq.designated_self(),
            CompilerTraitSelfRelation::LeftHandSide(CompilerTraitGenericParameter(0))
        );
        let eq_method = eq.method().unwrap();
        assert_eq!(eq_method.receiver(), CompilerTraitReceiverMode::None);
        assert_eq!(
            eq_method.callable(),
            &CompilerTraitCallablePattern::Fixed {
                parameters:
                    vec![
                        CompilerTraitTypePattern::SharedReference(Box::new(
                            CompilerTraitTypePattern::ExplicitGeneric(
                                CompilerTraitGenericParameter(0),
                            ),
                        )),
                        CompilerTraitTypePattern::SharedReference(Box::new(
                            CompilerTraitTypePattern::ExplicitGeneric(
                                CompilerTraitGenericParameter(1),
                            ),
                        )),
                    ]
                    .into_boxed_slice(),
                result: CompilerTraitTypePattern::Primitive(CompilerPrimitiveTypePattern::Bool),
            }
        );

        let iterator = typed.compiler_trait(CompilerTraitKind::Iterator);
        assert_eq!(iterator.explicit_generic_arity(), 2);
        assert_eq!(
            iterator.designated_self(),
            CompilerTraitSelfRelation::Iterator(CompilerTraitGenericParameter(0))
        );
        let next = iterator.method().unwrap();
        assert_eq!(next.receiver(), CompilerTraitReceiverMode::Mutable);
        assert_eq!(
            typed
                .compiler_trait_method(CompilerTraitMethodKind::IteratorNext)
                .map(CompilerTraitMethodAuthority::c1_method),
            Some(next.c1_method())
        );
        assert_eq!(
            typed
                .compiler_trait_method_for_c1_method(next.c1_method())
                .map(CompilerTraitMethodAuthority::kind),
            Some(CompilerTraitMethodKind::IteratorNext)
        );
        assert_eq!(
            next.callable(),
            &CompilerTraitCallablePattern::Fixed {
                parameters: Box::new([]),
                result: CompilerTraitTypePattern::Nominal {
                    kind: CompilerNominalKind::Option,
                    arguments: Box::new([CompilerTraitTypePattern::ExplicitGeneric(
                        CompilerTraitGenericParameter(1),
                    )]),
                },
            }
        );

        let callable = typed.compiler_trait(CompilerTraitKind::FnOnce);
        assert_eq!(callable.explicit_generic_arity(), 1);
        assert_eq!(
            callable.designated_self(),
            CompilerTraitSelfRelation::CallableType
        );
        let call = callable.method().unwrap();
        assert_eq!(call.receiver(), CompilerTraitReceiverMode::Value);
        assert_eq!(call.effects(), CompilerTraitEffectPattern::ExactSignature);
        assert_eq!(
            call.callable(),
            &CompilerTraitCallablePattern::ExactSignatureAndEffects {
                signature: CompilerTraitGenericParameter(0),
            }
        );

        let option = authority.compiler_nominal(CompilerNominalKind::Option);
        assert_eq!(option.declaration_kind(), VirtualDeclarationKind::Enum);
        assert_eq!(
            typed
                .nominal_for_c1_definition(option.c1_definition())
                .map(CompilerNominalAuthority::kind),
            Some(CompilerNominalKind::Option)
        );
        assert_eq!(
            typed
                .compiler_trait_for_c1_definition(clone.c1_definition())
                .map(CompilerTraitAuthority::kind),
            Some(CompilerTraitKind::Clone)
        );
    }

    #[test]
    fn typed_nominal_methods_are_complete_typed_and_lookupable() {
        let authority = verified_embedded_core_authority().unwrap();
        let typed = authority.typed_c2();
        assert_eq!(typed.nominal_methods().len(), 51);
        assert!(typed
            .nominal_methods()
            .windows(2)
            .all(|rows| rows[0].c1_method() < rows[1].c1_method()));

        let map = find_definition_id(
            &authority.projection().definitions,
            "Map",
            VirtualNamespace::Type,
        )
        .unwrap();
        let option = find_definition_id(
            &authority.projection().definitions,
            "Option",
            VirtualNamespace::Type,
        )
        .unwrap();
        let insert = authority
            .compiler_nominal_method(CompilerNominalKind::Map, "insert")
            .unwrap();
        assert_eq!(insert.owner(), CompilerNominalKind::Map);
        assert_eq!(
            insert.receiver(),
            CompilerNominalMethodReceiverMode::Mutable
        );
        assert_eq!(insert.generics().len(), 2);
        assert_eq!(insert.generics()[0].source_name(), "K");
        assert_eq!(
            insert.generics()[0].bounds(),
            [
                CompilerMethodGenericBoundPattern::CompilerTrait(CompilerTraitKind::Eq),
                CompilerMethodGenericBoundPattern::CompilerTrait(CompilerTraitKind::Ord),
            ]
        );
        assert_eq!(
            insert.receiver_type(),
            Some(&CompilerMethodTypePattern::MutableReference {
                lifetime: CompilerMethodLifetimePattern::Elided,
                referent: Box::new(CompilerMethodTypePattern::Definition {
                    definition: map,
                    arguments: Box::new([
                        CompilerMethodGenericArgumentPattern::Type(
                            CompilerMethodTypePattern::Generic(CompilerMethodGenericParameter(0),),
                        ),
                        CompilerMethodGenericArgumentPattern::Type(
                            CompilerMethodTypePattern::Generic(CompilerMethodGenericParameter(1),),
                        ),
                    ]),
                }),
            })
        );
        assert_eq!(
            insert.parameters(),
            [
                CompilerMethodTypePattern::Generic(CompilerMethodGenericParameter(0)),
                CompilerMethodTypePattern::Generic(CompilerMethodGenericParameter(1)),
            ]
        );
        assert_eq!(
            insert.result(),
            &CompilerMethodTypePattern::Definition {
                definition: option,
                arguments: Box::new([CompilerMethodGenericArgumentPattern::Type(
                    CompilerMethodTypePattern::Generic(CompilerMethodGenericParameter(1)),
                )]),
            }
        );
        assert_eq!(
            insert.requires(),
            [CompilerNominalMethodEffectPattern::Drop(
                CompilerMethodTypePattern::Generic(CompilerMethodGenericParameter(0)),
            )]
        );
        assert!(insert.throws().is_empty());
        assert_eq!(
            authority
                .compiler_nominal_method_for_c1_method(insert.c1_method())
                .map(CompilerNominalMethodAuthority::stable_name),
            Some("map.insert")
        );

        let app = authority
            .compiler_nominal_method(CompilerNominalKind::App, "run")
            .unwrap();
        assert_eq!(app.receiver(), CompilerNominalMethodReceiverMode::Mutable);
        assert_eq!(app.parameters().len(), 1);
        assert_eq!(app.selectors().len(), 1);
        assert_eq!(app.selectors()[0].source_name(), "schedule");
        assert_eq!(
            app.selectors()[0].kind(),
            CompilerMethodSelectorKind::DefinitionId
        );
        assert_eq!(
            app.requires(),
            [CompilerNominalMethodEffectPattern::Selector(
                CompilerMethodSelector(0),
            )]
        );
        assert_eq!(app.requires(), app.throws());

        let maybe_uninit = find_definition_id(
            &authority.projection().definitions,
            "MaybeUninit",
            VirtualNamespace::Type,
        )
        .unwrap();
        let assume_init = authority
            .compiler_nominal_method(CompilerNominalKind::MaybeUninit, "assume_init")
            .unwrap();
        assert!(assume_init.is_unsafe());
        assert_eq!(
            assume_init.lowering(),
            VirtualMethodLowering::CompilerOperation(CompilerOperation::MaybeUninitAssumeInit)
        );
        assert_eq!(
            assume_init.receiver(),
            CompilerNominalMethodReceiverMode::Value
        );
        assert_eq!(
            assume_init.receiver_type(),
            Some(&CompilerMethodTypePattern::Definition {
                definition: maybe_uninit,
                arguments: Box::new([CompilerMethodGenericArgumentPattern::Type(
                    CompilerMethodTypePattern::Generic(CompilerMethodGenericParameter(0)),
                )]),
            })
        );
        assert!(assume_init.parameters().is_empty());
        assert!(authority
            .compiler_nominal_method(CompilerNominalKind::Caps, "take")
            .is_none());
    }

    #[test]
    fn typed_nominal_method_mutations_and_noncanonical_grammar_fail_closed() {
        let (mut projection, _) = release_with_typed_projection();
        let insert = projection
            .methods
            .iter_mut()
            .find(|row| row.stable_name == "map.insert")
            .unwrap();
        insert.signature.push(' ');
        assert_eq!(
            build_typed_c2_projection(&projection).unwrap_err(),
            EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority
        );

        let (projection, mut typed) = release_with_typed_projection();
        let remove = typed
            .nominal_methods
            .iter_mut()
            .find(|row| row.stable_name == "map.remove")
            .unwrap();
        remove.source_name = "insert".to_owned();
        assert!(typed
            .nominal_method(CompilerNominalKind::Map, "insert")
            .is_none());
        assert_eq!(
            verify_typed_c2_projection(&projection, &typed),
            Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)
        );

        let map =
            find_definition_id(&projection.definitions, "Map", VirtualNamespace::Type).unwrap();
        assert_eq!(
            parse_nominal_method_signature(
                &projection,
                "Map",
                map,
                "new",
                "map.new<K:Unknown,V>() -> Map<K,V>",
            ),
            Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)
        );
        assert_eq!(
            parse_nominal_method_signature(
                &projection,
                "Map",
                map,
                "new",
                "map.new<K,V>() -> Map<K,V> trailing",
            ),
            Err(EmbeddedCoreVerificationError::InvalidTypedNominalMethodAuthority)
        );

        let (mut projection, typed) = release_with_typed_projection();
        let offset = projection
            .source
            .bytes()
            .windows(b"map.insert".len())
            .position(|window| window == b"map.insert")
            .unwrap();
        Arc::make_mut(&mut projection.source.bytes)[offset] = b'M';
        assert_eq!(
            verify_typed_c2_projection(&projection, &typed),
            Err(EmbeddedCoreVerificationError::InvalidTypedSourceAgreement)
        );
    }

    #[test]
    fn typed_c2_trait_string_and_designated_self_mismatches_fail_closed() {
        let (mut projection, _) = release_with_typed_projection();
        let clone = trait_definition(&projection, CompilerTraitKind::Clone);
        projection.definitions[usize::from(clone.0)].semantic_shape =
            "trait Clone { forged }".to_owned();
        assert_eq!(
            build_typed_c2_projection(&projection).unwrap_err(),
            EmbeddedCoreVerificationError::InvalidTypedTraitAuthority
        );

        let (projection, mut typed) = release_with_typed_projection();
        let from = typed
            .compiler_traits
            .iter_mut()
            .find(|row| row.kind == CompilerTraitKind::From)
            .unwrap();
        from.designated_self = CompilerTraitSelfRelation::Source(CompilerTraitGenericParameter(0));
        assert_eq!(
            verify_typed_c2_projection(&projection, &typed),
            Err(EmbeddedCoreVerificationError::InvalidTypedTraitAuthority)
        );
    }

    #[test]
    fn typed_c2_method_owner_and_callable_mismatches_fail_closed() {
        let (mut projection, _) = release_with_typed_projection();
        let clone = trait_definition(&projection, CompilerTraitKind::Clone);
        let clone_method = projection
            .methods
            .iter_mut()
            .find(|row| row.owner == Some(clone))
            .unwrap();
        clone_method.owner = None;
        assert_eq!(
            build_typed_c2_projection(&projection).unwrap_err(),
            EmbeddedCoreVerificationError::InvalidTypedMethodAuthority
        );

        let (mut projection, _) = release_with_typed_projection();
        let iterator = trait_definition(&projection, CompilerTraitKind::Iterator);
        let next = projection
            .methods
            .iter_mut()
            .find(|row| row.owner == Some(iterator))
            .unwrap();
        next.signature = "fn(Iter)->Option<Item>".to_owned();
        assert_eq!(
            build_typed_c2_projection(&projection).unwrap_err(),
            EmbeddedCoreVerificationError::InvalidTypedMethodAuthority
        );
    }

    #[test]
    fn typed_c2_nominal_and_synthetic_source_mismatches_fail_closed() {
        let (projection, mut typed) = release_with_typed_projection();
        typed.primitive_definitions[0].1 = CompilerPrimitiveTypePattern::Unit;
        assert_eq!(
            verify_typed_c2_projection(&projection, &typed).unwrap_err(),
            EmbeddedCoreVerificationError::InvalidTypedPrimitiveAuthority
        );

        let (mut projection, _) = release_with_typed_projection();
        let option =
            find_definition_id(&projection.definitions, "Option", VirtualNamespace::Type).unwrap();
        projection.definitions[usize::from(option.0)].declaration_kind =
            VirtualDeclarationKind::Struct;
        assert_eq!(
            build_typed_c2_projection(&projection).unwrap_err(),
            EmbeddedCoreVerificationError::InvalidTypedNominalAuthority
        );

        let (mut projection, typed) = release_with_typed_projection();
        let needle = b"trait Clone { fn clone";
        let offset = projection
            .source
            .bytes()
            .windows(needle.len())
            .position(|window| window == needle)
            .unwrap();
        Arc::make_mut(&mut projection.source.bytes)[offset] = b'T';
        assert_eq!(
            verify_typed_c2_projection(&projection, &typed),
            Err(EmbeddedCoreVerificationError::InvalidTypedSourceAgreement)
        );
    }

    #[test]
    fn declaration_kinds_are_typed_and_release_committed() {
        let authority = verified_embedded_core_authority().unwrap();
        let enum_names = authority
            .projection()
            .definitions()
            .iter()
            .filter(|row| row.declaration_kind() == VirtualDeclarationKind::Enum)
            .map(VirtualDefinitionRow::name)
            .collect::<Vec<_>>();
        assert_eq!(
            enum_names,
            [
                "AllocError",
                "AtomicRmw",
                "ChannelClosed",
                "GeneratorState",
                "Option",
                "Ordering",
                "Result",
                "SocketAddress",
            ]
        );
        for row in authority.projection().definitions() {
            let valid = match row.kind() {
                VirtualDefinitionKind::PrimitiveType => {
                    row.declaration_kind() == VirtualDeclarationKind::Primitive
                }
                VirtualDefinitionKind::NominalType => matches!(
                    row.declaration_kind(),
                    VirtualDeclarationKind::Struct | VirtualDeclarationKind::Enum
                ),
                VirtualDefinitionKind::CapabilityType | VirtualDefinitionKind::OpaqueType => {
                    row.declaration_kind() == VirtualDeclarationKind::Struct
                }
                VirtualDefinitionKind::Trait => {
                    row.declaration_kind() == VirtualDeclarationKind::Trait
                }
                VirtualDefinitionKind::Function => {
                    row.declaration_kind() == VirtualDeclarationKind::Function
                }
            };
            assert!(valid, "{}", row.name());
        }
    }

    #[test]
    fn enum_variants_and_record_constructors_are_typed_complete_and_exact() {
        let authority = verified_embedded_core_authority().unwrap();
        let variants = authority
            .projection()
            .enum_variants()
            .iter()
            .map(|row| {
                (
                    authority.definition(row.owner()).unwrap().name(),
                    row.ordinal(),
                    row.name(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            variants,
            [
                ("AllocError", 0, "OutOfMemory"),
                ("AtomicRmw", 0, "Add"),
                ("AtomicRmw", 1, "Sub"),
                ("AtomicRmw", 2, "And"),
                ("AtomicRmw", 3, "Or"),
                ("AtomicRmw", 4, "Xor"),
                ("AtomicRmw", 5, "Exchange"),
                ("AtomicRmw", 6, "Min"),
                ("AtomicRmw", 7, "Max"),
                ("ChannelClosed", 0, "Unit"),
                ("GeneratorState", 0, "Yielded"),
                ("GeneratorState", 1, "Complete"),
                ("Option", 0, "None"),
                ("Option", 1, "Some"),
                ("Ordering", 0, "Relaxed"),
                ("Ordering", 1, "Acquire"),
                ("Ordering", 2, "Release"),
                ("Ordering", 3, "AcqRel"),
                ("Ordering", 4, "SeqCst"),
                ("Result", 0, "Ok"),
                ("Result", 1, "Err"),
                ("SocketAddress", 0, "V4"),
                ("SocketAddress", 1, "V6"),
            ]
        );

        for (index, row) in authority.projection().enum_variants().iter().enumerate() {
            assert_eq!(usize::from(row.id().ordinal()), index);
            assert_eq!(authority.enum_variant(row.id()), Some(row));
            assert_eq!(
                authority.lookup_enum_variant(row.owner(), row.name()),
                Some(row.id())
            );
        }
        let option = authority
            .lookup_prelude_definition("Option", VirtualNamespace::Type)
            .unwrap();
        let some = authority.lookup_enum_variant(option, "Some").unwrap();
        assert_eq!(authority.enum_variant(some).unwrap().ordinal(), 1);
        assert_eq!(authority.lookup_enum_variant(option, "Ok"), None);

        let constructors = authority
            .projection()
            .record_constructors()
            .iter()
            .map(|row| authority.definition(row.owner()).unwrap().name())
            .collect::<Vec<_>>();
        assert_eq!(
            constructors,
            [
                "IoError",
                "OpenOptions",
                "ProcessError",
                "ProcessOutput",
                "ProcessSpec",
                "ThreadError",
            ]
        );
        for &name in &constructors {
            let owner = authority.lookup_record_constructor(name).unwrap();
            assert_eq!(authority.record_constructor(owner).unwrap().owner(), owner);
        }
        assert_eq!(authority.lookup_record_constructor("String"), None);
        assert_eq!(authority.lookup_record_constructor("Option"), None);
    }

    #[test]
    fn complete_prelude_resolves_to_branded_rows() {
        let authority = verified_embedded_core_authority().unwrap();
        for definition in authority.projection().definitions() {
            let resolved = authority.lookup_prelude(definition.name(), definition.namespace());
            assert_eq!(
                resolved,
                Some(VirtualPreludeTarget::Definition(definition.id())),
                "{}",
                definition.name()
            );
        }
        assert_eq!(
            authority.lookup_prelude("JoinHandle", VirtualNamespace::Type),
            Some(VirtualPreludeTarget::SemanticType(VirtualSemanticTypeId(0)))
        );
        assert!(authority
            .lookup_prelude("not_core", VirtualNamespace::Value)
            .is_none());
    }

    #[test]
    fn exact_intrinsic_set_and_holes_are_pinned() {
        let authority = verified_embedded_core_authority().unwrap();
        let mut ids = authority
            .projection()
            .methods()
            .iter()
            .filter_map(|row| match row.lowering() {
                VirtualMethodLowering::Intrinsic { id, .. } => Some(id),
                _ => None,
            })
            .chain(authority.projection().functions().iter().filter_map(
                |row| match row.lowering() {
                    VirtualFunctionLowering::Intrinsic { id, .. } => Some(id),
                    VirtualFunctionLowering::CompilerOwnedBody => None,
                },
            ))
            .collect::<Vec<_>>();
        ids.sort_unstable();
        assert_eq!(
            ids,
            [
                1, 2, 3, 4, 5, 10, 11, 12, 13, 14, 20, 21, 22, 23, 24, 25, 26, 30, 31, 32, 33, 40,
                41, 42, 43, 44, 50, 51, 52, 53, 54, 60, 61, 62, 70, 71, 100, 101, 102, 103, 104,
                105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 120, 121,
                122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 140,
                141, 142, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209, 210, 211,
            ]
        );
    }

    #[test]
    fn compiler_operations_are_complete_typed_and_unambiguous() {
        let authority = verified_embedded_core_authority().unwrap();
        let mut operations = authority
            .projection()
            .methods()
            .iter()
            .filter_map(|row| match row.lowering() {
                VirtualMethodLowering::CompilerOperation(operation) => {
                    Some((operation, row.stable_name()))
                }
                VirtualMethodLowering::TraitDispatch | VirtualMethodLowering::Intrinsic { .. } => {
                    None
                }
            })
            .collect::<Vec<_>>();
        operations.sort_unstable();
        assert_eq!(
            operations,
            [
                (CompilerOperation::PinNewChecked, "pin.new"),
                (CompilerOperation::PinNewUnchecked, "pin.new-unchecked"),
                (CompilerOperation::MaybeUninitUninit, "maybe-uninit.uninit"),
                (CompilerOperation::MaybeUninitNew, "maybe-uninit.new"),
                (
                    CompilerOperation::MaybeUninitAssumeInit,
                    "maybe-uninit.assume-init"
                ),
                (CompilerOperation::RawOffset, "raw.offset"),
                (CompilerOperation::RawWithAddress, "raw.with-address"),
                (CompilerOperation::RawExposeAddress, "raw.expose-address"),
                (CompilerOperation::CapsTake, "caps.take"),
                (CompilerOperation::PointerCast, "pointer.cast"),
            ]
        );

        assert_eq!(authority.lookup_method(None, "as"), None);
        let expose = authority
            .lookup_compiler_operation(CompilerOperation::RawExposeAddress)
            .unwrap();
        let pointer_cast = authority
            .lookup_compiler_operation(CompilerOperation::PointerCast)
            .unwrap();
        assert_ne!(expose, pointer_cast);
        assert_eq!(authority.method(expose).unwrap().source_name(), "as");
        assert_eq!(authority.method(pointer_cast).unwrap().source_name(), "as");
    }

    #[test]
    fn source_spans_and_all_row_orders_are_verified() {
        let projection = build_release_projection().unwrap();
        verify_definition_rows(&projection).unwrap();
        verify_type_rows(&projection).unwrap();
        verify_enum_variant_rows(&projection).unwrap();
        verify_record_constructor_rows(&projection).unwrap();
        verify_trait_rows(&projection).unwrap();
        verify_method_rows(&projection).unwrap();
        verify_function_rows(&projection).unwrap();
        verify_prelude_rows(&projection).unwrap();
        verify_panic_body(&projection).unwrap();
    }

    #[test]
    fn byte_digest_and_order_corruption_are_rejected() {
        let projection = build_release_projection().unwrap();

        let mut source_corrupt = projection.clone();
        Arc::make_mut(&mut source_corrupt.source.bytes)[0] ^= 1;
        assert_eq!(
            verify_projection(&source_corrupt),
            Err(EmbeddedCoreVerificationError::SyntheticSourceDigestMismatch)
        );

        let mut digest_corrupt = projection.clone();
        digest_corrupt.interface_digest[0] ^= 1;
        assert_eq!(
            verify_projection(&digest_corrupt),
            Err(EmbeddedCoreVerificationError::InterfaceDigestMismatch)
        );

        let mut definitions_corrupt = projection.clone();
        definitions_corrupt.definitions.swap(0, 1);
        definitions_corrupt.canonical_bytes = Arc::from(encode_projection(&definitions_corrupt));
        definitions_corrupt.interface_digest =
            digest_interface_projection(&definitions_corrupt.canonical_bytes);
        assert_eq!(
            verify_definition_rows(&definitions_corrupt),
            Err(EmbeddedCoreVerificationError::NonCanonicalOrder(
                "definition"
            ))
        );

        let mut variants_corrupt = projection.clone();
        variants_corrupt.enum_variants.swap(0, 1);
        assert_eq!(
            verify_enum_variant_rows(&variants_corrupt),
            Err(EmbeddedCoreVerificationError::NonCanonicalOrder(
                "enum variant"
            ))
        );

        let mut variant_payload_corrupt = projection.clone();
        variant_payload_corrupt.enum_variants[0].name = "Forged".to_owned();
        assert_eq!(
            verify_enum_variant_rows(&variant_payload_corrupt),
            Err(EmbeddedCoreVerificationError::InvalidReference(
                "enum variant"
            ))
        );
        recompute_local_commitments(&mut variant_payload_corrupt);
        assert_ne!(
            variant_payload_corrupt.public_interface_hash.into_bytes(),
            RELEASE_INTERFACE_HASH
        );
        assert_ne!(
            variant_payload_corrupt.interface_digest,
            RELEASE_INTERFACE_DIGEST
        );
        assert_eq!(
            verify_projection(&variant_payload_corrupt),
            Err(EmbeddedCoreVerificationError::InterfaceDigestMismatch)
        );

        let mut constructors_corrupt = projection.clone();
        constructors_corrupt.record_constructors[1] =
            constructors_corrupt.record_constructors[0].clone();
        assert_eq!(
            verify_record_constructor_rows(&constructors_corrupt),
            Err(EmbeddedCoreVerificationError::DuplicateRow(
                "record constructor"
            ))
        );

        let string =
            find_definition_id(&projection.definitions, "String", VirtualNamespace::Type).unwrap();
        let mut constructor_payload_corrupt = projection.clone();
        constructor_payload_corrupt.record_constructors[0].owner = string;
        assert_eq!(
            verify_record_constructor_rows(&constructor_payload_corrupt),
            Err(EmbeddedCoreVerificationError::InvalidReference(
                "record constructor"
            ))
        );
        recompute_local_commitments(&mut constructor_payload_corrupt);
        assert_ne!(
            constructor_payload_corrupt
                .public_interface_hash
                .into_bytes(),
            RELEASE_INTERFACE_HASH
        );
        assert_ne!(
            constructor_payload_corrupt.interface_digest,
            RELEASE_INTERFACE_DIGEST
        );
        assert_eq!(
            verify_projection(&constructor_payload_corrupt),
            Err(EmbeddedCoreVerificationError::InterfaceDigestMismatch)
        );

        let mut declaration_kind_corrupt = projection.clone();
        let option = declaration_kind_corrupt
            .definitions
            .iter_mut()
            .find(|row| row.name == "Option")
            .unwrap();
        option.declaration_kind = VirtualDeclarationKind::Struct;
        assert_eq!(
            verify_definition_rows(&declaration_kind_corrupt),
            Err(EmbeddedCoreVerificationError::InvalidReference(
                "definition declaration kind"
            ))
        );
        assert_eq!(
            verify_projection(&declaration_kind_corrupt),
            Err(EmbeddedCoreVerificationError::InterfaceDigestMismatch)
        );

        let mut compiler_operation_corrupt = projection.clone();
        let pointer_cast = compiler_operation_corrupt
            .methods
            .iter_mut()
            .find(|row| row.stable_name == "pointer.cast")
            .unwrap();
        pointer_cast.lowering =
            VirtualMethodLowering::CompilerOperation(CompilerOperation::RawExposeAddress);
        assert_eq!(
            verify_method_rows(&compiler_operation_corrupt),
            Err(EmbeddedCoreVerificationError::InvalidReference(
                "compiler operation"
            ))
        );
        assert_eq!(
            verify_projection(&compiler_operation_corrupt),
            Err(EmbeddedCoreVerificationError::InterfaceDigestMismatch)
        );

        let mut methods_corrupt = projection.clone();
        methods_corrupt.methods.swap(0, 1);
        assert_eq!(
            verify_method_rows(&methods_corrupt),
            Err(EmbeddedCoreVerificationError::NonCanonicalOrder("method"))
        );

        let mut prelude_corrupt = projection.clone();
        prelude_corrupt.prelude[1] = prelude_corrupt.prelude[0].clone();
        assert_eq!(
            verify_prelude_rows(&prelude_corrupt),
            Err(EmbeddedCoreVerificationError::DuplicateRow("prelude"))
        );

        let mut panic_corrupt = projection;
        panic_corrupt.panic_body.symbolic_body_bytes = Arc::from(b"forged".as_slice());
        assert_eq!(
            verify_panic_body(&panic_corrupt),
            Err(EmbeddedCoreVerificationError::InvalidPanicBody)
        );
    }
}
