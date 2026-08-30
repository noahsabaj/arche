//! Cryptographic dual-receipt generator (CTFE-RESULT, CTFE-TRACE) for M27-C5.

use std::fmt;

use crate::ctfe::def::{CtfeScalar, CtfeValue};

pub const CTFE_RESULT_PREFIX: &[u8] = b"ARCHE-CTFE-RESULT\0\x02\x00\x00\x00";
pub const CTFE_TRACE_PREFIX: &[u8] = b"ARCHE-CTFE-TRACE\0\x02\x00\x00\x00";

/// A cryptographic receipt verifying the hermetic result and execution trace of a CTFE evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CtfeReceipt {
    pub result_digest: [u8; 16],
    pub trace_digest: [u8; 16],
    pub steps_used: u64,
}

impl fmt::Display for CtfeReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "CtfeReceipt(result=")?;
        for b in &self.result_digest {
            write!(formatter, "{:02x}", b)?;
        }
        write!(formatter, ", trace=")?;
        for b in &self.trace_digest {
            write!(formatter, "{:02x}", b)?;
        }
        write!(formatter, ", steps={})", self.steps_used)
    }
}

/// Serializes a CTFE value into canonical preimage bytes.
pub fn serialize_ctfe_value(value: &CtfeValue, output: &mut Vec<u8>) {
    match value {
        CtfeValue::Scalar(scalar) => match scalar {
            CtfeScalar::I8(v) => {
                output.push(1);
                output.extend_from_slice(&v.to_le_bytes());
            }
            CtfeScalar::I16(v) => {
                output.push(2);
                output.extend_from_slice(&v.to_le_bytes());
            }
            CtfeScalar::I32(v) => {
                output.push(3);
                output.extend_from_slice(&v.to_le_bytes());
            }
            CtfeScalar::I64(v) => {
                output.push(4);
                output.extend_from_slice(&v.to_le_bytes());
            }
            CtfeScalar::U8(v) => {
                output.push(5);
                output.extend_from_slice(&v.to_le_bytes());
            }
            CtfeScalar::U16(v) => {
                output.push(6);
                output.extend_from_slice(&v.to_le_bytes());
            }
            CtfeScalar::U32(v) => {
                output.push(7);
                output.extend_from_slice(&v.to_le_bytes());
            }
            CtfeScalar::U64(v) => {
                output.push(8);
                output.extend_from_slice(&v.to_le_bytes());
            }
            CtfeScalar::Isize(v) => {
                output.push(9);
                output.extend_from_slice(&v.to_le_bytes());
            }
            CtfeScalar::Usize(v) => {
                output.push(10);
                output.extend_from_slice(&v.to_le_bytes());
            }
            CtfeScalar::F32(v) => {
                output.push(11);
                output.extend_from_slice(&v.to_bits().to_le_bytes());
            }
            CtfeScalar::F64(v) => {
                output.push(12);
                output.extend_from_slice(&v.to_bits().to_le_bytes());
            }
            CtfeScalar::Bool(v) => {
                output.push(13);
                output.push(if *v { 1 } else { 0 });
            }
            CtfeScalar::Char(v) => {
                output.push(14);
                output.extend_from_slice(&(*v as u32).to_le_bytes());
            }
            CtfeScalar::Unit => {
                output.push(15);
            }
        },
        CtfeValue::String(s) => {
            output.push(16);
            output.extend_from_slice(&(s.len() as u64).to_le_bytes());
            output.extend_from_slice(s.as_bytes());
        }
        CtfeValue::Tuple(fields) => {
            output.push(17);
            output.extend_from_slice(&(fields.len() as u64).to_le_bytes());
            for field in fields {
                serialize_ctfe_value(field, output);
            }
        }
        CtfeValue::Array(elements) => {
            output.push(18);
            output.extend_from_slice(&(elements.len() as u64).to_le_bytes());
            for elem in elements {
                serialize_ctfe_value(elem, output);
            }
        }
        CtfeValue::Struct { name, fields } => {
            output.push(19);
            output.extend_from_slice(&(name.len() as u64).to_le_bytes());
            output.extend_from_slice(name.as_bytes());
            output.extend_from_slice(&(fields.len() as u64).to_le_bytes());
            for (f_name, f_val) in fields {
                output.extend_from_slice(&(f_name.len() as u64).to_le_bytes());
                output.extend_from_slice(f_name.as_bytes());
                serialize_ctfe_value(f_val, output);
            }
        }
        CtfeValue::Enum {
            name,
            variant,
            discriminant,
            payload,
        } => {
            output.push(20);
            output.extend_from_slice(&(name.len() as u64).to_le_bytes());
            output.extend_from_slice(name.as_bytes());
            output.extend_from_slice(&(variant.len() as u64).to_le_bytes());
            output.extend_from_slice(variant.as_bytes());
            output.extend_from_slice(&discriminant.to_le_bytes());
            if let Some(p) = payload {
                output.push(1);
                serialize_ctfe_value(p, output);
            } else {
                output.push(0);
            }
        }
        CtfeValue::HeapRef(alloc_id) => {
            output.push(21);
            output.extend_from_slice(&alloc_id.0.to_le_bytes());
        }
    }
}

/// Generates a deterministic `CtfeReceipt` for an evaluation result and execution event log.
#[must_use]
pub fn compute_ctfe_receipt(value: &CtfeValue, events: &[Vec<u8>], steps_used: u64) -> CtfeReceipt {
    // 1. Result digest
    let mut val_bytes = Vec::new();
    serialize_ctfe_value(value, &mut val_bytes);

    let mut result_hasher = blake3::Hasher::new();
    result_hasher.update(CTFE_RESULT_PREFIX);
    result_hasher.update(&val_bytes);
    let result_full = result_hasher.finalize();
    let mut result_digest = [0u8; 16];
    result_digest.copy_from_slice(&result_full.as_bytes()[..16]);

    // 2. Trace digest
    let mut trace_hasher = blake3::Hasher::new();
    trace_hasher.update(CTFE_TRACE_PREFIX);
    for ev in events {
        trace_hasher.update(&(ev.len() as u32).to_le_bytes());
        trace_hasher.update(ev);
    }
    let trace_full = trace_hasher.finalize();
    let mut trace_digest = [0u8; 16];
    trace_digest.copy_from_slice(&trace_full.as_bytes()[..16]);

    CtfeReceipt {
        result_digest,
        trace_digest,
        steps_used,
    }
}
