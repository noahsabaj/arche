//! Canonical Value v1 codec and ARCHEVAL 64-byte container (M27-E).

use std::cmp::Ordering;

use crate::runtime::def::{CanonicalScalar, CanonicalValue};
use arche_foundation::identity::TypeId;

pub const ARCHEVAL_MAGIC: &[u8; 8] = b"ARCHEVAL";
pub const ARCHEVAL_VERSION: u32 = 1;
pub const DIRECTORY_ENTRY_SIZE: u64 = 64;
pub const HEADER_SIZE: u32 = 64;

/// Result of validating a Canonical Value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueValidationError {
    NonAscendingMapKeys,
    DuplicateMapKey,
    FloatMapKeyForbidden,
    MalformedUtf8String,
    PayloadTruncated,
}

/// Serializes a `CanonicalValue` into a deterministic byte stream.
pub fn encode_canonical_value(val: &CanonicalValue, out: &mut Vec<u8>) {
    match val {
        CanonicalValue::Unit => {
            out.push(0);
        }
        CanonicalValue::Scalar(s) => {
            out.push(1);
            match s {
                CanonicalScalar::I8(v) => {
                    out.push(1);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                CanonicalScalar::I16(v) => {
                    out.push(2);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                CanonicalScalar::I32(v) => {
                    out.push(3);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                CanonicalScalar::I64(v) => {
                    out.push(4);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                CanonicalScalar::Isize(v) => {
                    out.push(5);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                CanonicalScalar::U8(v) => {
                    out.push(6);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                CanonicalScalar::U16(v) => {
                    out.push(7);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                CanonicalScalar::U32(v) => {
                    out.push(8);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                CanonicalScalar::U64(v) => {
                    out.push(9);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                CanonicalScalar::Usize(v) => {
                    out.push(10);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                CanonicalScalar::F32(v) => {
                    out.push(11);
                    out.extend_from_slice(&v.to_bits().to_le_bytes());
                }
                CanonicalScalar::F64(v) => {
                    out.push(12);
                    out.extend_from_slice(&v.to_bits().to_le_bytes());
                }
                CanonicalScalar::Bool(v) => {
                    out.push(13);
                    out.push(if *v { 1 } else { 0 });
                }
                CanonicalScalar::Char(v) => {
                    out.push(14);
                    out.extend_from_slice(&(*v as u32).to_le_bytes());
                }
            }
        }
        CanonicalValue::String(s) => {
            out.push(2);
            out.extend_from_slice(&(s.len() as u64).to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        CanonicalValue::Bytes(b) => {
            out.push(3);
            out.extend_from_slice(&(b.len() as u64).to_le_bytes());
            out.extend_from_slice(b);
        }
        CanonicalValue::Array(items) | CanonicalValue::Tuple(items) => {
            out.push(4);
            out.extend_from_slice(&(items.len() as u64).to_le_bytes());
            for item in items {
                encode_canonical_value(item, out);
            }
        }
        CanonicalValue::Struct { type_id, fields } => {
            out.push(5);
            out.extend_from_slice(type_id.as_bytes());
            out.extend_from_slice(&(fields.len() as u64).to_le_bytes());
            for (name, field_val) in fields {
                out.extend_from_slice(&(name.len() as u64).to_le_bytes());
                out.extend_from_slice(name.as_bytes());
                encode_canonical_value(field_val, out);
            }
        }
        CanonicalValue::Enum {
            type_id,
            variant_tag,
            payload,
        } => {
            out.push(6);
            out.extend_from_slice(type_id.as_bytes());
            out.extend_from_slice(&variant_tag.to_le_bytes());
            encode_canonical_value(payload, out);
        }
        CanonicalValue::Map(entries) => {
            out.push(7);
            out.extend_from_slice(&(entries.len() as u64).to_le_bytes());
            for (k, v) in entries {
                encode_canonical_value(k, out);
                encode_canonical_value(v, out);
            }
        }
        CanonicalValue::Box(inner) => {
            out.push(8);
            encode_canonical_value(inner, out);
        }
    }
}

/// Validates that a `CanonicalValue` satisfies all runtime contracts,
/// specifically checking Map total ordering without float keys or duplicates.
pub fn validate_canonical_value(val: &CanonicalValue) -> Result<(), ValueValidationError> {
    match val {
        CanonicalValue::Map(entries) => {
            for i in 0..entries.len() {
                let (k, v) = &entries[i];
                validate_canonical_value(k)?;
                validate_canonical_value(v)?;

                if i > 0 {
                    let prev_key = &entries[i - 1].0;
                    match prev_key.ecs_key_cmp(k) {
                        Some(Ordering::Less) => {}
                        Some(Ordering::Equal) => return Err(ValueValidationError::DuplicateMapKey),
                        Some(Ordering::Greater) => {
                            return Err(ValueValidationError::NonAscendingMapKeys)
                        }
                        None => return Err(ValueValidationError::FloatMapKeyForbidden),
                    }
                } else if k.ecs_key_cmp(k).is_none() {
                    return Err(ValueValidationError::FloatMapKeyForbidden);
                }
            }
            Ok(())
        }
        CanonicalValue::Array(items) | CanonicalValue::Tuple(items) => {
            for item in items {
                validate_canonical_value(item)?;
            }
            Ok(())
        }
        CanonicalValue::Struct { fields, .. } => {
            for (_, field_val) in fields {
                validate_canonical_value(field_val)?;
            }
            Ok(())
        }
        CanonicalValue::Enum { payload, .. } => validate_canonical_value(payload),
        CanonicalValue::Box(inner) => validate_canonical_value(inner),
        _ => Ok(()),
    }
}

/// Packages a `CanonicalValue` into an `ARCHEVAL` v1 64-byte directory envelope container.
#[must_use]
pub fn serialize_archeval_container(type_id: TypeId, val: &CanonicalValue) -> Vec<u8> {
    let mut payload = Vec::new();
    encode_canonical_value(val, &mut payload);

    let mut buffer = Vec::new();
    // 1. 64-byte header
    buffer.extend_from_slice(ARCHEVAL_MAGIC);
    buffer.extend_from_slice(&ARCHEVAL_VERSION.to_le_bytes());
    buffer.extend_from_slice(&HEADER_SIZE.to_le_bytes());
    buffer.extend_from_slice(&0u64.to_le_bytes()); // flags
    buffer.resize(64, 0);

    // 2. Sections
    let val_header_offset = buffer.len() as u64;
    let mut val_header = Vec::new();
    val_header.extend_from_slice(type_id.as_bytes());
    val_header.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    val_header.extend_from_slice(&0u64.to_le_bytes()); // flags
    buffer.extend_from_slice(&val_header);

    let payload_offset = buffer.len() as u64;
    buffer.extend_from_slice(&payload);

    // 3. Directory Table
    let dir_offset = buffer.len() as u64;
    let dir_count = 2u64;

    // Entry 1: .value_header
    let mut e1 = [0u8; 64];
    e1[..13].copy_from_slice(b".value_header");
    e1[24..32].copy_from_slice(&val_header_offset.to_le_bytes());
    e1[32..40].copy_from_slice(&(val_header.len() as u64).to_le_bytes());
    buffer.extend_from_slice(&e1);

    // Entry 2: .payload
    let mut e2 = [0u8; 64];
    e2[..8].copy_from_slice(b".payload");
    e2[24..32].copy_from_slice(&payload_offset.to_le_bytes());
    e2[32..40].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    buffer.extend_from_slice(&e2);

    let total_len = buffer.len() as u64;
    buffer[24..32].copy_from_slice(&total_len.to_le_bytes());
    buffer[32..40].copy_from_slice(&dir_offset.to_le_bytes());
    buffer[40..48].copy_from_slice(&dir_count.to_le_bytes());
    buffer[48..56].copy_from_slice(&DIRECTORY_ENTRY_SIZE.to_le_bytes());

    buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archeval_envelope_structure() {
        let tid = TypeId::from_bytes([42; 16]);
        let val = CanonicalValue::Scalar(CanonicalScalar::I32(100));
        let bytes = serialize_archeval_container(tid, &val);

        assert_eq!(&bytes[0..8], ARCHEVAL_MAGIC);
        assert_eq!(&bytes[8..12], &1u32.to_le_bytes());
        assert_eq!(&bytes[12..16], &64u32.to_le_bytes());
    }

    #[test]
    fn map_validation_rejects_floats_and_duplicates() {
        let valid_map = CanonicalValue::Map(vec![
            (
                CanonicalValue::Scalar(CanonicalScalar::I32(1)),
                CanonicalValue::String("one".into()),
            ),
            (
                CanonicalValue::Scalar(CanonicalScalar::I32(2)),
                CanonicalValue::String("two".into()),
            ),
        ]);
        assert_eq!(validate_canonical_value(&valid_map), Ok(()));

        let duplicate_map = CanonicalValue::Map(vec![
            (
                CanonicalValue::Scalar(CanonicalScalar::I32(1)),
                CanonicalValue::String("one".into()),
            ),
            (
                CanonicalValue::Scalar(CanonicalScalar::I32(1)),
                CanonicalValue::String("dup".into()),
            ),
        ]);
        assert_eq!(
            validate_canonical_value(&duplicate_map),
            Err(ValueValidationError::DuplicateMapKey)
        );

        let float_map = CanonicalValue::Map(vec![(
            CanonicalValue::Scalar(CanonicalScalar::F32(1.0)),
            CanonicalValue::String("float".into()),
        )]);
        assert_eq!(
            validate_canonical_value(&float_map),
            Err(ValueValidationError::FloatMapKeyForbidden)
        );
    }
}
