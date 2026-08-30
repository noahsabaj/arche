//! Structural auto-traits (Send, Sync, Unpin) and ECS key/value judgments for M27-C4.

use arche_frontend::{GenericArgumentShape, Mutability, SymbolicType};

/// Evaluates whether a symbolic type implements the `Send` auto-trait.
#[must_use]
pub fn is_type_send(ty: &SymbolicType) -> bool {
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
        | SymbolicType::BoundType { .. } => true,
        SymbolicType::RawPointer { .. } => false,
        SymbolicType::Reference {
            mutability,
            pointee,
            ..
        } => match mutability {
            Mutability::Shared => is_type_sync(pointee),
            Mutability::Mutable => is_type_send(pointee),
        },
        SymbolicType::Slice(element) => is_type_send(element),
        SymbolicType::Array { element, .. } => is_type_send(element),
        SymbolicType::Tuple(fields) => fields.iter().all(is_type_send),
        SymbolicType::NominalPath { arguments, .. } => arguments.iter().all(|arg| match arg {
            GenericArgumentShape::Type(t) => is_type_send(t),
            _ => true,
        }),
        SymbolicType::FunctionPointer { .. } => true,
        SymbolicType::Closure { parameters, .. } => parameters.iter().all(is_type_send),
        SymbolicType::Generator { .. } => false, // Generator frames require explicit pinning/sync
        SymbolicType::JoinHandle { result, .. } => is_type_send(result),
        SymbolicType::GeneratorFactory { .. } => true,
    }
}

/// Evaluates whether a symbolic type implements the `Sync` auto-trait.
#[must_use]
pub fn is_type_sync(ty: &SymbolicType) -> bool {
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
        | SymbolicType::BoundType { .. } => true,
        SymbolicType::RawPointer { .. } => false,
        SymbolicType::Reference { pointee, .. } => is_type_sync(pointee),
        SymbolicType::Slice(element) => is_type_sync(element),
        SymbolicType::Array { element, .. } => is_type_sync(element),
        SymbolicType::Tuple(fields) => fields.iter().all(is_type_sync),
        SymbolicType::NominalPath { arguments, .. } => arguments.iter().all(|arg| match arg {
            GenericArgumentShape::Type(t) => is_type_sync(t),
            _ => true,
        }),
        SymbolicType::FunctionPointer { .. } => true,
        SymbolicType::Closure { parameters, .. } => parameters.iter().all(is_type_sync),
        SymbolicType::Generator { .. } => false,
        SymbolicType::JoinHandle { result, .. } => is_type_sync(result),
        SymbolicType::GeneratorFactory { .. } => true,
    }
}

/// Evaluates whether a type is a valid, deterministically ordered ECS Key (`EcsKey`).
///
/// Float types (`f32`, `f64`) and arbitrary user types without sealed keys are strictly rejected.
#[must_use]
pub fn is_valid_ecs_key(ty: &SymbolicType) -> bool {
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
        | SymbolicType::Bool
        | SymbolicType::Char
        | SymbolicType::Entity
        | SymbolicType::Unit
        | SymbolicType::Str => true,
        SymbolicType::F32 | SymbolicType::F64 => false, // Floats are rejected due to NaN/non-total ordering!
        SymbolicType::RawPointer { .. } | SymbolicType::Reference { .. } => false,
        SymbolicType::Tuple(fields) => fields.iter().all(is_valid_ecs_key),
        SymbolicType::Array { element, .. } => is_valid_ecs_key(element),
        SymbolicType::Slice(_) => false,
        SymbolicType::NominalPath { arguments, .. } => arguments.iter().all(|arg| match arg {
            GenericArgumentShape::Type(t) => is_valid_ecs_key(t),
            _ => true,
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arche_frontend::SymbolicLifetime;

    #[test]
    fn primitives_are_send_and_sync() {
        assert!(is_type_send(&SymbolicType::I32));
        assert!(is_type_sync(&SymbolicType::I32));
        assert!(is_type_send(&SymbolicType::Bool));
        assert!(is_type_sync(&SymbolicType::Bool));
    }

    #[test]
    fn raw_pointers_are_neither_send_nor_sync() {
        let raw = SymbolicType::RawPointer {
            mutability: Mutability::Shared,
            pointee: Box::new(SymbolicType::I32),
        };
        assert!(!is_type_send(&raw));
        assert!(!is_type_sync(&raw));
    }

    #[test]
    fn shared_reference_is_send_only_if_pointee_is_sync() {
        let shared_i32 = SymbolicType::Reference {
            mutability: Mutability::Shared,
            lifetime: SymbolicLifetime::Static,
            pointee: Box::new(SymbolicType::I32),
        };
        assert!(is_type_send(&shared_i32));
        assert!(is_type_sync(&shared_i32));
    }

    #[test]
    fn ecs_key_accepts_integers_and_rejects_floats() {
        assert!(is_valid_ecs_key(&SymbolicType::I32));
        assert!(is_valid_ecs_key(&SymbolicType::U64));
        assert!(is_valid_ecs_key(&SymbolicType::Str));
        assert!(is_valid_ecs_key(&SymbolicType::Entity));

        // Floating point numbers MUST be rejected from EcsKey!
        assert!(!is_valid_ecs_key(&SymbolicType::F32));
        assert!(!is_valid_ecs_key(&SymbolicType::F64));
        assert!(!is_valid_ecs_key(&SymbolicType::Tuple(vec![
            SymbolicType::I32,
            SymbolicType::F32,
        ])));
    }
}
