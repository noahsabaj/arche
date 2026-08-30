//! Generic formation and trait-frame substitution for C2.

use arche_frontend::{
    GeneratorTarget, GenericArgumentShape, GenericParameterKind, SymbolicCapture,
    SymbolicConstExpression, SymbolicConstNode, SymbolicLifetime, SymbolicPredicate,
    SymbolicShapeReadiness, SymbolicType, SymbolicTypeEffectSet,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GenericFormationError {
    Arity {
        expected: usize,
        actual: usize,
    },
    WrongArgumentKind {
        index: usize,
        expected: GenericParameterKind,
        actual: GenericParameterKind,
    },
    MissingSubstitution {
        depth: u64,
        index: u64,
    },
    WrongSubstitutionUse {
        depth: u64,
        index: u64,
        expected: GenericParameterKind,
    },
}

pub fn generic_argument_kind(argument: &GenericArgumentShape) -> GenericParameterKind {
    match argument {
        GenericArgumentShape::Type(_) => GenericParameterKind::Type,
        GenericArgumentShape::Lifetime(_) => GenericParameterKind::Lifetime,
        GenericArgumentShape::IntegerConst(value) => {
            GenericParameterKind::IntegerConst(value.integer_type)
        }
    }
}

pub fn validate_generic_arguments(
    formals: &[GenericParameterKind],
    actuals: &[GenericArgumentShape],
) -> Result<(), GenericFormationError> {
    if formals.len() != actuals.len() {
        return Err(GenericFormationError::Arity {
            expected: formals.len(),
            actual: actuals.len(),
        });
    }
    for (index, (expected, actual)) in formals.iter().zip(actuals).enumerate() {
        let actual = generic_argument_kind(actual);
        if *expected != actual {
            return Err(GenericFormationError::WrongArgumentKind {
                index,
                expected: expected.clone(),
                actual,
            });
        }
    }
    Ok(())
}

/// Capture-avoiding replacement for one trait declaration frame.
///
/// The implicit Self slot is appended only to this semantic substitution and
/// never to the trait's source argument list. Replacements are already lowered
/// in the destination use-site binder context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraitFrameSubstitution {
    source_formals: Vec<GenericParameterKind>,
    explicit_arguments: Vec<GenericArgumentShape>,
    self_type: SymbolicType,
}

impl TraitFrameSubstitution {
    pub fn new(
        source_formals: Vec<GenericParameterKind>,
        explicit_arguments: Vec<GenericArgumentShape>,
        self_type: SymbolicType,
    ) -> Result<Self, GenericFormationError> {
        validate_generic_arguments(&source_formals, &explicit_arguments)?;
        Ok(Self {
            source_formals,
            explicit_arguments,
            self_type,
        })
    }

    pub fn source_formals(&self) -> &[GenericParameterKind] {
        &self.source_formals
    }

    pub fn explicit_arguments(&self) -> &[GenericArgumentShape] {
        &self.explicit_arguments
    }

    pub const fn self_type(&self) -> &SymbolicType {
        &self.self_type
    }

    pub fn substitute_type(
        &self,
        ty: &SymbolicType,
        trait_depth: u64,
    ) -> Result<SymbolicType, GenericFormationError> {
        substitute_type(self, ty, trait_depth)
    }

    pub fn substitute_predicate(
        &self,
        predicate: &SymbolicPredicate,
        trait_depth: u64,
    ) -> Result<SymbolicPredicate, GenericFormationError> {
        substitute_predicate(self, predicate, trait_depth)
    }

    fn argument(
        &self,
        depth: u64,
        index: u64,
    ) -> Result<&GenericArgumentShape, GenericFormationError> {
        let source_len = u64::try_from(self.source_formals.len())
            .map_err(|_| GenericFormationError::MissingSubstitution { depth, index })?;
        if index >= source_len {
            return Err(GenericFormationError::MissingSubstitution { depth, index });
        }
        usize::try_from(index)
            .ok()
            .and_then(|index| self.explicit_arguments.get(index))
            .ok_or(GenericFormationError::MissingSubstitution { depth, index })
    }

    fn implicit_self_index(&self) -> Result<u64, GenericFormationError> {
        u64::try_from(self.source_formals.len()).map_err(|_| {
            GenericFormationError::MissingSubstitution {
                depth: 0,
                index: u64::MAX,
            }
        })
    }
}

fn substitute_type(
    substitution: &TraitFrameSubstitution,
    ty: &SymbolicType,
    trait_depth: u64,
) -> Result<SymbolicType, GenericFormationError> {
    Ok(match ty {
        SymbolicType::BoundType { depth, index } if *depth == trait_depth => {
            if *index == substitution.implicit_self_index()? {
                substitution.self_type.clone()
            } else {
                match substitution.argument(*depth, *index)? {
                    GenericArgumentShape::Type(ty) => ty.clone(),
                    GenericArgumentShape::Lifetime(_) | GenericArgumentShape::IntegerConst(_) => {
                        return Err(GenericFormationError::WrongSubstitutionUse {
                            depth: *depth,
                            index: *index,
                            expected: GenericParameterKind::Type,
                        });
                    }
                }
            }
        }
        SymbolicType::Slice(element) => SymbolicType::Slice(Box::new(substitute_type(
            substitution,
            element,
            trait_depth,
        )?)),
        SymbolicType::Array { element, length } => SymbolicType::Array {
            element: Box::new(substitute_type(substitution, element, trait_depth)?),
            length: substitute_const(substitution, length, trait_depth)?,
        },
        SymbolicType::Tuple(elements) => SymbolicType::Tuple(
            elements
                .iter()
                .map(|ty| substitute_type(substitution, ty, trait_depth))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        SymbolicType::Reference {
            mutability,
            lifetime,
            pointee,
        } => SymbolicType::Reference {
            mutability: *mutability,
            lifetime: substitute_lifetime(substitution, lifetime, trait_depth)?,
            pointee: Box::new(substitute_type(substitution, pointee, trait_depth)?),
        },
        SymbolicType::RawPointer {
            mutability,
            pointee,
        } => SymbolicType::RawPointer {
            mutability: *mutability,
            pointee: Box::new(substitute_type(substitution, pointee, trait_depth)?),
        },
        SymbolicType::NominalPath {
            declaration,
            arguments,
        } => SymbolicType::NominalPath {
            declaration: declaration.clone(),
            arguments: substitute_arguments(substitution, arguments, trait_depth)?,
        },
        SymbolicType::FunctionPointer {
            unsafe_,
            parameters,
            result,
            requires,
            throws,
        } => SymbolicType::FunctionPointer {
            unsafe_: *unsafe_,
            parameters: substitute_types(substitution, parameters, trait_depth)?,
            result: Box::new(substitute_type(substitution, result, trait_depth)?),
            requires: substitute_effect_set(substitution, requires, trait_depth)?,
            throws: substitute_effect_set(substitution, throws, trait_depth)?,
        },
        SymbolicType::Closure {
            owner,
            expression_ordinal,
            captures,
            parameters,
            result,
            requires,
            throws,
            arguments,
        } => SymbolicType::Closure {
            owner: owner.clone(),
            expression_ordinal: *expression_ordinal,
            captures: substitute_captures(substitution, captures, trait_depth)?,
            parameters: substitute_types(substitution, parameters, trait_depth)?,
            result: Box::new(substitute_type(substitution, result, trait_depth)?),
            requires: substitute_effect_set(substitution, requires, trait_depth)?,
            throws: substitute_effect_set(substitution, throws, trait_depth)?,
            arguments: substitute_arguments(substitution, arguments, trait_depth)?,
        },
        SymbolicType::Generator {
            target,
            captures,
            parameters,
            factory_unsafe,
            resume,
            yields,
            result,
            requires,
            throws,
        } => SymbolicType::Generator {
            target: Box::new(substitute_generator_target(
                substitution,
                target,
                trait_depth,
            )?),
            captures: substitute_captures(substitution, captures, trait_depth)?,
            parameters: substitute_types(substitution, parameters, trait_depth)?,
            factory_unsafe: *factory_unsafe,
            resume: Box::new(substitute_type(substitution, resume, trait_depth)?),
            yields: Box::new(substitute_type(substitution, yields, trait_depth)?),
            result: Box::new(substitute_type(substitution, result, trait_depth)?),
            requires: substitute_effect_set(substitution, requires, trait_depth)?,
            throws: substitute_effect_set(substitution, throws, trait_depth)?,
        },
        SymbolicType::JoinHandle { result, throws } => SymbolicType::JoinHandle {
            result: Box::new(substitute_type(substitution, result, trait_depth)?),
            throws: substitute_effect_set(substitution, throws, trait_depth)?,
        },
        SymbolicType::GeneratorFactory {
            target,
            captures,
            call_trait,
            parameters,
            factory_unsafe,
            produced_generator,
        } => SymbolicType::GeneratorFactory {
            target: Box::new(substitute_generator_target(
                substitution,
                target,
                trait_depth,
            )?),
            captures: substitute_captures(substitution, captures, trait_depth)?,
            call_trait: *call_trait,
            parameters: substitute_types(substitution, parameters, trait_depth)?,
            factory_unsafe: *factory_unsafe,
            produced_generator: Box::new(substitute_type(
                substitution,
                produced_generator,
                trait_depth,
            )?),
        },
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
        | SymbolicType::BoundType { .. } => ty.clone(),
    })
}

fn substitute_predicate(
    substitution: &TraitFrameSubstitution,
    predicate: &SymbolicPredicate,
    trait_depth: u64,
) -> Result<SymbolicPredicate, GenericFormationError> {
    Ok(match predicate {
        SymbolicPredicate::Trait {
            trait_path,
            self_type,
            arguments,
        } => SymbolicPredicate::Trait {
            trait_path: trait_path.clone(),
            self_type: substitute_type(substitution, self_type, trait_depth)?,
            arguments: substitute_arguments(substitution, arguments, trait_depth)?,
        },
        SymbolicPredicate::LifetimeOutlives { longer, shorter } => {
            SymbolicPredicate::LifetimeOutlives {
                longer: substitute_lifetime(substitution, longer, trait_depth)?,
                shorter: substitute_lifetime(substitution, shorter, trait_depth)?,
            }
        }
        SymbolicPredicate::TypeOutlives { ty, lifetime } => SymbolicPredicate::TypeOutlives {
            ty: substitute_type(substitution, ty, trait_depth)?,
            lifetime: substitute_lifetime(substitution, lifetime, trait_depth)?,
        },
    })
}

fn substitute_argument(
    substitution: &TraitFrameSubstitution,
    argument: &GenericArgumentShape,
    trait_depth: u64,
) -> Result<GenericArgumentShape, GenericFormationError> {
    Ok(match argument {
        GenericArgumentShape::Type(ty) => {
            GenericArgumentShape::Type(substitute_type(substitution, ty, trait_depth)?)
        }
        GenericArgumentShape::Lifetime(lifetime) => GenericArgumentShape::Lifetime(
            substitute_lifetime(substitution, lifetime, trait_depth)?,
        ),
        GenericArgumentShape::IntegerConst(value) => {
            GenericArgumentShape::IntegerConst(substitute_const(substitution, value, trait_depth)?)
        }
    })
}

fn substitute_lifetime(
    substitution: &TraitFrameSubstitution,
    lifetime: &SymbolicLifetime,
    trait_depth: u64,
) -> Result<SymbolicLifetime, GenericFormationError> {
    match lifetime {
        SymbolicLifetime::Bound { depth, index } if *depth == trait_depth => {
            match substitution.argument(*depth, *index)? {
                GenericArgumentShape::Lifetime(lifetime) => Ok(lifetime.clone()),
                GenericArgumentShape::Type(_) | GenericArgumentShape::IntegerConst(_) => {
                    Err(GenericFormationError::WrongSubstitutionUse {
                        depth: *depth,
                        index: *index,
                        expected: GenericParameterKind::Lifetime,
                    })
                }
            }
        }
        SymbolicLifetime::Static
        | SymbolicLifetime::ErasedLocal
        | SymbolicLifetime::Bound { .. } => Ok(lifetime.clone()),
    }
}

fn substitute_const(
    substitution: &TraitFrameSubstitution,
    value: &SymbolicConstExpression,
    trait_depth: u64,
) -> Result<SymbolicConstExpression, GenericFormationError> {
    let node =
        match &value.node {
            SymbolicConstNode::Bound { depth, index } if *depth == trait_depth => {
                match substitution.argument(*depth, *index)? {
                    GenericArgumentShape::IntegerConst(replacement)
                        if replacement.integer_type == value.integer_type =>
                    {
                        return Ok(replacement.clone());
                    }
                    GenericArgumentShape::IntegerConst(_)
                    | GenericArgumentShape::Type(_)
                    | GenericArgumentShape::Lifetime(_) => {
                        return Err(GenericFormationError::WrongSubstitutionUse {
                            depth: *depth,
                            index: *index,
                            expected: GenericParameterKind::IntegerConst(value.integer_type),
                        });
                    }
                }
            }
            SymbolicConstNode::WrappingNeg(child) => SymbolicConstNode::WrappingNeg(Box::new(
                substitute_const(substitution, child, trait_depth)?,
            )),
            SymbolicConstNode::BitNot(child) => SymbolicConstNode::BitNot(Box::new(
                substitute_const(substitution, child, trait_depth)?,
            )),
            SymbolicConstNode::WrappingMul(left, right) => binary_const(
                SymbolicConstNode::WrappingMul,
                substitution,
                left,
                right,
                trait_depth,
            )?,
            SymbolicConstNode::IntegerDivide(left, right) => binary_const(
                SymbolicConstNode::IntegerDivide,
                substitution,
                left,
                right,
                trait_depth,
            )?,
            SymbolicConstNode::IntegerRemainder(left, right) => binary_const(
                SymbolicConstNode::IntegerRemainder,
                substitution,
                left,
                right,
                trait_depth,
            )?,
            SymbolicConstNode::WrappingAdd(left, right) => binary_const(
                SymbolicConstNode::WrappingAdd,
                substitution,
                left,
                right,
                trait_depth,
            )?,
            SymbolicConstNode::WrappingSub(left, right) => binary_const(
                SymbolicConstNode::WrappingSub,
                substitution,
                left,
                right,
                trait_depth,
            )?,
            SymbolicConstNode::MaskedShiftLeft(left, right) => binary_const(
                SymbolicConstNode::MaskedShiftLeft,
                substitution,
                left,
                right,
                trait_depth,
            )?,
            SymbolicConstNode::MaskedShiftRight(left, right) => binary_const(
                SymbolicConstNode::MaskedShiftRight,
                substitution,
                left,
                right,
                trait_depth,
            )?,
            SymbolicConstNode::BitAnd(left, right) => binary_const(
                SymbolicConstNode::BitAnd,
                substitution,
                left,
                right,
                trait_depth,
            )?,
            SymbolicConstNode::BitXor(left, right) => binary_const(
                SymbolicConstNode::BitXor,
                substitution,
                left,
                right,
                trait_depth,
            )?,
            SymbolicConstNode::BitOr(left, right) => binary_const(
                SymbolicConstNode::BitOr,
                substitution,
                left,
                right,
                trait_depth,
            )?,
            SymbolicConstNode::IntegerLiteral(_)
            | SymbolicConstNode::ConstDefinitionPath(_)
            | SymbolicConstNode::Bound { .. } => value.node.clone(),
        };
    Ok(SymbolicConstExpression {
        integer_type: value.integer_type,
        node,
    })
}

fn binary_const(
    constructor: fn(
        Box<SymbolicConstExpression>,
        Box<SymbolicConstExpression>,
    ) -> SymbolicConstNode,
    substitution: &TraitFrameSubstitution,
    left: &SymbolicConstExpression,
    right: &SymbolicConstExpression,
    trait_depth: u64,
) -> Result<SymbolicConstNode, GenericFormationError> {
    Ok(constructor(
        Box::new(substitute_const(substitution, left, trait_depth)?),
        Box::new(substitute_const(substitution, right, trait_depth)?),
    ))
}

fn substitute_arguments(
    substitution: &TraitFrameSubstitution,
    arguments: &[GenericArgumentShape],
    trait_depth: u64,
) -> Result<Vec<GenericArgumentShape>, GenericFormationError> {
    arguments
        .iter()
        .map(|argument| substitute_argument(substitution, argument, trait_depth))
        .collect()
}

fn substitute_types(
    substitution: &TraitFrameSubstitution,
    types: &[SymbolicType],
    trait_depth: u64,
) -> Result<Vec<SymbolicType>, GenericFormationError> {
    types
        .iter()
        .map(|ty| substitute_type(substitution, ty, trait_depth))
        .collect()
}

fn substitute_captures(
    substitution: &TraitFrameSubstitution,
    captures: &[SymbolicCapture],
    trait_depth: u64,
) -> Result<Vec<SymbolicCapture>, GenericFormationError> {
    captures
        .iter()
        .map(|capture| {
            Ok(SymbolicCapture {
                ordinal: capture.ordinal,
                mode: capture.mode,
                ty: substitute_type(substitution, &capture.ty, trait_depth)?,
            })
        })
        .collect()
}

fn substitute_generator_target(
    substitution: &TraitFrameSubstitution,
    target: &GeneratorTarget,
    trait_depth: u64,
) -> Result<GeneratorTarget, GenericFormationError> {
    Ok(match target {
        GeneratorTarget::Named {
            declaration,
            arguments,
            hidden_lifetime_binders,
        } => GeneratorTarget::Named {
            declaration: declaration.clone(),
            arguments: substitute_arguments(substitution, arguments, trait_depth)?,
            hidden_lifetime_binders: hidden_lifetime_binders.clone(),
        },
        GeneratorTarget::Anonymous {
            owner,
            expression_ordinal,
            arguments,
        } => GeneratorTarget::Anonymous {
            owner: owner.clone(),
            expression_ordinal: *expression_ordinal,
            arguments: substitute_arguments(substitution, arguments, trait_depth)?,
        },
    })
}

fn substitute_effect_set(
    substitution: &TraitFrameSubstitution,
    effects: &SymbolicTypeEffectSet,
    trait_depth: u64,
) -> Result<SymbolicTypeEffectSet, GenericFormationError> {
    let members = substitute_types(substitution, effects.members(), trait_depth)?;
    Ok(
        if effects.readiness() == SymbolicShapeReadiness::PendingC4 {
            SymbolicTypeEffectSet::pending_c4(members)
        } else {
            SymbolicTypeEffectSet::resolved(members)
        },
    )
}

#[cfg(test)]
mod tests {
    use arche_frontend::{IntegerType, SymbolicLifetime};

    use super::*;

    #[test]
    fn generic_formation_pins_exact_arity_and_kind() {
        let formals = vec![
            GenericParameterKind::Type,
            GenericParameterKind::Lifetime,
            GenericParameterKind::IntegerConst(IntegerType::Usize),
        ];
        let actuals = vec![
            GenericArgumentShape::Type(SymbolicType::I32),
            GenericArgumentShape::Lifetime(SymbolicLifetime::Static),
            GenericArgumentShape::IntegerConst(SymbolicConstExpression {
                integer_type: IntegerType::Usize,
                node: SymbolicConstNode::IntegerLiteral(vec![0; 8]),
            }),
        ];
        assert_eq!(validate_generic_arguments(&formals, &actuals), Ok(()));
        assert!(matches!(
            validate_generic_arguments(&formals, &actuals[..2]),
            Err(GenericFormationError::Arity {
                expected: 3,
                actual: 2
            })
        ));
        let mut wrong_kind = actuals;
        wrong_kind[2] = GenericArgumentShape::Type(SymbolicType::Usize);
        assert!(matches!(
            validate_generic_arguments(&formals, &wrong_kind),
            Err(GenericFormationError::WrongArgumentKind { index: 2, .. })
        ));
    }

    #[test]
    fn trait_method_substitution_replaces_outer_generics_and_implicit_self_only() {
        let substitution = TraitFrameSubstitution::new(
            vec![GenericParameterKind::Type],
            vec![GenericArgumentShape::Type(SymbolicType::BoundType {
                depth: 1,
                index: 0,
            })],
            SymbolicType::Tuple(vec![SymbolicType::BoundType { depth: 1, index: 0 }]),
        )
        .expect("valid substitution");
        let method_type = SymbolicType::Tuple(vec![
            SymbolicType::BoundType { depth: 1, index: 1 },
            SymbolicType::BoundType { depth: 1, index: 0 },
            SymbolicType::BoundType { depth: 0, index: 0 },
        ]);
        assert_eq!(
            substitution
                .substitute_type(&method_type, 1)
                .expect("substitute method trait frame"),
            SymbolicType::Tuple(vec![
                SymbolicType::Tuple(vec![SymbolicType::BoundType { depth: 1, index: 0 }]),
                SymbolicType::BoundType { depth: 1, index: 0 },
                SymbolicType::BoundType { depth: 0, index: 0 },
            ])
        );
    }

    #[test]
    fn implicit_self_never_changes_source_arity() {
        assert!(matches!(
            TraitFrameSubstitution::new(
                vec![GenericParameterKind::Type],
                vec![
                    GenericArgumentShape::Type(SymbolicType::I32),
                    GenericArgumentShape::Type(SymbolicType::U32),
                ],
                SymbolicType::I64,
            ),
            Err(GenericFormationError::Arity {
                expected: 1,
                actual: 2
            })
        ));
    }

    #[test]
    fn substitution_covers_lifetime_and_integer_const_slots() {
        let constant = SymbolicConstExpression {
            integer_type: IntegerType::Usize,
            node: SymbolicConstNode::IntegerLiteral(vec![3, 0, 0, 0, 0, 0, 0, 0]),
        };
        let substitution = TraitFrameSubstitution::new(
            vec![
                GenericParameterKind::Lifetime,
                GenericParameterKind::IntegerConst(IntegerType::Usize),
            ],
            vec![
                GenericArgumentShape::Lifetime(SymbolicLifetime::Static),
                GenericArgumentShape::IntegerConst(constant.clone()),
            ],
            SymbolicType::I32,
        )
        .expect("valid substitution");
        let ty = SymbolicType::Reference {
            mutability: arche_frontend::Mutability::Shared,
            lifetime: SymbolicLifetime::Bound { depth: 0, index: 0 },
            pointee: Box::new(SymbolicType::Array {
                element: Box::new(SymbolicType::I32),
                length: SymbolicConstExpression {
                    integer_type: IntegerType::Usize,
                    node: SymbolicConstNode::Bound { depth: 0, index: 1 },
                },
            }),
        };
        assert_eq!(
            substitution
                .substitute_type(&ty, 0)
                .expect("substitute mixed trait frame"),
            SymbolicType::Reference {
                mutability: arche_frontend::Mutability::Shared,
                lifetime: SymbolicLifetime::Static,
                pointee: Box::new(SymbolicType::Array {
                    element: Box::new(SymbolicType::I32),
                    length: constant,
                }),
            }
        );
    }
}
