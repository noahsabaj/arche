//! Binder-coordinate authority for C2 symbolic types.

use arche_frontend::{
    GeneratorTarget, GenericArgumentShape, GenericParameterKind, SymbolicConstExpression,
    SymbolicConstNode, SymbolicLifetime, SymbolicPredicate, SymbolicType,
};

/// The context in which a trait's implicit designated `Self` is referenced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraitSelfContext {
    TraitDeclaration,
    ImmediatelyOwnedMethod,
}

/// Produces the sole legal bound-type coordinate for a trait's implicit Self.
/// The slot follows all source generic positions but does not change source
/// arity and is never inserted into the source generic vector.
pub fn trait_self_type(source_generic_count: u64, context: TraitSelfContext) -> SymbolicType {
    SymbolicType::BoundType {
        depth: match context {
            TraitSelfContext::TraitDeclaration => 0,
            TraitSelfContext::ImmediatelyOwnedMethod => 1,
        },
        index: source_generic_count,
    }
}

/// One active declaration binder, including the optional implicit trait-Self
/// type slot that follows its source generic positions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BinderFrame {
    source_kinds: Vec<GenericParameterKind>,
    implicit_trait_self: bool,
}

impl BinderFrame {
    pub fn declaration(source_kinds: Vec<GenericParameterKind>) -> Self {
        Self {
            source_kinds,
            implicit_trait_self: false,
        }
    }

    pub fn trait_declaration(source_kinds: Vec<GenericParameterKind>) -> Self {
        Self {
            source_kinds,
            implicit_trait_self: true,
        }
    }

    pub fn source_kinds(&self) -> &[GenericParameterKind] {
        &self.source_kinds
    }

    pub const fn has_implicit_trait_self(&self) -> bool {
        self.implicit_trait_self
    }

    fn kind_at(&self, index: u64) -> Option<GenericParameterKind> {
        let source_len = u64::try_from(self.source_kinds.len()).ok()?;
        if self.implicit_trait_self && index == source_len {
            return Some(GenericParameterKind::Type);
        }
        usize::try_from(index)
            .ok()
            .and_then(|index| self.source_kinds.get(index))
            .cloned()
    }
}

/// Innermost-first active binder stack. Depth zero indexes `frames[0]`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BinderStack {
    frames: Vec<BinderFrame>,
}

impl BinderStack {
    pub fn new(innermost_first: Vec<BinderFrame>) -> Self {
        Self {
            frames: innermost_first,
        }
    }

    pub fn frames(&self) -> &[BinderFrame] {
        &self.frames
    }

    pub fn push_innermost(&mut self, frame: BinderFrame) {
        self.frames.insert(0, frame);
    }

    pub fn pop_innermost(&mut self) -> Option<BinderFrame> {
        (!self.frames.is_empty()).then(|| self.frames.remove(0))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BinderValidationError {
    MissingFrame {
        depth: u64,
    },
    MissingSlot {
        depth: u64,
        index: u64,
    },
    WrongKind {
        depth: u64,
        index: u64,
        expected: GenericParameterKind,
        actual: GenericParameterKind,
    },
    ErasedLocalLifetime,
}

fn require_kind(
    binders: &BinderStack,
    depth: u64,
    index: u64,
    expected: GenericParameterKind,
) -> Result<(), BinderValidationError> {
    let Some(frame) = usize::try_from(depth)
        .ok()
        .and_then(|depth| binders.frames.get(depth))
    else {
        return Err(BinderValidationError::MissingFrame { depth });
    };
    let Some(actual) = frame.kind_at(index) else {
        return Err(BinderValidationError::MissingSlot { depth, index });
    };
    if actual != expected {
        return Err(BinderValidationError::WrongKind {
            depth,
            index,
            expected,
            actual,
        });
    }
    Ok(())
}

pub fn validate_symbolic_lifetime(
    lifetime: &SymbolicLifetime,
    binders: &BinderStack,
) -> Result<(), BinderValidationError> {
    validate_lifetime(lifetime, binders, false)
}

fn validate_lifetime(
    lifetime: &SymbolicLifetime,
    binders: &BinderStack,
    allow_erased_local: bool,
) -> Result<(), BinderValidationError> {
    match lifetime {
        SymbolicLifetime::Static => Ok(()),
        SymbolicLifetime::Bound { depth, index } => {
            require_kind(binders, *depth, *index, GenericParameterKind::Lifetime)
        }
        SymbolicLifetime::ErasedLocal if allow_erased_local => Ok(()),
        SymbolicLifetime::ErasedLocal => Err(BinderValidationError::ErasedLocalLifetime),
    }
}

pub fn validate_symbolic_const(
    value: &SymbolicConstExpression,
    binders: &BinderStack,
) -> Result<(), BinderValidationError> {
    match &value.node {
        SymbolicConstNode::IntegerLiteral(_) | SymbolicConstNode::ConstDefinitionPath(_) => Ok(()),
        SymbolicConstNode::Bound { depth, index } => require_kind(
            binders,
            *depth,
            *index,
            GenericParameterKind::IntegerConst(value.integer_type),
        ),
        SymbolicConstNode::WrappingNeg(child) | SymbolicConstNode::BitNot(child) => {
            validate_symbolic_const(child, binders)
        }
        SymbolicConstNode::WrappingMul(left, right)
        | SymbolicConstNode::IntegerDivide(left, right)
        | SymbolicConstNode::IntegerRemainder(left, right)
        | SymbolicConstNode::WrappingAdd(left, right)
        | SymbolicConstNode::WrappingSub(left, right)
        | SymbolicConstNode::MaskedShiftLeft(left, right)
        | SymbolicConstNode::MaskedShiftRight(left, right)
        | SymbolicConstNode::BitAnd(left, right)
        | SymbolicConstNode::BitXor(left, right)
        | SymbolicConstNode::BitOr(left, right) => {
            validate_symbolic_const(left, binders)?;
            validate_symbolic_const(right, binders)
        }
    }
}

pub fn validate_generic_argument(
    argument: &GenericArgumentShape,
    binders: &BinderStack,
) -> Result<(), BinderValidationError> {
    match argument {
        GenericArgumentShape::Type(ty) => validate_symbolic_type(ty, binders),
        GenericArgumentShape::Lifetime(lifetime) => validate_symbolic_lifetime(lifetime, binders),
        GenericArgumentShape::IntegerConst(value) => validate_symbolic_const(value, binders),
    }
}

pub fn validate_symbolic_predicate(
    predicate: &SymbolicPredicate,
    binders: &BinderStack,
) -> Result<(), BinderValidationError> {
    match predicate {
        SymbolicPredicate::Trait {
            self_type,
            arguments,
            ..
        } => {
            validate_symbolic_type(self_type, binders)?;
            for argument in arguments {
                validate_generic_argument(argument, binders)?;
            }
            Ok(())
        }
        SymbolicPredicate::LifetimeOutlives { longer, shorter } => {
            validate_symbolic_lifetime(longer, binders)?;
            validate_symbolic_lifetime(shorter, binders)
        }
        SymbolicPredicate::TypeOutlives { ty, lifetime } => {
            validate_symbolic_type(ty, binders)?;
            validate_symbolic_lifetime(lifetime, binders)
        }
    }
}

pub fn validate_symbolic_type(
    ty: &SymbolicType,
    binders: &BinderStack,
) -> Result<(), BinderValidationError> {
    validate_symbolic_type_with_local_lifetimes(ty, binders, false)
}

/// Validates a type used inside a checked body. Body-local region origins are
/// represented by `ErasedLocal`; declaration identity inputs continue to use
/// [`validate_symbolic_type`] and reject that marker.
pub fn validate_body_symbolic_type(
    ty: &SymbolicType,
    binders: &BinderStack,
) -> Result<(), BinderValidationError> {
    validate_symbolic_type_with_local_lifetimes(ty, binders, true)
}

fn validate_symbolic_type_with_local_lifetimes(
    ty: &SymbolicType,
    binders: &BinderStack,
    allow_erased_local: bool,
) -> Result<(), BinderValidationError> {
    match ty {
        SymbolicType::BoundType { depth, index } => {
            require_kind(binders, *depth, *index, GenericParameterKind::Type)
        }
        SymbolicType::Slice(element)
        | SymbolicType::RawPointer {
            pointee: element, ..
        } => validate_symbolic_type_with_local_lifetimes(element, binders, allow_erased_local),
        SymbolicType::Array { element, length } => {
            validate_symbolic_type_with_local_lifetimes(element, binders, allow_erased_local)?;
            validate_symbolic_const(length, binders)
        }
        SymbolicType::Tuple(elements) => {
            for element in elements {
                validate_symbolic_type_with_local_lifetimes(element, binders, allow_erased_local)?;
            }
            Ok(())
        }
        SymbolicType::Reference {
            lifetime, pointee, ..
        } => {
            validate_lifetime(lifetime, binders, allow_erased_local)?;
            validate_symbolic_type_with_local_lifetimes(pointee, binders, allow_erased_local)
        }
        SymbolicType::NominalPath { arguments, .. } => {
            validate_arguments_with_local_lifetimes(arguments, binders, allow_erased_local)
        }
        SymbolicType::FunctionPointer {
            parameters,
            result,
            requires,
            throws,
            ..
        } => {
            validate_types_with_local_lifetimes(parameters, binders, allow_erased_local)?;
            validate_symbolic_type_with_local_lifetimes(result, binders, allow_erased_local)?;
            validate_types_with_local_lifetimes(requires.members(), binders, allow_erased_local)?;
            validate_types_with_local_lifetimes(throws.members(), binders, allow_erased_local)
        }
        SymbolicType::Closure {
            captures,
            parameters,
            result,
            requires,
            throws,
            arguments,
            ..
        } => {
            for capture in captures {
                validate_symbolic_type_with_local_lifetimes(
                    &capture.ty,
                    binders,
                    allow_erased_local,
                )?;
            }
            validate_types_with_local_lifetimes(parameters, binders, allow_erased_local)?;
            validate_symbolic_type_with_local_lifetimes(result, binders, allow_erased_local)?;
            validate_types_with_local_lifetimes(requires.members(), binders, allow_erased_local)?;
            validate_types_with_local_lifetimes(throws.members(), binders, allow_erased_local)?;
            validate_arguments_with_local_lifetimes(arguments, binders, allow_erased_local)
        }
        SymbolicType::Generator {
            target,
            captures,
            parameters,
            resume,
            yields,
            result,
            requires,
            throws,
            ..
        } => {
            validate_generator_target(target, binders, allow_erased_local)?;
            for capture in captures {
                validate_symbolic_type_with_local_lifetimes(
                    &capture.ty,
                    binders,
                    allow_erased_local,
                )?;
            }
            validate_types_with_local_lifetimes(parameters, binders, allow_erased_local)?;
            validate_symbolic_type_with_local_lifetimes(resume, binders, allow_erased_local)?;
            validate_symbolic_type_with_local_lifetimes(yields, binders, allow_erased_local)?;
            validate_symbolic_type_with_local_lifetimes(result, binders, allow_erased_local)?;
            validate_types_with_local_lifetimes(requires.members(), binders, allow_erased_local)?;
            validate_types_with_local_lifetimes(throws.members(), binders, allow_erased_local)
        }
        SymbolicType::JoinHandle { result, throws } => {
            validate_symbolic_type_with_local_lifetimes(result, binders, allow_erased_local)?;
            validate_types_with_local_lifetimes(throws.members(), binders, allow_erased_local)
        }
        SymbolicType::GeneratorFactory {
            target,
            captures,
            parameters,
            produced_generator,
            ..
        } => {
            validate_generator_target(target, binders, allow_erased_local)?;
            for capture in captures {
                validate_symbolic_type_with_local_lifetimes(
                    &capture.ty,
                    binders,
                    allow_erased_local,
                )?;
            }
            validate_types_with_local_lifetimes(parameters, binders, allow_erased_local)?;
            validate_symbolic_type_with_local_lifetimes(
                produced_generator,
                binders,
                allow_erased_local,
            )
        }
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
        | SymbolicType::Str => Ok(()),
    }
}

fn validate_generator_target(
    target: &GeneratorTarget,
    binders: &BinderStack,
    allow_erased_local: bool,
) -> Result<(), BinderValidationError> {
    let arguments = match target {
        GeneratorTarget::Named { arguments, .. } | GeneratorTarget::Anonymous { arguments, .. } => {
            arguments
        }
    };
    validate_arguments_with_local_lifetimes(arguments, binders, allow_erased_local)
}

fn validate_arguments_with_local_lifetimes(
    arguments: &[GenericArgumentShape],
    binders: &BinderStack,
    allow_erased_local: bool,
) -> Result<(), BinderValidationError> {
    for argument in arguments {
        match argument {
            GenericArgumentShape::Type(ty) => {
                validate_symbolic_type_with_local_lifetimes(ty, binders, allow_erased_local)?
            }
            GenericArgumentShape::Lifetime(lifetime) => {
                validate_lifetime(lifetime, binders, allow_erased_local)?;
            }
            GenericArgumentShape::IntegerConst(value) => validate_symbolic_const(value, binders)?,
        }
    }
    Ok(())
}

fn validate_types_with_local_lifetimes(
    types: &[SymbolicType],
    binders: &BinderStack,
    allow_erased_local: bool,
) -> Result<(), BinderValidationError> {
    for ty in types {
        validate_symbolic_type_with_local_lifetimes(ty, binders, allow_erased_local)?;
    }
    Ok(())
}

impl std::fmt::Display for BinderValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingFrame { depth } => {
                write!(formatter, "no binder frame exists at depth {depth}")
            }
            Self::MissingSlot { depth, index } => write!(
                formatter,
                "no binder slot exists at depth {depth} index {index}"
            ),
            Self::WrongKind {
                depth,
                index,
                expected,
                actual,
            } => write!(
                formatter,
                "binder depth {depth} index {index} must be a {} binder, found {}",
                crate::golden::generic_parameter_prose(expected),
                crate::golden::generic_parameter_prose(actual)
            ),
            Self::ErasedLocalLifetime => {
                formatter.write_str("the erased local lifetime cannot appear in this position")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use arche_frontend::{IntegerType, Mutability, SymbolicLifetime};

    use super::*;

    #[test]
    fn trait_self_zero_and_generic_coordinates_are_exact() {
        assert_eq!(
            trait_self_type(0, TraitSelfContext::TraitDeclaration),
            SymbolicType::BoundType { depth: 0, index: 0 }
        );
        assert_eq!(
            trait_self_type(2, TraitSelfContext::TraitDeclaration),
            SymbolicType::BoundType { depth: 0, index: 2 }
        );
        assert_eq!(
            trait_self_type(2, TraitSelfContext::ImmediatelyOwnedMethod),
            SymbolicType::BoundType { depth: 1, index: 2 }
        );
    }

    #[test]
    fn generic_trait_method_separates_method_trait_and_self_coordinates() {
        let binders = BinderStack::new(vec![
            BinderFrame::declaration(vec![
                GenericParameterKind::Type,
                GenericParameterKind::Lifetime,
            ]),
            BinderFrame::trait_declaration(vec![GenericParameterKind::Type]),
        ]);
        let ty = SymbolicType::Tuple(vec![
            SymbolicType::BoundType { depth: 1, index: 1 },
            SymbolicType::BoundType { depth: 1, index: 0 },
            SymbolicType::BoundType { depth: 0, index: 0 },
            SymbolicType::Reference {
                mutability: Mutability::Shared,
                lifetime: SymbolicLifetime::Bound { depth: 0, index: 1 },
                pointee: Box::new(SymbolicType::BoundType { depth: 1, index: 1 }),
            },
        ]);
        assert_eq!(validate_symbolic_type(&ty, &binders), Ok(()));
    }

    #[test]
    fn validation_rejects_wrong_depth_index_and_kind() {
        let binders = BinderStack::new(vec![BinderFrame::trait_declaration(vec![
            GenericParameterKind::Lifetime,
            GenericParameterKind::IntegerConst(IntegerType::Usize),
        ])]);
        assert_eq!(
            validate_symbolic_type(&SymbolicType::BoundType { depth: 1, index: 2 }, &binders),
            Err(BinderValidationError::MissingFrame { depth: 1 })
        );
        assert_eq!(
            validate_symbolic_type(&SymbolicType::BoundType { depth: 0, index: 3 }, &binders),
            Err(BinderValidationError::MissingSlot { depth: 0, index: 3 })
        );
        assert_eq!(
            validate_symbolic_type(&SymbolicType::BoundType { depth: 0, index: 0 }, &binders),
            Err(BinderValidationError::WrongKind {
                depth: 0,
                index: 0,
                expected: GenericParameterKind::Type,
                actual: GenericParameterKind::Lifetime,
            })
        );
        assert_eq!(
            validate_symbolic_const(
                &SymbolicConstExpression {
                    integer_type: IntegerType::I32,
                    node: SymbolicConstNode::Bound { depth: 0, index: 1 },
                },
                &binders,
            ),
            Err(BinderValidationError::WrongKind {
                depth: 0,
                index: 1,
                expected: GenericParameterKind::IntegerConst(IntegerType::I32),
                actual: GenericParameterKind::IntegerConst(IntegerType::Usize),
            })
        );
    }

    #[test]
    fn erased_local_lifetime_is_never_a_declaration_coordinate() {
        let binders = BinderStack::default();
        assert_eq!(
            validate_symbolic_lifetime(&SymbolicLifetime::ErasedLocal, &binders),
            Err(BinderValidationError::ErasedLocalLifetime)
        );
        let body_reference = SymbolicType::Reference {
            mutability: Mutability::Shared,
            lifetime: SymbolicLifetime::ErasedLocal,
            pointee: Box::new(SymbolicType::I32),
        };
        assert_eq!(
            validate_symbolic_type(&body_reference, &binders),
            Err(BinderValidationError::ErasedLocalLifetime)
        );
        assert_eq!(
            validate_body_symbolic_type(&body_reference, &binders),
            Ok(())
        );
    }
}
