//! Frozen empty-envelope vectors shared by the first M27 wire formats.

use std::fmt;

pub const HEADER_SIZE: u32 = 64;
pub const DIRECTORY_ENTRY_SIZE: u64 = 64;

pub mod wire {
    pub const MAGIC: usize = 0;
    pub const VERSION: usize = 8;
    pub const HEADER_SIZE: usize = 12;
    pub const FLAGS: usize = 16;
    pub const TOTAL_LENGTH: usize = 24;
    pub const DIRECTORY_OFFSET: usize = 32;
    pub const DIRECTORY_COUNT: usize = 40;
    pub const DIRECTORY_ENTRY_SIZE: usize = 48;
    pub const RESERVED: usize = 56;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnvelopeSpec {
    pub name: &'static str,
    pub magic: [u8; 8],
    pub version: u32,
    pub kind: EnvelopeKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvelopeKind {
    Directory,
    ProtocolFrame,
}

pub const ARCHE_OBJECT_V1: EnvelopeSpec = EnvelopeSpec::directory("ARCHEOBJ v1", *b"ARCHEOBJ", 1);
pub const CANONICAL_CORE_V2: EnvelopeSpec =
    EnvelopeSpec::directory("Canonical Core v2", *b"ARCHECOR", 2);
pub const ARCHE_ECS_V3: EnvelopeSpec = EnvelopeSpec::directory("ARCHEECS v3", *b"ARCHEECS", 3);
pub const ARCHE_OBSERVATION_V3: EnvelopeSpec =
    EnvelopeSpec::directory("ARCHEOBS v3", *b"ARCHEOBS", 3);
pub const CANONICAL_VALUE_V1: EnvelopeSpec =
    EnvelopeSpec::directory("Canonical Value v1", *b"ARCHEVAL", 1);
pub const ENVIRONMENT_PROTOCOL_V1: EnvelopeSpec =
    EnvelopeSpec::protocol_frame("environment protocol v1", *b"ARCHEENV", 1);

pub const ALL_FORMATS: [EnvelopeSpec; 6] = [
    ARCHE_OBJECT_V1,
    CANONICAL_CORE_V2,
    ARCHE_ECS_V3,
    ARCHE_OBSERVATION_V3,
    CANONICAL_VALUE_V1,
    ENVIRONMENT_PROTOCOL_V1,
];

impl EnvelopeSpec {
    pub const fn directory(name: &'static str, magic: [u8; 8], version: u32) -> Self {
        Self {
            name,
            magic,
            version,
            kind: EnvelopeKind::Directory,
        }
    }

    pub const fn protocol_frame(name: &'static str, magic: [u8; 8], version: u32) -> Self {
        Self {
            name,
            magic,
            version,
            kind: EnvelopeKind::ProtocolFrame,
        }
    }

    pub const fn empty_vector(self) -> [u8; HEADER_SIZE as usize] {
        let mut bytes = [0_u8; HEADER_SIZE as usize];
        copy(&mut bytes, wire::MAGIC, &self.magic);
        copy(&mut bytes, wire::VERSION, &self.version.to_le_bytes());
        copy(&mut bytes, wire::HEADER_SIZE, &HEADER_SIZE.to_le_bytes());
        if matches!(self.kind, EnvelopeKind::ProtocolFrame) {
            return bytes;
        }
        copy(&mut bytes, wire::FLAGS, &0_u64.to_le_bytes());
        copy(
            &mut bytes,
            wire::TOTAL_LENGTH,
            &(HEADER_SIZE as u64).to_le_bytes(),
        );
        copy(
            &mut bytes,
            wire::DIRECTORY_OFFSET,
            &(HEADER_SIZE as u64).to_le_bytes(),
        );
        copy(&mut bytes, wire::DIRECTORY_COUNT, &0_u64.to_le_bytes());
        copy(
            &mut bytes,
            wire::DIRECTORY_ENTRY_SIZE,
            &DIRECTORY_ENTRY_SIZE.to_le_bytes(),
        );
        copy(&mut bytes, wire::RESERVED, &0_u64.to_le_bytes());
        bytes
    }

    pub fn validate_empty(self, bytes: &[u8]) -> Result<(), EnvelopeError> {
        let actual_length =
            u64::try_from(bytes.len()).map_err(|_| EnvelopeError::LengthOverflow)?;
        if bytes.len() < HEADER_SIZE as usize {
            return Err(EnvelopeError::TruncatedHeader { actual_length });
        }
        if bytes[wire::MAGIC..wire::MAGIC + 8] != self.magic {
            return Err(EnvelopeError::InvalidMagic);
        }

        let version = read_u32(bytes, wire::VERSION);
        if version != self.version {
            return Err(EnvelopeError::UnsupportedVersion {
                expected: self.version,
                actual: version,
            });
        }
        let header_size = read_u32(bytes, wire::HEADER_SIZE);
        if header_size != HEADER_SIZE {
            return Err(EnvelopeError::InvalidHeaderSize {
                actual: header_size,
            });
        }
        if matches!(self.kind, EnvelopeKind::ProtocolFrame) {
            if actual_length != u64::from(HEADER_SIZE) {
                return Err(EnvelopeError::ProtocolFrameLength {
                    actual: actual_length,
                });
            }
            if let Some(offset) = bytes[16..].iter().position(|byte| *byte != 0) {
                return Err(EnvelopeError::NonZeroProtocolReserved {
                    offset: u64::try_from(offset + 16).expect("header offsets fit u64"),
                });
            }
            return Ok(());
        }
        let flags = read_u64(bytes, wire::FLAGS);
        if flags != 0 {
            return Err(EnvelopeError::UnsupportedFlags { actual: flags });
        }
        let total_length = read_u64(bytes, wire::TOTAL_LENGTH);
        if total_length != actual_length {
            return Err(EnvelopeError::TotalLengthMismatch {
                declared: total_length,
                actual: actual_length,
            });
        }
        let directory_offset = read_u64(bytes, wire::DIRECTORY_OFFSET);
        let directory_count = read_u64(bytes, wire::DIRECTORY_COUNT);
        let directory_entry_size = read_u64(bytes, wire::DIRECTORY_ENTRY_SIZE);
        let reserved = read_u64(bytes, wire::RESERVED);

        if directory_entry_size != DIRECTORY_ENTRY_SIZE {
            return Err(EnvelopeError::InvalidDirectoryEntrySize {
                actual: directory_entry_size,
            });
        }
        let directory_length = directory_count
            .checked_mul(directory_entry_size)
            .ok_or(EnvelopeError::DirectoryLengthOverflow)?;
        let directory_end = directory_offset
            .checked_add(directory_length)
            .ok_or(EnvelopeError::DirectoryEndOverflow)?;
        if directory_end > total_length {
            return Err(EnvelopeError::DirectoryOutOfBounds);
        }
        if reserved != 0 {
            return Err(EnvelopeError::NonZeroReserved { actual: reserved });
        }
        if total_length != u64::from(HEADER_SIZE)
            || directory_offset != u64::from(HEADER_SIZE)
            || directory_count != 0
        {
            return Err(EnvelopeError::NonCanonicalEmptyDirectory);
        }
        Ok(())
    }
}

const fn copy<const N: usize>(
    target: &mut [u8; HEADER_SIZE as usize],
    offset: usize,
    value: &[u8; N],
) {
    let mut index = 0;
    while index < N {
        target[offset + index] = value[index];
        index += 1;
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("header was length checked"),
    )
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("header was length checked"),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvelopeError {
    LengthOverflow,
    TruncatedHeader { actual_length: u64 },
    InvalidMagic,
    UnsupportedVersion { expected: u32, actual: u32 },
    InvalidHeaderSize { actual: u32 },
    UnsupportedFlags { actual: u64 },
    TotalLengthMismatch { declared: u64, actual: u64 },
    InvalidDirectoryEntrySize { actual: u64 },
    DirectoryLengthOverflow,
    DirectoryEndOverflow,
    DirectoryOutOfBounds,
    NonZeroReserved { actual: u64 },
    NonCanonicalEmptyDirectory,
    ProtocolFrameLength { actual: u64 },
    NonZeroProtocolReserved { offset: u64 },
}

impl fmt::Display for EnvelopeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthOverflow => {
                formatter.write_str("host length does not fit the u64 envelope")
            }
            Self::TruncatedHeader { actual_length } => {
                write!(
                    formatter,
                    "envelope header is truncated at {actual_length} bytes"
                )
            }
            Self::InvalidMagic => formatter.write_str("invalid envelope magic"),
            Self::UnsupportedVersion { expected, actual } => {
                write!(
                    formatter,
                    "unsupported envelope version {actual}; expected {expected}"
                )
            }
            Self::InvalidHeaderSize { actual } => write!(formatter, "invalid header size {actual}"),
            Self::UnsupportedFlags { actual } => {
                write!(formatter, "unsupported flags 0x{actual:016X}")
            }
            Self::TotalLengthMismatch { declared, actual } => {
                write!(
                    formatter,
                    "declared length {declared} does not match input length {actual}"
                )
            }
            Self::InvalidDirectoryEntrySize { actual } => {
                write!(formatter, "invalid directory entry size {actual}")
            }
            Self::DirectoryLengthOverflow => formatter.write_str("directory length overflows u64"),
            Self::DirectoryEndOverflow => formatter.write_str("directory end overflows u64"),
            Self::DirectoryOutOfBounds => {
                formatter.write_str("directory extends beyond the envelope")
            }
            Self::NonZeroReserved { actual } => {
                write!(formatter, "reserved field is nonzero: 0x{actual:016X}")
            }
            Self::NonCanonicalEmptyDirectory => {
                formatter.write_str("envelope is not the canonical empty-directory vector")
            }
            Self::ProtocolFrameLength { actual } => {
                write!(
                    formatter,
                    "environment protocol frame has length {actual}; expected 64"
                )
            }
            Self::NonZeroProtocolReserved { offset } => {
                write!(
                    formatter,
                    "environment protocol reserved byte at {offset} is nonzero"
                )
            }
        }
    }
}

impl std::error::Error for EnvelopeError {}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_SUFFIX: &str = concat!(
        "40000000",         // header size
        "0000000000000000", // flags
        "4000000000000000", // total length
        "4000000000000000", // directory offset
        "0000000000000000", // directory count
        "4000000000000000", // directory entry size
        "0000000000000000", // reserved
    );

    #[test]
    fn empty_envelope_vectors_are_exact_goldens() {
        let goldens = [
            (ARCHE_OBJECT_V1, "41524348454F424A01000000"),
            (CANONICAL_CORE_V2, "4152434845434F5202000000"),
            (ARCHE_ECS_V3, "415243484545435303000000"),
            (ARCHE_OBSERVATION_V3, "41524348454F425303000000"),
            (CANONICAL_VALUE_V1, "415243484556414C01000000"),
        ];
        for (spec, prefix) in goldens {
            assert_eq!(
                upper_hex(&spec.empty_vector()),
                format!("{prefix}{EMPTY_SUFFIX}")
            );
            spec.validate_empty(&spec.empty_vector())
                .expect("golden empty envelope validates");
        }

        assert_eq!(
            upper_hex(&ENVIRONMENT_PROTOCOL_V1.empty_vector()),
            concat!(
                "4152434845454E56", // ARCHEENV
                "01000000",         // version 1
                "40000000",         // frame size 64
                "00000000000000000000000000000000",
                "00000000000000000000000000000000",
                "00000000000000000000000000000000",
            )
        );
        ENVIRONMENT_PROTOCOL_V1
            .validate_empty(&ENVIRONMENT_PROTOCOL_V1.empty_vector())
            .expect("golden empty protocol frame validates");
    }

    #[test]
    fn format_magics_are_unique() {
        let magics = ALL_FORMATS
            .map(|spec| spec.magic)
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(magics.len(), ALL_FORMATS.len());
    }

    #[test]
    fn empty_validation_rejects_each_frozen_invariant() {
        let spec = ARCHE_OBJECT_V1;
        assert!(matches!(
            spec.validate_empty(&[0; 63]),
            Err(EnvelopeError::TruncatedHeader { .. })
        ));

        let mut bytes = spec.empty_vector();
        bytes[wire::MAGIC] ^= 1;
        assert_eq!(
            spec.validate_empty(&bytes),
            Err(EnvelopeError::InvalidMagic)
        );

        let mut bytes = spec.empty_vector();
        bytes[wire::VERSION..wire::VERSION + 4].copy_from_slice(&2_u32.to_le_bytes());
        assert!(matches!(
            spec.validate_empty(&bytes),
            Err(EnvelopeError::UnsupportedVersion { .. })
        ));

        let mut bytes = spec.empty_vector();
        bytes[wire::FLAGS..wire::FLAGS + 8].copy_from_slice(&1_u64.to_le_bytes());
        assert!(matches!(
            spec.validate_empty(&bytes),
            Err(EnvelopeError::UnsupportedFlags { .. })
        ));

        let mut bytes = spec.empty_vector();
        bytes[wire::RESERVED..wire::RESERVED + 8].copy_from_slice(&1_u64.to_le_bytes());
        assert!(matches!(
            spec.validate_empty(&bytes),
            Err(EnvelopeError::NonZeroReserved { .. })
        ));

        let mut bytes = spec.empty_vector();
        bytes[wire::DIRECTORY_COUNT..wire::DIRECTORY_COUNT + 8]
            .copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(
            spec.validate_empty(&bytes),
            Err(EnvelopeError::DirectoryLengthOverflow)
        );
    }

    #[test]
    fn protocol_validation_does_not_interpret_directory_fields() {
        let spec = ENVIRONMENT_PROTOCOL_V1;
        let mut bytes = spec.empty_vector();
        bytes[wire::FLAGS] = 1;
        assert_eq!(
            spec.validate_empty(&bytes),
            Err(EnvelopeError::NonZeroProtocolReserved { offset: 16 })
        );
    }

    fn upper_hex(bytes: &[u8]) -> String {
        use std::fmt::Write;

        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            write!(output, "{byte:02X}").expect("writing to a String cannot fail");
        }
        output
    }
}
