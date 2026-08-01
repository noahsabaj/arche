use std::fmt;
use std::io::{self, Read, Seek, SeekFrom, Write};

pub const MAGIC: &[u8; 8] = b"ARCHEECS";
pub const VERSION: u32 = 2;
pub const HEADER_SIZE: u32 = 64;
pub const DIRECTORY_ENTRY_SIZE: u64 = 64;

pub mod wire {
    pub mod header {
        pub const MAGIC: u64 = 0;
        pub const VERSION: u64 = 8;
        pub const HEADER_SIZE: u64 = 12;
        pub const FLAGS: u64 = 16;
        pub const TOTAL_LENGTH: u64 = 24;
        pub const DIRECTORY_OFFSET: u64 = 32;
        pub const DIRECTORY_COUNT: u64 = 40;
        pub const DIRECTORY_ENTRY_SIZE: u64 = 48;
        pub const RESERVED: u64 = 56;
    }

    pub mod directory {
        pub const KIND: u64 = 0;
        pub const FLAGS: u64 = 8;
        pub const OFFSET: u64 = 16;
        pub const BYTE_LENGTH: u64 = 24;
        pub const RECORD_COUNT: u64 = 32;
        pub const RECORD_STRIDE: u64 = 40;
        pub const ALIGNMENT: u64 = 48;
        pub const RESERVED: u64 = 56;
    }

    pub const HEADER_FLAGS: u64 = 0;
    pub const SECTION_FLAGS: u64 = 0;
}

const HEADER_FLAGS: u64 = wire::HEADER_FLAGS;
const SECTION_FLAGS: u64 = wire::SECTION_FLAGS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Section<'a> {
    pub kind: u64,
    pub alignment: u64,
    pub record_count: u64,
    pub record_stride: u64,
    pub payload: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectionDescriptor {
    pub kind: u64,
    pub alignment: u64,
    pub record_count: u64,
    pub record_stride: u64,
    pub byte_len: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectionView<'a> {
    pub kind: u64,
    pub alignment: u64,
    pub record_count: u64,
    pub record_stride: u64,
    pub payload: &'a [u8],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedSection {
    pub kind: u64,
    pub alignment: u64,
    pub record_count: u64,
    pub record_stride: u64,
    pub payload: Vec<u8>,
}

impl OwnedSection {
    pub fn as_view(&self) -> SectionView<'_> {
        SectionView {
            kind: self.kind,
            alignment: self.alignment,
            record_count: self.record_count,
            record_stride: self.record_stride,
            payload: &self.payload,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedMetadata {
    sections: Vec<OwnedSection>,
}

impl OwnedMetadata {
    pub fn sections(&self) -> &[OwnedSection] {
        &self.sections
    }

    pub fn section(&self, kind: u64) -> Option<&OwnedSection> {
        self.sections.iter().find(|section| section.kind == kind)
    }
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataView<'a> {
    sections: Vec<SectionView<'a>>,
}

#[cfg(test)]
impl<'a> MetadataView<'a> {
    pub fn sections(&self) -> &[SectionView<'a>] {
        &self.sections
    }

    pub fn section(&self, kind: u64) -> Option<&SectionView<'a>> {
        self.sections.iter().find(|section| section.kind == kind)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataV2Error {
    TruncatedHeader {
        actual: u64,
    },
    InvalidMagic,
    UnsupportedVersion {
        actual: u32,
    },
    InvalidHeaderSize {
        actual: u32,
    },
    UnsupportedHeaderFlags {
        actual: u64,
    },
    TotalLengthMismatch {
        declared: u64,
        actual: u64,
    },
    InvalidDirectoryOffset {
        actual: u64,
    },
    InvalidDirectoryEntrySize {
        actual: u64,
    },
    NonZeroHeaderReserved {
        actual: u64,
    },
    DirectoryOutOfBounds,
    ZeroSectionKind {
        index: u64,
    },
    DuplicateSectionKind {
        kind: u64,
    },
    NonCanonicalSectionOrder {
        previous: u64,
        actual: u64,
    },
    UnsupportedSectionFlags {
        kind: u64,
        actual: u64,
    },
    NonZeroSectionReserved {
        kind: u64,
        actual: u64,
    },
    InvalidSectionAlignment {
        kind: u64,
        alignment: u64,
    },
    NonCanonicalSectionOffset {
        kind: u64,
        expected: u64,
        actual: u64,
    },
    NonZeroPadding {
        offset: u64,
    },
    SectionOutOfBounds {
        kind: u64,
    },
    RawSectionHasRecords {
        kind: u64,
        record_count: u64,
    },
    RecordByteLengthMismatch {
        kind: u64,
        expected: u64,
        actual: u64,
    },
    ArithmeticOverflow {
        context: &'static str,
    },
    AllocationFailed {
        context: &'static str,
    },
    PayloadLengthMismatch {
        kind: u64,
        expected: u64,
        actual: u64,
    },
    Io {
        operation: &'static str,
        kind: io::ErrorKind,
    },
}

impl fmt::Display for MetadataV2Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedHeader { actual } => {
                write!(
                    formatter,
                    "ARCHEECS v2 header is truncated at {actual} bytes"
                )
            }
            Self::InvalidMagic => formatter.write_str("invalid ARCHEECS v2 magic"),
            Self::UnsupportedVersion { actual } => {
                write!(formatter, "unsupported ARCHEECS version {actual}")
            }
            Self::InvalidHeaderSize { actual } => {
                write!(formatter, "invalid ARCHEECS v2 header size {actual}")
            }
            Self::UnsupportedHeaderFlags { actual } => {
                write!(
                    formatter,
                    "unsupported ARCHEECS v2 header flags 0x{actual:016X}"
                )
            }
            Self::TotalLengthMismatch { declared, actual } => write!(
                formatter,
                "ARCHEECS v2 total length {declared} does not match input length {actual}"
            ),
            Self::InvalidDirectoryOffset { actual } => {
                write!(formatter, "invalid ARCHEECS v2 directory offset {actual}")
            }
            Self::InvalidDirectoryEntrySize { actual } => {
                write!(
                    formatter,
                    "invalid ARCHEECS v2 directory entry size {actual}"
                )
            }
            Self::NonZeroHeaderReserved { actual } => write!(
                formatter,
                "ARCHEECS v2 reserved header field is nonzero: 0x{actual:016X}"
            ),
            Self::DirectoryOutOfBounds => {
                formatter.write_str("ARCHEECS v2 directory extends beyond the envelope")
            }
            Self::ZeroSectionKind { index } => {
                write!(
                    formatter,
                    "ARCHEECS v2 directory entry {index} has kind zero"
                )
            }
            Self::DuplicateSectionKind { kind } => {
                write!(formatter, "duplicate ARCHEECS v2 section kind {kind}")
            }
            Self::NonCanonicalSectionOrder { previous, actual } => write!(
                formatter,
                "ARCHEECS v2 section kind {actual} follows kind {previous}"
            ),
            Self::UnsupportedSectionFlags { kind, actual } => write!(
                formatter,
                "ARCHEECS v2 section {kind} has unsupported flags 0x{actual:016X}"
            ),
            Self::NonZeroSectionReserved { kind, actual } => write!(
                formatter,
                "ARCHEECS v2 section {kind} has nonzero reserved field 0x{actual:016X}"
            ),
            Self::InvalidSectionAlignment { kind, alignment } => write!(
                formatter,
                "ARCHEECS v2 section {kind} has invalid alignment {alignment}"
            ),
            Self::NonCanonicalSectionOffset {
                kind,
                expected,
                actual,
            } => write!(
                formatter,
                "ARCHEECS v2 section {kind} starts at {actual}, expected {expected}"
            ),
            Self::NonZeroPadding { offset } => {
                write!(formatter, "ARCHEECS v2 padding byte at {offset} is nonzero")
            }
            Self::SectionOutOfBounds { kind } => {
                write!(formatter, "ARCHEECS v2 section {kind} is out of bounds")
            }
            Self::RawSectionHasRecords { kind, record_count } => write!(
                formatter,
                "raw ARCHEECS v2 section {kind} declares {record_count} records"
            ),
            Self::RecordByteLengthMismatch {
                kind,
                expected,
                actual,
            } => write!(
                formatter,
                "ARCHEECS v2 section {kind} has {actual} bytes, expected {expected}"
            ),
            Self::ArithmeticOverflow { context } => {
                write!(formatter, "ARCHEECS v2 {context} overflows u64")
            }
            Self::AllocationFailed { context } => {
                write!(formatter, "failed to allocate ARCHEECS v2 {context}")
            }
            Self::PayloadLengthMismatch {
                kind,
                expected,
                actual,
            } => write!(
                formatter,
                "ARCHEECS v2 section {kind} writer produced {actual} bytes, expected {expected}"
            ),
            Self::Io { operation, kind } => {
                write!(formatter, "ARCHEECS v2 {operation} failed: {kind}")
            }
        }
    }
}

impl std::error::Error for MetadataV2Error {}

#[derive(Clone, Copy, Debug)]
struct PlannedSection {
    descriptor: SectionDescriptor,
    offset: u64,
}

#[cfg(test)]
pub fn encode(sections: &[Section<'_>]) -> Result<Vec<u8>, MetadataV2Error> {
    let mut output = io::Cursor::new(Vec::new());
    write(&mut output, sections)?;
    Ok(output.into_inner())
}

pub fn write<W: Write + Seek>(
    output: &mut W,
    sections: &[Section<'_>],
) -> Result<u64, MetadataV2Error> {
    let mut descriptors = Vec::new();
    descriptors.try_reserve_exact(sections.len()).map_err(|_| {
        MetadataV2Error::AllocationFailed {
            context: "section descriptors",
        }
    })?;
    for section in sections {
        descriptors.push(SectionDescriptor {
            kind: section.kind,
            alignment: section.alignment,
            record_count: section.record_count,
            record_stride: section.record_stride,
            byte_len: u64::try_from(section.payload.len()).map_err(|_| {
                MetadataV2Error::ArithmeticOverflow {
                    context: "section byte length",
                }
            })?,
        });
    }

    write_streaming(output, &descriptors, |kind, output| {
        let section = sections
            .iter()
            .find(|section| section.kind == kind)
            .expect("validated unique section descriptor has a matching payload");
        output.write_all(section.payload)
    })
}

pub fn write_streaming<W, F>(
    output: &mut W,
    sections: &[SectionDescriptor],
    mut write_payload: F,
) -> Result<u64, MetadataV2Error>
where
    W: Write + Seek,
    F: FnMut(u64, &mut W) -> io::Result<()>,
{
    let base = stream_position(output, "output position")?;
    let (planned, directory_offset, section_count, total_length) = plan_sections(sections)?;
    let directory_bytes = section_count.checked_mul(DIRECTORY_ENTRY_SIZE).ok_or(
        MetadataV2Error::ArithmeticOverflow {
            context: "directory byte length",
        },
    )?;
    let directory_end = directory_offset.checked_add(directory_bytes).ok_or(
        MetadataV2Error::ArithmeticOverflow {
            context: "directory end",
        },
    )?;

    seek_relative(output, base, 0, "seek to envelope start")?;
    write_zeroes(
        output,
        directory_end,
        "write header and directory placeholders",
    )?;

    let mut cursor = directory_end;
    for section in &planned {
        let padding =
            section
                .offset
                .checked_sub(cursor)
                .ok_or(MetadataV2Error::ArithmeticOverflow {
                    context: "section padding",
                })?;
        write_zeroes(output, padding, "write section padding")?;
        let start = stream_position(output, "section start position")?;
        write_payload(section.descriptor.kind, output).map_err(|error| MetadataV2Error::Io {
            operation: "write section payload",
            kind: error.kind(),
        })?;
        let end = stream_position(output, "section end position")?;
        let actual = end
            .checked_sub(start)
            .ok_or(MetadataV2Error::ArithmeticOverflow {
                context: "written section byte length",
            })?;
        if actual != section.descriptor.byte_len {
            return Err(MetadataV2Error::PayloadLengthMismatch {
                kind: section.descriptor.kind,
                expected: section.descriptor.byte_len,
                actual,
            });
        }
        cursor = section
            .offset
            .checked_add(section.descriptor.byte_len)
            .ok_or(MetadataV2Error::ArithmeticOverflow {
                context: "section end",
            })?;
    }
    debug_assert_eq!(cursor, total_length);

    let envelope_end = checked_absolute(base, total_length, "envelope end")?;
    seek_relative(output, base, 0, "backpatch header")?;
    write_bytes(output, MAGIC, "write header magic")?;
    write_u32(output, VERSION, "write header version")?;
    write_u32(output, HEADER_SIZE, "write header size")?;
    write_u64(output, HEADER_FLAGS, "write header flags")?;
    write_u64(output, total_length, "write header total length")?;
    write_u64(output, directory_offset, "write header directory offset")?;
    write_u64(output, section_count, "write header directory count")?;
    write_u64(
        output,
        DIRECTORY_ENTRY_SIZE,
        "write header directory entry size",
    )?;
    write_u64(output, 0, "write header reserved field")?;

    for section in &planned {
        write_u64(output, section.descriptor.kind, "write directory kind")?;
        write_u64(output, SECTION_FLAGS, "write directory flags")?;
        write_u64(output, section.offset, "write directory offset")?;
        write_u64(
            output,
            section.descriptor.byte_len,
            "write directory byte length",
        )?;
        write_u64(
            output,
            section.descriptor.record_count,
            "write directory record count",
        )?;
        write_u64(
            output,
            section.descriptor.record_stride,
            "write directory record stride",
        )?;
        write_u64(
            output,
            section.descriptor.alignment,
            "write directory alignment",
        )?;
        write_u64(output, 0, "write directory reserved field")?;
    }

    seek_absolute(output, envelope_end, "restore envelope end")?;
    Ok(total_length)
}

fn plan_sections(
    sections: &[SectionDescriptor],
) -> Result<(Vec<PlannedSection>, u64, u64, u64), MetadataV2Error> {
    let section_count =
        u64::try_from(sections.len()).map_err(|_| MetadataV2Error::ArithmeticOverflow {
            context: "section count",
        })?;
    let directory_bytes = section_count.checked_mul(DIRECTORY_ENTRY_SIZE).ok_or(
        MetadataV2Error::ArithmeticOverflow {
            context: "directory byte length",
        },
    )?;
    let directory_offset = u64::from(HEADER_SIZE);
    let mut cursor = directory_offset.checked_add(directory_bytes).ok_or(
        MetadataV2Error::ArithmeticOverflow {
            context: "directory end",
        },
    )?;

    let mut ordered: Vec<SectionDescriptor> = Vec::new();
    ordered
        .try_reserve_exact(sections.len())
        .map_err(|_| MetadataV2Error::AllocationFailed {
            context: "section plan",
        })?;
    ordered.extend_from_slice(sections);
    ordered.sort_unstable_by_key(|section| section.kind);

    let mut planned = Vec::new();
    planned
        .try_reserve_exact(ordered.len())
        .map_err(|_| MetadataV2Error::AllocationFailed {
            context: "section layout",
        })?;
    let mut previous_kind = None;
    for section in ordered {
        validate_kind_order(section.kind, previous_kind, 0)?;
        validate_alignment(section.kind, section.alignment)?;
        validate_record_shape(
            section.kind,
            section.byte_len,
            section.record_count,
            section.record_stride,
        )?;
        let offset = align_up(cursor, section.alignment)?;
        cursor =
            offset
                .checked_add(section.byte_len)
                .ok_or(MetadataV2Error::ArithmeticOverflow {
                    context: "section end",
                })?;
        planned.push(PlannedSection {
            descriptor: section,
            offset,
        });
        previous_kind = Some(section.kind);
    }

    Ok((planned, directory_offset, section_count, cursor))
}

#[cfg(test)]
pub fn decode(metadata: &[u8]) -> Result<MetadataView<'_>, MetadataV2Error> {
    let actual_len =
        u64::try_from(metadata.len()).map_err(|_| MetadataV2Error::ArithmeticOverflow {
            context: "input byte length",
        })?;
    if metadata.len() < HEADER_SIZE as usize {
        return Err(MetadataV2Error::TruncatedHeader { actual: actual_len });
    }
    if &metadata[..MAGIC.len()] != MAGIC {
        return Err(MetadataV2Error::InvalidMagic);
    }

    let version = read_u32(metadata, 8);
    if version != VERSION {
        return Err(MetadataV2Error::UnsupportedVersion { actual: version });
    }
    let header_size = read_u32(metadata, 12);
    if header_size != HEADER_SIZE {
        return Err(MetadataV2Error::InvalidHeaderSize {
            actual: header_size,
        });
    }
    let header_flags = read_u64(metadata, 16);
    if header_flags != HEADER_FLAGS {
        return Err(MetadataV2Error::UnsupportedHeaderFlags {
            actual: header_flags,
        });
    }
    let declared_total = read_u64(metadata, 24);
    if declared_total != actual_len {
        return Err(MetadataV2Error::TotalLengthMismatch {
            declared: declared_total,
            actual: actual_len,
        });
    }
    let directory_offset = read_u64(metadata, 32);
    if directory_offset != u64::from(HEADER_SIZE) {
        return Err(MetadataV2Error::InvalidDirectoryOffset {
            actual: directory_offset,
        });
    }
    let section_count = read_u64(metadata, 40);
    let directory_entry_size = read_u64(metadata, 48);
    if directory_entry_size != DIRECTORY_ENTRY_SIZE {
        return Err(MetadataV2Error::InvalidDirectoryEntrySize {
            actual: directory_entry_size,
        });
    }
    let header_reserved = read_u64(metadata, 56);
    if header_reserved != 0 {
        return Err(MetadataV2Error::NonZeroHeaderReserved {
            actual: header_reserved,
        });
    }

    let directory_bytes = section_count.checked_mul(directory_entry_size).ok_or(
        MetadataV2Error::ArithmeticOverflow {
            context: "directory byte length",
        },
    )?;
    let directory_end = directory_offset.checked_add(directory_bytes).ok_or(
        MetadataV2Error::ArithmeticOverflow {
            context: "directory end",
        },
    )?;
    if directory_end > declared_total {
        return Err(MetadataV2Error::DirectoryOutOfBounds);
    }

    let section_capacity =
        usize::try_from(section_count).map_err(|_| MetadataV2Error::AllocationFailed {
            context: "decoded section directory",
        })?;
    let mut sections = Vec::new();
    sections.try_reserve_exact(section_capacity).map_err(|_| {
        MetadataV2Error::AllocationFailed {
            context: "decoded section directory",
        }
    })?;

    let mut expected_offset = directory_end;
    let mut previous_kind = None;
    for index in 0..section_count {
        let entry_offset = directory_offset
            .checked_add(index.checked_mul(directory_entry_size).ok_or(
                MetadataV2Error::ArithmeticOverflow {
                    context: "directory entry offset",
                },
            )?)
            .ok_or(MetadataV2Error::ArithmeticOverflow {
                context: "directory entry offset",
            })?;
        let entry =
            usize::try_from(entry_offset).map_err(|_| MetadataV2Error::DirectoryOutOfBounds)?;

        let kind = read_u64(metadata, entry);
        validate_kind_order(kind, previous_kind, index)?;
        let flags = read_u64(metadata, entry + 8);
        if flags != SECTION_FLAGS {
            return Err(MetadataV2Error::UnsupportedSectionFlags {
                kind,
                actual: flags,
            });
        }
        let offset = read_u64(metadata, entry + 16);
        let byte_len = read_u64(metadata, entry + 24);
        let record_count = read_u64(metadata, entry + 32);
        let record_stride = read_u64(metadata, entry + 40);
        let alignment = read_u64(metadata, entry + 48);
        validate_alignment(kind, alignment)?;
        let reserved = read_u64(metadata, entry + 56);
        if reserved != 0 {
            return Err(MetadataV2Error::NonZeroSectionReserved {
                kind,
                actual: reserved,
            });
        }
        validate_record_shape(kind, byte_len, record_count, record_stride)?;

        let canonical_offset = align_up(expected_offset, alignment)?;
        if offset != canonical_offset {
            return Err(MetadataV2Error::NonCanonicalSectionOffset {
                kind,
                expected: canonical_offset,
                actual: offset,
            });
        }
        if canonical_offset > declared_total {
            return Err(MetadataV2Error::SectionOutOfBounds { kind });
        }
        validate_zero_padding(metadata, expected_offset, offset)?;
        let section_end =
            offset
                .checked_add(byte_len)
                .ok_or(MetadataV2Error::ArithmeticOverflow {
                    context: "section end",
                })?;
        if section_end > declared_total {
            return Err(MetadataV2Error::SectionOutOfBounds { kind });
        }
        let payload_start =
            usize::try_from(offset).map_err(|_| MetadataV2Error::SectionOutOfBounds { kind })?;
        let payload_end = usize::try_from(section_end)
            .map_err(|_| MetadataV2Error::SectionOutOfBounds { kind })?;
        sections.push(SectionView {
            kind,
            alignment,
            record_count,
            record_stride,
            payload: &metadata[payload_start..payload_end],
        });

        expected_offset = section_end;
        previous_kind = Some(kind);
    }

    if expected_offset != declared_total {
        return Err(MetadataV2Error::TotalLengthMismatch {
            declared: expected_offset,
            actual: declared_total,
        });
    }

    Ok(MetadataView { sections })
}

pub fn read<R: Read + Seek>(input: &mut R) -> Result<OwnedMetadata, MetadataV2Error> {
    let base = stream_position(input, "input position")?;
    let mut header = [0u8; HEADER_SIZE as usize];
    let mut header_read = 0usize;
    while header_read < header.len() {
        match input.read(&mut header[header_read..]) {
            Ok(0) => {
                return Err(MetadataV2Error::TruncatedHeader {
                    actual: u64::try_from(header_read).unwrap_or(u64::MAX),
                });
            }
            Ok(count) => header_read += count,
            Err(error) => {
                return Err(MetadataV2Error::Io {
                    operation: "read header",
                    kind: error.kind(),
                });
            }
        }
    }
    if &header[..MAGIC.len()] != MAGIC {
        return Err(MetadataV2Error::InvalidMagic);
    }
    let version = read_u32(&header, wire::header::VERSION as usize);
    if version != VERSION {
        return Err(MetadataV2Error::UnsupportedVersion { actual: version });
    }
    let header_size = read_u32(&header, wire::header::HEADER_SIZE as usize);
    if header_size != HEADER_SIZE {
        return Err(MetadataV2Error::InvalidHeaderSize {
            actual: header_size,
        });
    }
    let header_flags = read_u64(&header, wire::header::FLAGS as usize);
    if header_flags != HEADER_FLAGS {
        return Err(MetadataV2Error::UnsupportedHeaderFlags {
            actual: header_flags,
        });
    }
    let declared_total = read_u64(&header, wire::header::TOTAL_LENGTH as usize);
    let directory_offset = read_u64(&header, wire::header::DIRECTORY_OFFSET as usize);
    if directory_offset != u64::from(HEADER_SIZE) {
        return Err(MetadataV2Error::InvalidDirectoryOffset {
            actual: directory_offset,
        });
    }
    let section_count = read_u64(&header, wire::header::DIRECTORY_COUNT as usize);
    let directory_entry_size = read_u64(&header, wire::header::DIRECTORY_ENTRY_SIZE as usize);
    if directory_entry_size != DIRECTORY_ENTRY_SIZE {
        return Err(MetadataV2Error::InvalidDirectoryEntrySize {
            actual: directory_entry_size,
        });
    }
    let reserved = read_u64(&header, wire::header::RESERVED as usize);
    if reserved != 0 {
        return Err(MetadataV2Error::NonZeroHeaderReserved { actual: reserved });
    }

    let directory_bytes = section_count.checked_mul(directory_entry_size).ok_or(
        MetadataV2Error::ArithmeticOverflow {
            context: "directory byte length",
        },
    )?;
    let directory_end = directory_offset.checked_add(directory_bytes).ok_or(
        MetadataV2Error::ArithmeticOverflow {
            context: "directory end",
        },
    )?;
    if directory_end > declared_total {
        return Err(MetadataV2Error::DirectoryOutOfBounds);
    }

    let capacity =
        usize::try_from(section_count).map_err(|_| MetadataV2Error::AllocationFailed {
            context: "streamed section directory",
        })?;
    let mut planned = Vec::new();
    planned
        .try_reserve_exact(capacity)
        .map_err(|_| MetadataV2Error::AllocationFailed {
            context: "streamed section directory",
        })?;
    seek_relative(input, base, directory_offset, "seek to section directory")?;
    let mut expected_offset = directory_end;
    let mut previous_kind = None;
    for index in 0..section_count {
        let mut row = [0u8; DIRECTORY_ENTRY_SIZE as usize];
        read_exact(input, &mut row, "read directory entry")?;
        let kind = read_u64(&row, wire::directory::KIND as usize);
        validate_kind_order(kind, previous_kind, index)?;
        let flags = read_u64(&row, wire::directory::FLAGS as usize);
        if flags != SECTION_FLAGS {
            return Err(MetadataV2Error::UnsupportedSectionFlags {
                kind,
                actual: flags,
            });
        }
        let offset = read_u64(&row, wire::directory::OFFSET as usize);
        let byte_len = read_u64(&row, wire::directory::BYTE_LENGTH as usize);
        let record_count = read_u64(&row, wire::directory::RECORD_COUNT as usize);
        let record_stride = read_u64(&row, wire::directory::RECORD_STRIDE as usize);
        let alignment = read_u64(&row, wire::directory::ALIGNMENT as usize);
        validate_alignment(kind, alignment)?;
        let reserved = read_u64(&row, wire::directory::RESERVED as usize);
        if reserved != 0 {
            return Err(MetadataV2Error::NonZeroSectionReserved {
                kind,
                actual: reserved,
            });
        }
        validate_record_shape(kind, byte_len, record_count, record_stride)?;
        let canonical_offset = align_up(expected_offset, alignment)?;
        if offset != canonical_offset {
            return Err(MetadataV2Error::NonCanonicalSectionOffset {
                kind,
                expected: canonical_offset,
                actual: offset,
            });
        }
        let section_end =
            offset
                .checked_add(byte_len)
                .ok_or(MetadataV2Error::ArithmeticOverflow {
                    context: "section end",
                })?;
        if section_end > declared_total {
            return Err(MetadataV2Error::SectionOutOfBounds { kind });
        }
        planned.push(PlannedSection {
            descriptor: SectionDescriptor {
                kind,
                alignment,
                record_count,
                record_stride,
                byte_len,
            },
            offset,
        });
        expected_offset = section_end;
        previous_kind = Some(kind);
    }
    if expected_offset != declared_total {
        return Err(MetadataV2Error::TotalLengthMismatch {
            declared: expected_offset,
            actual: declared_total,
        });
    }

    let mut sections = Vec::new();
    sections
        .try_reserve_exact(capacity)
        .map_err(|_| MetadataV2Error::AllocationFailed {
            context: "streamed sections",
        })?;
    let mut cursor = directory_end;
    for section in planned {
        seek_relative(input, base, cursor, "seek to section padding")?;
        validate_zero_padding_reader(input, cursor, section.offset)?;
        let payload_size = usize::try_from(section.descriptor.byte_len).map_err(|_| {
            MetadataV2Error::AllocationFailed {
                context: "streamed section payload",
            }
        })?;
        let mut payload = Vec::new();
        payload
            .try_reserve_exact(payload_size)
            .map_err(|_| MetadataV2Error::AllocationFailed {
                context: "streamed section payload",
            })?;
        payload.resize(payload_size, 0);
        read_exact(input, &mut payload, "read section payload")?;
        sections.push(OwnedSection {
            kind: section.descriptor.kind,
            alignment: section.descriptor.alignment,
            record_count: section.descriptor.record_count,
            record_stride: section.descriptor.record_stride,
            payload,
        });
        cursor = section
            .offset
            .checked_add(section.descriptor.byte_len)
            .ok_or(MetadataV2Error::ArithmeticOverflow {
                context: "section end",
            })?;
    }
    seek_relative(input, base, declared_total, "restore envelope end")?;
    Ok(OwnedMetadata { sections })
}

fn validate_kind_order(
    kind: u64,
    previous_kind: Option<u64>,
    index: u64,
) -> Result<(), MetadataV2Error> {
    if kind == 0 {
        return Err(MetadataV2Error::ZeroSectionKind { index });
    }
    if let Some(previous) = previous_kind {
        if kind == previous {
            return Err(MetadataV2Error::DuplicateSectionKind { kind });
        }
        if kind < previous {
            return Err(MetadataV2Error::NonCanonicalSectionOrder {
                previous,
                actual: kind,
            });
        }
    }
    Ok(())
}

fn validate_alignment(kind: u64, alignment: u64) -> Result<(), MetadataV2Error> {
    if !alignment.is_power_of_two() {
        return Err(MetadataV2Error::InvalidSectionAlignment { kind, alignment });
    }
    Ok(())
}

fn validate_record_shape(
    kind: u64,
    byte_len: u64,
    record_count: u64,
    record_stride: u64,
) -> Result<(), MetadataV2Error> {
    if record_stride == 0 {
        if record_count != 0 {
            return Err(MetadataV2Error::RawSectionHasRecords { kind, record_count });
        }
        return Ok(());
    }

    let expected =
        record_count
            .checked_mul(record_stride)
            .ok_or(MetadataV2Error::ArithmeticOverflow {
                context: "fixed-record section byte length",
            })?;
    if byte_len != expected {
        return Err(MetadataV2Error::RecordByteLengthMismatch {
            kind,
            expected,
            actual: byte_len,
        });
    }
    Ok(())
}

fn align_up(value: u64, alignment: u64) -> Result<u64, MetadataV2Error> {
    debug_assert!(alignment.is_power_of_two());
    value
        .checked_add(alignment - 1)
        .map(|aligned| aligned & !(alignment - 1))
        .ok_or(MetadataV2Error::ArithmeticOverflow {
            context: "section alignment",
        })
}

#[cfg(test)]
fn validate_zero_padding(metadata: &[u8], start: u64, end: u64) -> Result<(), MetadataV2Error> {
    let start = usize::try_from(start).map_err(|_| MetadataV2Error::DirectoryOutOfBounds)?;
    let end = usize::try_from(end).map_err(|_| MetadataV2Error::DirectoryOutOfBounds)?;
    if let Some(relative) = metadata[start..end].iter().position(|byte| *byte != 0) {
        let offset =
            u64::try_from(start + relative).map_err(|_| MetadataV2Error::ArithmeticOverflow {
                context: "padding offset",
            })?;
        return Err(MetadataV2Error::NonZeroPadding { offset });
    }
    Ok(())
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + size_of::<u32>()]
            .try_into()
            .expect("validated fixed-size ARCHEECS v2 field"),
    )
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + size_of::<u64>()]
            .try_into()
            .expect("validated fixed-size ARCHEECS v2 field"),
    )
}

fn stream_position<S: Seek>(
    stream: &mut S,
    operation: &'static str,
) -> Result<u64, MetadataV2Error> {
    stream
        .stream_position()
        .map_err(|error| MetadataV2Error::Io {
            operation,
            kind: error.kind(),
        })
}

fn checked_absolute(
    base: u64,
    relative: u64,
    context: &'static str,
) -> Result<u64, MetadataV2Error> {
    base.checked_add(relative)
        .ok_or(MetadataV2Error::ArithmeticOverflow { context })
}

fn seek_relative<S: Seek>(
    stream: &mut S,
    base: u64,
    relative: u64,
    operation: &'static str,
) -> Result<(), MetadataV2Error> {
    let absolute = checked_absolute(base, relative, "absolute stream position")?;
    seek_absolute(stream, absolute, operation)
}

fn seek_absolute<S: Seek>(
    stream: &mut S,
    absolute: u64,
    operation: &'static str,
) -> Result<(), MetadataV2Error> {
    stream
        .seek(SeekFrom::Start(absolute))
        .map(|_| ())
        .map_err(|error| MetadataV2Error::Io {
            operation,
            kind: error.kind(),
        })
}

fn write_bytes<W: Write>(
    output: &mut W,
    bytes: &[u8],
    operation: &'static str,
) -> Result<(), MetadataV2Error> {
    output
        .write_all(bytes)
        .map_err(|error| MetadataV2Error::Io {
            operation,
            kind: error.kind(),
        })
}

fn write_u32<W: Write>(
    output: &mut W,
    value: u32,
    operation: &'static str,
) -> Result<(), MetadataV2Error> {
    write_bytes(output, &value.to_le_bytes(), operation)
}

fn write_u64<W: Write>(
    output: &mut W,
    value: u64,
    operation: &'static str,
) -> Result<(), MetadataV2Error> {
    write_bytes(output, &value.to_le_bytes(), operation)
}

fn write_zeroes<W: Write>(
    output: &mut W,
    mut count: u64,
    operation: &'static str,
) -> Result<(), MetadataV2Error> {
    const ZEROES: [u8; 4096] = [0; 4096];
    while count != 0 {
        let chunk = usize::try_from(count.min(ZEROES.len() as u64))
            .expect("zero-write chunk is bounded by a host-sized buffer");
        write_bytes(output, &ZEROES[..chunk], operation)?;
        count -= chunk as u64;
    }
    Ok(())
}

fn read_exact<R: Read>(
    input: &mut R,
    bytes: &mut [u8],
    operation: &'static str,
) -> Result<(), MetadataV2Error> {
    input
        .read_exact(bytes)
        .map_err(|error| MetadataV2Error::Io {
            operation,
            kind: error.kind(),
        })
}

fn validate_zero_padding_reader<R: Read>(
    input: &mut R,
    start: u64,
    end: u64,
) -> Result<(), MetadataV2Error> {
    let mut remaining = end
        .checked_sub(start)
        .ok_or(MetadataV2Error::DirectoryOutOfBounds)?;
    let mut offset = start;
    let mut buffer = [0u8; 4096];
    while remaining != 0 {
        let chunk = usize::try_from(remaining.min(buffer.len() as u64))
            .expect("padding-read chunk is bounded by a host-sized buffer");
        read_exact(input, &mut buffer[..chunk], "read section padding")?;
        if let Some(index) = buffer[..chunk].iter().position(|byte| *byte != 0) {
            return Err(MetadataV2Error::NonZeroPadding {
                offset: offset + index as u64,
            });
        }
        remaining -= chunk as u64;
        offset += chunk as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SparseBuffer {
        base: u64,
        position: u64,
        bytes: Vec<u8>,
        seek_count: usize,
        maximum_position: u64,
    }

    impl SparseBuffer {
        fn new(base: u64) -> Self {
            Self {
                base,
                position: base,
                bytes: Vec::new(),
                seek_count: 0,
                maximum_position: base,
            }
        }
    }

    impl Write for SparseBuffer {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            let relative = self
                .position
                .checked_sub(self.base)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "before sparse base"))?;
            let start = usize::try_from(relative)
                .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "write offset"))?;
            let end = start
                .checked_add(buffer.len())
                .ok_or_else(|| io::Error::new(io::ErrorKind::OutOfMemory, "write end"))?;
            if self.bytes.len() < end {
                self.bytes.resize(end, 0);
            }
            self.bytes[start..end].copy_from_slice(buffer);
            self.position = self
                .position
                .checked_add(buffer.len() as u64)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "position overflow"))?;
            self.maximum_position = self.maximum_position.max(self.position);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Seek for SparseBuffer {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            let absolute = match position {
                SeekFrom::Start(position) => i128::from(position),
                SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
                SeekFrom::End(delta) => {
                    i128::from(self.base) + self.bytes.len() as i128 + i128::from(delta)
                }
            };
            self.position = u64::try_from(absolute)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid seek"))?;
            self.seek_count += 1;
            Ok(self.position)
        }
    }

    struct FailAfter {
        inner: io::Cursor<Vec<u8>>,
        remaining: usize,
    }

    impl Write for FailAfter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Err(io::Error::other("injected failure"));
            }
            let count = self.remaining.min(buffer.len());
            let written = self.inner.write(&buffer[..count])?;
            self.remaining -= written;
            Ok(written)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    impl Seek for FailAfter {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.inner.seek(position)
        }
    }

    fn raw_section<'a>(kind: u64, alignment: u64, payload: &'a [u8]) -> Section<'a> {
        Section {
            kind,
            alignment,
            record_count: 0,
            record_stride: 0,
            payload,
        }
    }

    fn record_section<'a>(
        kind: u64,
        alignment: u64,
        record_count: u64,
        record_stride: u64,
        payload: &'a [u8],
    ) -> Section<'a> {
        Section {
            kind,
            alignment,
            record_count,
            record_stride,
            payload,
        }
    }

    #[test]
    fn empty_metadata_has_the_exact_v2_header() {
        let metadata = encode(&[]).expect("empty metadata should encode");

        assert_eq!(metadata.len(), HEADER_SIZE as usize);
        assert_eq!(&metadata[0..8], MAGIC);
        assert_eq!(
            u32::from_le_bytes(metadata[8..12].try_into().unwrap()),
            VERSION
        );
        assert_eq!(
            u32::from_le_bytes(metadata[12..16].try_into().unwrap()),
            HEADER_SIZE
        );
        assert_eq!(u64::from_le_bytes(metadata[16..24].try_into().unwrap()), 0);
        assert_eq!(
            u64::from_le_bytes(metadata[24..32].try_into().unwrap()),
            HEADER_SIZE as u64
        );
        assert_eq!(
            u64::from_le_bytes(metadata[32..40].try_into().unwrap()),
            HEADER_SIZE as u64
        );
        assert_eq!(u64::from_le_bytes(metadata[40..48].try_into().unwrap()), 0);
        assert_eq!(
            u64::from_le_bytes(metadata[48..56].try_into().unwrap()),
            DIRECTORY_ENTRY_SIZE
        );
        assert_eq!(u64::from_le_bytes(metadata[56..64].try_into().unwrap()), 0);
        assert!(decode(&metadata).unwrap().sections().is_empty());
    }

    #[test]
    fn sections_encode_in_canonical_kind_order_with_zero_alignment_padding() {
        let metadata = encode(&[
            raw_section(9, 16, b"blob"),
            record_section(2, 8, 2, 2, b"abcd"),
        ])
        .unwrap();
        let decoded = decode(&metadata).unwrap();

        assert_eq!(decoded.sections().len(), 2);
        assert_eq!(decoded.sections()[0].kind, 2);
        assert_eq!(decoded.sections()[0].payload, b"abcd");
        assert_eq!(decoded.sections()[1].kind, 9);
        assert_eq!(decoded.sections()[1].payload, b"blob");
        assert!(metadata[196..208].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn malformed_header_and_directory_fields_are_rejected() {
        let valid = encode(&[record_section(2, 8, 1, 4, b"abcd")]).unwrap();
        let mutations: &[(usize, u8)] = &[
            (0, b'X'),
            (8, 3),
            (12, 63),
            (16, 1),
            (24, 0),
            (32, 0),
            (48, 63),
            (56, 1),
            (64, 0),
            (72, 1),
            (104, 3),
            (112, 0),
            (120, 1),
        ];

        for &(offset, value) in mutations {
            let mut malformed = valid.clone();
            malformed[offset] = value;
            assert!(
                decode(&malformed).is_err(),
                "mutation at byte {offset} was accepted"
            );
        }
    }

    #[test]
    fn v1_is_rejected_by_the_v2_decoder() {
        let mut metadata = encode(&[]).unwrap();
        metadata[8..12].copy_from_slice(&1u32.to_le_bytes());

        assert_eq!(
            decode(&metadata),
            Err(MetadataV2Error::UnsupportedVersion { actual: 1 })
        );
    }

    #[test]
    fn malformed_section_ranges_and_record_shapes_are_rejected() {
        let valid = encode(&[
            record_section(2, 8, 1, 4, b"abcd"),
            raw_section(9, 16, b"blob"),
        ])
        .unwrap();

        for (field_offset, value) in [
            (80usize, u64::MAX),
            (88, u64::MAX),
            (96, 2),
            (104, 8),
            (112, 3),
        ] {
            let mut malformed = valid.clone();
            malformed[field_offset..field_offset + 8].copy_from_slice(&value.to_le_bytes());
            assert!(
                decode(&malformed).is_err(),
                "directory field at {field_offset} accepted {value}"
            );
        }
    }

    #[test]
    fn duplicate_kinds_and_noncanonical_padding_are_rejected() {
        assert!(encode(&[raw_section(2, 1, b"a"), raw_section(2, 1, b"b")]).is_err());

        let mut metadata = encode(&[
            record_section(2, 8, 2, 2, b"abcd"),
            raw_section(9, 16, b"blob"),
        ])
        .unwrap();
        metadata[200] = 1;
        assert!(decode(&metadata).is_err());
    }

    #[test]
    fn truncated_inputs_never_decode() {
        let metadata = encode(&[raw_section(7, 8, b"payload")]).unwrap();

        for length in 0..metadata.len() {
            assert!(
                decode(&metadata[..length]).is_err(),
                "accepted length {length}"
            );
        }
    }

    #[test]
    fn huge_aligned_empty_section_is_rejected_without_panicking() {
        let mut metadata = encode(&[raw_section(7, 1, b"")]).unwrap();
        let huge_alignment = 1u64 << 63;
        metadata[80..88].copy_from_slice(&huge_alignment.to_le_bytes());
        metadata[112..120].copy_from_slice(&huge_alignment.to_le_bytes());

        assert!(decode(&metadata).is_err());
    }

    #[test]
    fn streaming_writer_backpatches_at_sparse_u64_positions_with_exact_bytes() {
        let sections = [
            raw_section(9, 16, b"blob"),
            record_section(2, 8, 2, 2, b"abcd"),
        ];
        let canonical = encode(&sections).unwrap();
        let base = u64::from(u32::MAX) + 65_537;
        let mut streamed = SparseBuffer::new(base);

        let length = write(&mut streamed, &sections).unwrap();

        assert_eq!(length, canonical.len() as u64);
        assert_eq!(streamed.bytes, canonical);
        assert_eq!(streamed.maximum_position, base + length);
        assert!(
            streamed.seek_count >= 3,
            "writer must seek to backpatch and restore"
        );
    }

    #[test]
    fn streaming_writer_reports_payload_length_and_io_failures() {
        let mut output = io::Cursor::new(Vec::new());
        let mismatch = write_streaming(
            &mut output,
            &[SectionDescriptor {
                kind: 1,
                alignment: 1,
                record_count: 0,
                record_stride: 0,
                byte_len: 4,
            }],
            |_, output| output.write_all(b"abc"),
        );
        assert!(matches!(
            mismatch,
            Err(MetadataV2Error::PayloadLengthMismatch {
                expected: 4,
                actual: 3,
                ..
            })
        ));

        let mut failing = FailAfter {
            inner: io::Cursor::new(Vec::new()),
            remaining: 17,
        };
        assert!(matches!(
            write(&mut failing, &[raw_section(1, 1, b"payload")]),
            Err(MetadataV2Error::Io {
                kind: io::ErrorKind::Other,
                ..
            })
        ));
    }

    #[test]
    fn seek_reader_decodes_sections_without_allocating_a_whole_envelope() {
        let sections = [
            record_section(2, 8, 2, 2, b"abcd"),
            raw_section(9, 16, b"blob"),
        ];
        let canonical = encode(&sections).unwrap();
        let prefix_len = 37u64;
        let mut bytes = vec![0xA5; prefix_len as usize];
        bytes.extend_from_slice(&canonical);
        bytes.extend_from_slice(b"trailing container bytes");
        let mut input = io::Cursor::new(bytes);
        input.set_position(prefix_len);

        let decoded = read(&mut input).unwrap();

        assert_eq!(input.position(), prefix_len + canonical.len() as u64);
        assert_eq!(decoded.sections().len(), 2);
        assert_eq!(decoded.section(2).unwrap().payload, b"abcd");
        assert_eq!(decoded.section(9).unwrap().payload, b"blob");
    }
}
