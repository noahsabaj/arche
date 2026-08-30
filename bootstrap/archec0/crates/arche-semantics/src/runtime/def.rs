//! Core data definitions for runtime values, reentrant WorldContext, and OBS3 (M27-E).

use std::cmp::Ordering;

use arche_foundation::identity::TypeId;

/// Strongly typed scalar runtime value.
#[derive(Clone, Debug, PartialEq)]
pub enum CanonicalScalar {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    Isize(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    Usize(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    Char(char),
}

impl CanonicalScalar {
    /// Compares two scalar keys under sealed `EcsKey` total ordering.
    /// Rejects floating-point keys (f32, f64) with `None`.
    #[must_use]
    pub fn ecs_key_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::I8(a), Self::I8(b)) => Some(a.cmp(b)),
            (Self::I16(a), Self::I16(b)) => Some(a.cmp(b)),
            (Self::I32(a), Self::I32(b)) => Some(a.cmp(b)),
            (Self::I64(a), Self::I64(b)) => Some(a.cmp(b)),
            (Self::Isize(a), Self::Isize(b)) => Some(a.cmp(b)),
            (Self::U8(a), Self::U8(b)) => Some(a.cmp(b)),
            (Self::U16(a), Self::U16(b)) => Some(a.cmp(b)),
            (Self::U32(a), Self::U32(b)) => Some(a.cmp(b)),
            (Self::U64(a), Self::U64(b)) => Some(a.cmp(b)),
            (Self::Usize(a), Self::Usize(b)) => Some(a.cmp(b)),
            (Self::Bool(a), Self::Bool(b)) => Some(a.cmp(b)),
            (Self::Char(a), Self::Char(b)) => Some(a.cmp(b)),
            // Floats are explicitly rejected as ECS map keys
            (Self::F32(_), _) | (Self::F64(_), _) | (_, Self::F32(_)) | (_, Self::F64(_)) => None,
            _ => None, // Mismatched scalar kinds
        }
    }
}

/// A logical Canonical Value v1 in the Arche runtime.
#[derive(Clone, Debug, PartialEq)]
pub enum CanonicalValue {
    Unit,
    Scalar(CanonicalScalar),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<CanonicalValue>),
    Tuple(Vec<CanonicalValue>),
    Struct {
        type_id: TypeId,
        fields: Vec<(String, CanonicalValue)>,
    },
    Enum {
        type_id: TypeId,
        variant_tag: u32,
        payload: Box<CanonicalValue>,
    },
    Map(Vec<(CanonicalValue, CanonicalValue)>),
    Box(Box<CanonicalValue>),
}

impl CanonicalValue {
    /// Compares two canonical values under sealed `EcsKey` total ordering rules.
    /// Returns `None` if keys contain floats or incompatible structures.
    #[must_use]
    pub fn ecs_key_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Unit, Self::Unit) => Some(Ordering::Equal),
            (Self::Scalar(a), Self::Scalar(b)) => a.ecs_key_cmp(b),
            (Self::String(a), Self::String(b)) => Some(a.cmp(b)),
            (Self::Bytes(a), Self::Bytes(b)) => Some(a.cmp(b)),
            (Self::Tuple(a), Self::Tuple(b)) => {
                if a.len() != b.len() {
                    return Some(a.len().cmp(&b.len()));
                }
                for (x, y) in a.iter().zip(b.iter()) {
                    match x.ecs_key_cmp(y) {
                        Some(Ordering::Equal) => continue,
                        non_eq => return non_eq,
                    }
                }
                Some(Ordering::Equal)
            }
            // Maps cannot contain float-based or invalid compound keys
            _ => None,
        }
    }
}

/// Handle to an entity within a WorldContext.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntityHandle {
    pub slot_index: u32,
    pub generation: u32,
}

/// Description of a materialized archetype table in the ECS.
#[derive(Clone, Debug, PartialEq)]
pub struct ArchetypeTable {
    pub table_ordinal: u32,
    pub component_type_ids: Vec<TypeId>,
    pub entity_handles: Vec<EntityHandle>,
    pub birth_ordinals: Vec<u64>,
}
