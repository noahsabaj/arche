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
            Ok(Arc::new(VerifiedEmbeddedCoreAuthority {
                projection,
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

    for &(name, shape) in PRIMITIVE_TYPES {
        rows.push(DefinitionSpec {
            name,
            namespace: VirtualNamespace::Type,
            kind: VirtualDefinitionKind::PrimitiveType,
            declaration_kind: VirtualDeclarationKind::Primitive,
            shape,
            flavor: Some(VirtualTypeFlavor::Primitive),
            trait_policy: None,
            prelude: true,
        });
    }
    for &(name, shape, flavor, declaration_kind) in NOMINAL_TYPES {
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
    for &(name, shape, policy) in TRAITS {
        rows.push(DefinitionSpec {
            name,
            namespace: VirtualNamespace::Type,
            kind: VirtualDefinitionKind::Trait,
            declaration_kind: VirtualDeclarationKind::Trait,
            shape,
            flavor: None,
            trait_policy: Some(policy),
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

const PRIMITIVE_TYPES: &[(&str, &str)] = &[
    ("!", "never; uninhabited"),
    ("()", "unit; zero fields"),
    ("bool", "scalar bool; width=1"),
    ("char", "scalar Unicode; width=4"),
    ("entity", "scalar entity; width=8"),
    ("f32", "scalar IEEE754 binary32"),
    ("f64", "scalar IEEE754 binary64"),
    ("i16", "scalar signed; width=2"),
    ("i32", "scalar signed; width=4"),
    ("i64", "scalar signed; width=8"),
    ("i8", "scalar signed; width=1"),
    ("isize", "scalar signed; width=8"),
    ("str", "dynamically sized UTF-8 string slice"),
    ("u16", "scalar unsigned; width=2"),
    ("u32", "scalar unsigned; width=4"),
    ("u64", "scalar unsigned; width=8"),
    ("u8", "scalar unsigned; width=1"),
    ("usize", "scalar unsigned; width=8"),
];

const NOMINAL_TYPES: &[(&str, &str, VirtualTypeFlavor, VirtualDeclarationKind)] = &[
    (
        "AllocError",
        "pub enum AllocError { OutOfMemory }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        "App",
        "sealed compiler-owned App<W>; !Send + !Sync",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "Arc",
        "owned atomic shared allocation Arc<T>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "ArcWeak",
        "nonowning atomic weak allocation ArcWeak<T>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "AtomicRmw",
        "pub enum AtomicRmw { Add, Sub, And, Or, Xor, Exchange, Min, Max }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        "Box",
        "unique owned allocation Box<T>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "Caps",
        "sealed affine capability projection Caps<C...>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "ChannelClosed",
        "pub enum ChannelClosed { Unit }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        "Commands",
        "sealed command buffer handle Commands<W>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "GeneratorState",
        "pub enum GeneratorState<Y,R> { Yielded(Y), Complete(R) }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        "IoError",
        "pub struct IoError { pub code:i32, pub message:String }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Struct,
    ),
    (
        "Map",
        "owned ordered Map<K:Eq+Ord,V>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "MapIter",
        "shared-borrow iterator MapIter<'a,K,V>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "MaybeUninit",
        "compiler-checked maybe-initialized storage MaybeUninit<T>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "OpenOptions",
        "pub struct OpenOptions { pub read:bool, pub write:bool, pub append:bool, pub truncate:bool, pub create:bool, pub create_new:bool }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Struct,
    ),
    (
        "Option",
        "pub enum Option<T> { None, Some(T) }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        "Ordering",
        "pub enum Ordering { Relaxed, Acquire, Release, AcqRel, SeqCst }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        "Pin",
        "pinning invariant wrapper Pin<P>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "ProcessError",
        "pub struct ProcessError { pub code:i32, pub message:String }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Struct,
    ),
    (
        "ProcessOutput",
        "pub struct ProcessOutput { pub status:i32, pub stdout:Vec<u8>, pub stderr:Vec<u8> }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Struct,
    ),
    (
        "ProcessSpec",
        "pub struct ProcessSpec { pub program:String, pub arguments:Vec<String>, pub environment:Map<String,String>, pub current_directory:Option<String>, pub stdin:Vec<u8> }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Struct,
    ),
    (
        "Query",
        "sealed query marker Query<Q>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "Rc",
        "owned non-atomic shared allocation Rc<T>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "RcWeak",
        "nonowning weak allocation RcWeak<T>",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "Result",
        "pub enum Result<T,E> { Ok(T), Err(E) }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        "SocketAddress",
        "pub enum SocketAddress { V4 { octets:[u8;4], port:u16 }, V6 { octets:[u8;16], port:u16, flow_info:u32, scope_id:u32 } }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Enum,
    ),
    (
        "String",
        "owned UTF-8 String",
        VirtualTypeFlavor::Managed,
        VirtualDeclarationKind::Struct,
    ),
    (
        "ThreadError",
        "pub struct ThreadError { pub code:i32, pub message:String }",
        VirtualTypeFlavor::Transparent,
        VirtualDeclarationKind::Struct,
    ),
    (
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

const TRAITS: &[(&str, &str, UserImplPolicy)] = &[
    (
        "Add",
        "trait Add<Lhs,Rhs,Output> { fn add(Lhs,Rhs)->Output throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "BitAnd",
        "trait BitAnd<Lhs,Rhs,Output> { fn bit_and(Lhs,Rhs)->Output throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "BitNot",
        "trait BitNot<Input,Output> { fn bit_not(Input)->Output throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "BitOr",
        "trait BitOr<Lhs,Rhs,Output> { fn bit_or(Lhs,Rhs)->Output throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "BitXor",
        "trait BitXor<Lhs,Rhs,Output> { fn bit_xor(Lhs,Rhs)->Output throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Clone",
        "trait Clone { fn clone(&Self)->Self requires {} throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Copy",
        "trait Copy { structural; no method; no Drop }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Div",
        "trait Div<Lhs,Rhs,Output> { fn div(Lhs,Rhs)->Output throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Drop",
        "trait Drop { fn drop(&mut Self)->() throws {}; one impl maximum }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "EcsKey",
        "sealed structural EcsKey evidence; no method",
        UserImplPolicy::Forbidden,
    ),
    (
        "EcsValue",
        "sealed structural EcsValue evidence; no method",
        UserImplPolicy::Forbidden,
    ),
    (
        "Eq",
        "trait Eq<Lhs,Rhs> { fn eq(&Lhs,&Rhs)->bool requires {} throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Fn",
        "compiler-derived trait Fn<Signature> { fn call with exact signature/effects }",
        UserImplPolicy::CompilerDerivedOnly,
    ),
    (
        "FnMut",
        "compiler-derived trait FnMut<Signature> { fn call with exact signature/effects }",
        UserImplPolicy::CompilerDerivedOnly,
    ),
    (
        "FnOnce",
        "compiler-derived trait FnOnce<Signature> { fn call with exact signature/effects }",
        UserImplPolicy::CompilerDerivedOnly,
    ),
    (
        "From",
        "trait From<Source,Target> { fn from(Source)->Target requires {} throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "IntoIterator",
        "trait IntoIterator<Source,Iter> { fn into_iter(Source)->Iter requires {} throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Iterator",
        "trait Iterator<Iter,Item> { fn next(&mut Iter)->Option<Item> requires {} throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "LogicalNot",
        "trait LogicalNot<Input,Output> { fn logical_not(Input)->Output throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Mul",
        "trait Mul<Lhs,Rhs,Output> { fn mul(Lhs,Rhs)->Output throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Neg",
        "trait Neg<Input,Output> { fn neg(Input)->Output throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Ord",
        "trait Ord<Lhs,Rhs> { fn compare(&Lhs,&Rhs)->i32 requires {} throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Rem",
        "trait Rem<Lhs,Rhs,Output> { fn rem(Lhs,Rhs)->Output throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Send",
        "compiler-derived structural Send judgment; no method",
        UserImplPolicy::CompilerDerivedOnly,
    ),
    (
        "ShiftLeft",
        "trait ShiftLeft<Lhs,Rhs,Output> { fn shift_left(Lhs,Rhs)->Output throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "ShiftRight",
        "trait ShiftRight<Lhs,Rhs,Output> { fn shift_right(Lhs,Rhs)->Output throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Sub",
        "trait Sub<Lhs,Rhs,Output> { fn sub(Lhs,Rhs)->Output throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Sync",
        "compiler-derived structural Sync judgment; no method",
        UserImplPolicy::CompilerDerivedOnly,
    ),
    (
        "TryFrom",
        "trait TryFrom<Source,Target,Error> { fn try_from(Source)->Result<Target,Error> requires {} throws {} }",
        UserImplPolicy::AllowedAndValidated,
    ),
    (
        "Unpin",
        "compiler-derived structural Unpin judgment; no method",
        UserImplPolicy::CompilerDerivedOnly,
    ),
    (
        "UnwindPayload",
        "sealed owned sized static unwind-payload judgment; no method",
        UserImplPolicy::Forbidden,
    ),
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

const fn trait_method(
    owner: &'static str,
    source_name: &'static str,
    stable_name: &'static str,
    signature: &'static str,
) -> MethodSpec {
    MethodSpec {
        owner: Some(owner),
        source_name,
        stable_name,
        signature,
        requires: "{}",
        throws: "{}",
        lowering: VirtualMethodLowering::TraitDispatch,
    }
}

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
    METHOD_SPECS.to_vec()
}

const H: IntrinsicCtfeSupport = IntrinsicCtfeSupport::Hermetic;
const N: IntrinsicCtfeSupport = IntrinsicCtfeSupport::NotExecutable;

const METHOD_SPECS: &[MethodSpec] = &[
    trait_method("Add", "add", "trait.add.add", "fn(Lhs,Rhs)->Output"),
    trait_method(
        "BitAnd",
        "bit_and",
        "trait.bit-and.bit-and",
        "fn(Lhs,Rhs)->Output",
    ),
    trait_method(
        "BitNot",
        "bit_not",
        "trait.bit-not.bit-not",
        "fn(Input)->Output",
    ),
    trait_method(
        "BitOr",
        "bit_or",
        "trait.bit-or.bit-or",
        "fn(Lhs,Rhs)->Output",
    ),
    trait_method(
        "BitXor",
        "bit_xor",
        "trait.bit-xor.bit-xor",
        "fn(Lhs,Rhs)->Output",
    ),
    trait_method("Clone", "clone", "trait.clone.clone", "fn(&Self)->Self"),
    trait_method("Div", "div", "trait.div.div", "fn(Lhs,Rhs)->Output"),
    trait_method("Drop", "drop", "trait.drop.drop", "fn(&mut Self)->()"),
    trait_method("Eq", "eq", "trait.eq.eq", "fn(&Lhs,&Rhs)->bool"),
    trait_method(
        "Fn",
        "call",
        "trait.fn.call",
        "callable exact signature/effects",
    ),
    trait_method(
        "FnMut",
        "call",
        "trait.fn-mut.call",
        "callable exact signature/effects",
    ),
    trait_method(
        "FnOnce",
        "call",
        "trait.fn-once.call",
        "callable exact signature/effects",
    ),
    trait_method("From", "from", "trait.from.from", "fn(Source)->Target"),
    trait_method(
        "IntoIterator",
        "into_iter",
        "trait.into-iterator.into-iter",
        "fn(Source)->Iter",
    ),
    trait_method(
        "Iterator",
        "next",
        "trait.iterator.next",
        "fn(&mut Iter)->Option<Item>",
    ),
    trait_method(
        "LogicalNot",
        "logical_not",
        "trait.logical-not.logical-not",
        "fn(Input)->Output",
    ),
    trait_method("Mul", "mul", "trait.mul.mul", "fn(Lhs,Rhs)->Output"),
    trait_method("Neg", "neg", "trait.neg.neg", "fn(Input)->Output"),
    trait_method("Ord", "compare", "trait.ord.compare", "fn(&Lhs,&Rhs)->i32"),
    trait_method("Rem", "rem", "trait.rem.rem", "fn(Lhs,Rhs)->Output"),
    trait_method(
        "ShiftLeft",
        "shift_left",
        "trait.shift-left.shift-left",
        "fn(Lhs,Rhs)->Output",
    ),
    trait_method(
        "ShiftRight",
        "shift_right",
        "trait.shift-right.shift-right",
        "fn(Lhs,Rhs)->Output",
    ),
    trait_method("Sub", "sub", "trait.sub.sub", "fn(Lhs,Rhs)->Output"),
    trait_method(
        "TryFrom",
        "try_from",
        "trait.try-from.try-from",
        "fn(Source)->Result<Target,Error>",
    ),
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
