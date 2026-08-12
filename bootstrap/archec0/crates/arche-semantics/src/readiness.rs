//! Structural C2 readiness audit.
//!
//! C1's scalar readiness is useful for its own encoding boundary, but it is not
//! a proof that a nested C2 continuation is absent. This visitor records the
//! orthogonal C2, C4, and CTFE facts without allowing one enum value to mask
//! another.

use std::collections::BTreeSet;

use arche_frontend::{
    GeneratorTarget, GenericArgumentShape, PendingShapeKind, SemanticDeclarationPath,
    SymbolicConstExpression, SymbolicConstNode, SymbolicDeclarationPayloadSkeleton,
    SymbolicDeclarationShapeSkeleton, SymbolicDefinitionOwnerSkeleton, SymbolicEffectSetsSkeleton,
    SymbolicEffectShapeSkeleton, SymbolicPendingShape, SymbolicPredicate,
    SymbolicPredicateShapeSkeleton, SymbolicShapeReadiness, SymbolicSystemAccessShapeSkeleton,
    SymbolicType, SymbolicTypeShapeSkeleton,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShapeReadinessInconsistency {
    PendingLeafNotPendingC2 {
        kind: PendingShapeKind,
        readiness: SymbolicShapeReadiness,
    },
    ResolvedLeafMarkedPendingC2,
    ResolvedLeafClaimsConstIndependent,
    ImpliedCapabilityNotPendingC4(SymbolicShapeReadiness),
    ScheduleNotPendingC4(SymbolicShapeReadiness),
    NestedEffectSetMarkedPendingC2,
}

/// Complete orthogonal gate facts found by recursively walking one shape.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ShapeGateAudit {
    pending_c2: Vec<SymbolicPendingShape>,
    pending_c4_count: u64,
    ctfe_dependencies: BTreeSet<SemanticDeclarationPath>,
    inconsistencies: Vec<ShapeReadinessInconsistency>,
}

impl ShapeGateAudit {
    pub fn pending_c2(&self) -> &[SymbolicPendingShape] {
        &self.pending_c2
    }

    pub const fn pending_c4_count(&self) -> u64 {
        self.pending_c4_count
    }

    pub fn ctfe_dependencies(&self) -> &BTreeSet<SemanticDeclarationPath> {
        &self.ctfe_dependencies
    }

    pub fn inconsistencies(&self) -> &[ShapeReadinessInconsistency] {
        &self.inconsistencies
    }

    pub fn post_c2_is_closed(&self) -> bool {
        self.pending_c2.is_empty() && self.inconsistencies.is_empty()
    }
}

pub fn audit_declaration_shape(shape: &SymbolicDeclarationShapeSkeleton) -> ShapeGateAudit {
    let mut audit = ShapeGateAudit::default();
    visit_declaration_shape(shape, &mut audit);
    audit
}

pub fn audit_definition_owner(owner: &SymbolicDefinitionOwnerSkeleton) -> ShapeGateAudit {
    let mut audit = ShapeGateAudit::default();
    match owner {
        SymbolicDefinitionOwnerSkeleton::TopLevel => {}
        SymbolicDefinitionOwnerSkeleton::Trait { shape, .. }
        | SymbolicDefinitionOwnerSkeleton::SystemQuery { shape, .. } => {
            visit_declaration_shape(shape, &mut audit);
        }
        SymbolicDefinitionOwnerSkeleton::InherentImpl {
            target, predicates, ..
        } => {
            visit_type_shape(target, &mut audit);
            visit_predicate_shapes(predicates, &mut audit);
        }
        SymbolicDefinitionOwnerSkeleton::TraitImpl {
            trait_ref,
            target,
            predicates,
            ..
        } => {
            visit_type_shape(trait_ref, &mut audit);
            visit_type_shape(target, &mut audit);
            visit_predicate_shapes(predicates, &mut audit);
        }
    }
    audit
}

fn visit_declaration_shape(shape: &SymbolicDeclarationShapeSkeleton, audit: &mut ShapeGateAudit) {
    visit_predicate_shapes(&shape.predicates, audit);
    match &shape.payload {
        SymbolicDeclarationPayloadSkeleton::World | SymbolicDeclarationPayloadSkeleton::Tag => {}
        SymbolicDeclarationPayloadSkeleton::Record(record) => {
            for field in &record.fields {
                visit_type_shape(&field.ty, audit);
            }
        }
        SymbolicDeclarationPayloadSkeleton::Enum(variants) => {
            for variant in variants {
                for field in &variant.fields {
                    visit_type_shape(&field.ty, audit);
                }
            }
        }
        SymbolicDeclarationPayloadSkeleton::Callable(callable) => {
            for parameter in &callable.parameters {
                visit_type_shape(&parameter.ty, audit);
            }
            visit_type_shape(&callable.result, audit);
            if let Some(resume) = &callable.resume {
                visit_type_shape(resume, audit);
            }
            if let Some(yields) = &callable.yields {
                visit_type_shape(yields, audit);
            }
            visit_effect_sets(&callable.effects, audit);
        }
        SymbolicDeclarationPayloadSkeleton::System {
            accesses,
            implied_requires,
            result,
            effects,
        } => {
            for access in accesses {
                match access {
                    SymbolicSystemAccessShapeSkeleton::CapabilityShared(ty)
                    | SymbolicSystemAccessShapeSkeleton::CapabilityMutable(ty)
                    | SymbolicSystemAccessShapeSkeleton::ResourceRead(ty)
                    | SymbolicSystemAccessShapeSkeleton::ResourceWrite(ty) => {
                        visit_type_shape(ty, audit);
                    }
                    SymbolicSystemAccessShapeSkeleton::Query(terms) => {
                        for term in terms {
                            visit_type_shape(&term.ty, audit);
                        }
                    }
                    SymbolicSystemAccessShapeSkeleton::Commands => {}
                }
            }
            for implied in implied_requires {
                visit_type_shape(&implied.referent, audit);
                if implied.readiness == SymbolicShapeReadiness::PendingC4 {
                    mark_pending_c4(audit);
                } else {
                    audit.inconsistencies.push(
                        ShapeReadinessInconsistency::ImpliedCapabilityNotPendingC4(
                            implied.readiness,
                        ),
                    );
                }
            }
            visit_type_shape(result, audit);
            visit_effect_sets(effects, audit);
        }
        SymbolicDeclarationPayloadSkeleton::Trait { methods } => {
            for method in methods {
                visit_declaration_shape(&method.shape, audit);
            }
        }
        SymbolicDeclarationPayloadSkeleton::Impl {
            trait_ref,
            target,
            methods,
            ..
        } => {
            if let Some(trait_ref) = trait_ref {
                visit_type_shape(trait_ref, audit);
            }
            visit_type_shape(target, audit);
            for method in methods {
                visit_declaration_shape(&method.shape, audit);
            }
        }
        SymbolicDeclarationPayloadSkeleton::Alias { target } => visit_type_shape(target, audit),
        SymbolicDeclarationPayloadSkeleton::Const { ty }
        | SymbolicDeclarationPayloadSkeleton::Static { ty, .. } => visit_type_shape(ty, audit),
        SymbolicDeclarationPayloadSkeleton::Query { terms } => {
            for term in terms {
                visit_type_shape(&term.ty, audit);
            }
        }
        SymbolicDeclarationPayloadSkeleton::Schedule { effects, readiness } => {
            visit_effect_sets(effects, audit);
            if *readiness == SymbolicShapeReadiness::PendingC4 {
                mark_pending_c4(audit);
            } else {
                audit
                    .inconsistencies
                    .push(ShapeReadinessInconsistency::ScheduleNotPendingC4(
                        *readiness,
                    ));
            }
        }
    }
}

fn visit_type_shape(shape: &SymbolicTypeShapeSkeleton, audit: &mut ShapeGateAudit) {
    match shape {
        SymbolicTypeShapeSkeleton::Pending(pending) => visit_pending(pending, audit),
        SymbolicTypeShapeSkeleton::Resolved { value, readiness } => {
            let before_ctfe = audit.ctfe_dependencies.len();
            let before_c4 = audit.pending_c4_count;
            visit_type(value, audit);
            validate_resolved_readiness(*readiness, before_ctfe, before_c4, audit);
        }
    }
}

fn visit_effect_shape(shape: &SymbolicEffectShapeSkeleton, audit: &mut ShapeGateAudit) {
    match shape {
        SymbolicEffectShapeSkeleton::Pending(pending) => visit_pending(pending, audit),
        SymbolicEffectShapeSkeleton::Resolved { value, readiness } => {
            let before_ctfe = audit.ctfe_dependencies.len();
            let before_c4 = audit.pending_c4_count;
            visit_type(value, audit);
            if *readiness == SymbolicShapeReadiness::PendingC4 {
                mark_pending_c4(audit);
            }
            validate_resolved_readiness(*readiness, before_ctfe, before_c4, audit);
        }
    }
}

fn visit_predicate_shapes(
    predicates: &[SymbolicPredicateShapeSkeleton],
    audit: &mut ShapeGateAudit,
) {
    for predicate in predicates {
        match predicate {
            SymbolicPredicateShapeSkeleton::Pending(pending) => visit_pending(pending, audit),
            SymbolicPredicateShapeSkeleton::Resolved { value, readiness } => {
                let before_ctfe = audit.ctfe_dependencies.len();
                let before_c4 = audit.pending_c4_count;
                visit_predicate(value, audit);
                validate_resolved_readiness(*readiness, before_ctfe, before_c4, audit);
            }
        }
    }
}

fn validate_resolved_readiness(
    readiness: SymbolicShapeReadiness,
    before_ctfe: usize,
    before_c4: u64,
    audit: &mut ShapeGateAudit,
) {
    match readiness {
        SymbolicShapeReadiness::PendingC2 => audit
            .inconsistencies
            .push(ShapeReadinessInconsistency::ResolvedLeafMarkedPendingC2),
        SymbolicShapeReadiness::ConstIndependent
            if audit.ctfe_dependencies.len() != before_ctfe
                || audit.pending_c4_count != before_c4 =>
        {
            audit
                .inconsistencies
                .push(ShapeReadinessInconsistency::ResolvedLeafClaimsConstIndependent);
        }
        SymbolicShapeReadiness::ConstIndependent
        | SymbolicShapeReadiness::NeedsCtfe
        | SymbolicShapeReadiness::PendingC4 => {}
    }
}

fn visit_pending(pending: &SymbolicPendingShape, audit: &mut ShapeGateAudit) {
    if pending.readiness == SymbolicShapeReadiness::PendingC2 {
        audit.pending_c2.push(pending.clone());
    } else {
        audit
            .inconsistencies
            .push(ShapeReadinessInconsistency::PendingLeafNotPendingC2 {
                kind: pending.kind,
                readiness: pending.readiness,
            });
    }
}

fn visit_effect_sets(effects: &SymbolicEffectSetsSkeleton, audit: &mut ShapeGateAudit) {
    for effect in effects.requires.iter().chain(&effects.throws) {
        visit_effect_shape(effect, audit);
    }
}

fn visit_predicate(predicate: &SymbolicPredicate, audit: &mut ShapeGateAudit) {
    match predicate {
        SymbolicPredicate::Trait {
            self_type,
            arguments,
            ..
        } => {
            visit_type(self_type, audit);
            for argument in arguments {
                visit_argument(argument, audit);
            }
        }
        SymbolicPredicate::LifetimeOutlives { .. } => {}
        SymbolicPredicate::TypeOutlives { ty, .. } => visit_type(ty, audit),
    }
}

fn visit_argument(argument: &GenericArgumentShape, audit: &mut ShapeGateAudit) {
    match argument {
        GenericArgumentShape::Type(ty) => visit_type(ty, audit),
        GenericArgumentShape::Lifetime(_) => {}
        GenericArgumentShape::IntegerConst(value) => visit_const(value, audit),
    }
}

fn visit_type(ty: &SymbolicType, audit: &mut ShapeGateAudit) {
    match ty {
        SymbolicType::Slice(element)
        | SymbolicType::RawPointer {
            pointee: element, ..
        } => {
            visit_type(element, audit);
        }
        SymbolicType::Array { element, length } => {
            visit_type(element, audit);
            visit_const(length, audit);
        }
        SymbolicType::Tuple(elements) => {
            for element in elements {
                visit_type(element, audit);
            }
        }
        SymbolicType::Reference { pointee, .. } => visit_type(pointee, audit),
        SymbolicType::NominalPath { arguments, .. } => {
            for argument in arguments {
                visit_argument(argument, audit);
            }
        }
        SymbolicType::FunctionPointer {
            parameters,
            result,
            requires,
            throws,
            ..
        } => {
            visit_types(parameters, audit);
            visit_type(result, audit);
            visit_nested_effect_set(requires.readiness(), requires.members(), audit);
            visit_nested_effect_set(throws.readiness(), throws.members(), audit);
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
                visit_type(&capture.ty, audit);
            }
            visit_types(parameters, audit);
            visit_type(result, audit);
            visit_nested_effect_set(requires.readiness(), requires.members(), audit);
            visit_nested_effect_set(throws.readiness(), throws.members(), audit);
            for argument in arguments {
                visit_argument(argument, audit);
            }
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
            visit_generator_target(target, audit);
            for capture in captures {
                visit_type(&capture.ty, audit);
            }
            visit_types(parameters, audit);
            visit_type(resume, audit);
            visit_type(yields, audit);
            visit_type(result, audit);
            visit_nested_effect_set(requires.readiness(), requires.members(), audit);
            visit_nested_effect_set(throws.readiness(), throws.members(), audit);
        }
        SymbolicType::JoinHandle { result, throws } => {
            visit_type(result, audit);
            visit_nested_effect_set(throws.readiness(), throws.members(), audit);
        }
        SymbolicType::GeneratorFactory {
            target,
            captures,
            parameters,
            produced_generator,
            ..
        } => {
            visit_generator_target(target, audit);
            for capture in captures {
                visit_type(&capture.ty, audit);
            }
            visit_types(parameters, audit);
            visit_type(produced_generator, audit);
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
        | SymbolicType::Str
        | SymbolicType::BoundType { .. } => {}
    }
}

fn visit_generator_target(target: &GeneratorTarget, audit: &mut ShapeGateAudit) {
    let arguments = match target {
        GeneratorTarget::Named { arguments, .. } | GeneratorTarget::Anonymous { arguments, .. } => {
            arguments
        }
    };
    for argument in arguments {
        visit_argument(argument, audit);
    }
}

fn visit_types(types: &[SymbolicType], audit: &mut ShapeGateAudit) {
    for ty in types {
        visit_type(ty, audit);
    }
}

fn visit_nested_effect_set(
    readiness: SymbolicShapeReadiness,
    members: &[SymbolicType],
    audit: &mut ShapeGateAudit,
) {
    match readiness {
        SymbolicShapeReadiness::PendingC4 => mark_pending_c4(audit),
        SymbolicShapeReadiness::PendingC2 => audit
            .inconsistencies
            .push(ShapeReadinessInconsistency::NestedEffectSetMarkedPendingC2),
        SymbolicShapeReadiness::ConstIndependent | SymbolicShapeReadiness::NeedsCtfe => {}
    }
    visit_types(members, audit);
}

fn visit_const(value: &SymbolicConstExpression, audit: &mut ShapeGateAudit) {
    match &value.node {
        SymbolicConstNode::ConstDefinitionPath(path) => {
            audit.ctfe_dependencies.insert(path.clone());
        }
        SymbolicConstNode::WrappingNeg(child) | SymbolicConstNode::BitNot(child) => {
            visit_const(child, audit);
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
            visit_const(left, audit);
            visit_const(right, audit);
        }
        SymbolicConstNode::IntegerLiteral(_) | SymbolicConstNode::Bound { .. } => {}
    }
}

fn mark_pending_c4(audit: &mut ShapeGateAudit) {
    audit.pending_c4_count = audit.pending_c4_count.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use arche_frontend::{
        DeclarationKind, IntegerType, SymbolicCallableKind, SymbolicCallableParameterMode,
        SymbolicCallableParameterSkeleton, SymbolicCallableShapeSkeleton,
        SymbolicEffectShapeSkeleton, SymbolicSourceSpan, SymbolicTypeEffectSet, TargetRoot,
    };

    use super::*;

    fn path(name: &str) -> SemanticDeclarationPath {
        SemanticDeclarationPath {
            registry_origin: "registry".to_owned(),
            package_name: "test/package".to_owned(),
            target: TargetRoot::Library,
            modules: vec!["root".to_owned()],
            kind: DeclarationKind::Const,
            name: name.to_owned(),
        }
    }

    fn span() -> SymbolicSourceSpan {
        SymbolicSourceSpan {
            file: 1,
            start_byte: 2,
            end_byte: 6,
            start_line: 1,
            start_column: 3,
            end_line: 1,
            end_column: 7,
        }
    }

    #[test]
    fn nested_pending_c2_remains_visible_beside_pending_c4_and_needs_ctfe() {
        let dependency = path("N");
        let shape = SymbolicDeclarationShapeSkeleton {
            generic_parameters: Vec::new(),
            predicates: Vec::new(),
            payload: SymbolicDeclarationPayloadSkeleton::Callable(Box::new(
                SymbolicCallableShapeSkeleton {
                    kind: SymbolicCallableKind::Function,
                    parameters: vec![SymbolicCallableParameterSkeleton {
                        mode: SymbolicCallableParameterMode::Value,
                        ty: SymbolicTypeShapeSkeleton::pending(
                            SymbolicShapeReadiness::PendingC2,
                            span(),
                            PendingShapeKind::ContextualSelf,
                            "Self",
                        ),
                    }],
                    result: SymbolicTypeShapeSkeleton::Resolved {
                        value: SymbolicType::Array {
                            element: Box::new(SymbolicType::I32),
                            length: SymbolicConstExpression {
                                integer_type: IntegerType::Usize,
                                node: SymbolicConstNode::ConstDefinitionPath(dependency.clone()),
                            },
                        },
                        readiness: SymbolicShapeReadiness::NeedsCtfe,
                    },
                    unsafe_: false,
                    resume: None,
                    yields: None,
                    effects: SymbolicEffectSetsSkeleton {
                        requires: vec![SymbolicEffectShapeSkeleton::resolved_pending_c4(
                            SymbolicType::I32,
                        )],
                        throws: Vec::new(),
                    },
                },
            )),
        };
        let audit = audit_declaration_shape(&shape);
        assert_eq!(audit.pending_c2().len(), 1);
        assert!(audit.pending_c4_count() > 0);
        assert_eq!(audit.ctfe_dependencies(), &BTreeSet::from([dependency]));
        assert!(audit.inconsistencies().is_empty());
        assert!(!audit.post_c2_is_closed());
    }

    #[test]
    fn scalar_readiness_cannot_hide_structural_corruption() {
        let shape = SymbolicDeclarationShapeSkeleton {
            generic_parameters: Vec::new(),
            predicates: Vec::new(),
            payload: SymbolicDeclarationPayloadSkeleton::Alias {
                target: SymbolicTypeShapeSkeleton::Resolved {
                    value: SymbolicType::FunctionPointer {
                        unsafe_: false,
                        parameters: Vec::new(),
                        result: Box::new(SymbolicType::Unit),
                        requires: SymbolicTypeEffectSet::pending_c4(vec![SymbolicType::I32]),
                        throws: SymbolicTypeEffectSet::default(),
                    },
                    readiness: SymbolicShapeReadiness::ConstIndependent,
                },
            },
        };
        let audit = audit_declaration_shape(&shape);
        assert!(audit.pending_c4_count() > 0);
        assert_eq!(
            audit.inconsistencies(),
            &[ShapeReadinessInconsistency::ResolvedLeafClaimsConstIndependent]
        );
    }

    #[test]
    fn owner_audit_walks_trait_and_impl_heads() {
        let pending = SymbolicTypeShapeSkeleton::pending(
            SymbolicShapeReadiness::PendingC2,
            span(),
            PendingShapeKind::GenericFormation,
            "Box<T>",
        );
        let owner = SymbolicDefinitionOwnerSkeleton::TraitImpl {
            trait_ref: pending,
            target: SymbolicTypeShapeSkeleton::resolved(SymbolicType::I32),
            generic_parameters: Vec::new(),
            predicates: Vec::new(),
            is_default: false,
        };
        let audit = audit_definition_owner(&owner);
        assert_eq!(audit.pending_c2().len(), 1);
    }
}
