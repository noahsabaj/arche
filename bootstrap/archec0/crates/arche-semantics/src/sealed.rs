//! Closed compiler evidence that C2 may derive without an ordinary impl row.
//!
//! These values are session-only checking facts. They are deliberately not
//! stable identities, interface rows, callable bodies, or Generic Core trait
//! selections.

use arche_frontend::{GenericArgumentShape, Mutability, SymbolicType};

/// Compiler-known operator traits that participate in the closed primitive
/// matrix. `LogicalNot` is retained so callers can ask the complete question;
/// the matrix intentionally never returns evidence for it.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PrimitiveOperatorTrait {
    Neg,
    LogicalNot,
    BitNot,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    ShiftLeft,
    ShiftRight,
    BitAnd,
    BitXor,
    BitOr,
    Eq,
    Ord,
}

impl PrimitiveOperatorTrait {
    const fn explicit_arity(self) -> usize {
        match self {
            Self::Neg | Self::LogicalNot | Self::BitNot => 2,
            Self::Eq | Self::Ord => 2,
            Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Rem
            | Self::ShiftLeft
            | Self::ShiftRight
            | Self::BitAnd
            | Self::BitXor
            | Self::BitOr => 3,
        }
    }
}

/// The primitive domain used by one sealed operator selection.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PrimitiveDomain {
    Unit,
    Bool,
    SignedInteger,
    UnsignedInteger,
    Float,
    Char,
    Entity,
    RawPointer,
}

/// Exact bodyless evidence for one admitted primitive operator obligation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SealedPrimitiveOperator {
    trait_kind: PrimitiveOperatorTrait,
    self_type: SymbolicType,
    arguments: Vec<SymbolicType>,
    domain: PrimitiveDomain,
}

impl SealedPrimitiveOperator {
    pub const fn trait_kind(&self) -> PrimitiveOperatorTrait {
        self.trait_kind
    }

    pub const fn self_type(&self) -> &SymbolicType {
        &self.self_type
    }

    pub fn arguments(&self) -> &[SymbolicType] {
        &self.arguments
    }

    pub const fn domain(&self) -> PrimitiveDomain {
        self.domain
    }
}

/// Selects the exact sealed primitive row for a compiler-trait obligation.
///
/// `arguments` are the trait's explicit source arguments; contextual `Self` is
/// supplied separately. Non-type arguments, the wrong arity/designated-Self
/// relation, or any nonmatrix pair return `None` and must continue through the
/// ordinary solver (or ultimately become `TRAIT002`).
pub fn select_sealed_primitive_operator(
    trait_kind: PrimitiveOperatorTrait,
    self_type: &SymbolicType,
    arguments: &[GenericArgumentShape],
) -> Option<SealedPrimitiveOperator> {
    if arguments.len() != trait_kind.explicit_arity() {
        return None;
    }
    let arguments = arguments
        .iter()
        .map(|argument| match argument {
            GenericArgumentShape::Type(ty) => Some(ty.clone()),
            GenericArgumentShape::Lifetime(_) | GenericArgumentShape::IntegerConst(_) => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let first = arguments.first()?;
    if first != self_type {
        return None;
    }

    let domain = primitive_domain(first)?;
    let admitted = match trait_kind {
        PrimitiveOperatorTrait::Neg => {
            arguments.as_slice() == [first.clone(), first.clone()]
                && matches!(
                    domain,
                    PrimitiveDomain::SignedInteger | PrimitiveDomain::Float
                )
        }
        PrimitiveOperatorTrait::LogicalNot => false,
        PrimitiveOperatorTrait::BitNot => {
            arguments.as_slice() == [first.clone(), first.clone()]
                && matches!(
                    domain,
                    PrimitiveDomain::SignedInteger | PrimitiveDomain::UnsignedInteger
                )
        }
        PrimitiveOperatorTrait::Add
        | PrimitiveOperatorTrait::Sub
        | PrimitiveOperatorTrait::Mul
        | PrimitiveOperatorTrait::Div => {
            arguments.as_slice() == [first.clone(), first.clone(), first.clone()]
                && matches!(
                    domain,
                    PrimitiveDomain::SignedInteger
                        | PrimitiveDomain::UnsignedInteger
                        | PrimitiveDomain::Float
                )
        }
        PrimitiveOperatorTrait::Rem
        | PrimitiveOperatorTrait::ShiftLeft
        | PrimitiveOperatorTrait::ShiftRight
        | PrimitiveOperatorTrait::BitAnd
        | PrimitiveOperatorTrait::BitXor
        | PrimitiveOperatorTrait::BitOr => {
            arguments.as_slice() == [first.clone(), first.clone(), first.clone()]
                && matches!(
                    domain,
                    PrimitiveDomain::SignedInteger | PrimitiveDomain::UnsignedInteger
                )
        }
        PrimitiveOperatorTrait::Eq => {
            arguments.as_slice() == [first.clone(), first.clone()]
                && (is_primitive_key_domain(domain) || domain == PrimitiveDomain::RawPointer)
        }
        PrimitiveOperatorTrait::Ord => {
            arguments.as_slice() == [first.clone(), first.clone()]
                && is_primitive_key_domain(domain)
        }
    };
    admitted.then_some(SealedPrimitiveOperator {
        trait_kind,
        self_type: self_type.clone(),
        arguments,
        domain,
    })
}

fn primitive_domain(ty: &SymbolicType) -> Option<PrimitiveDomain> {
    Some(match ty {
        SymbolicType::I8
        | SymbolicType::I16
        | SymbolicType::I32
        | SymbolicType::I64
        | SymbolicType::Isize => PrimitiveDomain::SignedInteger,
        SymbolicType::U8
        | SymbolicType::U16
        | SymbolicType::U32
        | SymbolicType::U64
        | SymbolicType::Usize => PrimitiveDomain::UnsignedInteger,
        SymbolicType::F32 | SymbolicType::F64 => PrimitiveDomain::Float,
        SymbolicType::Bool => PrimitiveDomain::Bool,
        SymbolicType::Char => PrimitiveDomain::Char,
        SymbolicType::Entity => PrimitiveDomain::Entity,
        SymbolicType::Unit => PrimitiveDomain::Unit,
        SymbolicType::RawPointer { .. } => PrimitiveDomain::RawPointer,
        SymbolicType::Never
        | SymbolicType::Str
        | SymbolicType::Slice(_)
        | SymbolicType::Array { .. }
        | SymbolicType::Tuple(_)
        | SymbolicType::Reference { .. }
        | SymbolicType::NominalPath { .. }
        | SymbolicType::FunctionPointer { .. }
        | SymbolicType::BoundType { .. }
        | SymbolicType::Closure { .. }
        | SymbolicType::Generator { .. }
        | SymbolicType::JoinHandle { .. }
        | SymbolicType::GeneratorFactory { .. } => return None,
    })
}

const fn is_primitive_key_domain(domain: PrimitiveDomain) -> bool {
    matches!(
        domain,
        PrimitiveDomain::Unit
            | PrimitiveDomain::Bool
            | PrimitiveDomain::SignedInteger
            | PrimitiveDomain::UnsignedInteger
            | PrimitiveDomain::Char
            | PrimitiveDomain::Entity
    )
}

/// Closed base facts from which C2 may construct `SealedCopy` evidence.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SealedCopyBase {
    Scalar,
    Unit,
    Never,
    SharedReference,
    RawPointer,
    FunctionPointer,
}

/// Exact recursive compiler proof for the closed subset of `Copy`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SealedCopyProof {
    Base(SealedCopyBase),
    Array(Box<SealedCopyProof>),
    Tuple(Vec<SealedCopyProof>),
}

/// Derives only the compiler-sealed portion of Copy.
///
/// Bound witnesses and ordinary user nominal impls belong to the trait solver
/// and intentionally return `None` here. Array length zero does not waive the
/// element proof.
pub fn derive_sealed_copy(ty: &SymbolicType) -> Option<SealedCopyProof> {
    let proof = match ty {
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
        | SymbolicType::Entity => SealedCopyProof::Base(SealedCopyBase::Scalar),
        SymbolicType::Unit => SealedCopyProof::Base(SealedCopyBase::Unit),
        SymbolicType::Never => SealedCopyProof::Base(SealedCopyBase::Never),
        SymbolicType::Reference {
            mutability: Mutability::Shared,
            ..
        } => SealedCopyProof::Base(SealedCopyBase::SharedReference),
        SymbolicType::RawPointer { .. } => SealedCopyProof::Base(SealedCopyBase::RawPointer),
        SymbolicType::FunctionPointer { .. } => {
            SealedCopyProof::Base(SealedCopyBase::FunctionPointer)
        }
        SymbolicType::Array { element, .. } => {
            SealedCopyProof::Array(Box::new(derive_sealed_copy(element)?))
        }
        SymbolicType::Tuple(elements) => SealedCopyProof::Tuple(
            elements
                .iter()
                .map(derive_sealed_copy)
                .collect::<Option<Vec<_>>>()?,
        ),
        SymbolicType::Reference {
            mutability: Mutability::Mutable,
            ..
        }
        | SymbolicType::Str
        | SymbolicType::Slice(_)
        | SymbolicType::NominalPath { .. }
        | SymbolicType::BoundType { .. }
        | SymbolicType::Closure { .. }
        | SymbolicType::Generator { .. }
        | SymbolicType::JoinHandle { .. }
        | SymbolicType::GeneratorFactory { .. } => return None,
    };
    Some(proof)
}

#[cfg(test)]
mod tests {
    use arche_frontend::{
        IntegerType, SymbolicConstExpression, SymbolicConstNode, SymbolicLifetime,
        SymbolicTypeEffectSet,
    };

    use super::*;

    fn type_arguments(types: &[SymbolicType]) -> Vec<GenericArgumentShape> {
        types
            .iter()
            .cloned()
            .map(GenericArgumentShape::Type)
            .collect()
    }

    fn select(kind: PrimitiveOperatorTrait, types: &[SymbolicType]) -> bool {
        select_sealed_primitive_operator(kind, &types[0], &type_arguments(types)).is_some()
    }

    #[test]
    fn sealed_primitive_matrix_accepts_every_admitted_family() {
        for ty in [SymbolicType::I8, SymbolicType::I32, SymbolicType::I64] {
            assert!(select(
                PrimitiveOperatorTrait::Neg,
                &[ty.clone(), ty.clone()]
            ));
        }
        for ty in [SymbolicType::F32, SymbolicType::F64] {
            assert!(select(
                PrimitiveOperatorTrait::Neg,
                &[ty.clone(), ty.clone()]
            ));
            for kind in [
                PrimitiveOperatorTrait::Add,
                PrimitiveOperatorTrait::Sub,
                PrimitiveOperatorTrait::Mul,
                PrimitiveOperatorTrait::Div,
            ] {
                assert!(select(kind, &[ty.clone(), ty.clone(), ty.clone()]));
            }
        }
        for ty in [SymbolicType::I32, SymbolicType::U64, SymbolicType::Usize] {
            assert!(select(
                PrimitiveOperatorTrait::BitNot,
                &[ty.clone(), ty.clone()]
            ));
            for kind in [
                PrimitiveOperatorTrait::Add,
                PrimitiveOperatorTrait::Sub,
                PrimitiveOperatorTrait::Mul,
                PrimitiveOperatorTrait::Div,
                PrimitiveOperatorTrait::Rem,
                PrimitiveOperatorTrait::ShiftLeft,
                PrimitiveOperatorTrait::ShiftRight,
                PrimitiveOperatorTrait::BitAnd,
                PrimitiveOperatorTrait::BitXor,
                PrimitiveOperatorTrait::BitOr,
            ] {
                assert!(select(kind, &[ty.clone(), ty.clone(), ty.clone()]));
            }
        }
        for ty in [
            SymbolicType::Unit,
            SymbolicType::Bool,
            SymbolicType::I32,
            SymbolicType::U32,
            SymbolicType::Char,
            SymbolicType::Entity,
        ] {
            assert!(select(
                PrimitiveOperatorTrait::Eq,
                &[ty.clone(), ty.clone()]
            ));
            assert!(select(
                PrimitiveOperatorTrait::Ord,
                &[ty.clone(), ty.clone()]
            ));
        }
        let pointer = SymbolicType::RawPointer {
            mutability: Mutability::Mutable,
            pointee: Box::new(SymbolicType::I32),
        };
        assert!(select(
            PrimitiveOperatorTrait::Eq,
            &[pointer.clone(), pointer]
        ));
    }

    #[test]
    fn sealed_primitive_matrix_rejects_every_required_near_miss() {
        assert!(!select(
            PrimitiveOperatorTrait::Neg,
            &[SymbolicType::U32, SymbolicType::U32]
        ));
        assert!(!select(
            PrimitiveOperatorTrait::Rem,
            &[SymbolicType::F32, SymbolicType::F32, SymbolicType::F32]
        ));
        assert!(!select(
            PrimitiveOperatorTrait::BitAnd,
            &[SymbolicType::Bool, SymbolicType::Bool, SymbolicType::Bool]
        ));
        assert!(!select(
            PrimitiveOperatorTrait::LogicalNot,
            &[SymbolicType::Bool, SymbolicType::Bool]
        ));
        assert!(!select(
            PrimitiveOperatorTrait::Eq,
            &[SymbolicType::F64, SymbolicType::F64]
        ));
        assert!(!select(
            PrimitiveOperatorTrait::Ord,
            &[SymbolicType::F64, SymbolicType::F64]
        ));
        assert!(!select(
            PrimitiveOperatorTrait::Add,
            &[SymbolicType::I32, SymbolicType::U32, SymbolicType::I32]
        ));
        let pointer = SymbolicType::RawPointer {
            mutability: Mutability::Shared,
            pointee: Box::new(SymbolicType::I32),
        };
        assert!(!select(
            PrimitiveOperatorTrait::Ord,
            &[pointer.clone(), pointer]
        ));
    }

    #[test]
    fn primitive_selection_validates_designated_self_arity_and_argument_kind() {
        let types = type_arguments(&[SymbolicType::I32, SymbolicType::I32, SymbolicType::I32]);
        assert!(select_sealed_primitive_operator(
            PrimitiveOperatorTrait::Add,
            &SymbolicType::U32,
            &types,
        )
        .is_none());
        assert!(select_sealed_primitive_operator(
            PrimitiveOperatorTrait::Add,
            &SymbolicType::I32,
            &types[..2],
        )
        .is_none());
        let mut wrong_kind = types;
        wrong_kind[1] = GenericArgumentShape::Lifetime(SymbolicLifetime::Static);
        assert!(select_sealed_primitive_operator(
            PrimitiveOperatorTrait::Add,
            &SymbolicType::I32,
            &wrong_kind,
        )
        .is_none());
    }

    #[test]
    fn sealed_copy_pins_base_and_recursive_positive_rows() {
        for ty in [
            SymbolicType::I32,
            SymbolicType::F64,
            SymbolicType::Bool,
            SymbolicType::Char,
            SymbolicType::Entity,
            SymbolicType::Unit,
            SymbolicType::Never,
            SymbolicType::Reference {
                mutability: Mutability::Shared,
                lifetime: SymbolicLifetime::Static,
                pointee: Box::new(SymbolicType::Str),
            },
            SymbolicType::RawPointer {
                mutability: Mutability::Mutable,
                pointee: Box::new(SymbolicType::Str),
            },
            SymbolicType::FunctionPointer {
                unsafe_: true,
                parameters: vec![SymbolicType::I32],
                result: Box::new(SymbolicType::I32),
                requires: SymbolicTypeEffectSet::pending_c4(vec![SymbolicType::Str]),
                throws: SymbolicTypeEffectSet::pending_c4(vec![SymbolicType::Str]),
            },
        ] {
            assert!(derive_sealed_copy(&ty).is_some(), "{ty:?}");
        }

        let array = |value: u8| SymbolicType::Array {
            element: Box::new(SymbolicType::I32),
            length: SymbolicConstExpression {
                integer_type: IntegerType::Usize,
                node: SymbolicConstNode::IntegerLiteral(vec![value, 0, 0, 0, 0, 0, 0, 0]),
            },
        };
        assert!(matches!(
            derive_sealed_copy(&array(0)),
            Some(SealedCopyProof::Array(_))
        ));
        assert!(matches!(
            derive_sealed_copy(&array(3)),
            Some(SealedCopyProof::Array(_))
        ));
        assert!(matches!(
            derive_sealed_copy(&SymbolicType::Tuple(vec![
                SymbolicType::I32,
                SymbolicType::Bool,
            ])),
            Some(SealedCopyProof::Tuple(children)) if children.len() == 2
        ));
    }

    #[test]
    fn sealed_copy_rejects_mutable_reference_unsized_bound_and_nominal_rows() {
        let rejected = [
            SymbolicType::Reference {
                mutability: Mutability::Mutable,
                lifetime: SymbolicLifetime::Static,
                pointee: Box::new(SymbolicType::I32),
            },
            SymbolicType::Slice(Box::new(SymbolicType::I32)),
            SymbolicType::Str,
            SymbolicType::BoundType { depth: 0, index: 0 },
            SymbolicType::Tuple(vec![SymbolicType::I32, SymbolicType::Str]),
        ];
        for ty in rejected {
            assert!(derive_sealed_copy(&ty).is_none(), "{ty:?}");
        }
    }
}
