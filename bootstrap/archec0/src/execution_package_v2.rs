use crate::ecs_metadata_v2::{self, MetadataV2Error, SectionDescriptor, SectionView};
use crate::ids_v2::{AbiHash, BodyHash, DeclId, PrimitiveType, SchemaField, SchemaId, SchemaKind};
use std::collections::HashSet;
use std::fmt;
use std::io::{self, Read, Seek, Write};

pub mod wire {
    pub use crate::ecs_metadata_v2::wire as envelope;
    pub use crate::ecs_metadata_v2::{DIRECTORY_ENTRY_SIZE, HEADER_SIZE, MAGIC, VERSION};

    pub mod section_kind {
        pub const STRINGS: u64 = 1;
        pub const WORLD: u64 = 2;
        pub const SCHEMAS: u64 = 3;
        pub const FIELDS: u64 = 4;
        pub const SYSTEMS: u64 = 5;
        pub const PARAMETERS: u64 = 6;
        pub const QUERIES: u64 = 7;
        pub const TERMS: u64 = 8;
        pub const SCHEDULES: u64 = 9;
        pub const SCHEDULE_ITEMS: u64 = 10;
        pub const STARTUP_OPERATIONS: u64 = 11;
        pub const PAYLOADS: u64 = 12;
        pub const FUNCTION_LINKS: u64 = 13;
        pub const SOURCE_SPANS: u64 = 14;
    }

    pub const SECTION_ALIGNMENT: u64 = 8;
    pub const NONE_REFERENCE: u64 = u64::MAX;

    pub mod strings {
        pub const HEADER_SIZE: u64 = 16;
        pub const COUNT: u64 = 0;
        pub const BYTE_LENGTH: u64 = 8;
        pub const RECORD_SIZE: u64 = 16;
        pub const RECORD_OFFSET: u64 = 0;
        pub const RECORD_BYTE_LENGTH: u64 = 8;
    }

    pub mod world {
        pub const RECORD_SIZE: u64 = 64;
        pub const NAME: u64 = 0;
        pub const SOURCE_SPAN: u64 = 8;
        pub const STARTUP_ABI_HASH: u64 = 16;
        pub const STARTUP_BODY_HASH: u64 = 32;
        pub const RESERVED: u64 = 48;
    }

    pub mod schema {
        pub const RECORD_SIZE: u64 = 96;
        pub const ID: u64 = 0;
        pub const KIND: u64 = 16;
        pub const NAME: u64 = 24;
        pub const BYTE_SIZE: u64 = 32;
        pub const ALIGNMENT: u64 = 40;
        pub const SOURCE_SPAN: u64 = 48;
        pub const FLAGS: u64 = 56;
        pub const RESERVED: u64 = 64;

        pub const COMPONENT: u64 = 1;
        pub const RESOURCE: u64 = 2;
        pub const TAG: u64 = 3;

        pub mod flags {
            pub const TAG: u64 = 1;
            pub const KNOWN_MASK: u64 = TAG;
        }
    }

    pub mod field {
        pub const RECORD_SIZE: u64 = 64;
        pub const SCHEMA: u64 = 0;
        pub const NAME: u64 = 8;
        pub const PRIMITIVE: u64 = 16;
        pub const BYTE_OFFSET: u64 = 24;
        pub const SOURCE_SPAN: u64 = 32;
        pub const RESERVED: u64 = 40;

        pub const I32: u64 = 1;
        pub const F32: u64 = 2;
        pub const BOOL: u64 = 3;
    }

    pub mod system {
        pub const RECORD_SIZE: u64 = 128;
        pub const ID: u64 = 0;
        pub const NAME: u64 = 16;
        pub const ABI_HASH: u64 = 24;
        pub const BODY_HASH: u64 = 40;
        pub const SOURCE_SPAN: u64 = 56;
        pub const RESERVED: u64 = 64;
    }

    pub mod parameter {
        pub const RECORD_SIZE: u64 = 64;
        pub const SYSTEM: u64 = 0;
        pub const NAME: u64 = 8;
        pub const KIND: u64 = 16;
        pub const TARGET: u64 = 24;
        pub const SOURCE_SPAN: u64 = 32;
        pub const RESERVED: u64 = 40;

        pub const READ_RESOURCE: u64 = 1;
        pub const MUT_RESOURCE: u64 = 2;
        pub const QUERY: u64 = 3;
    }

    pub mod query {
        pub const RECORD_SIZE: u64 = 80;
        pub const ID: u64 = 0;
        pub const SYSTEM: u64 = 16;
        pub const PARAMETER: u64 = 24;
        pub const SOURCE_SPAN: u64 = 32;
        pub const RESERVED: u64 = 40;
    }

    pub mod term {
        pub const RECORD_SIZE: u64 = 64;
        pub const QUERY: u64 = 0;
        pub const ACCESS: u64 = 8;
        pub const SCHEMA: u64 = 16;
        pub const SOURCE_SPAN: u64 = 24;
        pub const RESERVED: u64 = 32;

        pub const READ: u64 = 1;
        pub const MUT: u64 = 2;
        pub const EXCLUDE: u64 = 3;
    }

    pub mod schedule {
        pub const RECORD_SIZE: u64 = 64;
        pub const ID: u64 = 0;
        pub const NAME: u64 = 16;
        pub const SOURCE_SPAN: u64 = 24;
        pub const RESERVED: u64 = 32;
    }

    pub mod schedule_item {
        pub const RECORD_SIZE: u64 = 48;
        pub const SCHEDULE: u64 = 0;
        pub const KIND: u64 = 8;
        pub const TARGET: u64 = 16;
        pub const SOURCE_SPAN: u64 = 24;
        pub const RESERVED: u64 = 32;

        pub const RUN_SYSTEM: u64 = 1;
    }

    pub mod startup_operation {
        pub const RECORD_SIZE: u64 = 64;
        pub const KIND: u64 = 0;
        pub const FIRST: u64 = 8;
        pub const SECOND: u64 = 16;
        pub const RESERVED_ARGUMENT: u64 = 24;
        pub const SOURCE_SPAN: u64 = 32;
        pub const RESERVED: u64 = 40;

        pub const RESOURCE_PAYLOAD: u64 = 1;
        pub const SPAWN: u64 = 2;
        pub const RUN_SCHEDULE: u64 = 3;
    }

    pub mod payload {
        pub const HEADER_SIZE: u64 = 16;
        pub const COUNT: u64 = 0;
        pub const BYTE_LENGTH: u64 = 8;
        pub const RECORD_SIZE: u64 = 32;
        pub const SCHEMA: u64 = 0;
        pub const OFFSET: u64 = 8;
        pub const LENGTH: u64 = 16;
        pub const RESERVED: u64 = 24;
    }

    pub mod function_link {
        pub const RECORD_SIZE: u64 = 96;
        pub const KIND: u64 = 0;
        pub const SYSTEM: u64 = 8;
        pub const SYMBOL_NAME: u64 = 16;
        pub const ABI_HASH: u64 = 24;
        pub const BODY_HASH: u64 = 40;
        pub const CODE_OFFSET: u64 = 56;
        pub const CODE_BYTE_LENGTH: u64 = 64;
        pub const SOURCE_SPAN: u64 = 72;
        pub const FIRST_BODY_SPAN: u64 = 80;
        pub const BODY_SPAN_COUNT: u64 = 88;

        pub const STARTUP: u64 = 1;
        pub const SYSTEM_TARGET: u64 = 2;
    }

    pub mod source_span {
        pub const RECORD_SIZE: u64 = 64;
        pub const FILE_NAME: u64 = 0;
        pub const START_BYTE: u64 = 8;
        pub const END_BYTE: u64 = 16;
        pub const START_LINE: u64 = 24;
        pub const START_COLUMN: u64 = 32;
        pub const END_LINE: u64 = 40;
        pub const END_COLUMN: u64 = 48;
        pub const RESERVED: u64 = 56;
    }
}

pub mod section_kind {
    pub use super::wire::section_kind::*;
}

pub const SECTION_KINDS: [u64; 14] = [
    section_kind::STRINGS,
    section_kind::WORLD,
    section_kind::SCHEMAS,
    section_kind::FIELDS,
    section_kind::SYSTEMS,
    section_kind::PARAMETERS,
    section_kind::QUERIES,
    section_kind::TERMS,
    section_kind::SCHEDULES,
    section_kind::SCHEDULE_ITEMS,
    section_kind::STARTUP_OPERATIONS,
    section_kind::PAYLOADS,
    section_kind::FUNCTION_LINKS,
    section_kind::SOURCE_SPANS,
];

pub const SECTION_ALIGNMENT: u64 = wire::SECTION_ALIGNMENT;
pub const STRING_SECTION_HEADER_SIZE: u64 = wire::strings::HEADER_SIZE;
pub const STRING_RECORD_SIZE: u64 = wire::strings::RECORD_SIZE;
pub const WORLD_RECORD_SIZE: u64 = wire::world::RECORD_SIZE;
pub const SCHEMA_RECORD_SIZE: u64 = wire::schema::RECORD_SIZE;
pub const FIELD_RECORD_SIZE: u64 = wire::field::RECORD_SIZE;
pub const SYSTEM_RECORD_SIZE: u64 = wire::system::RECORD_SIZE;
pub const PARAMETER_RECORD_SIZE: u64 = wire::parameter::RECORD_SIZE;
pub const QUERY_RECORD_SIZE: u64 = wire::query::RECORD_SIZE;
pub const TERM_RECORD_SIZE: u64 = wire::term::RECORD_SIZE;
pub const SCHEDULE_RECORD_SIZE: u64 = wire::schedule::RECORD_SIZE;
pub const SCHEDULE_ITEM_RECORD_SIZE: u64 = wire::schedule_item::RECORD_SIZE;
pub const STARTUP_OPERATION_RECORD_SIZE: u64 = wire::startup_operation::RECORD_SIZE;
pub const PAYLOAD_SECTION_HEADER_SIZE: u64 = wire::payload::HEADER_SIZE;
pub const PAYLOAD_RECORD_SIZE: u64 = wire::payload::RECORD_SIZE;
pub const FUNCTION_LINK_RECORD_SIZE: u64 = wire::function_link::RECORD_SIZE;
pub const SOURCE_SPAN_RECORD_SIZE: u64 = wire::source_span::RECORD_SIZE;
pub const NONE_REFERENCE: u64 = wire::NONE_REFERENCE;

macro_rules! record_reference {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(index: u64) -> Self {
                Self(index)
            }

            pub const fn index(self) -> u64 {
                self.0
            }
        }
    };
}

record_reference!(StringRef);
record_reference!(SchemaRef);
record_reference!(FieldRef);
record_reference!(SystemRef);
record_reference!(ParameterRef);
record_reference!(QueryRef);
record_reference!(TermRef);
record_reference!(ScheduleRef);
record_reference!(ScheduleItemRef);
record_reference!(StartupOperationRef);
record_reference!(PayloadRef);
record_reference!(FunctionLinkRef);
record_reference!(SourceSpanRef);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeImageRange {
    pub offset: u64,
    pub byte_len: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionPackage {
    pub strings: Vec<String>,
    pub world: WorldRecord,
    pub schemas: Vec<SchemaRecord>,
    pub fields: Vec<FieldRecord>,
    pub systems: Vec<SystemRecord>,
    pub parameters: Vec<ParameterRecord>,
    pub queries: Vec<QueryRecord>,
    pub terms: Vec<TermRecord>,
    pub schedules: Vec<ScheduleRecord>,
    pub schedule_items: Vec<ScheduleItemRecord>,
    pub startup_operations: Vec<StartupOperationRecord>,
    pub payloads: Vec<PayloadRecord>,
    pub function_links: Vec<FunctionLinkRecord>,
    pub source_spans: Vec<SourceSpanRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorldRecord {
    pub name: StringRef,
    pub source_span: Option<SourceSpanRef>,
    pub startup_abi_hash: AbiHash,
    pub startup_body_hash: BodyHash,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchemaFlags(u64);

impl SchemaFlags {
    pub const NONE: Self = Self(0);
    pub const TAG: Self = Self(wire::schema::flags::TAG);

    pub const fn for_kind(kind: SchemaKind) -> Self {
        match kind {
            SchemaKind::Tag => Self::TAG,
            SchemaKind::Component | SchemaKind::Resource => Self::NONE,
        }
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchemaRecord {
    pub id: SchemaId,
    pub kind: SchemaKind,
    pub flags: SchemaFlags,
    pub name: StringRef,
    pub byte_size: u64,
    pub alignment: u64,
    pub source_span: Option<SourceSpanRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FieldRecord {
    pub schema: SchemaRef,
    pub name: StringRef,
    pub primitive: PrimitiveType,
    pub byte_offset: u64,
    pub source_span: Option<SourceSpanRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SystemRecord {
    pub id: DeclId,
    pub name: StringRef,
    pub abi_hash: AbiHash,
    pub body_hash: BodyHash,
    pub source_span: Option<SourceSpanRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterKind {
    ReadResource { resource: SchemaRef },
    MutResource { resource: SchemaRef },
    Query { query: QueryRef },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParameterRecord {
    pub system: SystemRef,
    pub name: StringRef,
    pub kind: ParameterKind,
    pub source_span: Option<SourceSpanRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryRecord {
    pub id: DeclId,
    pub system: SystemRef,
    pub parameter: ParameterRef,
    pub source_span: Option<SourceSpanRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryAccess {
    Read,
    Mut,
    Exclude,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TermRecord {
    pub query: QueryRef,
    pub access: QueryAccess,
    pub schema: SchemaRef,
    pub source_span: Option<SourceSpanRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleRecord {
    pub id: DeclId,
    pub name: StringRef,
    pub source_span: Option<SourceSpanRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleItemKind {
    RunSystem { system: SystemRef },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleItemRecord {
    pub schedule: ScheduleRef,
    pub kind: ScheduleItemKind,
    pub source_span: Option<SourceSpanRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupOperationKind {
    ResourcePayload {
        resource: SchemaRef,
        payload: PayloadRef,
    },
    Spawn {
        first_payload: PayloadRef,
        payload_count: u64,
    },
    RunSchedule {
        schedule: ScheduleRef,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StartupOperationRecord {
    pub kind: StartupOperationKind,
    pub source_span: Option<SourceSpanRef>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayloadRecord {
    pub schema: SchemaRef,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FunctionTarget {
    Startup,
    System { system: SystemRef },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FunctionLinkRecord {
    pub target: FunctionTarget,
    pub symbol_name: StringRef,
    pub abi_hash: AbiHash,
    pub body_hash: BodyHash,
    pub code_offset: u64,
    pub code_byte_len: u64,
    pub source_span: Option<SourceSpanRef>,
    pub first_body_span: Option<SourceSpanRef>,
    pub body_span_count: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSpanRecord {
    pub file_name: StringRef,
    pub start_byte: u64,
    pub end_byte: u64,
    pub start_line: u64,
    pub start_column: u64,
    pub end_line: u64,
    pub end_column: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionPackageV2Error {
    Envelope(MetadataV2Error),
    MissingSection {
        kind: u64,
    },
    UnexpectedSection {
        kind: u64,
    },
    InvalidSectionShape {
        kind: u64,
        reason: &'static str,
    },
    InvalidRecord {
        section: u64,
        index: u64,
        reason: &'static str,
    },
    InvalidReference {
        owner: &'static str,
        index: u64,
        target: &'static str,
        reference: u64,
        target_count: u64,
    },
    InvalidOrdering {
        table: &'static str,
        index: u64,
    },
    InvalidIdentifier {
        table: &'static str,
        index: u64,
    },
    InvalidUtf8 {
        string_index: u64,
    },
    InvalidPayload {
        index: u64,
        reason: &'static str,
    },
    InvalidFunctionLink {
        index: u64,
        reason: &'static str,
    },
    UnusedRecord {
        table: &'static str,
        index: u64,
    },
    ArithmeticOverflow {
        context: &'static str,
    },
    AllocationFailed {
        context: &'static str,
    },
    NonCanonicalEncoding,
}

impl fmt::Display for ExecutionPackageV2Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Envelope(error) => write!(formatter, "{error}"),
            Self::MissingSection { kind } => {
                write!(formatter, "ARCHEECS v2 package section {kind} is missing")
            }
            Self::UnexpectedSection { kind } => {
                write!(formatter, "unexpected ARCHEECS v2 package section {kind}")
            }
            Self::InvalidSectionShape { kind, reason } => {
                write!(formatter, "ARCHEECS v2 package section {kind}: {reason}")
            }
            Self::InvalidRecord {
                section,
                index,
                reason,
            } => write!(
                formatter,
                "ARCHEECS v2 package section {section} record {index}: {reason}"
            ),
            Self::InvalidReference {
                owner,
                index,
                target,
                reference,
                target_count,
            } => write!(
                formatter,
                "{owner} record {index} references {target} record {reference}, but the table has {target_count} records"
            ),
            Self::InvalidOrdering { table, index } => {
                write!(formatter, "{table} record {index} is not in canonical order")
            }
            Self::InvalidIdentifier { table, index } => {
                write!(formatter, "{table} record {index} has a noncanonical identifier")
            }
            Self::InvalidUtf8 { string_index } => {
                write!(formatter, "string record {string_index} is not valid UTF-8")
            }
            Self::InvalidPayload { index, reason } => {
                write!(formatter, "payload record {index}: {reason}")
            }
            Self::InvalidFunctionLink { index, reason } => {
                write!(formatter, "function-link record {index}: {reason}")
            }
            Self::UnusedRecord { table, index } => {
                write!(formatter, "{table} record {index} is not referenced")
            }
            Self::ArithmeticOverflow { context } => {
                write!(formatter, "ARCHEECS v2 package {context} overflows u64")
            }
            Self::AllocationFailed { context } => {
                write!(formatter, "failed to allocate ARCHEECS v2 package {context}")
            }
            Self::NonCanonicalEncoding => {
                formatter.write_str("ARCHEECS v2 execution package is not canonically encoded")
            }
        }
    }
}

impl std::error::Error for ExecutionPackageV2Error {}

impl From<MetadataV2Error> for ExecutionPackageV2Error {
    fn from(error: MetadataV2Error) -> Self {
        Self::Envelope(error)
    }
}

#[cfg(test)]
pub fn encode_package(package: &ExecutionPackage) -> Result<Vec<u8>, ExecutionPackageV2Error> {
    let mut output = io::Cursor::new(Vec::new());
    write_package(&mut output, package)?;
    Ok(output.into_inner())
}

#[cfg(test)]
pub fn encode_package_with_code_range(
    package: &ExecutionPackage,
    code_range: CodeImageRange,
) -> Result<Vec<u8>, ExecutionPackageV2Error> {
    let mut output = io::Cursor::new(Vec::new());
    write_package_with_code_range(&mut output, package, code_range)?;
    Ok(output.into_inner())
}

pub fn write_package<W: Write + Seek>(
    output: &mut W,
    package: &ExecutionPackage,
) -> Result<u64, ExecutionPackageV2Error> {
    write_package_internal(output, package, None)
}

pub fn write_package_with_code_range<W: Write + Seek>(
    output: &mut W,
    package: &ExecutionPackage,
    code_range: CodeImageRange,
) -> Result<u64, ExecutionPackageV2Error> {
    write_package_internal(output, package, Some(code_range))
}

#[cfg(test)]
pub fn decode_package(metadata: &[u8]) -> Result<ExecutionPackage, ExecutionPackageV2Error> {
    decode_package_internal(metadata, None)
}

#[cfg(test)]
pub fn decode_package_with_code_range(
    metadata: &[u8],
    code_range: CodeImageRange,
) -> Result<ExecutionPackage, ExecutionPackageV2Error> {
    decode_package_internal(metadata, Some(code_range))
}

pub fn decode_package_from<R: Read + Seek>(
    input: &mut R,
) -> Result<ExecutionPackage, ExecutionPackageV2Error> {
    decode_package_from_internal(input, None)
}

pub fn decode_package_from_with_code_range<R: Read + Seek>(
    input: &mut R,
    code_range: CodeImageRange,
) -> Result<ExecutionPackage, ExecutionPackageV2Error> {
    decode_package_from_internal(input, Some(code_range))
}

pub fn validate_package(package: &ExecutionPackage) -> Result<(), ExecutionPackageV2Error> {
    validate_package_internal(package, None).map(|_| ())
}

pub fn validate_package_with_code_range(
    package: &ExecutionPackage,
    code_range: CodeImageRange,
) -> Result<(), ExecutionPackageV2Error> {
    validate_package_internal(package, Some(code_range)).map(|_| ())
}

fn write_package_internal<W: Write + Seek>(
    output: &mut W,
    package: &ExecutionPackage,
    code_range: Option<CodeImageRange>,
) -> Result<u64, ExecutionPackageV2Error> {
    validate_package_internal(package, code_range)?;
    let sections = package_section_descriptors(package)?;
    Ok(ecs_metadata_v2::write_streaming(
        output,
        &sections,
        |kind, output| write_package_section(output, package, kind),
    )?)
}

#[cfg(test)]
fn decode_package_internal(
    metadata: &[u8],
    code_range: Option<CodeImageRange>,
) -> Result<ExecutionPackage, ExecutionPackageV2Error> {
    let envelope = ecs_metadata_v2::decode(metadata)?;
    decode_package_sections(envelope.sections(), code_range)
}

fn decode_package_from_internal<R: Read + Seek>(
    input: &mut R,
    code_range: Option<CodeImageRange>,
) -> Result<ExecutionPackage, ExecutionPackageV2Error> {
    let envelope = ecs_metadata_v2::read(input)?;
    let mut sections = Vec::new();
    sections
        .try_reserve_exact(envelope.sections().len())
        .map_err(|_| ExecutionPackageV2Error::AllocationFailed {
            context: "streamed package section views",
        })?;
    sections.extend(envelope.sections().iter().map(|section| section.as_view()));
    decode_package_sections(&sections, code_range)
}

fn decode_package_sections(
    sections: &[SectionView<'_>],
    code_range: Option<CodeImageRange>,
) -> Result<ExecutionPackage, ExecutionPackageV2Error> {
    if sections.len() != SECTION_KINDS.len() {
        for kind in SECTION_KINDS {
            if sections.iter().all(|section| section.kind != kind) {
                return Err(ExecutionPackageV2Error::MissingSection { kind });
            }
        }
        let unexpected = sections
            .iter()
            .find(|section| !SECTION_KINDS.contains(&section.kind))
            .expect("different count with no missing section implies an unexpected section");
        return Err(ExecutionPackageV2Error::UnexpectedSection {
            kind: unexpected.kind,
        });
    }
    for section in sections {
        if !SECTION_KINDS.contains(&section.kind) {
            return Err(ExecutionPackageV2Error::UnexpectedSection { kind: section.kind });
        }
        if section.alignment != SECTION_ALIGNMENT {
            return Err(ExecutionPackageV2Error::InvalidSectionShape {
                kind: section.kind,
                reason: "section alignment is not the canonical package alignment",
            });
        }
    }

    let strings = decode_strings(required_section(sections, section_kind::STRINGS)?)?;
    let world_section =
        required_fixed_section(sections, section_kind::WORLD, 1, WORLD_RECORD_SIZE)?;
    let world = decode_world(world_section.payload);
    let schemas = decode_schemas(required_fixed_stride(
        sections,
        section_kind::SCHEMAS,
        SCHEMA_RECORD_SIZE,
    )?)?;
    let fields = decode_fields(required_fixed_stride(
        sections,
        section_kind::FIELDS,
        FIELD_RECORD_SIZE,
    )?)?;
    let systems = decode_systems(required_fixed_stride(
        sections,
        section_kind::SYSTEMS,
        SYSTEM_RECORD_SIZE,
    )?)?;
    let parameters = decode_parameters(required_fixed_stride(
        sections,
        section_kind::PARAMETERS,
        PARAMETER_RECORD_SIZE,
    )?)?;
    let queries = decode_queries(required_fixed_stride(
        sections,
        section_kind::QUERIES,
        QUERY_RECORD_SIZE,
    )?)?;
    let terms = decode_terms(required_fixed_stride(
        sections,
        section_kind::TERMS,
        TERM_RECORD_SIZE,
    )?)?;
    let schedules = decode_schedules(required_fixed_stride(
        sections,
        section_kind::SCHEDULES,
        SCHEDULE_RECORD_SIZE,
    )?)?;
    let schedule_items = decode_schedule_items(required_fixed_stride(
        sections,
        section_kind::SCHEDULE_ITEMS,
        SCHEDULE_ITEM_RECORD_SIZE,
    )?)?;
    let startup_operations = decode_startup_operations(required_fixed_stride(
        sections,
        section_kind::STARTUP_OPERATIONS,
        STARTUP_OPERATION_RECORD_SIZE,
    )?)?;
    let payloads = decode_payloads(required_section(sections, section_kind::PAYLOADS)?)?;
    let function_links = decode_function_links(required_fixed_stride(
        sections,
        section_kind::FUNCTION_LINKS,
        FUNCTION_LINK_RECORD_SIZE,
    )?)?;
    let source_spans = decode_source_spans(required_fixed_stride(
        sections,
        section_kind::SOURCE_SPANS,
        SOURCE_SPAN_RECORD_SIZE,
    )?)?;

    let package = ExecutionPackage {
        strings,
        world,
        schemas,
        fields,
        systems,
        parameters,
        queries,
        terms,
        schedules,
        schedule_items,
        startup_operations,
        payloads,
        function_links,
        source_spans,
    };
    validate_package_internal(&package, code_range)?;
    validate_canonical_section_payloads(&package, sections)?;
    Ok(package)
}

fn validate_package_internal(
    package: &ExecutionPackage,
    code_range: Option<CodeImageRange>,
) -> Result<(), ExecutionPackageV2Error> {
    let counts = TableCounts {
        strings: count(package.strings.len(), "string count")?,
        schemas: count(package.schemas.len(), "schema count")?,
        systems: count(package.systems.len(), "system count")?,
        parameters: count(package.parameters.len(), "parameter count")?,
        queries: count(package.queries.len(), "query count")?,
        schedules: count(package.schedules.len(), "schedule count")?,
        payloads: count(package.payloads.len(), "payload count")?,
        spans: count(package.source_spans.len(), "source-span count")?,
    };
    let mut used_strings = false_table(package.strings.len(), "string usage table")?;
    let mut used_spans = false_table(package.source_spans.len(), "source-span usage table")?;
    require_canonical_strings(&package.strings)?;
    {
        let mut usage = ValidationUsage {
            strings: &mut used_strings,
            spans: &mut used_spans,
        };
        mark_string(
            package.world.name,
            counts.strings,
            usage.strings,
            "world",
            0,
        )?;
        mark_optional_span(
            package.world.source_span,
            counts.spans,
            usage.spans,
            "world",
            0,
        )?;
        validate_source_spans(package, &counts, &mut usage)?;
        validate_schemas(package, &counts, &mut usage)?;
        validate_systems_and_queries(package, &counts, &mut usage)?;
        validate_schedules(package, &counts, &mut usage)?;
        validate_payloads_and_startup(package, &counts, &mut usage)?;
        validate_function_links(package, &counts, code_range, &mut usage)?;
    }

    for (index, used) in used_strings.into_iter().enumerate() {
        if !used {
            return Err(ExecutionPackageV2Error::UnusedRecord {
                table: "strings",
                index: count(index, "string index")?,
            });
        }
    }
    for (index, used) in used_spans.into_iter().enumerate() {
        if !used {
            return Err(ExecutionPackageV2Error::UnusedRecord {
                table: "source spans",
                index: count(index, "source-span index")?,
            });
        }
    }
    Ok(())
}

struct TableCounts {
    strings: u64,
    schemas: u64,
    systems: u64,
    parameters: u64,
    queries: u64,
    schedules: u64,
    payloads: u64,
    spans: u64,
}

struct ValidationUsage<'a> {
    strings: &'a mut [bool],
    spans: &'a mut [bool],
}

fn validate_source_spans(
    package: &ExecutionPackage,
    counts: &TableCounts,
    usage: &mut ValidationUsage<'_>,
) -> Result<(), ExecutionPackageV2Error> {
    let mut previous = None;
    for (index, span) in package.source_spans.iter().enumerate() {
        let index = count(index, "source-span index")?;
        mark_string(
            span.file_name,
            counts.strings,
            usage.strings,
            "source spans",
            index,
        )?;
        if span.start_byte > span.end_byte {
            return invalid_record(
                section_kind::SOURCE_SPANS,
                index,
                "source span starts after it ends",
            );
        }
        if span.start_line == 0
            || span.start_column == 0
            || span.end_line == 0
            || span.end_column == 0
        {
            return invalid_record(
                section_kind::SOURCE_SPANS,
                index,
                "source span line and column coordinates are one-based",
            );
        }
        if (span.start_line, span.start_column) > (span.end_line, span.end_column) {
            return invalid_record(
                section_kind::SOURCE_SPANS,
                index,
                "source span location starts after it ends",
            );
        }
        if (span.start_byte == span.end_byte)
            != ((span.start_line, span.start_column) == (span.end_line, span.end_column))
        {
            return invalid_record(
                section_kind::SOURCE_SPANS,
                index,
                "empty source span byte and location endpoints disagree",
            );
        }
        let key = (span.file_name.index(), span.start_byte, span.end_byte);
        if previous.is_some_and(|value| value >= key) {
            return Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "source spans",
                index,
            });
        }
        previous = Some(key);
    }
    Ok(())
}

fn validate_schemas(
    package: &ExecutionPackage,
    counts: &TableCounts,
    usage: &mut ValidationUsage<'_>,
) -> Result<(), ExecutionPackageV2Error> {
    let mut previous_id = None;
    let mut previous_field_schema = None;
    for (index, field) in package.fields.iter().enumerate() {
        let index = count(index, "field index")?;
        reference(
            "fields",
            index,
            "schemas",
            field.schema.index(),
            counts.schemas,
        )?;
        if previous_field_schema.is_some_and(|schema| schema > field.schema.index()) {
            return Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "fields",
                index,
            });
        }
        previous_field_schema = Some(field.schema.index());
        mark_string(field.name, counts.strings, usage.strings, "fields", index)?;
        mark_optional_span(
            field.source_span,
            counts.spans,
            usage.spans,
            "fields",
            index,
        )?;
    }

    let mut schema_names = HashSet::new();
    schema_names
        .try_reserve(package.schemas.len())
        .map_err(|_| ExecutionPackageV2Error::AllocationFailed {
            context: "schema-name validation",
        })?;
    for (schema_index, schema) in package.schemas.iter().enumerate() {
        let index = count(schema_index, "schema index")?;
        if schema.flags.bits() & !wire::schema::flags::KNOWN_MASK != 0 {
            return invalid_record(section_kind::SCHEMAS, index, "unknown schema flag bits");
        }
        if schema.flags != SchemaFlags::for_kind(schema.kind) {
            return invalid_record(
                section_kind::SCHEMAS,
                index,
                "schema flags do not match schema kind",
            );
        }
        if previous_id.is_some_and(|id| id >= schema.id) {
            return Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "schemas",
                index,
            });
        }
        previous_id = Some(schema.id);
        mark_string(schema.name, counts.strings, usage.strings, "schemas", index)?;
        mark_optional_span(
            schema.source_span,
            counts.spans,
            usage.spans,
            "schemas",
            index,
        )?;

        let schema_name = package.strings[schema.name.index() as usize].as_str();
        let resource_namespace = schema.kind == SchemaKind::Resource;
        if !schema_names.insert((resource_namespace, schema_name)) {
            return Err(ExecutionPackageV2Error::InvalidRecord {
                section: section_kind::SCHEMAS,
                index,
                reason: "schema name is duplicated",
            });
        }

        let field_count = package
            .fields
            .iter()
            .filter(|field| field.schema.index() == index)
            .count();
        if schema.kind == SchemaKind::Tag && field_count != 0 {
            return invalid_record(
                section_kind::SCHEMAS,
                index,
                "tag schemas cannot have fields",
            );
        }
        let mut expected_offset = 0u64;
        let mut expected_alignment = 1u64;
        let mut field_names = HashSet::new();
        field_names.try_reserve(field_count).map_err(|_| {
            ExecutionPackageV2Error::AllocationFailed {
                context: "schema field-name validation",
            }
        })?;
        let mut fingerprint_fields = Vec::new();
        fingerprint_fields
            .try_reserve_exact(field_count)
            .map_err(|_| ExecutionPackageV2Error::AllocationFailed {
                context: "schema fingerprint fields",
            })?;
        for field in package
            .fields
            .iter()
            .filter(|field| field.schema.index() == index)
        {
            let name = &package.strings[field.name.index() as usize];
            if !field_names.insert(name.as_str()) {
                return invalid_record(
                    section_kind::FIELDS,
                    field.schema.index(),
                    "field name is duplicated within its schema",
                );
            }
            let alignment = primitive_alignment(field.primitive);
            expected_alignment = expected_alignment.max(alignment);
            expected_offset = align_value(expected_offset, alignment, "field layout")?;
            if field.byte_offset != expected_offset {
                return invalid_record(
                    section_kind::FIELDS,
                    field.schema.index(),
                    "field byte offset does not match declaration-order layout",
                );
            }
            expected_offset = expected_offset
                .checked_add(primitive_size(field.primitive))
                .ok_or(ExecutionPackageV2Error::ArithmeticOverflow {
                    context: "schema byte size",
                })?;
            fingerprint_fields.push(SchemaField {
                name,
                primitive: field.primitive,
            });
        }
        let expected_size = align_value(expected_offset, expected_alignment, "schema layout")?;
        if schema.byte_size != expected_size || schema.alignment != expected_alignment {
            return invalid_record(
                section_kind::SCHEMAS,
                index,
                "schema size or alignment does not match its fields",
            );
        }
        let expected_id = SchemaId::derive(
            schema.kind,
            &package.strings[package.world.name.index() as usize],
            &package.strings[schema.name.index() as usize],
            &fingerprint_fields,
        );
        if schema.id != expected_id {
            return Err(ExecutionPackageV2Error::InvalidIdentifier {
                table: "schemas",
                index,
            });
        }
    }
    Ok(())
}

fn validate_systems_and_queries(
    package: &ExecutionPackage,
    counts: &TableCounts,
    usage: &mut ValidationUsage<'_>,
) -> Result<(), ExecutionPackageV2Error> {
    let world = &package.strings[package.world.name.index() as usize];
    let mut previous_id = None;
    for (index, system) in package.systems.iter().enumerate() {
        let index = count(index, "system index")?;
        mark_string(system.name, counts.strings, usage.strings, "systems", index)?;
        mark_optional_span(
            system.source_span,
            counts.spans,
            usage.spans,
            "systems",
            index,
        )?;
        if previous_id.is_some_and(|id| id >= system.id) {
            return Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "systems",
                index,
            });
        }
        previous_id = Some(system.id);
        if system.id != DeclId::system(world, &package.strings[system.name.index() as usize]) {
            return Err(ExecutionPackageV2Error::InvalidIdentifier {
                table: "systems",
                index,
            });
        }
    }

    let mut previous_system = None;
    for (index, parameter) in package.parameters.iter().enumerate() {
        let index = count(index, "parameter index")?;
        reference(
            "parameters",
            index,
            "systems",
            parameter.system.index(),
            counts.systems,
        )?;
        if previous_system.is_some_and(|system| system > parameter.system.index()) {
            return Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "parameters",
                index,
            });
        }
        previous_system = Some(parameter.system.index());
        mark_string(
            parameter.name,
            counts.strings,
            usage.strings,
            "parameters",
            index,
        )?;
        mark_optional_span(
            parameter.source_span,
            counts.spans,
            usage.spans,
            "parameters",
            index,
        )?;
        for earlier in &package.parameters[..index as usize] {
            if earlier.system == parameter.system
                && package.strings[earlier.name.index() as usize]
                    == package.strings[parameter.name.index() as usize]
            {
                return invalid_record(
                    section_kind::PARAMETERS,
                    index,
                    "parameter name is duplicated within its system",
                );
            }
        }
        match parameter.kind {
            ParameterKind::ReadResource { resource } | ParameterKind::MutResource { resource } => {
                reference(
                    "parameters",
                    index,
                    "schemas",
                    resource.index(),
                    counts.schemas,
                )?;
                if package.schemas[resource.index() as usize].kind != SchemaKind::Resource {
                    return invalid_record(
                        section_kind::PARAMETERS,
                        index,
                        "resource parameter references a non-resource schema",
                    );
                }
                let mutable = matches!(parameter.kind, ParameterKind::MutResource { .. });
                for earlier in &package.parameters[..index as usize] {
                    let earlier_access = match earlier.kind {
                        ParameterKind::ReadResource { resource } => Some((resource, false)),
                        ParameterKind::MutResource { resource } => Some((resource, true)),
                        ParameterKind::Query { .. } => None,
                    };
                    if earlier.system == parameter.system
                        && earlier_access.is_some_and(|(earlier_resource, earlier_mutable)| {
                            earlier_resource == resource && (earlier_mutable || mutable)
                        })
                    {
                        return invalid_record(
                            section_kind::PARAMETERS,
                            index,
                            "mutable resource access conflicts with another alias",
                        );
                    }
                }
            }
            ParameterKind::Query { query } => {
                reference(
                    "parameters",
                    index,
                    "queries",
                    query.index(),
                    counts.queries,
                )?;
            }
        }
    }

    previous_id = None;
    for (index, query) in package.queries.iter().enumerate() {
        let index = count(index, "query index")?;
        reference(
            "queries",
            index,
            "systems",
            query.system.index(),
            counts.systems,
        )?;
        reference(
            "queries",
            index,
            "parameters",
            query.parameter.index(),
            counts.parameters,
        )?;
        mark_optional_span(
            query.source_span,
            counts.spans,
            usage.spans,
            "queries",
            index,
        )?;
        if previous_id.is_some_and(|id| id >= query.id) {
            return Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "queries",
                index,
            });
        }
        previous_id = Some(query.id);
        let parameter = package.parameters[query.parameter.index() as usize];
        if parameter.system != query.system
            || parameter.kind
                != (ParameterKind::Query {
                    query: QueryRef::new(index),
                })
        {
            return invalid_record(
                section_kind::QUERIES,
                index,
                "query and parameter do not link to each other",
            );
        }
        let expected = DeclId::query(
            package.systems[query.system.index() as usize].id,
            &package.strings[parameter.name.index() as usize],
        );
        if query.id != expected {
            return Err(ExecutionPackageV2Error::InvalidIdentifier {
                table: "queries",
                index,
            });
        }
    }

    let mut previous_query = None;
    for (index, term) in package.terms.iter().enumerate() {
        let index = count(index, "term index")?;
        reference(
            "terms",
            index,
            "queries",
            term.query.index(),
            counts.queries,
        )?;
        reference(
            "terms",
            index,
            "schemas",
            term.schema.index(),
            counts.schemas,
        )?;
        if previous_query.is_some_and(|query| query > term.query.index()) {
            return Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "terms",
                index,
            });
        }
        previous_query = Some(term.query.index());
        mark_optional_span(term.source_span, counts.spans, usage.spans, "terms", index)?;
        let schema = package.schemas[term.schema.index() as usize];
        if schema.kind == SchemaKind::Resource {
            return invalid_record(
                section_kind::TERMS,
                index,
                "query term references a resource schema",
            );
        }
        if term.access == QueryAccess::Mut && schema.kind == SchemaKind::Tag {
            return invalid_record(
                section_kind::TERMS,
                index,
                "tag query terms cannot be mutable",
            );
        }
        for earlier in &package.terms[..index as usize] {
            if earlier.query == term.query
                && earlier.schema == term.schema
                && (earlier.access != QueryAccess::Read || term.access != QueryAccess::Read)
            {
                return invalid_record(
                    section_kind::TERMS,
                    index,
                    "query has conflicting access to one schema",
                );
            }
        }
    }
    Ok(())
}

fn validate_schedules(
    package: &ExecutionPackage,
    counts: &TableCounts,
    usage: &mut ValidationUsage<'_>,
) -> Result<(), ExecutionPackageV2Error> {
    let world = &package.strings[package.world.name.index() as usize];
    let mut previous_id = None;
    for (index, schedule) in package.schedules.iter().enumerate() {
        let index = count(index, "schedule index")?;
        mark_string(
            schedule.name,
            counts.strings,
            usage.strings,
            "schedules",
            index,
        )?;
        mark_optional_span(
            schedule.source_span,
            counts.spans,
            usage.spans,
            "schedules",
            index,
        )?;
        if previous_id.is_some_and(|id| id >= schedule.id) {
            return Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "schedules",
                index,
            });
        }
        previous_id = Some(schedule.id);
        if schedule.id != DeclId::schedule(world, &package.strings[schedule.name.index() as usize])
        {
            return Err(ExecutionPackageV2Error::InvalidIdentifier {
                table: "schedules",
                index,
            });
        }
    }

    let mut previous_schedule = None;
    for (index, item) in package.schedule_items.iter().enumerate() {
        let index = count(index, "schedule-item index")?;
        reference(
            "schedule items",
            index,
            "schedules",
            item.schedule.index(),
            counts.schedules,
        )?;
        if previous_schedule.is_some_and(|schedule| schedule > item.schedule.index()) {
            return Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "schedule items",
                index,
            });
        }
        previous_schedule = Some(item.schedule.index());
        mark_optional_span(
            item.source_span,
            counts.spans,
            usage.spans,
            "schedule items",
            index,
        )?;
        match item.kind {
            ScheduleItemKind::RunSystem { system } => reference(
                "schedule items",
                index,
                "systems",
                system.index(),
                counts.systems,
            )?,
        }
    }
    Ok(())
}

fn validate_payloads_and_startup(
    package: &ExecutionPackage,
    counts: &TableCounts,
    usage: &mut ValidationUsage<'_>,
) -> Result<(), ExecutionPackageV2Error> {
    for (index, payload) in package.payloads.iter().enumerate() {
        let index = count(index, "payload index")?;
        reference(
            "payloads",
            index,
            "schemas",
            payload.schema.index(),
            counts.schemas,
        )?;
        let expected = package.schemas[payload.schema.index() as usize].byte_size;
        if count(payload.bytes.len(), "payload byte length")? != expected {
            return Err(ExecutionPackageV2Error::InvalidPayload {
                index,
                reason: "payload byte length does not match its schema",
            });
        }
        for field in package
            .fields
            .iter()
            .filter(|field| field.schema == payload.schema)
        {
            if field.primitive != PrimitiveType::Bool {
                continue;
            }
            let offset = usize::try_from(field.byte_offset).map_err(|_| {
                ExecutionPackageV2Error::InvalidPayload {
                    index,
                    reason: "bool field offset does not fit the host address space",
                }
            })?;
            if !matches!(payload.bytes.get(offset), Some(0 | 1)) {
                return Err(ExecutionPackageV2Error::InvalidPayload {
                    index,
                    reason: "bool fields must be encoded as 0 or 1",
                });
            }
        }
    }

    let mut initialized = false_table(package.schemas.len(), "resource initialization table")?;
    let mut used_payloads = false_table(package.payloads.len(), "payload usage table")?;
    for (index, operation) in package.startup_operations.iter().enumerate() {
        let index = count(index, "startup-operation index")?;
        mark_optional_span(
            operation.source_span,
            counts.spans,
            usage.spans,
            "startup operations",
            index,
        )?;
        match operation.kind {
            StartupOperationKind::ResourcePayload { resource, payload } => {
                reference(
                    "startup operations",
                    index,
                    "schemas",
                    resource.index(),
                    counts.schemas,
                )?;
                reference(
                    "startup operations",
                    index,
                    "payloads",
                    payload.index(),
                    counts.payloads,
                )?;
                if package.schemas[resource.index() as usize].kind != SchemaKind::Resource
                    || package.payloads[payload.index() as usize].schema != resource
                {
                    return invalid_record(
                        section_kind::STARTUP_OPERATIONS,
                        index,
                        "resource initialization does not link one resource to its payload",
                    );
                }
                if std::mem::replace(&mut initialized[resource.index() as usize], true) {
                    return invalid_record(
                        section_kind::STARTUP_OPERATIONS,
                        index,
                        "resource is initialized more than once",
                    );
                }
                if std::mem::replace(&mut used_payloads[payload.index() as usize], true) {
                    return invalid_record(
                        section_kind::STARTUP_OPERATIONS,
                        index,
                        "payload record is used by more than one startup operation",
                    );
                }
            }
            StartupOperationKind::Spawn {
                first_payload,
                payload_count: spawn_count,
            } => {
                let end = first_payload.index().checked_add(spawn_count).ok_or(
                    ExecutionPackageV2Error::ArithmeticOverflow {
                        context: "spawn payload range",
                    },
                )?;
                if end > counts.payloads {
                    return Err(ExecutionPackageV2Error::InvalidReference {
                        owner: "startup operations",
                        index,
                        target: "payloads",
                        reference: end,
                        target_count: counts.payloads,
                    });
                }
                let mut prior_schema = None;
                for payload in &package.payloads[first_payload.index() as usize..end as usize] {
                    let schema = package.schemas[payload.schema.index() as usize];
                    if schema.kind == SchemaKind::Resource {
                        return invalid_record(
                            section_kind::STARTUP_OPERATIONS,
                            index,
                            "spawn payload references a resource",
                        );
                    }
                    if prior_schema.is_some_and(|value| value >= payload.schema.index()) {
                        return invalid_record(
                            section_kind::STARTUP_OPERATIONS,
                            index,
                            "spawn payloads are not in canonical schema order",
                        );
                    }
                    prior_schema = Some(payload.schema.index());
                }
                for used in &mut used_payloads[first_payload.index() as usize..end as usize] {
                    if std::mem::replace(used, true) {
                        return invalid_record(
                            section_kind::STARTUP_OPERATIONS,
                            index,
                            "payload record is used by more than one startup operation",
                        );
                    }
                }
            }
            StartupOperationKind::RunSchedule { schedule } => {
                reference(
                    "startup operations",
                    index,
                    "schedules",
                    schedule.index(),
                    counts.schedules,
                )?;
                for item in package
                    .schedule_items
                    .iter()
                    .filter(|item| item.schedule == schedule)
                {
                    let ScheduleItemKind::RunSystem { system } = item.kind;
                    for parameter in package
                        .parameters
                        .iter()
                        .filter(|parameter| parameter.system == system)
                    {
                        if let ParameterKind::ReadResource { resource }
                        | ParameterKind::MutResource { resource } = parameter.kind
                        {
                            if !initialized[resource.index() as usize] {
                                return invalid_record(
                                    section_kind::STARTUP_OPERATIONS,
                                    index,
                                    "schedule reads a resource before initialization",
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    for (index, used) in used_payloads.into_iter().enumerate() {
        if !used {
            return Err(ExecutionPackageV2Error::UnusedRecord {
                table: "payloads",
                index: count(index, "payload index")?,
            });
        }
    }
    Ok(())
}

fn validate_function_links(
    package: &ExecutionPackage,
    counts: &TableCounts,
    code_range: Option<CodeImageRange>,
    usage: &mut ValidationUsage<'_>,
) -> Result<(), ExecutionPackageV2Error> {
    let expected_link_count = package.systems.len().checked_add(1).ok_or(
        ExecutionPackageV2Error::ArithmeticOverflow {
            context: "function-link count",
        },
    )?;
    if package.function_links.len() != expected_link_count {
        return Err(ExecutionPackageV2Error::InvalidFunctionLink {
            index: 0,
            reason: "there must be one startup link and exactly one link per system",
        });
    }
    let code_end = code_range
        .map(|range| {
            range.offset.checked_add(range.byte_len).ok_or(
                ExecutionPackageV2Error::ArithmeticOverflow {
                    context: "code image range",
                },
            )
        })
        .transpose()?;
    for (position, link) in package.function_links.iter().enumerate() {
        let index = count(position, "function-link index")?;
        let (expected_abi_hash, expected_body_hash) = match link.target {
            FunctionTarget::Startup if position == 0 => (
                package.world.startup_abi_hash,
                package.world.startup_body_hash,
            ),
            FunctionTarget::System { system } if position != 0 => {
                reference(
                    "function links",
                    index,
                    "systems",
                    system.index(),
                    counts.systems,
                )?;
                if system.index() != index - 1 {
                    return Err(ExecutionPackageV2Error::InvalidOrdering {
                        table: "function links",
                        index,
                    });
                }
                let system = package.systems[system.index() as usize];
                (system.abi_hash, system.body_hash)
            }
            FunctionTarget::Startup | FunctionTarget::System { .. } => {
                return Err(ExecutionPackageV2Error::InvalidOrdering {
                    table: "function links",
                    index,
                });
            }
        };
        mark_string(
            link.symbol_name,
            counts.strings,
            usage.strings,
            "function links",
            index,
        )?;
        mark_optional_span(
            link.source_span,
            counts.spans,
            usage.spans,
            "function links",
            index,
        )?;
        match (link.first_body_span, link.body_span_count) {
            (None, 0) => {}
            (Some(_), 0) | (None, _) => {
                return Err(ExecutionPackageV2Error::InvalidFunctionLink {
                    index,
                    reason: "native body-span reference and count disagree",
                });
            }
            (Some(first), count) => {
                let end = first.index().checked_add(count).ok_or(
                    ExecutionPackageV2Error::InvalidFunctionLink {
                        index,
                        reason: "native body-span range overflows u64",
                    },
                )?;
                if end > counts.spans {
                    return Err(ExecutionPackageV2Error::InvalidFunctionLink {
                        index,
                        reason: "native body-span range is outside the source-span table",
                    });
                }
                let owner_ref =
                    link.source_span
                        .ok_or(ExecutionPackageV2Error::InvalidFunctionLink {
                            index,
                            reason: "native body-span slice requires a function source span",
                        })?;
                let owner = package.source_spans[owner_ref.index() as usize];
                for earlier in &package.function_links[..position] {
                    let Some(earlier_first) = earlier.first_body_span else {
                        continue;
                    };
                    let earlier_end = earlier_first
                        .index()
                        .checked_add(earlier.body_span_count)
                        .ok_or(ExecutionPackageV2Error::InvalidFunctionLink {
                            index,
                            reason: "native body-span range overflows u64",
                        })?;
                    if first.index() < earlier_end && earlier_first.index() < end {
                        return Err(ExecutionPackageV2Error::InvalidFunctionLink {
                            index,
                            reason: "native function body-span ranges overlap",
                        });
                    }
                }
                for span_index in first.index()..end {
                    let body_span = package.source_spans[span_index as usize];
                    if body_span.file_name != owner.file_name
                        || body_span.start_byte < owner.start_byte
                        || body_span.end_byte > owner.end_byte
                    {
                        return Err(ExecutionPackageV2Error::InvalidFunctionLink {
                            index,
                            reason:
                                "native body span is not nested within its function source span",
                        });
                    }
                    mark_optional_span(
                        Some(SourceSpanRef::new(span_index)),
                        counts.spans,
                        usage.spans,
                        "function body spans",
                        index,
                    )?;
                }
            }
        }
        if link.abi_hash != expected_abi_hash {
            return Err(ExecutionPackageV2Error::InvalidFunctionLink {
                index,
                reason: "ABI hash does not match the linked Core function",
            });
        }
        if link.body_hash != expected_body_hash {
            return Err(ExecutionPackageV2Error::InvalidFunctionLink {
                index,
                reason: "Core-body hash does not match the linked Core function",
            });
        }
        if link.code_byte_len == 0 {
            return Err(ExecutionPackageV2Error::InvalidFunctionLink {
                index,
                reason: "native function has zero byte length",
            });
        }
        let function_end = link.code_offset.checked_add(link.code_byte_len).ok_or(
            ExecutionPackageV2Error::InvalidFunctionLink {
                index,
                reason: "native function range overflows u64",
            },
        )?;
        for earlier in &package.function_links[..index as usize] {
            let earlier_end = earlier
                .code_offset
                .checked_add(earlier.code_byte_len)
                .ok_or(ExecutionPackageV2Error::InvalidFunctionLink {
                    index,
                    reason: "native function range overflows u64",
                })?;
            if link.code_offset < earlier_end && earlier.code_offset < function_end {
                return Err(ExecutionPackageV2Error::InvalidFunctionLink {
                    index,
                    reason: "native function ranges overlap",
                });
            }
        }
        if let (Some(range), Some(code_end)) = (code_range, code_end) {
            if link.code_offset < range.offset || function_end > code_end {
                return Err(ExecutionPackageV2Error::InvalidFunctionLink {
                    index,
                    reason: "native function is outside the selected code image",
                });
            }
        }
    }
    Ok(())
}

fn require_canonical_strings(strings: &[String]) -> Result<(), ExecutionPackageV2Error> {
    for (index, pair) in strings.windows(2).enumerate() {
        if pair[0] >= pair[1] {
            return Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "strings",
                index: count(index + 1, "string index")?,
            });
        }
    }
    Ok(())
}

fn required_section<'sections, 'payload>(
    sections: &'sections [SectionView<'payload>],
    kind: u64,
) -> Result<&'sections SectionView<'payload>, ExecutionPackageV2Error> {
    sections
        .iter()
        .find(|section| section.kind == kind)
        .ok_or(ExecutionPackageV2Error::MissingSection { kind })
}

fn required_fixed_section<'sections, 'payload>(
    sections: &'sections [SectionView<'payload>],
    kind: u64,
    count: u64,
    stride: u64,
) -> Result<&'sections SectionView<'payload>, ExecutionPackageV2Error> {
    let section = required_section(sections, kind)?;
    if section.record_count != count || section.record_stride != stride {
        return Err(ExecutionPackageV2Error::InvalidSectionShape {
            kind,
            reason: "fixed-record section has the wrong count or stride",
        });
    }
    Ok(section)
}

fn required_fixed_stride<'sections, 'payload>(
    sections: &'sections [SectionView<'payload>],
    kind: u64,
    stride: u64,
) -> Result<&'sections SectionView<'payload>, ExecutionPackageV2Error> {
    let section = required_section(sections, kind)?;
    if section.record_stride != stride {
        return Err(ExecutionPackageV2Error::InvalidSectionShape {
            kind,
            reason: "fixed-record section has the wrong stride",
        });
    }
    Ok(section)
}

fn validate_canonical_section_payloads(
    package: &ExecutionPackage,
    sections: &[SectionView<'_>],
) -> Result<(), ExecutionPackageV2Error> {
    for kind in SECTION_KINDS {
        let section = required_section(sections, kind)?;
        let mut comparator = CanonicalSectionComparator::new(section.payload);
        if write_package_section(&mut comparator, package, kind).is_err()
            || !comparator.is_exact_match()
        {
            return Err(ExecutionPackageV2Error::NonCanonicalEncoding);
        }
    }
    Ok(())
}

struct CanonicalSectionComparator<'a> {
    actual: &'a [u8],
    position: usize,
    matches: bool,
}

impl<'a> CanonicalSectionComparator<'a> {
    fn new(actual: &'a [u8]) -> Self {
        Self {
            actual,
            position: 0,
            matches: true,
        }
    }

    fn is_exact_match(&self) -> bool {
        self.matches && self.position == self.actual.len()
    }
}

impl Write for CanonicalSectionComparator<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Some(end) = self.position.checked_add(bytes.len()) else {
            self.matches = false;
            return Ok(bytes.len());
        };
        if end > self.actual.len() {
            self.matches = false;
            self.position = self.actual.len();
            return Ok(bytes.len());
        }
        if self.actual[self.position..end] != *bytes {
            self.matches = false;
        }
        self.position = end;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn raw_descriptor(kind: u64, byte_len: u64) -> SectionDescriptor {
    SectionDescriptor {
        kind,
        alignment: SECTION_ALIGNMENT,
        record_count: 0,
        record_stride: 0,
        byte_len,
    }
}

fn fixed_descriptor(
    kind: u64,
    count_value: usize,
    stride: u64,
) -> Result<SectionDescriptor, ExecutionPackageV2Error> {
    let record_count = count(count_value, "section record count")?;
    let byte_len =
        record_count
            .checked_mul(stride)
            .ok_or(ExecutionPackageV2Error::ArithmeticOverflow {
                context: "fixed section byte length",
            })?;
    Ok(SectionDescriptor {
        kind,
        alignment: SECTION_ALIGNMENT,
        record_count,
        record_stride: stride,
        byte_len,
    })
}

fn package_section_descriptors(
    package: &ExecutionPackage,
) -> Result<[SectionDescriptor; 14], ExecutionPackageV2Error> {
    Ok([
        raw_descriptor(
            section_kind::STRINGS,
            string_section_byte_len(&package.strings)?,
        ),
        fixed_descriptor(section_kind::WORLD, 1, WORLD_RECORD_SIZE)?,
        fixed_descriptor(
            section_kind::SCHEMAS,
            package.schemas.len(),
            SCHEMA_RECORD_SIZE,
        )?,
        fixed_descriptor(
            section_kind::FIELDS,
            package.fields.len(),
            FIELD_RECORD_SIZE,
        )?,
        fixed_descriptor(
            section_kind::SYSTEMS,
            package.systems.len(),
            SYSTEM_RECORD_SIZE,
        )?,
        fixed_descriptor(
            section_kind::PARAMETERS,
            package.parameters.len(),
            PARAMETER_RECORD_SIZE,
        )?,
        fixed_descriptor(
            section_kind::QUERIES,
            package.queries.len(),
            QUERY_RECORD_SIZE,
        )?,
        fixed_descriptor(section_kind::TERMS, package.terms.len(), TERM_RECORD_SIZE)?,
        fixed_descriptor(
            section_kind::SCHEDULES,
            package.schedules.len(),
            SCHEDULE_RECORD_SIZE,
        )?,
        fixed_descriptor(
            section_kind::SCHEDULE_ITEMS,
            package.schedule_items.len(),
            SCHEDULE_ITEM_RECORD_SIZE,
        )?,
        fixed_descriptor(
            section_kind::STARTUP_OPERATIONS,
            package.startup_operations.len(),
            STARTUP_OPERATION_RECORD_SIZE,
        )?,
        raw_descriptor(
            section_kind::PAYLOADS,
            payload_section_byte_len(&package.payloads)?,
        ),
        fixed_descriptor(
            section_kind::FUNCTION_LINKS,
            package.function_links.len(),
            FUNCTION_LINK_RECORD_SIZE,
        )?,
        fixed_descriptor(
            section_kind::SOURCE_SPANS,
            package.source_spans.len(),
            SOURCE_SPAN_RECORD_SIZE,
        )?,
    ])
}

fn string_section_byte_len(strings: &[String]) -> Result<u64, ExecutionPackageV2Error> {
    let count_value = count(strings.len(), "string count")?;
    let record_bytes = count_value.checked_mul(STRING_RECORD_SIZE).ok_or(
        ExecutionPackageV2Error::ArithmeticOverflow {
            context: "string record byte length",
        },
    )?;
    let data_bytes = strings.iter().try_fold(0u64, |total, string| {
        total
            .checked_add(count(string.len(), "string byte length")?)
            .ok_or(ExecutionPackageV2Error::ArithmeticOverflow {
                context: "string-pool byte length",
            })
    })?;
    STRING_SECTION_HEADER_SIZE
        .checked_add(record_bytes)
        .and_then(|length| length.checked_add(data_bytes))
        .ok_or(ExecutionPackageV2Error::ArithmeticOverflow {
            context: "string section byte length",
        })
}

fn payload_section_byte_len(records: &[PayloadRecord]) -> Result<u64, ExecutionPackageV2Error> {
    let count_value = count(records.len(), "payload count")?;
    let record_bytes = count_value.checked_mul(PAYLOAD_RECORD_SIZE).ok_or(
        ExecutionPackageV2Error::ArithmeticOverflow {
            context: "payload record byte length",
        },
    )?;
    let data_bytes = records.iter().try_fold(0u64, |total, record| {
        total
            .checked_add(count(record.bytes.len(), "payload byte length")?)
            .ok_or(ExecutionPackageV2Error::ArithmeticOverflow {
                context: "payload-pool byte length",
            })
    })?;
    PAYLOAD_SECTION_HEADER_SIZE
        .checked_add(record_bytes)
        .and_then(|length| length.checked_add(data_bytes))
        .ok_or(ExecutionPackageV2Error::ArithmeticOverflow {
            context: "payload section byte length",
        })
}

fn write_package_section<W: Write>(
    output: &mut W,
    package: &ExecutionPackage,
    kind: u64,
) -> io::Result<()> {
    match kind {
        section_kind::STRINGS => write_strings(output, &package.strings),
        section_kind::WORLD => write_world(output, package.world),
        section_kind::SCHEMAS => write_schemas(output, &package.schemas),
        section_kind::FIELDS => write_fields(output, &package.fields),
        section_kind::SYSTEMS => write_systems(output, &package.systems),
        section_kind::PARAMETERS => write_parameters(output, &package.parameters),
        section_kind::QUERIES => write_queries(output, &package.queries),
        section_kind::TERMS => write_terms(output, &package.terms),
        section_kind::SCHEDULES => write_schedules(output, &package.schedules),
        section_kind::SCHEDULE_ITEMS => write_schedule_items(output, &package.schedule_items),
        section_kind::STARTUP_OPERATIONS => {
            write_startup_operations(output, &package.startup_operations)
        }
        section_kind::PAYLOADS => write_payloads(output, &package.payloads),
        section_kind::FUNCTION_LINKS => write_function_links(output, &package.function_links),
        section_kind::SOURCE_SPANS => write_source_spans(output, &package.source_spans),
        _ => unreachable!("package section descriptors contain only canonical section kinds"),
    }
}

fn write_strings<W: Write>(output: &mut W, strings: &[String]) -> io::Result<()> {
    write_u64(output, validated_usize(strings.len()))?;
    let byte_len = strings
        .iter()
        .fold(0u64, |total, string| total + validated_usize(string.len()));
    write_u64(output, byte_len)?;
    let mut offset = 0u64;
    for string in strings {
        let len = validated_usize(string.len());
        write_u64(output, offset)?;
        write_u64(output, len)?;
        offset += len;
    }
    for string in strings {
        output.write_all(string.as_bytes())?;
    }
    Ok(())
}

fn decode_strings(section: &SectionView<'_>) -> Result<Vec<String>, ExecutionPackageV2Error> {
    require_raw(section, section_kind::STRINGS)?;
    if section.payload.len() < STRING_SECTION_HEADER_SIZE as usize {
        return invalid_shape(section.kind, "string section header is truncated");
    }
    let count_value = read_u64(section.payload, 0);
    let bytes_len = read_u64(section.payload, 8);
    let records_len = count_value.checked_mul(STRING_RECORD_SIZE).ok_or(
        ExecutionPackageV2Error::ArithmeticOverflow {
            context: "string record byte length",
        },
    )?;
    let bytes_start = STRING_SECTION_HEADER_SIZE.checked_add(records_len).ok_or(
        ExecutionPackageV2Error::ArithmeticOverflow {
            context: "string bytes offset",
        },
    )?;
    let expected_len =
        bytes_start
            .checked_add(bytes_len)
            .ok_or(ExecutionPackageV2Error::ArithmeticOverflow {
                context: "string section byte length",
            })?;
    if expected_len != count(section.payload.len(), "string section byte length")? {
        return invalid_shape(section.kind, "string section length is inconsistent");
    }
    let capacity =
        usize::try_from(count_value).map_err(|_| ExecutionPackageV2Error::AllocationFailed {
            context: "decoded strings",
        })?;
    let mut strings = Vec::new();
    strings
        .try_reserve_exact(capacity)
        .map_err(|_| ExecutionPackageV2Error::AllocationFailed {
            context: "decoded strings",
        })?;
    let mut expected_offset = 0u64;
    for index in 0..count_value {
        let record = index
            .checked_mul(STRING_RECORD_SIZE)
            .and_then(|offset| STRING_SECTION_HEADER_SIZE.checked_add(offset))
            .ok_or(ExecutionPackageV2Error::ArithmeticOverflow {
                context: "string record offset",
            })?;
        let record =
            usize::try_from(record).map_err(|_| ExecutionPackageV2Error::AllocationFailed {
                context: "string record offset",
            })?;
        let offset = read_u64(section.payload, record);
        let len = read_u64(section.payload, record + 8);
        if offset != expected_offset {
            return Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "strings",
                index,
            });
        }
        let end = offset
            .checked_add(len)
            .ok_or(ExecutionPackageV2Error::ArithmeticOverflow {
                context: "string byte range",
            })?;
        if end > bytes_len {
            return Err(ExecutionPackageV2Error::InvalidReference {
                owner: "strings",
                index,
                target: "string bytes",
                reference: end,
                target_count: bytes_len,
            });
        }
        let start = usize::try_from(bytes_start + offset).map_err(|_| {
            ExecutionPackageV2Error::AllocationFailed {
                context: "string byte offset",
            }
        })?;
        let end_index = usize::try_from(bytes_start + end).map_err(|_| {
            ExecutionPackageV2Error::AllocationFailed {
                context: "string byte offset",
            }
        })?;
        let decoded = std::str::from_utf8(&section.payload[start..end_index]).map_err(|_| {
            ExecutionPackageV2Error::InvalidUtf8 {
                string_index: index,
            }
        })?;
        let string = copy_string(decoded, "decoded string bytes")?;
        strings.push(string);
        expected_offset = end;
    }
    Ok(strings)
}

fn write_world<W: Write>(output: &mut W, record: WorldRecord) -> io::Result<()> {
    write_u64(output, record.name.index())?;
    write_optional_ref(output, record.source_span.map(SourceSpanRef::index))?;
    output.write_all(record.startup_abi_hash.as_bytes())?;
    output.write_all(record.startup_body_hash.as_bytes())?;
    write_zeros(output, 16)
}

fn decode_world(bytes: &[u8]) -> WorldRecord {
    WorldRecord {
        name: StringRef::new(read_u64(bytes, 0)),
        source_span: optional_ref(read_u64(bytes, 8), SourceSpanRef::new),
        startup_abi_hash: AbiHash::from_bytes(read_id(bytes, 16)),
        startup_body_hash: BodyHash::from_bytes(read_id(bytes, 32)),
    }
}

fn write_schemas<W: Write>(output: &mut W, records: &[SchemaRecord]) -> io::Result<()> {
    for record in records {
        output.write_all(record.id.as_bytes())?;
        write_u64(output, record.kind as u64)?;
        write_u64(output, record.name.index())?;
        write_u64(output, record.byte_size)?;
        write_u64(output, record.alignment)?;
        write_optional_ref(output, record.source_span.map(SourceSpanRef::index))?;
        write_u64(output, record.flags.bits())?;
        write_zeros(output, 32)?;
    }
    Ok(())
}

fn decode_schemas(section: &SectionView<'_>) -> Result<Vec<SchemaRecord>, ExecutionPackageV2Error> {
    decode_records(section, |bytes, index| {
        let kind = decode_schema_kind(
            read_u64(bytes, wire::schema::KIND as usize),
            section.kind,
            index,
        )?;
        let flags = decode_schema_flags(
            read_u64(bytes, wire::schema::FLAGS as usize),
            kind,
            section.kind,
            index,
        )?;
        Ok(SchemaRecord {
            id: SchemaId::from_bytes(read_id(bytes, 0)),
            kind,
            flags,
            name: StringRef::new(read_u64(bytes, 24)),
            byte_size: read_u64(bytes, 32),
            alignment: read_u64(bytes, 40),
            source_span: optional_ref(read_u64(bytes, 48), SourceSpanRef::new),
        })
    })
}

fn write_fields<W: Write>(output: &mut W, records: &[FieldRecord]) -> io::Result<()> {
    for record in records {
        write_u64(output, record.schema.index())?;
        write_u64(output, record.name.index())?;
        write_u64(output, record.primitive as u64)?;
        write_u64(output, record.byte_offset)?;
        write_optional_ref(output, record.source_span.map(SourceSpanRef::index))?;
        write_zeros(output, 24)?;
    }
    Ok(())
}

fn decode_fields(section: &SectionView<'_>) -> Result<Vec<FieldRecord>, ExecutionPackageV2Error> {
    decode_records(section, |bytes, index| {
        Ok(FieldRecord {
            schema: SchemaRef::new(read_u64(bytes, 0)),
            name: StringRef::new(read_u64(bytes, 8)),
            primitive: decode_primitive(read_u64(bytes, 16), section.kind, index)?,
            byte_offset: read_u64(bytes, 24),
            source_span: optional_ref(read_u64(bytes, 32), SourceSpanRef::new),
        })
    })
}

fn write_systems<W: Write>(output: &mut W, records: &[SystemRecord]) -> io::Result<()> {
    for record in records {
        output.write_all(record.id.as_bytes())?;
        write_u64(output, record.name.index())?;
        output.write_all(record.abi_hash.as_bytes())?;
        output.write_all(record.body_hash.as_bytes())?;
        write_optional_ref(output, record.source_span.map(SourceSpanRef::index))?;
        write_zeros(output, 64)?;
    }
    Ok(())
}

fn decode_systems(section: &SectionView<'_>) -> Result<Vec<SystemRecord>, ExecutionPackageV2Error> {
    decode_records(section, |bytes, _| {
        Ok(SystemRecord {
            id: DeclId::from_bytes(read_id(bytes, 0)),
            name: StringRef::new(read_u64(bytes, 16)),
            abi_hash: AbiHash::from_bytes(read_id(bytes, 24)),
            body_hash: BodyHash::from_bytes(read_id(bytes, 40)),
            source_span: optional_ref(read_u64(bytes, 56), SourceSpanRef::new),
        })
    })
}

fn write_parameters<W: Write>(output: &mut W, records: &[ParameterRecord]) -> io::Result<()> {
    for record in records {
        write_u64(output, record.system.index())?;
        write_u64(output, record.name.index())?;
        match record.kind {
            ParameterKind::ReadResource { resource } => {
                write_u64(output, wire::parameter::READ_RESOURCE)?;
                write_u64(output, resource.index())?;
            }
            ParameterKind::MutResource { resource } => {
                write_u64(output, wire::parameter::MUT_RESOURCE)?;
                write_u64(output, resource.index())?;
            }
            ParameterKind::Query { query } => {
                write_u64(output, wire::parameter::QUERY)?;
                write_u64(output, query.index())?;
            }
        }
        write_optional_ref(output, record.source_span.map(SourceSpanRef::index))?;
        write_zeros(output, 24)?;
    }
    Ok(())
}

fn decode_parameters(
    section: &SectionView<'_>,
) -> Result<Vec<ParameterRecord>, ExecutionPackageV2Error> {
    decode_records(section, |bytes, index| {
        let target = read_u64(bytes, 24);
        let kind = match read_u64(bytes, 16) {
            wire::parameter::READ_RESOURCE => ParameterKind::ReadResource {
                resource: SchemaRef::new(target),
            },
            wire::parameter::MUT_RESOURCE => ParameterKind::MutResource {
                resource: SchemaRef::new(target),
            },
            wire::parameter::QUERY => ParameterKind::Query {
                query: QueryRef::new(target),
            },
            _ => return invalid_record(section.kind, index, "unknown parameter kind discriminant"),
        };
        Ok(ParameterRecord {
            system: SystemRef::new(read_u64(bytes, 0)),
            name: StringRef::new(read_u64(bytes, 8)),
            kind,
            source_span: optional_ref(read_u64(bytes, 32), SourceSpanRef::new),
        })
    })
}

fn write_queries<W: Write>(output: &mut W, records: &[QueryRecord]) -> io::Result<()> {
    for record in records {
        output.write_all(record.id.as_bytes())?;
        write_u64(output, record.system.index())?;
        write_u64(output, record.parameter.index())?;
        write_optional_ref(output, record.source_span.map(SourceSpanRef::index))?;
        write_zeros(output, 40)?;
    }
    Ok(())
}

fn decode_queries(section: &SectionView<'_>) -> Result<Vec<QueryRecord>, ExecutionPackageV2Error> {
    decode_records(section, |bytes, _| {
        Ok(QueryRecord {
            id: DeclId::from_bytes(read_id(bytes, 0)),
            system: SystemRef::new(read_u64(bytes, 16)),
            parameter: ParameterRef::new(read_u64(bytes, 24)),
            source_span: optional_ref(read_u64(bytes, 32), SourceSpanRef::new),
        })
    })
}

fn write_terms<W: Write>(output: &mut W, records: &[TermRecord]) -> io::Result<()> {
    for record in records {
        write_u64(output, record.query.index())?;
        write_u64(
            output,
            match record.access {
                QueryAccess::Read => wire::term::READ,
                QueryAccess::Mut => wire::term::MUT,
                QueryAccess::Exclude => wire::term::EXCLUDE,
            },
        )?;
        write_u64(output, record.schema.index())?;
        write_optional_ref(output, record.source_span.map(SourceSpanRef::index))?;
        write_zeros(output, 32)?;
    }
    Ok(())
}

fn decode_terms(section: &SectionView<'_>) -> Result<Vec<TermRecord>, ExecutionPackageV2Error> {
    decode_records(section, |bytes, index| {
        let access = match read_u64(bytes, 8) {
            wire::term::READ => QueryAccess::Read,
            wire::term::MUT => QueryAccess::Mut,
            wire::term::EXCLUDE => QueryAccess::Exclude,
            _ => return invalid_record(section.kind, index, "unknown query-access discriminant"),
        };
        Ok(TermRecord {
            query: QueryRef::new(read_u64(bytes, 0)),
            access,
            schema: SchemaRef::new(read_u64(bytes, 16)),
            source_span: optional_ref(read_u64(bytes, 24), SourceSpanRef::new),
        })
    })
}

fn write_schedules<W: Write>(output: &mut W, records: &[ScheduleRecord]) -> io::Result<()> {
    for record in records {
        output.write_all(record.id.as_bytes())?;
        write_u64(output, record.name.index())?;
        write_optional_ref(output, record.source_span.map(SourceSpanRef::index))?;
        write_zeros(output, 32)?;
    }
    Ok(())
}

fn decode_schedules(
    section: &SectionView<'_>,
) -> Result<Vec<ScheduleRecord>, ExecutionPackageV2Error> {
    decode_records(section, |bytes, _| {
        Ok(ScheduleRecord {
            id: DeclId::from_bytes(read_id(bytes, 0)),
            name: StringRef::new(read_u64(bytes, 16)),
            source_span: optional_ref(read_u64(bytes, 24), SourceSpanRef::new),
        })
    })
}

fn write_schedule_items<W: Write>(
    output: &mut W,
    records: &[ScheduleItemRecord],
) -> io::Result<()> {
    for record in records {
        write_u64(output, record.schedule.index())?;
        match record.kind {
            ScheduleItemKind::RunSystem { system } => {
                write_u64(output, wire::schedule_item::RUN_SYSTEM)?;
                write_u64(output, system.index())?;
            }
        }
        write_optional_ref(output, record.source_span.map(SourceSpanRef::index))?;
        write_zeros(output, 16)?;
    }
    Ok(())
}

fn decode_schedule_items(
    section: &SectionView<'_>,
) -> Result<Vec<ScheduleItemRecord>, ExecutionPackageV2Error> {
    decode_records(section, |bytes, index| {
        let target = read_u64(bytes, 16);
        let kind = match read_u64(bytes, 8) {
            wire::schedule_item::RUN_SYSTEM => ScheduleItemKind::RunSystem {
                system: SystemRef::new(target),
            },
            _ => return invalid_record(section.kind, index, "unknown schedule-item discriminant"),
        };
        Ok(ScheduleItemRecord {
            schedule: ScheduleRef::new(read_u64(bytes, 0)),
            kind,
            source_span: optional_ref(read_u64(bytes, 24), SourceSpanRef::new),
        })
    })
}

fn write_startup_operations<W: Write>(
    output: &mut W,
    records: &[StartupOperationRecord],
) -> io::Result<()> {
    for record in records {
        match record.kind {
            StartupOperationKind::ResourcePayload { resource, payload } => {
                write_u64(output, wire::startup_operation::RESOURCE_PAYLOAD)?;
                write_u64(output, resource.index())?;
                write_u64(output, payload.index())?;
                write_u64(output, 0)?;
            }
            StartupOperationKind::Spawn {
                first_payload,
                payload_count,
            } => {
                write_u64(output, wire::startup_operation::SPAWN)?;
                write_u64(output, first_payload.index())?;
                write_u64(output, payload_count)?;
                write_u64(output, 0)?;
            }
            StartupOperationKind::RunSchedule { schedule } => {
                write_u64(output, wire::startup_operation::RUN_SCHEDULE)?;
                write_u64(output, schedule.index())?;
                write_u64(output, 0)?;
                write_u64(output, 0)?;
            }
        }
        write_optional_ref(output, record.source_span.map(SourceSpanRef::index))?;
        write_zeros(output, 24)?;
    }
    Ok(())
}

fn decode_startup_operations(
    section: &SectionView<'_>,
) -> Result<Vec<StartupOperationRecord>, ExecutionPackageV2Error> {
    decode_records(section, |bytes, index| {
        let first = read_u64(bytes, 8);
        let second = read_u64(bytes, 16);
        let kind = match read_u64(bytes, 0) {
            wire::startup_operation::RESOURCE_PAYLOAD => StartupOperationKind::ResourcePayload {
                resource: SchemaRef::new(first),
                payload: PayloadRef::new(second),
            },
            wire::startup_operation::SPAWN => StartupOperationKind::Spawn {
                first_payload: PayloadRef::new(first),
                payload_count: second,
            },
            wire::startup_operation::RUN_SCHEDULE => StartupOperationKind::RunSchedule {
                schedule: ScheduleRef::new(first),
            },
            _ => {
                return invalid_record(
                    section.kind,
                    index,
                    "unknown startup-operation discriminant",
                )
            }
        };
        Ok(StartupOperationRecord {
            kind,
            source_span: optional_ref(read_u64(bytes, 32), SourceSpanRef::new),
        })
    })
}

fn write_payloads<W: Write>(output: &mut W, records: &[PayloadRecord]) -> io::Result<()> {
    write_u64(output, validated_usize(records.len()))?;
    let data_len = records.iter().fold(0u64, |total, record| {
        total + validated_usize(record.bytes.len())
    });
    write_u64(output, data_len)?;
    let mut offset = 0u64;
    for record in records {
        let len = validated_usize(record.bytes.len());
        write_u64(output, record.schema.index())?;
        write_u64(output, offset)?;
        write_u64(output, len)?;
        write_u64(output, 0)?;
        offset += len;
    }
    for record in records {
        output.write_all(&record.bytes)?;
    }
    Ok(())
}

fn decode_payloads(
    section: &SectionView<'_>,
) -> Result<Vec<PayloadRecord>, ExecutionPackageV2Error> {
    require_raw(section, section_kind::PAYLOADS)?;
    if section.payload.len() < PAYLOAD_SECTION_HEADER_SIZE as usize {
        return invalid_shape(section.kind, "payload section header is truncated");
    }
    let count_value = read_u64(section.payload, 0);
    let data_len = read_u64(section.payload, 8);
    let records_len = count_value.checked_mul(PAYLOAD_RECORD_SIZE).ok_or(
        ExecutionPackageV2Error::ArithmeticOverflow {
            context: "payload record byte length",
        },
    )?;
    let data_start = PAYLOAD_SECTION_HEADER_SIZE.checked_add(records_len).ok_or(
        ExecutionPackageV2Error::ArithmeticOverflow {
            context: "payload bytes offset",
        },
    )?;
    let expected_len =
        data_start
            .checked_add(data_len)
            .ok_or(ExecutionPackageV2Error::ArithmeticOverflow {
                context: "payload section byte length",
            })?;
    if expected_len != count(section.payload.len(), "payload section byte length")? {
        return invalid_shape(section.kind, "payload section length is inconsistent");
    }
    let mut records = Vec::new();
    records
        .try_reserve_exact(usize::try_from(count_value).map_err(|_| {
            ExecutionPackageV2Error::AllocationFailed {
                context: "decoded payload records",
            }
        })?)
        .map_err(|_| ExecutionPackageV2Error::AllocationFailed {
            context: "decoded payload records",
        })?;
    let mut expected_offset = 0u64;
    for index in 0..count_value {
        let record_offset = index
            .checked_mul(PAYLOAD_RECORD_SIZE)
            .and_then(|offset| PAYLOAD_SECTION_HEADER_SIZE.checked_add(offset))
            .ok_or(ExecutionPackageV2Error::ArithmeticOverflow {
                context: "payload record offset",
            })?;
        let record_offset = usize::try_from(record_offset).map_err(|_| {
            ExecutionPackageV2Error::AllocationFailed {
                context: "payload record offset",
            }
        })?;
        let schema = SchemaRef::new(read_u64(section.payload, record_offset));
        let offset = read_u64(section.payload, record_offset + 8);
        let len = read_u64(section.payload, record_offset + 16);
        if read_u64(section.payload, record_offset + 24) != 0 {
            return invalid_record(
                section.kind,
                index,
                "payload record reserved field is nonzero",
            );
        }
        if offset != expected_offset {
            return Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "payloads",
                index,
            });
        }
        let end = offset
            .checked_add(len)
            .ok_or(ExecutionPackageV2Error::ArithmeticOverflow {
                context: "payload byte range",
            })?;
        if end > data_len {
            return Err(ExecutionPackageV2Error::InvalidPayload {
                index,
                reason: "payload byte range is outside the payload pool",
            });
        }
        let start = usize::try_from(data_start + offset).map_err(|_| {
            ExecutionPackageV2Error::AllocationFailed {
                context: "payload byte offset",
            }
        })?;
        let end_index = usize::try_from(data_start + end).map_err(|_| {
            ExecutionPackageV2Error::AllocationFailed {
                context: "payload byte offset",
            }
        })?;
        records.push(PayloadRecord {
            schema,
            bytes: copy_bytes(&section.payload[start..end_index], "decoded payload bytes")?,
        });
        expected_offset = end;
    }
    Ok(records)
}

fn write_function_links<W: Write>(
    output: &mut W,
    records: &[FunctionLinkRecord],
) -> io::Result<()> {
    for record in records {
        let (kind, system) = match record.target {
            FunctionTarget::Startup => (wire::function_link::STARTUP, NONE_REFERENCE),
            FunctionTarget::System { system } => {
                (wire::function_link::SYSTEM_TARGET, system.index())
            }
        };
        write_u64(output, kind)?;
        write_u64(output, system)?;
        write_u64(output, record.symbol_name.index())?;
        output.write_all(record.abi_hash.as_bytes())?;
        output.write_all(record.body_hash.as_bytes())?;
        write_u64(output, record.code_offset)?;
        write_u64(output, record.code_byte_len)?;
        write_optional_ref(output, record.source_span.map(SourceSpanRef::index))?;
        write_optional_ref(output, record.first_body_span.map(SourceSpanRef::index))?;
        write_u64(output, record.body_span_count)?;
    }
    Ok(())
}

fn decode_function_links(
    section: &SectionView<'_>,
) -> Result<Vec<FunctionLinkRecord>, ExecutionPackageV2Error> {
    decode_records(section, |bytes, index| {
        let target = match (read_u64(bytes, 0), read_u64(bytes, 8)) {
            (wire::function_link::STARTUP, NONE_REFERENCE) => FunctionTarget::Startup,
            (wire::function_link::SYSTEM_TARGET, system) if system != NONE_REFERENCE => {
                FunctionTarget::System {
                    system: SystemRef::new(system),
                }
            }
            _ => {
                return invalid_record(
                    section.kind,
                    index,
                    "invalid native-function target encoding",
                );
            }
        };
        Ok(FunctionLinkRecord {
            target,
            symbol_name: StringRef::new(read_u64(bytes, 16)),
            abi_hash: AbiHash::from_bytes(read_id(bytes, 24)),
            body_hash: BodyHash::from_bytes(read_id(bytes, 40)),
            code_offset: read_u64(bytes, 56),
            code_byte_len: read_u64(bytes, 64),
            source_span: optional_ref(read_u64(bytes, 72), SourceSpanRef::new),
            first_body_span: optional_ref(read_u64(bytes, 80), SourceSpanRef::new),
            body_span_count: read_u64(bytes, 88),
        })
    })
}

fn write_source_spans<W: Write>(output: &mut W, records: &[SourceSpanRecord]) -> io::Result<()> {
    for record in records {
        write_u64(output, record.file_name.index())?;
        write_u64(output, record.start_byte)?;
        write_u64(output, record.end_byte)?;
        write_u64(output, record.start_line)?;
        write_u64(output, record.start_column)?;
        write_u64(output, record.end_line)?;
        write_u64(output, record.end_column)?;
        write_zeros(output, 8)?;
    }
    Ok(())
}

fn decode_source_spans(
    section: &SectionView<'_>,
) -> Result<Vec<SourceSpanRecord>, ExecutionPackageV2Error> {
    decode_records(section, |bytes, _| {
        Ok(SourceSpanRecord {
            file_name: StringRef::new(read_u64(bytes, 0)),
            start_byte: read_u64(bytes, 8),
            end_byte: read_u64(bytes, 16),
            start_line: read_u64(bytes, 24),
            start_column: read_u64(bytes, 32),
            end_line: read_u64(bytes, 40),
            end_column: read_u64(bytes, 48),
        })
    })
}

fn decode_records<T>(
    section: &SectionView<'_>,
    mut decode: impl FnMut(&[u8], u64) -> Result<T, ExecutionPackageV2Error>,
) -> Result<Vec<T>, ExecutionPackageV2Error> {
    let capacity = usize::try_from(section.record_count).map_err(|_| {
        ExecutionPackageV2Error::AllocationFailed {
            context: "decoded record table",
        }
    })?;
    let stride = usize::try_from(section.record_stride).map_err(|_| {
        ExecutionPackageV2Error::AllocationFailed {
            context: "decoded record stride",
        }
    })?;
    let mut records = Vec::new();
    records
        .try_reserve_exact(capacity)
        .map_err(|_| ExecutionPackageV2Error::AllocationFailed {
            context: "decoded record table",
        })?;
    for index in 0..section.record_count {
        let index_usize =
            usize::try_from(index).map_err(|_| ExecutionPackageV2Error::AllocationFailed {
                context: "decoded record index",
            })?;
        let start =
            index_usize
                .checked_mul(stride)
                .ok_or(ExecutionPackageV2Error::ArithmeticOverflow {
                    context: "decoded record offset",
                })?;
        let bytes = &section.payload[start..start + stride];
        let record = decode(bytes, index)?;
        if reserved_bytes_nonzero(section.kind, bytes) {
            return invalid_record(section.kind, index, "record reserved bytes are nonzero");
        }
        records.push(record);
    }
    Ok(records)
}

fn reserved_bytes_nonzero(kind: u64, bytes: &[u8]) -> bool {
    let start = match kind {
        section_kind::WORLD => 48,
        section_kind::SCHEMAS => 64,
        section_kind::FIELDS => 40,
        section_kind::SYSTEMS => 64,
        section_kind::PARAMETERS => 40,
        section_kind::QUERIES => 40,
        section_kind::TERMS => 32,
        section_kind::SCHEDULES => 32,
        section_kind::SCHEDULE_ITEMS => 32,
        section_kind::STARTUP_OPERATIONS => 40,
        section_kind::FUNCTION_LINKS => 96,
        section_kind::SOURCE_SPANS => 56,
        _ => return false,
    };
    bytes[start..].iter().any(|byte| *byte != 0)
}

fn require_raw(section: &SectionView<'_>, kind: u64) -> Result<(), ExecutionPackageV2Error> {
    if section.record_count != 0 || section.record_stride != 0 {
        return invalid_shape(kind, "variable-byte section declares fixed records");
    }
    Ok(())
}

fn decode_schema_kind(
    value: u64,
    section: u64,
    index: u64,
) -> Result<SchemaKind, ExecutionPackageV2Error> {
    match value {
        1 => Ok(SchemaKind::Component),
        2 => Ok(SchemaKind::Resource),
        3 => Ok(SchemaKind::Tag),
        _ => invalid_record(section, index, "unknown schema-kind discriminant"),
    }
}

fn decode_schema_flags(
    value: u64,
    kind: SchemaKind,
    section: u64,
    index: u64,
) -> Result<SchemaFlags, ExecutionPackageV2Error> {
    if value & !wire::schema::flags::KNOWN_MASK != 0 {
        return invalid_record(section, index, "unknown schema flag bits");
    }
    let flags = SchemaFlags::from_bits(value);
    if flags != SchemaFlags::for_kind(kind) {
        return invalid_record(section, index, "schema flags do not match schema kind");
    }
    Ok(flags)
}

fn decode_primitive(
    value: u64,
    section: u64,
    index: u64,
) -> Result<PrimitiveType, ExecutionPackageV2Error> {
    match value {
        1 => Ok(PrimitiveType::I32),
        2 => Ok(PrimitiveType::F32),
        3 => Ok(PrimitiveType::Bool),
        _ => invalid_record(section, index, "unknown primitive-type discriminant"),
    }
}

fn primitive_size(primitive: PrimitiveType) -> u64 {
    match primitive {
        PrimitiveType::I32 | PrimitiveType::F32 => 4,
        PrimitiveType::Bool => 1,
    }
}

fn primitive_alignment(primitive: PrimitiveType) -> u64 {
    primitive_size(primitive)
}

fn align_value(
    value: u64,
    alignment: u64,
    context: &'static str,
) -> Result<u64, ExecutionPackageV2Error> {
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or(ExecutionPackageV2Error::ArithmeticOverflow { context })
}

fn mark_string(
    reference_value: StringRef,
    target_count: u64,
    used: &mut [bool],
    owner: &'static str,
    index: u64,
) -> Result<(), ExecutionPackageV2Error> {
    reference(
        owner,
        index,
        "strings",
        reference_value.index(),
        target_count,
    )?;
    used[reference_value.index() as usize] = true;
    Ok(())
}

fn mark_optional_span(
    reference_value: Option<SourceSpanRef>,
    target_count: u64,
    used: &mut [bool],
    owner: &'static str,
    index: u64,
) -> Result<(), ExecutionPackageV2Error> {
    if let Some(reference_value) = reference_value {
        reference(
            owner,
            index,
            "source spans",
            reference_value.index(),
            target_count,
        )?;
        used[reference_value.index() as usize] = true;
    }
    Ok(())
}

fn reference(
    owner: &'static str,
    index: u64,
    target: &'static str,
    reference_value: u64,
    target_count: u64,
) -> Result<(), ExecutionPackageV2Error> {
    if reference_value >= target_count {
        return Err(ExecutionPackageV2Error::InvalidReference {
            owner,
            index,
            target,
            reference: reference_value,
            target_count,
        });
    }
    Ok(())
}

fn count(value: usize, context: &'static str) -> Result<u64, ExecutionPackageV2Error> {
    u64::try_from(value).map_err(|_| ExecutionPackageV2Error::ArithmeticOverflow { context })
}

fn false_table(len: usize, context: &'static str) -> Result<Vec<bool>, ExecutionPackageV2Error> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| ExecutionPackageV2Error::AllocationFailed { context })?;
    values.resize(len, false);
    Ok(values)
}

fn copy_bytes(bytes: &[u8], context: &'static str) -> Result<Vec<u8>, ExecutionPackageV2Error> {
    let mut owned = Vec::new();
    owned
        .try_reserve_exact(bytes.len())
        .map_err(|_| ExecutionPackageV2Error::AllocationFailed { context })?;
    owned.extend_from_slice(bytes);
    Ok(owned)
}

fn copy_string(value: &str, context: &'static str) -> Result<String, ExecutionPackageV2Error> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(value.len())
        .map_err(|_| ExecutionPackageV2Error::AllocationFailed { context })?;
    owned.push_str(value);
    Ok(owned)
}

fn validated_usize(value: usize) -> u64 {
    u64::try_from(value).expect("execution package was validated before streaming")
}

fn invalid_shape<T>(kind: u64, reason: &'static str) -> Result<T, ExecutionPackageV2Error> {
    Err(ExecutionPackageV2Error::InvalidSectionShape { kind, reason })
}

fn invalid_record<T>(
    section: u64,
    index: u64,
    reason: &'static str,
) -> Result<T, ExecutionPackageV2Error> {
    Err(ExecutionPackageV2Error::InvalidRecord {
        section,
        index,
        reason,
    })
}

fn optional_ref<T>(value: u64, constructor: impl FnOnce(u64) -> T) -> Option<T> {
    (value != NONE_REFERENCE).then(|| constructor(value))
}

fn write_optional_ref<W: Write>(output: &mut W, value: Option<u64>) -> io::Result<()> {
    write_u64(output, value.unwrap_or(NONE_REFERENCE))
}

fn write_u64<W: Write>(output: &mut W, value: u64) -> io::Result<()> {
    output.write_all(&value.to_le_bytes())
}

fn write_zeros<W: Write>(output: &mut W, count: usize) -> io::Result<()> {
    const ZEROES: [u8; 128] = [0; 128];
    debug_assert!(count <= ZEROES.len());
    output.write_all(&ZEROES[..count])
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("validated fixed-width execution-package field"),
    )
}

fn read_id(bytes: &[u8], offset: usize) -> [u8; 16] {
    bytes[offset..offset + 16]
        .try_into()
        .expect("validated 128-bit execution-package identifier")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids_v2::{AbiHasher, BodyHasher, SchemaField};
    use std::io::{Read, SeekFrom};

    struct TrackingCursor {
        inner: io::Cursor<Vec<u8>>,
        largest_read: usize,
    }

    impl TrackingCursor {
        fn new(bytes: Vec<u8>) -> Self {
            Self {
                inner: io::Cursor::new(bytes),
                largest_read: 0,
            }
        }

        fn set_position(&mut self, position: u64) {
            self.inner.set_position(position);
        }

        fn position(&self) -> u64 {
            self.inner.position()
        }
    }

    impl Read for TrackingCursor {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            self.largest_read = self.largest_read.max(output.len());
            self.inner.read(output)
        }
    }

    impl Seek for TrackingCursor {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.inner.seek(position)
        }
    }

    struct SparsePackageReader {
        base: u64,
        bytes: Vec<u8>,
        position: u64,
        fail_at: Option<u64>,
    }

    impl SparsePackageReader {
        fn new(base: u64, bytes: Vec<u8>) -> Self {
            Self {
                base,
                bytes,
                position: base,
                fail_at: None,
            }
        }

        fn with_failure(mut self, relative_offset: u64) -> Self {
            self.fail_at = Some(self.base.checked_add(relative_offset).unwrap());
            self
        }

        fn logical_end(&self) -> io::Result<u64> {
            self.base
                .checked_add(u64::try_from(self.bytes.len()).unwrap())
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "logical end overflow"))
        }
    }

    impl Read for SparsePackageReader {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if self
                .fail_at
                .is_some_and(|failure_position| self.position >= failure_position)
            {
                return Err(io::Error::other("injected package read failure"));
            }
            let end = self.logical_end()?;
            if self.position < self.base || self.position >= end {
                return Ok(0);
            }
            let relative = usize::try_from(self.position - self.base)
                .map_err(|_| io::Error::other("relative offset does not fit usize"))?;
            let available = self.bytes.len() - relative;
            let before_failure = self.fail_at.map_or(u64::MAX, |failure_position| {
                failure_position - self.position
            });
            let count = available
                .min(output.len())
                .min(usize::try_from(before_failure).unwrap_or(usize::MAX));
            output[..count].copy_from_slice(&self.bytes[relative..relative + count]);
            self.position = self
                .position
                .checked_add(u64::try_from(count).unwrap())
                .ok_or_else(|| io::Error::other("read position overflow"))?;
            Ok(count)
        }
    }

    impl Seek for SparsePackageReader {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            let end = self.logical_end()?;
            let next = match position {
                SeekFrom::Start(position) => i128::from(position),
                SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
                SeekFrom::End(delta) => i128::from(end) + i128::from(delta),
            };
            self.position = u64::try_from(next)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid seek"))?;
            Ok(self.position)
        }
    }

    fn sample_package() -> ExecutionPackage {
        let strings = [
            "Demo",
            "Main",
            "Move",
            "Position",
            "Time",
            "_arche_Demo_Move",
            "_arche_Demo_startup",
            "delta",
            "demo.arc",
            "movers",
            "time",
            "x",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        let position_id = SchemaId::derive(
            SchemaKind::Component,
            "Demo",
            "Position",
            &[SchemaField {
                name: "x",
                primitive: PrimitiveType::F32,
            }],
        );
        let time_id = SchemaId::derive(
            SchemaKind::Resource,
            "Demo",
            "Time",
            &[SchemaField {
                name: "delta",
                primitive: PrimitiveType::F32,
            }],
        );
        let (schemas, fields, position, time) = if position_id < time_id {
            (
                vec![
                    SchemaRecord {
                        id: position_id,
                        kind: SchemaKind::Component,
                        flags: SchemaFlags::NONE,
                        name: StringRef::new(3),
                        byte_size: 4,
                        alignment: 4,
                        source_span: Some(SourceSpanRef::new(1)),
                    },
                    SchemaRecord {
                        id: time_id,
                        kind: SchemaKind::Resource,
                        flags: SchemaFlags::NONE,
                        name: StringRef::new(4),
                        byte_size: 4,
                        alignment: 4,
                        source_span: Some(SourceSpanRef::new(2)),
                    },
                ],
                vec![
                    FieldRecord {
                        schema: SchemaRef::new(0),
                        name: StringRef::new(11),
                        primitive: PrimitiveType::F32,
                        byte_offset: 0,
                        source_span: Some(SourceSpanRef::new(1)),
                    },
                    FieldRecord {
                        schema: SchemaRef::new(1),
                        name: StringRef::new(7),
                        primitive: PrimitiveType::F32,
                        byte_offset: 0,
                        source_span: Some(SourceSpanRef::new(2)),
                    },
                ],
                SchemaRef::new(0),
                SchemaRef::new(1),
            )
        } else {
            (
                vec![
                    SchemaRecord {
                        id: time_id,
                        kind: SchemaKind::Resource,
                        flags: SchemaFlags::NONE,
                        name: StringRef::new(4),
                        byte_size: 4,
                        alignment: 4,
                        source_span: Some(SourceSpanRef::new(2)),
                    },
                    SchemaRecord {
                        id: position_id,
                        kind: SchemaKind::Component,
                        flags: SchemaFlags::NONE,
                        name: StringRef::new(3),
                        byte_size: 4,
                        alignment: 4,
                        source_span: Some(SourceSpanRef::new(1)),
                    },
                ],
                vec![
                    FieldRecord {
                        schema: SchemaRef::new(0),
                        name: StringRef::new(7),
                        primitive: PrimitiveType::F32,
                        byte_offset: 0,
                        source_span: Some(SourceSpanRef::new(2)),
                    },
                    FieldRecord {
                        schema: SchemaRef::new(1),
                        name: StringRef::new(11),
                        primitive: PrimitiveType::F32,
                        byte_offset: 0,
                        source_span: Some(SourceSpanRef::new(1)),
                    },
                ],
                SchemaRef::new(1),
                SchemaRef::new(0),
            )
        };

        let system_id = DeclId::system("Demo", "Move");
        let mut abi = AbiHasher::new();
        abi.append_id(&system_id)
            .append_id(&schemas[position.index() as usize].id)
            .append_id(&schemas[time.index() as usize].id);
        let abi_hash = abi.finalize();
        let mut body = BodyHasher::new();
        body.append_id(&system_id)
            .append_u64(1)
            .append_string("position");
        let body_hash = body.finalize();
        let mut startup_abi = AbiHasher::new();
        startup_abi.append_string("startup");
        let startup_abi_hash = startup_abi.finalize();
        let mut startup_body = BodyHasher::new();
        startup_body.append_string("startup").append_u64(47);
        let startup_body_hash = startup_body.finalize();

        ExecutionPackage {
            strings,
            world: WorldRecord {
                name: StringRef::new(0),
                source_span: Some(SourceSpanRef::new(0)),
                startup_abi_hash,
                startup_body_hash,
            },
            schemas,
            fields,
            systems: vec![SystemRecord {
                id: system_id,
                name: StringRef::new(2),
                abi_hash,
                body_hash,
                source_span: Some(SourceSpanRef::new(3)),
            }],
            parameters: vec![
                ParameterRecord {
                    system: SystemRef::new(0),
                    name: StringRef::new(10),
                    kind: ParameterKind::ReadResource { resource: time },
                    source_span: Some(SourceSpanRef::new(3)),
                },
                ParameterRecord {
                    system: SystemRef::new(0),
                    name: StringRef::new(9),
                    kind: ParameterKind::Query {
                        query: QueryRef::new(0),
                    },
                    source_span: Some(SourceSpanRef::new(3)),
                },
            ],
            queries: vec![QueryRecord {
                id: DeclId::query(system_id, "movers"),
                system: SystemRef::new(0),
                parameter: ParameterRef::new(1),
                source_span: Some(SourceSpanRef::new(3)),
            }],
            terms: vec![TermRecord {
                query: QueryRef::new(0),
                access: QueryAccess::Mut,
                schema: position,
                source_span: Some(SourceSpanRef::new(3)),
            }],
            schedules: vec![ScheduleRecord {
                id: DeclId::schedule("Demo", "Main"),
                name: StringRef::new(1),
                source_span: Some(SourceSpanRef::new(4)),
            }],
            schedule_items: vec![ScheduleItemRecord {
                schedule: ScheduleRef::new(0),
                kind: ScheduleItemKind::RunSystem {
                    system: SystemRef::new(0),
                },
                source_span: Some(SourceSpanRef::new(4)),
            }],
            startup_operations: vec![
                StartupOperationRecord {
                    kind: StartupOperationKind::ResourcePayload {
                        resource: time,
                        payload: PayloadRef::new(0),
                    },
                    source_span: Some(SourceSpanRef::new(5)),
                },
                StartupOperationRecord {
                    kind: StartupOperationKind::Spawn {
                        first_payload: PayloadRef::new(1),
                        payload_count: 1,
                    },
                    source_span: Some(SourceSpanRef::new(5)),
                },
                StartupOperationRecord {
                    kind: StartupOperationKind::RunSchedule {
                        schedule: ScheduleRef::new(0),
                    },
                    source_span: Some(SourceSpanRef::new(5)),
                },
            ],
            payloads: vec![
                PayloadRecord {
                    schema: time,
                    bytes: 0.5f32.to_le_bytes().to_vec(),
                },
                PayloadRecord {
                    schema: position,
                    bytes: 1.0f32.to_le_bytes().to_vec(),
                },
            ],
            function_links: vec![
                FunctionLinkRecord {
                    target: FunctionTarget::Startup,
                    symbol_name: StringRef::new(6),
                    abi_hash: startup_abi_hash,
                    body_hash: startup_body_hash,
                    code_offset: 0,
                    code_byte_len: 16,
                    source_span: Some(SourceSpanRef::new(5)),
                    first_body_span: None,
                    body_span_count: 0,
                },
                FunctionLinkRecord {
                    target: FunctionTarget::System {
                        system: SystemRef::new(0),
                    },
                    symbol_name: StringRef::new(5),
                    abi_hash,
                    body_hash,
                    code_offset: 16,
                    code_byte_len: 32,
                    source_span: Some(SourceSpanRef::new(3)),
                    first_body_span: None,
                    body_span_count: 0,
                },
            ],
            source_spans: vec![
                SourceSpanRecord {
                    file_name: StringRef::new(8),
                    start_byte: 0,
                    end_byte: 4,
                    start_line: 1,
                    start_column: 1,
                    end_line: 1,
                    end_column: 5,
                },
                SourceSpanRecord {
                    file_name: StringRef::new(8),
                    start_byte: 10,
                    end_byte: 18,
                    start_line: 2,
                    start_column: 1,
                    end_line: 2,
                    end_column: 9,
                },
                SourceSpanRecord {
                    file_name: StringRef::new(8),
                    start_byte: 30,
                    end_byte: 40,
                    start_line: 3,
                    start_column: 1,
                    end_line: 3,
                    end_column: 11,
                },
                SourceSpanRecord {
                    file_name: StringRef::new(8),
                    start_byte: 50,
                    end_byte: 75,
                    start_line: 4,
                    start_column: 1,
                    end_line: 5,
                    end_column: 4,
                },
                SourceSpanRecord {
                    file_name: StringRef::new(8),
                    start_byte: 80,
                    end_byte: 89,
                    start_line: 6,
                    start_column: 1,
                    end_line: 6,
                    end_column: 10,
                },
                SourceSpanRecord {
                    file_name: StringRef::new(8),
                    start_byte: 90,
                    end_byte: 110,
                    start_line: 7,
                    start_column: 1,
                    end_line: 8,
                    end_column: 2,
                },
            ],
        }
    }

    #[test]
    fn canonical_package_has_frozen_header_directory_and_round_trips() {
        let package = sample_package();
        let code_range = CodeImageRange {
            offset: 0,
            byte_len: 64,
        };
        let encoded =
            encode_package_with_code_range(&package, code_range).expect("sample package encodes");

        let prefix_len = 31u64;
        let mut streamed = io::Cursor::new(vec![0xA5; prefix_len as usize]);
        streamed.set_position(prefix_len);
        let streamed_len =
            write_package_with_code_range(&mut streamed, &package, code_range).unwrap();
        assert_eq!(streamed_len, encoded.len() as u64);
        assert_eq!(
            &streamed.into_inner()[prefix_len as usize..],
            encoded.as_slice()
        );

        assert_eq!(&encoded[..8], b"ARCHEECS");
        assert_eq!(read_u32(&encoded, 8), 2);
        assert_eq!(read_u32(&encoded, 12), 64);
        assert_eq!(read_u64(&encoded, 24), 3_008);
        assert_eq!(read_u64(&encoded, 32), 64);
        assert_eq!(read_u64(&encoded, 40), 14);
        assert_eq!(read_u64(&encoded, 48), 64);
        assert_eq!(wire::schema::RECORD_SIZE, 96);
        assert_eq!(wire::schema::FLAGS, 56);
        assert_eq!(wire::schema::RESERVED, 64);
        assert_eq!(wire::schema::flags::TAG, 1);

        let expected = [
            (1, 960, 291, 0, 0),
            (2, 1_256, 64, 1, 64),
            (3, 1_320, 192, 2, 96),
            (4, 1_512, 128, 2, 64),
            (5, 1_640, 128, 1, 128),
            (6, 1_768, 128, 2, 64),
            (7, 1_896, 80, 1, 80),
            (8, 1_976, 64, 1, 64),
            (9, 2_040, 64, 1, 64),
            (10, 2_104, 48, 1, 48),
            (11, 2_152, 192, 3, 64),
            (12, 2_344, 88, 0, 0),
            (13, 2_432, 192, 2, 96),
            (14, 2_624, 384, 6, 64),
        ];
        for (index, (kind, offset, byte_len, count, stride)) in expected.into_iter().enumerate() {
            let entry = 64 + index * 64;
            assert_eq!(read_u64(&encoded, entry), kind);
            assert_eq!(read_u64(&encoded, entry + 16), offset);
            assert_eq!(read_u64(&encoded, entry + 24), byte_len);
            assert_eq!(read_u64(&encoded, entry + 32), count);
            assert_eq!(read_u64(&encoded, entry + 40), stride);
            assert_eq!(read_u64(&encoded, entry + 48), SECTION_ALIGNMENT);
        }
        for index in 0..package.schemas.len() {
            let record = 1_320 + index * wire::schema::RECORD_SIZE as usize;
            assert_eq!(
                read_u64(&encoded, record + wire::schema::FLAGS as usize),
                package.schemas[index].flags.bits()
            );
            assert!(encoded[record + wire::schema::RESERVED as usize
                ..record + wire::schema::RECORD_SIZE as usize]
                .iter()
                .all(|byte| *byte == 0));
        }

        assert_eq!(
            decode_package_with_code_range(&encoded, code_range,).expect("sample package decodes"),
            package
        );
    }

    #[test]
    fn streaming_decoder_uses_the_current_position_as_its_package_base() {
        let package = sample_package();
        let encoded = encode_package(&package).unwrap();
        let prefix_len = 37u64;
        let mut bytes = vec![0xA5; prefix_len as usize];
        bytes.extend_from_slice(&encoded);
        bytes.extend_from_slice(b"unrelated suffix");
        let mut input = TrackingCursor::new(bytes);
        input.set_position(prefix_len);

        let decoded = decode_package_from(&mut input).unwrap();

        assert_eq!(decoded, package);
        assert_eq!(input.position(), prefix_len + encoded.len() as u64);
        assert!(
            input.largest_read < encoded.len(),
            "the streaming decoder requested the entire package in one read"
        );
    }

    #[test]
    fn streaming_decoder_rejects_a_truncated_final_section() {
        let package = sample_package();
        let code_range = CodeImageRange {
            offset: 0,
            byte_len: 64,
        };
        let mut encoded = encode_package_with_code_range(&package, code_range).unwrap();
        encoded.pop();
        let mut input = io::Cursor::new(encoded);

        assert!(matches!(
            decode_package_from_with_code_range(&mut input, code_range),
            Err(ExecutionPackageV2Error::Envelope(MetadataV2Error::Io {
                kind: io::ErrorKind::UnexpectedEof,
                ..
            }))
        ));
    }

    #[test]
    fn streaming_decoder_supports_a_sparse_base_beyond_four_gib() {
        let package = sample_package();
        let code_range = CodeImageRange {
            offset: 0,
            byte_len: 64,
        };
        let encoded = encode_package_with_code_range(&package, code_range).unwrap();
        let base = u64::from(u32::MAX) + 4_096;
        let expected_end = base + encoded.len() as u64;
        let mut input = SparsePackageReader::new(base, encoded);

        let decoded = decode_package_from_with_code_range(&mut input, code_range).unwrap();

        assert_eq!(decoded, package);
        assert_eq!(input.position, expected_end);
    }

    #[test]
    fn streaming_decoder_propagates_an_injected_section_read_failure() {
        let package = sample_package();
        let code_range = CodeImageRange {
            offset: 0,
            byte_len: 64,
        };
        let encoded = encode_package_with_code_range(&package, code_range).unwrap();
        let first_section_offset = read_u64(&encoded, 64 + 16);
        let mut input = SparsePackageReader::new(0, encoded).with_failure(first_section_offset);

        assert!(matches!(
            decode_package_from_with_code_range(&mut input, code_range),
            Err(ExecutionPackageV2Error::Envelope(MetadataV2Error::Io {
                kind: io::ErrorKind::Other,
                ..
            }))
        ));
    }

    #[test]
    fn streaming_decoder_preserves_canonical_reencode_validation_at_a_prefixed_base() {
        let package = sample_package();
        let code_range = CodeImageRange {
            offset: 0,
            byte_len: 64,
        };
        let mut encoded = encode_package_with_code_range(&package, code_range).unwrap();
        let startup_entry = 64 + (section_kind::STARTUP_OPERATIONS as usize - 1) * 64;
        let startup_offset = read_u64(&encoded, startup_entry + 16) as usize;
        encoded[startup_offset + wire::startup_operation::RESERVED_ARGUMENT as usize] = 1;
        let prefix_len = 43u64;
        let mut bytes = vec![0x5A; prefix_len as usize];
        bytes.extend_from_slice(&encoded);
        let mut input = io::Cursor::new(bytes);
        input.set_position(prefix_len);

        assert!(matches!(
            decode_package_from_with_code_range(&mut input, code_range),
            Err(ExecutionPackageV2Error::NonCanonicalEncoding)
        ));
    }

    #[test]
    fn decoder_rejects_corrupt_cross_references_identifiers_and_reserved_bytes() {
        let package = sample_package();
        let encoded = encode_package_with_code_range(
            &package,
            CodeImageRange {
                offset: 0,
                byte_len: 64,
            },
        )
        .unwrap();

        let mutations = [
            (1_320usize, 0xFF), // schema identifier
            (1_512, 0xFF),      // field schema reference
            (1_704, 0xFF),      // system reserved bytes
            (2_456, 0xFF),      // startup function ABI hash
            (2_680, 0xFF),      // source span reserved bytes
        ];
        for (offset, value) in mutations {
            let mut corrupt = encoded.clone();
            corrupt[offset] = value;
            assert!(
                decode_package_with_code_range(
                    &corrupt,
                    CodeImageRange {
                        offset: 0,
                        byte_len: 64,
                    },
                )
                .is_err(),
                "corruption at byte {offset} was accepted"
            );
        }
    }

    #[test]
    fn host_rejects_unknown_and_kind_mismatched_schema_flags() {
        let code_range = CodeImageRange {
            offset: 0,
            byte_len: 64,
        };
        let package = sample_package();
        let encoded = encode_package_with_code_range(&package, code_range).unwrap();
        let schema_directory =
            64 + (section_kind::SCHEMAS as usize - 1) * wire::DIRECTORY_ENTRY_SIZE as usize;
        let schemas = read_u64(&encoded, schema_directory + 16) as usize;
        let flags = schemas + wire::schema::FLAGS as usize;

        for (bits, expected_reason) in [
            (2, "unknown schema flag bits"),
            (
                wire::schema::flags::TAG,
                "schema flags do not match schema kind",
            ),
        ] {
            let mut corrupt = encoded.clone();
            corrupt[flags..flags + 8].copy_from_slice(&bits.to_le_bytes());
            assert!(matches!(
                decode_package_with_code_range(&corrupt, code_range),
                Err(ExecutionPackageV2Error::InvalidRecord {
                    section: section_kind::SCHEMAS,
                    index: 0,
                    reason,
                }) if reason == expected_reason
            ));
        }

        let mut unknown = package.clone();
        unknown.schemas[0].flags = SchemaFlags::from_bits(2);
        assert!(matches!(
            validate_package(&unknown),
            Err(ExecutionPackageV2Error::InvalidRecord {
                section: section_kind::SCHEMAS,
                index: 0,
                reason: "unknown schema flag bits",
            })
        ));

        let mut mismatched = package;
        mismatched.schemas[0].flags = SchemaFlags::TAG;
        assert!(matches!(
            validate_package(&mismatched),
            Err(ExecutionPackageV2Error::InvalidRecord {
                section: section_kind::SCHEMAS,
                index: 0,
                reason: "schema flags do not match schema kind",
            })
        ));
    }

    #[test]
    fn package_validation_rejects_out_of_range_native_functions_and_bad_payloads() {
        let mut package = sample_package();
        package.function_links[0].code_offset = 48;
        package.function_links[0].code_byte_len = 17;
        assert!(matches!(
            validate_package_with_code_range(
                &package,
                CodeImageRange {
                    offset: 0,
                    byte_len: 64,
                },
            ),
            Err(ExecutionPackageV2Error::InvalidFunctionLink { .. })
        ));

        let mut package = sample_package();
        package.payloads[0].bytes.pop();
        assert!(matches!(
            validate_package(&package),
            Err(ExecutionPackageV2Error::InvalidPayload { .. })
        ));
    }

    #[test]
    fn package_validation_requires_canonical_startup_and_system_function_links() {
        let mut package = sample_package();
        package.function_links.remove(0);
        assert!(matches!(
            validate_package(&package),
            Err(ExecutionPackageV2Error::InvalidFunctionLink { .. })
        ));

        let mut package = sample_package();
        package.function_links.swap(0, 1);
        assert!(matches!(
            validate_package(&package),
            Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "function links",
                ..
            })
        ));

        let mut package = sample_package();
        package.world.startup_body_hash = BodyHash::from_bytes([0xA5; 16]);
        assert!(matches!(
            validate_package(&package),
            Err(ExecutionPackageV2Error::InvalidFunctionLink { .. })
        ));
    }

    #[test]
    fn function_body_span_slices_round_trip_and_reject_corruption() {
        let mut package = sample_package();
        package.function_links[0].first_body_span = Some(SourceSpanRef::new(5));
        package.function_links[0].body_span_count = 1;
        package.function_links[1].first_body_span = Some(SourceSpanRef::new(3));
        package.function_links[1].body_span_count = 1;
        let encoded = encode_package(&package).expect("body span slices encode");
        assert_eq!(
            decode_package(&encoded).expect("body span slices decode"),
            package
        );

        let mut mismatched = sample_package();
        mismatched.function_links[0].body_span_count = 1;
        assert!(matches!(
            validate_package(&mismatched),
            Err(ExecutionPackageV2Error::InvalidFunctionLink { .. })
        ));

        let mut out_of_range = sample_package();
        out_of_range.function_links[0].first_body_span = Some(SourceSpanRef::new(5));
        out_of_range.function_links[0].body_span_count = 2;
        assert!(matches!(
            validate_package(&out_of_range),
            Err(ExecutionPackageV2Error::InvalidFunctionLink { .. })
        ));

        let mut not_nested = sample_package();
        not_nested.function_links[0].first_body_span = Some(SourceSpanRef::new(0));
        not_nested.function_links[0].body_span_count = 1;
        assert!(matches!(
            validate_package(&not_nested),
            Err(ExecutionPackageV2Error::InvalidFunctionLink { .. })
        ));

        let mut overlapping = sample_package();
        overlapping.function_links[0].source_span = Some(SourceSpanRef::new(3));
        overlapping.function_links[0].first_body_span = Some(SourceSpanRef::new(3));
        overlapping.function_links[0].body_span_count = 1;
        overlapping.function_links[1].first_body_span = Some(SourceSpanRef::new(3));
        overlapping.function_links[1].body_span_count = 1;
        assert!(matches!(
            validate_package(&overlapping),
            Err(ExecutionPackageV2Error::InvalidFunctionLink { .. })
        ));
    }

    #[test]
    fn package_validation_rejects_noncanonical_ids_and_resource_flow() {
        let mut package = sample_package();
        package.systems[0].id = DeclId::from_bytes([0xA5; 16]);
        assert!(matches!(
            validate_package(&package),
            Err(ExecutionPackageV2Error::InvalidIdentifier {
                table: "systems",
                ..
            })
        ));

        let mut package = sample_package();
        package.startup_operations.swap(0, 2);
        assert!(matches!(
            validate_package(&package),
            Err(ExecutionPackageV2Error::InvalidRecord {
                section: section_kind::STARTUP_OPERATIONS,
                ..
            })
        ));
    }

    #[test]
    fn package_validation_requires_strictly_sorted_unique_strings() {
        let mut package = sample_package();
        package.strings.swap(0, 1);
        assert!(matches!(
            validate_package(&package),
            Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "strings",
                ..
            })
        ));

        let mut package = sample_package();
        package.strings[1] = package.strings[0].clone();
        assert!(matches!(
            validate_package(&package),
            Err(ExecutionPackageV2Error::InvalidOrdering {
                table: "strings",
                ..
            })
        ));
    }

    #[test]
    fn mutable_resource_aliases_and_inclusion_exclusion_conflicts_are_rejected() {
        let mut package = sample_package();
        let resource = match package.parameters[0].kind {
            ParameterKind::ReadResource { resource } => resource,
            _ => unreachable!(),
        };
        package.parameters[1].kind = ParameterKind::MutResource { resource };
        assert!(matches!(
            validate_package(&package),
            Err(ExecutionPackageV2Error::InvalidRecord {
                section: section_kind::PARAMETERS,
                ..
            })
        ));

        let mut package = sample_package();
        package.terms[0].access = QueryAccess::Exclude;
        package.terms.push(TermRecord {
            query: package.terms[0].query,
            access: QueryAccess::Read,
            schema: package.terms[0].schema,
            source_span: package.terms[0].source_span,
        });
        assert!(matches!(
            validate_package(&package),
            Err(ExecutionPackageV2Error::InvalidRecord {
                section: section_kind::TERMS,
                ..
            })
        ));
    }

    #[test]
    fn source_span_byte_and_location_endpoints_must_be_consistent() {
        let mut package = sample_package();
        package.source_spans[0].start_line = 0;
        assert!(matches!(
            validate_package(&package),
            Err(ExecutionPackageV2Error::InvalidRecord {
                section: section_kind::SOURCE_SPANS,
                ..
            })
        ));

        let mut package = sample_package();
        package.source_spans[0].end_byte = package.source_spans[0].start_byte;
        assert!(matches!(
            validate_package(&package),
            Err(ExecutionPackageV2Error::InvalidRecord {
                section: section_kind::SOURCE_SPANS,
                ..
            })
        ));

        let mut package = sample_package();
        package.source_spans[0].end_line = package.source_spans[0].start_line;
        package.source_spans[0].end_column = package.source_spans[0].start_column;
        assert!(matches!(
            validate_package(&package),
            Err(ExecutionPackageV2Error::InvalidRecord {
                section: section_kind::SOURCE_SPANS,
                ..
            })
        ));
    }

    fn read_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn read_u64(bytes: &[u8], offset: usize) -> u64 {
        u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
    }
}
