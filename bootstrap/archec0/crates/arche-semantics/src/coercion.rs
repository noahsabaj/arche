//! The closed 0.1 safe-coercion relation.

use std::collections::BTreeSet;

use arche_frontend::{Mutability, SymbolicLifetime, SymbolicShapeReadiness, SymbolicType};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoercionKind {
    Identity,
    NeverToAny,
    LifetimeShortening,
    MutableReborrowToShared,
    ArrayReferenceToSlice,
    FunctionPointer,
    NoncapturingClosureToFunctionPointer,
}

/// Exact C2 result of the closed coercion check. Effect-set completion is an
/// orthogonal C4 dependency and never becomes a C2 inference placeholder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckedCoercion {
    kind: CoercionKind,
    effects_pending_c4: bool,
}

impl CheckedCoercion {
    pub const fn kind(self) -> CoercionKind {
        self.kind
    }

    pub const fn effects_pending_c4(self) -> bool {
        self.effects_pending_c4
    }
}

/// Reflexive/transitive lifetime-outlives authority used by C2 coercion.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LifetimeOutlives {
    edges: BTreeSet<(SymbolicLifetime, SymbolicLifetime)>,
}

impl LifetimeOutlives {
    pub fn new(edges: impl IntoIterator<Item = (SymbolicLifetime, SymbolicLifetime)>) -> Self {
        Self {
            edges: edges.into_iter().collect(),
        }
    }

    pub fn contains(&self, longer: &SymbolicLifetime, shorter: &SymbolicLifetime) -> bool {
        if longer == shorter || matches!(longer, SymbolicLifetime::Static) {
            return true;
        }
        let mut frontier = vec![longer.clone()];
        let mut visited = BTreeSet::new();
        while let Some(current) = frontier.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            for (_, next) in self.edges.iter().filter(|(from, _)| from == &current) {
                if next == shorter {
                    return true;
                }
                frontier.push(next.clone());
            }
        }
        false
    }
}

pub fn classify_coercion(
    source: &SymbolicType,
    target: &SymbolicType,
    outlives: &LifetimeOutlives,
) -> Option<CheckedCoercion> {
    if source == target {
        return Some(CheckedCoercion {
            kind: CoercionKind::Identity,
            effects_pending_c4: false,
        });
    }
    if matches!(source, SymbolicType::Never) {
        return Some(CheckedCoercion {
            kind: CoercionKind::NeverToAny,
            effects_pending_c4: false,
        });
    }
    if let Some(kind) = reference_coercion(source, target, outlives) {
        return Some(CheckedCoercion {
            kind,
            effects_pending_c4: false,
        });
    }
    match (source, target) {
        (
            SymbolicType::FunctionPointer {
                unsafe_: source_unsafe,
                parameters: source_parameters,
                result: source_result,
                requires: source_requires,
                throws: source_throws,
            },
            SymbolicType::FunctionPointer {
                unsafe_: target_unsafe,
                parameters: target_parameters,
                result: target_result,
                requires: target_requires,
                throws: target_throws,
            },
        ) => function_pointer_coercion(
            CoercionKind::FunctionPointer,
            CallableSide {
                unsafe_: *source_unsafe,
                parameters: source_parameters,
                result: source_result,
                requires: source_requires,
                throws: source_throws,
            },
            CallableSide {
                unsafe_: *target_unsafe,
                parameters: target_parameters,
                result: target_result,
                requires: target_requires,
                throws: target_throws,
            },
            outlives,
        ),
        (
            SymbolicType::Closure {
                captures,
                parameters: source_parameters,
                result: source_result,
                requires: source_requires,
                throws: source_throws,
                ..
            },
            SymbolicType::FunctionPointer {
                unsafe_: target_unsafe,
                parameters: target_parameters,
                result: target_result,
                requires: target_requires,
                throws: target_throws,
            },
        ) if captures.is_empty() => function_pointer_coercion(
            CoercionKind::NoncapturingClosureToFunctionPointer,
            CallableSide {
                unsafe_: false,
                parameters: source_parameters,
                result: source_result,
                requires: source_requires,
                throws: source_throws,
            },
            CallableSide {
                unsafe_: *target_unsafe,
                parameters: target_parameters,
                result: target_result,
                requires: target_requires,
                throws: target_throws,
            },
            outlives,
        ),
        _ => None,
    }
}

fn reference_coercion(
    source: &SymbolicType,
    target: &SymbolicType,
    outlives: &LifetimeOutlives,
) -> Option<CoercionKind> {
    let (
        SymbolicType::Reference {
            mutability: source_mutability,
            lifetime: source_lifetime,
            pointee: source_pointee,
        },
        SymbolicType::Reference {
            mutability: target_mutability,
            lifetime: target_lifetime,
            pointee: target_pointee,
        },
    ) = (source, target)
    else {
        return None;
    };
    if !outlives.contains(source_lifetime, target_lifetime) {
        return None;
    }
    let mutability_ok = source_mutability == target_mutability
        || (*source_mutability == Mutability::Mutable && *target_mutability == Mutability::Shared);
    if !mutability_ok {
        return None;
    }
    if source_pointee == target_pointee {
        return Some(if source_mutability == target_mutability {
            CoercionKind::LifetimeShortening
        } else {
            CoercionKind::MutableReborrowToShared
        });
    }
    match (source_pointee.as_ref(), target_pointee.as_ref()) {
        (SymbolicType::Array { element, .. }, SymbolicType::Slice(target_element))
            if element.as_ref() == target_element.as_ref() =>
        {
            Some(CoercionKind::ArrayReferenceToSlice)
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct CallableSide<'a> {
    unsafe_: bool,
    parameters: &'a [SymbolicType],
    result: &'a SymbolicType,
    requires: &'a arche_frontend::SymbolicTypeEffectSet,
    throws: &'a arche_frontend::SymbolicTypeEffectSet,
}

fn function_pointer_coercion(
    kind: CoercionKind,
    source: CallableSide<'_>,
    target: CallableSide<'_>,
    outlives: &LifetimeOutlives,
) -> Option<CheckedCoercion> {
    if (source.unsafe_ && !target.unsafe_) || source.parameters.len() != target.parameters.len() {
        return None;
    }
    // Contravariant inputs: a value promised to receive `target_parameter`
    // must be acceptable to the source callable.
    for (source_parameter, target_parameter) in source.parameters.iter().zip(target.parameters) {
        classify_coercion(target_parameter, source_parameter, outlives)?;
    }
    // Covariant result.
    classify_coercion(source.result, target.result, outlives)?;

    let effects_pending_c4 = effect_set_is_pending(source.requires)
        || effect_set_is_pending(source.throws)
        || effect_set_is_pending(target.requires)
        || effect_set_is_pending(target.throws);
    if !effects_pending_c4
        && (!is_effect_subset(source.requires.members(), target.requires.members())
            || !is_effect_subset(source.throws.members(), target.throws.members()))
    {
        return None;
    }
    Some(CheckedCoercion {
        kind,
        effects_pending_c4,
    })
}

fn effect_set_is_pending(set: &arche_frontend::SymbolicTypeEffectSet) -> bool {
    set.readiness() == SymbolicShapeReadiness::PendingC4
}

fn is_effect_subset(left: &[SymbolicType], right: &[SymbolicType]) -> bool {
    let right = right.iter().collect::<BTreeSet<_>>();
    left.iter().all(|member| right.contains(member))
}

#[cfg(test)]
mod tests {
    use arche_frontend::{
        DeclarationKind, SemanticDeclarationPath, SymbolicCapture, SymbolicTypeEffectSet,
        TargetRoot,
    };

    use super::*;

    fn lifetime(index: u64) -> SymbolicLifetime {
        SymbolicLifetime::Bound { depth: 0, index }
    }

    fn reference(
        mutability: Mutability,
        lifetime: SymbolicLifetime,
        pointee: SymbolicType,
    ) -> SymbolicType {
        SymbolicType::Reference {
            mutability,
            lifetime,
            pointee: Box::new(pointee),
        }
    }

    fn function(unsafe_: bool, parameter: SymbolicType, result: SymbolicType) -> SymbolicType {
        SymbolicType::FunctionPointer {
            unsafe_,
            parameters: vec![parameter],
            result: Box::new(result),
            requires: SymbolicTypeEffectSet::default(),
            throws: SymbolicTypeEffectSet::default(),
        }
    }

    #[test]
    fn lifetime_closure_is_reflexive_transitive_and_static_rooted() {
        let relation =
            LifetimeOutlives::new([(lifetime(0), lifetime(1)), (lifetime(1), lifetime(2))]);
        assert!(relation.contains(&lifetime(0), &lifetime(0)));
        assert!(relation.contains(&lifetime(0), &lifetime(2)));
        assert!(relation.contains(&SymbolicLifetime::Static, &lifetime(2)));
        assert!(!relation.contains(&lifetime(2), &lifetime(0)));
    }

    #[test]
    fn closed_reference_coercions_are_exact() {
        let relation = LifetimeOutlives::new([(lifetime(0), lifetime(1))]);
        let mutable = reference(Mutability::Mutable, lifetime(0), SymbolicType::I32);
        let shared = reference(Mutability::Shared, lifetime(1), SymbolicType::I32);
        assert_eq!(
            classify_coercion(&mutable, &shared, &relation)
                .expect("mutable reborrow")
                .kind(),
            CoercionKind::MutableReborrowToShared
        );
        assert!(classify_coercion(&shared, &mutable, &relation).is_none());

        let array = reference(
            Mutability::Mutable,
            lifetime(0),
            SymbolicType::Array {
                element: Box::new(SymbolicType::I32),
                length: arche_frontend::SymbolicConstExpression {
                    integer_type: arche_frontend::IntegerType::Usize,
                    node: arche_frontend::SymbolicConstNode::IntegerLiteral(vec![3; 8]),
                },
            },
        );
        let slice = reference(
            Mutability::Shared,
            lifetime(1),
            SymbolicType::Slice(Box::new(SymbolicType::I32)),
        );
        assert_eq!(
            classify_coercion(&array, &slice, &relation)
                .expect("array-reference unsizing")
                .kind(),
            CoercionKind::ArrayReferenceToSlice
        );
    }

    #[test]
    fn function_pointer_variance_and_safety_are_exact() {
        let relation = LifetimeOutlives::new([(lifetime(0), lifetime(1))]);
        let broad_input = reference(Mutability::Shared, lifetime(1), SymbolicType::I32);
        let narrow_input = reference(Mutability::Shared, lifetime(0), SymbolicType::I32);
        let source = function(false, broad_input, narrow_input.clone());
        let target = function(true, narrow_input, SymbolicType::Never);
        // Result covariance does not permit i32-reference -> never.
        assert!(classify_coercion(&source, &target, &relation).is_none());

        let source = function(false, SymbolicType::I32, SymbolicType::Never);
        let target = function(true, SymbolicType::I32, SymbolicType::I32);
        assert_eq!(
            classify_coercion(&source, &target, &relation)
                .expect("safe-to-unsafe pointer coercion")
                .kind(),
            CoercionKind::FunctionPointer
        );
        assert!(classify_coercion(&target, &source, &relation).is_none());
    }

    #[test]
    fn callable_effect_subtyping_is_subset_or_explicitly_pending_c4() {
        let effect = SymbolicType::I32;
        let source = SymbolicType::FunctionPointer {
            unsafe_: false,
            parameters: Vec::new(),
            result: Box::new(SymbolicType::Unit),
            requires: SymbolicTypeEffectSet::resolved(Vec::new()),
            throws: SymbolicTypeEffectSet::resolved(Vec::new()),
        };
        let target = SymbolicType::FunctionPointer {
            unsafe_: false,
            parameters: Vec::new(),
            result: Box::new(SymbolicType::Unit),
            requires: SymbolicTypeEffectSet::resolved(vec![effect.clone()]),
            throws: SymbolicTypeEffectSet::resolved(vec![effect.clone()]),
        };
        assert!(classify_coercion(&source, &target, &LifetimeOutlives::default()).is_some());
        assert!(classify_coercion(&target, &source, &LifetimeOutlives::default()).is_none());

        let pending = SymbolicType::FunctionPointer {
            unsafe_: false,
            parameters: Vec::new(),
            result: Box::new(SymbolicType::Unit),
            requires: SymbolicTypeEffectSet::pending_c4(vec![effect]),
            throws: SymbolicTypeEffectSet::default(),
        };
        assert!(
            classify_coercion(&pending, &source, &LifetimeOutlives::default())
                .expect("C2 shape check with C4 effect continuation")
                .effects_pending_c4()
        );
    }

    #[test]
    fn only_noncapturing_closures_coerce_to_function_pointers() {
        let owner = SemanticDeclarationPath {
            registry_origin: "registry".to_owned(),
            package_name: "test/package".to_owned(),
            target: TargetRoot::Library,
            modules: Vec::new(),
            kind: DeclarationKind::Function,
            name: "owner".to_owned(),
        };
        let closure = |captures| SymbolicType::Closure {
            owner: Box::new(owner.clone()),
            expression_ordinal: 1,
            captures,
            parameters: vec![SymbolicType::I32],
            result: Box::new(SymbolicType::I32),
            requires: SymbolicTypeEffectSet::default(),
            throws: SymbolicTypeEffectSet::default(),
            arguments: Vec::new(),
        };
        let pointer = function(false, SymbolicType::I32, SymbolicType::I32);
        assert_eq!(
            classify_coercion(&closure(Vec::new()), &pointer, &LifetimeOutlives::default(),)
                .expect("noncapturing closure")
                .kind(),
            CoercionKind::NoncapturingClosureToFunctionPointer
        );
        assert!(classify_coercion(
            &closure(vec![SymbolicCapture {
                ordinal: 1,
                mode: arche_frontend::CaptureMode::Move,
                ty: SymbolicType::I32,
            }]),
            &pointer,
            &LifetimeOutlives::default(),
        )
        .is_none());
    }
}
