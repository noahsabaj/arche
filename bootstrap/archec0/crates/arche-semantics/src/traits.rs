//! Deterministic pre-identity trait obligations, evidence, and selection.
//!
//! Ordinary declaration and implementation keys in this module are complete
//! C1 `SemanticDefinitionKey` session bytes. They are traversal/selection
//! authority only and are never advertised as stable identities. Compiler
//! traits require the raw stable `DefinitionId` carried by the verified
//! Embedded Core authority; a C1 virtual row ordinal is deliberately not an
//! accepted substitute.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use arche_foundation::identity::{DefinitionId, PackageId};
use arche_frontend::embedded_core::{
    CompilerTraitKind, CompilerTraitSelfRelation, UserImplPolicy, VerifiedEmbeddedCoreAuthority,
};
use arche_frontend::{
    encode_generic_arguments, encode_symbolic_predicate, encode_symbolic_type, DeclarationKind,
    GenericArgumentShape, GenericParameterKind, ShapeEncodingError, SymbolicConstExpression,
    SymbolicConstNode, SymbolicLifetime, SymbolicPredicate, SymbolicType,
};
use arche_package::{canonical_package_id, PackageName};

use crate::declarations::{DeclarationView, OrdinaryImplCandidateUniverse};
use crate::formation::{GenericFormationError, TraitFrameSubstitution};
use crate::model::{NeedsCtfeObligation, NeedsCtfeObligations};
use crate::sealed::{
    derive_sealed_copy, select_sealed_primitive_operator, PrimitiveOperatorTrait, SealedCopyProof,
    SealedPrimitiveOperator,
};

/// A semantic trait key usable before C4 assigns ordinary stable identities.
#[derive(Clone, Debug)]
pub struct SemanticTraitKey {
    kind: SemanticTraitKeyKind,
    canonical: Box<[u8]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SemanticTraitKeyKind {
    Ordinary {
        owner_package: PackageId,
        definition_key: Box<[u8]>,
    },
    CompilerKnown(CompilerTraitKeyAuthority),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompilerTraitKeyAuthority {
    owner_package: PackageId,
    interface_version: u32,
    interface_digest: [u8; 32],
    definition: DefinitionId,
    kind: CompilerTraitKind,
    explicit_generic_arity: u8,
    designated_self: CompilerTraitSelfRelation,
    user_impl_policy: UserImplPolicy,
}

impl PartialEq for SemanticTraitKey {
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}

impl Eq for SemanticTraitKey {}

impl PartialOrd for SemanticTraitKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemanticTraitKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.canonical.cmp(&other.canonical)
    }
}

impl SemanticTraitKey {
    /// Constructs an ordinary key from the complete declaration row.
    pub fn from_ordinary_declaration(
        declaration: DeclarationView<'_>,
    ) -> Result<Self, TraitModelError> {
        if declaration.kind() != DeclarationKind::Trait {
            return Err(TraitModelError::ExpectedTraitDeclaration);
        }
        let definition_key = declaration.session_traversal_bytes().to_vec();
        if definition_key.is_empty() {
            return Err(TraitModelError::EmptySemanticDefinitionKey);
        }
        let mut canonical = vec![1];
        push_bytes(&mut canonical, &definition_key)?;
        Ok(Self {
            kind: SemanticTraitKeyKind::Ordinary {
                owner_package: declaration.package(),
                definition_key: definition_key.into_boxed_slice(),
            },
            canonical: canonical.into_boxed_slice(),
        })
    }

    /// Constructs the compiler-known key from branded Embedded Core.
    ///
    /// The typed projection mints every compiler trait row's raw stable
    /// `DefinitionId` at authority construction, so key formation is total.
    /// Neither `CompilerTraitKind` nor `VirtualDefinitionId` substitutes for
    /// that identity, and no caller can supply the bytes directly.
    pub fn from_verified_embedded_core(
        authority: &VerifiedEmbeddedCoreAuthority,
        kind: CompilerTraitKind,
    ) -> Self {
        let definition = authority.compiler_trait(kind).definition_id();
        Self::from_verified_compiler_parts(authority, kind, definition)
    }

    /// Returns the exact tagged pre-identity bytes.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    /// Returns the compiler-known trait kind when this is a compiler key.
    pub const fn compiler_kind(&self) -> Option<CompilerTraitKind> {
        match &self.kind {
            SemanticTraitKeyKind::Ordinary { .. } => None,
            SemanticTraitKeyKind::CompilerKnown(authority) => Some(authority.kind),
        }
    }

    fn owner_package(&self) -> PackageId {
        match &self.kind {
            SemanticTraitKeyKind::Ordinary { owner_package, .. } => *owner_package,
            SemanticTraitKeyKind::CompilerKnown(authority) => authority.owner_package,
        }
    }

    fn compiler_authority(&self) -> Option<&CompilerTraitKeyAuthority> {
        match &self.kind {
            SemanticTraitKeyKind::Ordinary { .. } => None,
            SemanticTraitKeyKind::CompilerKnown(authority) => Some(authority),
        }
    }

    fn from_verified_compiler_parts(
        authority: &VerifiedEmbeddedCoreAuthority,
        kind: CompilerTraitKind,
        definition: DefinitionId,
    ) -> Self {
        let row = authority.compiler_trait(kind);
        let compiler = CompilerTraitKeyAuthority {
            owner_package: authority.package_id(),
            interface_version: authority.interface_version(),
            interface_digest: *authority.interface_digest(),
            definition,
            kind,
            explicit_generic_arity: row.explicit_generic_arity(),
            designated_self: row.designated_self(),
            user_impl_policy: row.user_impl_policy(),
        };
        let mut canonical = vec![2];
        canonical.extend_from_slice(&compiler.interface_version.to_le_bytes());
        canonical.extend_from_slice(&compiler.interface_digest);
        canonical.extend_from_slice(compiler.definition.as_bytes());
        Self {
            kind: SemanticTraitKeyKind::CompilerKnown(compiler),
            canonical: canonical.into_boxed_slice(),
        }
    }

    #[cfg(test)]
    fn ordinary_for_test(owner_package: PackageId, bytes: &[u8]) -> Self {
        let mut canonical = vec![1];
        push_bytes(&mut canonical, bytes).unwrap();
        Self {
            kind: SemanticTraitKeyKind::Ordinary {
                owner_package,
                definition_key: bytes.to_vec().into_boxed_slice(),
            },
            canonical: canonical.into_boxed_slice(),
        }
    }
}

/// One typed trait predicate independent of any body environment.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TraitPredicate {
    trait_key: SemanticTraitKey,
    self_type: SymbolicType,
    arguments: Box<[GenericArgumentShape]>,
    canonical: Box<[u8]>,
}

impl TraitPredicate {
    pub fn new(
        trait_key: SemanticTraitKey,
        self_type: SymbolicType,
        arguments: Vec<GenericArgumentShape>,
    ) -> Result<Self, TraitModelError> {
        validate_compiler_trait_application(&trait_key, &self_type, &arguments)?;
        let canonical = encode_trait_predicate(&trait_key, &self_type, &arguments)?;
        Ok(Self {
            trait_key,
            self_type,
            arguments: arguments.into_boxed_slice(),
            canonical: canonical.into_boxed_slice(),
        })
    }

    pub const fn trait_key(&self) -> &SemanticTraitKey {
        &self.trait_key
    }

    pub const fn self_type(&self) -> &SymbolicType {
        &self.self_type
    }

    pub fn arguments(&self) -> &[GenericArgumentShape] {
        &self.arguments
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    fn contains_bound_input(&self) -> bool {
        type_contains_bound(&self.self_type)
            || self.arguments.iter().any(generic_argument_contains_bound)
    }
}

/// One canonical body/impl predicate used for exact entailment evidence.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SemanticPredicate {
    kind: SemanticPredicateKind,
    canonical: Box<[u8]>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SemanticPredicateKind {
    Trait(TraitPredicate),
    LifetimeOutlives {
        longer: SymbolicLifetime,
        shorter: SymbolicLifetime,
    },
    TypeOutlives {
        ty: SymbolicType,
        lifetime: SymbolicLifetime,
    },
}

impl SemanticPredicate {
    pub fn trait_bound(predicate: TraitPredicate) -> Self {
        Self {
            canonical: predicate.canonical.clone(),
            kind: SemanticPredicateKind::Trait(predicate),
        }
    }

    pub fn lifetime_outlives(
        longer: SymbolicLifetime,
        shorter: SymbolicLifetime,
    ) -> Result<Self, TraitModelError> {
        let frontend = SymbolicPredicate::LifetimeOutlives {
            longer: longer.clone(),
            shorter: shorter.clone(),
        };
        Ok(Self {
            canonical: encode_symbolic_predicate(&frontend)?.into_boxed_slice(),
            kind: SemanticPredicateKind::LifetimeOutlives { longer, shorter },
        })
    }

    pub fn type_outlives(
        ty: SymbolicType,
        lifetime: SymbolicLifetime,
    ) -> Result<Self, TraitModelError> {
        let frontend = SymbolicPredicate::TypeOutlives {
            ty: ty.clone(),
            lifetime: lifetime.clone(),
        };
        Ok(Self {
            canonical: encode_symbolic_predicate(&frontend)?.into_boxed_slice(),
            kind: SemanticPredicateKind::TypeOutlives { ty, lifetime },
        })
    }

    pub fn as_trait(&self) -> Option<&TraitPredicate> {
        match &self.kind {
            SemanticPredicateKind::Trait(predicate) => Some(predicate),
            SemanticPredicateKind::LifetimeOutlives { .. }
            | SemanticPredicateKind::TypeOutlives { .. } => None,
        }
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }
}

/// Sorted, exact-deduplicated predicate environment.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct TraitEnvironment(Box<[SemanticPredicate]>);

impl TraitEnvironment {
    pub fn new(mut predicates: Vec<SemanticPredicate>) -> Result<Self, TraitModelError> {
        predicates.sort_by(|left, right| left.canonical.cmp(&right.canonical));
        if predicates
            .windows(2)
            .any(|pair| pair[0].canonical == pair[1].canonical)
        {
            return Err(TraitModelError::DuplicateEnvironmentPredicate);
        }
        Ok(Self(predicates.into_boxed_slice()))
    }

    pub fn predicates(&self) -> &[SemanticPredicate] {
        &self.0
    }

    fn exact(&self, predicate: &SemanticPredicate) -> Option<(u64, &SemanticPredicate)> {
        self.0
            .binary_search_by(|row| row.canonical.as_ref().cmp(predicate.canonical.as_ref()))
            .ok()
            .and_then(|index| {
                u64::try_from(index)
                    .ok()
                    .map(|index_u64| (index_u64, &self.0[index]))
            })
    }
}

/// Complete canonical pre-identity obligation and its body environment.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CanonicalTraitObligation {
    predicate: TraitPredicate,
    environment: TraitEnvironment,
    canonical: Box<[u8]>,
}

impl CanonicalTraitObligation {
    pub fn new(
        predicate: TraitPredicate,
        environment: TraitEnvironment,
    ) -> Result<Self, TraitModelError> {
        let mut canonical = Vec::new();
        push_bytes(&mut canonical, predicate.canonical_bytes())?;
        push_count(&mut canonical, environment.predicates().len())?;
        for member in environment.predicates() {
            push_bytes(&mut canonical, member.canonical_bytes())?;
        }
        Ok(Self {
            predicate,
            environment,
            canonical: canonical.into_boxed_slice(),
        })
    }

    pub const fn predicate(&self) -> &TraitPredicate {
        &self.predicate
    }

    pub const fn environment(&self) -> &TraitEnvironment {
        &self.environment
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }
}

/// Exact indexed use of a canonical bound predicate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundWitness {
    obligation: TraitPredicate,
    environment_index: u64,
}

impl BoundWitness {
    pub const fn obligation(&self) -> &TraitPredicate {
        &self.obligation
    }

    pub const fn environment_index(&self) -> u64 {
        self.environment_index
    }
}

/// Typed C4 continuation furnished by an exact `K: EcsKey` environment row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingC4EcsKeyComparison {
    obligation: TraitPredicate,
    key_type: SymbolicType,
    ecs_key_witness: BoundWitness,
}

impl PendingC4EcsKeyComparison {
    pub const fn obligation(&self) -> &TraitPredicate {
        &self.obligation
    }

    pub const fn key_type(&self) -> &SymbolicType {
        &self.key_type
    }

    pub const fn ecs_key_witness(&self) -> &BoundWitness {
        &self.ecs_key_witness
    }
}

/// Complete session-only substitution selected for an ordinary impl.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraitSubstitution {
    formals: Box<[GenericParameterKind]>,
    arguments: Box<[GenericArgumentShape]>,
}

impl TraitSubstitution {
    pub fn formals(&self) -> &[GenericParameterKind] {
        &self.formals
    }

    pub fn arguments(&self) -> &[GenericArgumentShape] {
        &self.arguments
    }

    fn frame(&self) -> Result<TraitFrameSubstitution, GenericFormationError> {
        TraitFrameSubstitution::new(
            self.formals.to_vec(),
            self.arguments.to_vec(),
            SymbolicType::Unit,
        )
    }
}

/// Evidence for one substituted impl requirement, in canonical predicate order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PredicateEvidence {
    ExactEnvironment {
        predicate: Box<SemanticPredicate>,
        environment_index: u64,
    },
    Trait(Box<TraitEvidence>),
}

/// Complete ordinary semantic impl key before C4 stable identity assignment.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct OrdinarySemanticImplKey {
    owner_package: PackageId,
    definition_key: Box<[u8]>,
}

impl OrdinarySemanticImplKey {
    pub const fn owner_package(&self) -> PackageId {
        self.owner_package
    }

    pub fn definition_key_bytes(&self) -> &[u8] {
        &self.definition_key
    }
}

/// Unique ordinary selection with its exact substitution and predicate proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrdinaryImplSelection {
    implementation: OrdinarySemanticImplKey,
    substitution: TraitSubstitution,
    predicate_evidence: Box<[PredicateEvidence]>,
}

impl OrdinaryImplSelection {
    pub const fn implementation(&self) -> &OrdinarySemanticImplKey {
        &self.implementation
    }

    pub const fn substitution(&self) -> &TraitSubstitution {
        &self.substitution
    }

    pub fn predicate_evidence(&self) -> &[PredicateEvidence] {
        &self.predicate_evidence
    }
}

/// Closed C2 evidence variants. None contains a stable ordinary identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraitEvidence {
    BoundWitness(Box<BoundWitness>),
    SealedPrimitiveOperator(Box<SealedPrimitiveOperator>),
    SealedCopy(Box<SealedCopyProof>),
    PendingC4EcsKeyComparison(Box<PendingC4EcsKeyComparison>),
    Ordinary(Box<OrdinaryImplSelection>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraitSolveResult {
    Selected(TraitEvidence),
    Unsatisfied,
    Ambiguous(Box<[OrdinarySemanticImplKey]>),
    NeedsCtfe(NeedsCtfeObligations),
}

/// Typed description supplied for one trait impl in the exact declaration
/// universe. Inherent impl rows return `None` from the universe mapper.
#[derive(Clone, Debug)]
pub struct OrdinaryImplCandidateSpec {
    is_default: bool,
    generic_parameters: Box<[GenericParameterKind]>,
    head: TraitPredicate,
    predicates: TraitEnvironment,
}

impl OrdinaryImplCandidateSpec {
    pub const fn is_default(&self) -> bool {
        self.is_default
    }

    pub fn generic_parameters(&self) -> &[GenericParameterKind] {
        &self.generic_parameters
    }

    pub const fn head(&self) -> &TraitPredicate {
        &self.head
    }

    pub const fn environment(&self) -> &TraitEnvironment {
        &self.predicates
    }

    pub fn new(
        is_default: bool,
        generic_parameters: Vec<GenericParameterKind>,
        head: TraitPredicate,
        predicates: TraitEnvironment,
    ) -> Self {
        Self {
            is_default,
            generic_parameters: generic_parameters.into_boxed_slice(),
            head,
            predicates,
        }
    }
}

#[derive(Clone, Debug)]
struct OrdinaryImplCandidate {
    key: OrdinarySemanticImplKey,
    is_default: bool,
    generic_parameters: Box<[GenericParameterKind]>,
    head: TraitPredicate,
    predicates: TraitEnvironment,
}

impl OrdinaryImplCandidate {
    fn from_declaration(
        declaration: DeclarationView<'_>,
        spec: OrdinaryImplCandidateSpec,
    ) -> Result<Self, TraitSolverBuildError> {
        if declaration.kind() != DeclarationKind::Impl {
            return Err(TraitSolverBuildError::ExpectedImplDeclaration);
        }
        Self::from_parts(
            OrdinarySemanticImplKey {
                owner_package: declaration.package(),
                definition_key: declaration
                    .session_traversal_bytes()
                    .to_vec()
                    .into_boxed_slice(),
            },
            spec,
        )
    }

    fn from_parts(
        key: OrdinarySemanticImplKey,
        spec: OrdinaryImplCandidateSpec,
    ) -> Result<Self, TraitSolverBuildError> {
        let trait_owner = spec.head.trait_key.owner_package();
        let outermost_nominal_owner = outermost_nominal_owner(&spec.head.self_type)?;
        if key.owner_package != trait_owner && outermost_nominal_owner != Some(key.owner_package) {
            return Err(TraitSolverBuildError::OrphanRuleViolation {
                implementation: key,
                trait_owner,
                outermost_nominal_owner,
            });
        }
        if let Some(authority) = spec.head.trait_key.compiler_authority() {
            if authority.user_impl_policy != UserImplPolicy::AllowedAndValidated {
                return Err(TraitSolverBuildError::CompilerTraitUserImplForbidden(
                    authority.kind,
                ));
            }
        }
        validate_candidate_formals(&spec.generic_parameters, &spec.head)?;
        Ok(Self {
            key,
            is_default: spec.is_default,
            generic_parameters: spec.generic_parameters,
            head: spec.head,
            predicates: spec.predicates,
        })
    }
}

fn outermost_nominal_owner(
    self_type: &SymbolicType,
) -> Result<Option<PackageId>, TraitSolverBuildError> {
    let SymbolicType::NominalPath { declaration, .. } = self_type else {
        return Ok(None);
    };
    let package = PackageName::from_str(&declaration.package_name).map_err(|_| {
        TraitSolverBuildError::InvalidNominalPackageName(declaration.package_name.clone())
    })?;
    Ok(Some(canonical_package_id(&package)))
}

/// Solver constructed only from the exact package/target candidate universe.
#[derive(Debug, Default)]
pub struct TraitSolver {
    candidates: BTreeMap<OrdinarySemanticImplKey, OrdinaryImplCandidate>,
    memo: BTreeMap<Box<[u8]>, TraitSolveResult>,
}

impl TraitSolver {
    /// Maps every candidate row in the exact declaration universe. Returning
    /// `None` is valid only for an inherent impl.
    pub fn from_universe<F>(
        universe: &OrdinaryImplCandidateUniverse<'_>,
        mut describe: F,
    ) -> Result<Self, TraitSolverBuildError>
    where
        F: FnMut(
            DeclarationView<'_>,
        ) -> Result<Option<OrdinaryImplCandidateSpec>, TraitSolverBuildError>,
    {
        let mut candidates = Vec::new();
        for declaration in universe.candidates() {
            if let Some(spec) = describe(declaration)? {
                candidates.push(OrdinaryImplCandidate::from_declaration(declaration, spec)?);
            }
        }
        Self::from_candidates(candidates)
    }

    fn from_candidates(
        candidates: Vec<OrdinaryImplCandidate>,
    ) -> Result<Self, TraitSolverBuildError> {
        let mut by_key = BTreeMap::new();
        for candidate in candidates {
            let key = candidate.key.clone();
            if by_key.insert(key.clone(), candidate).is_some() {
                return Err(TraitSolverBuildError::DuplicateImplDescriptor(key));
            }
        }
        validate_coherence(&by_key)?;
        Ok(Self {
            candidates: by_key,
            memo: BTreeMap::new(),
        })
    }

    /// Solves through a canonical least-first worklist and memoizes only by
    /// complete obligation bytes.
    pub fn solve(
        &mut self,
        obligation: CanonicalTraitObligation,
    ) -> Result<TraitSolveResult, TraitSolveError> {
        let root = obligation.canonical.clone();
        if let Some(result) = self.memo.get(&root) {
            return Ok(result.clone());
        }

        let mut obligations = BTreeMap::from([(root.clone(), obligation)]);
        let mut ready = BTreeSet::from([root.clone()]);
        let mut blocked: BTreeMap<Box<[u8]>, BTreeSet<Box<[u8]>>> = BTreeMap::new();

        while !self.memo.contains_key(&root) {
            if let Some(key) = ready.pop_first() {
                if self.memo.contains_key(&key) {
                    continue;
                }
                let current = obligations
                    .get(&key)
                    .expect("worklist key retains its typed obligation")
                    .clone();
                match self.evaluate(&current)? {
                    Evaluation::Ready(result) => {
                        self.memo.insert(key.clone(), result);
                        blocked.remove(&key);
                        wake_ready(&mut blocked, &self.memo, &mut ready);
                    }
                    Evaluation::Blocked(children) => {
                        let mut dependencies = BTreeSet::new();
                        for child in children {
                            let child_key = child.canonical.clone();
                            if !self.memo.contains_key(&child_key) {
                                obligations.entry(child_key.clone()).or_insert(child);
                                dependencies.insert(child_key.clone());
                                if child_key != key {
                                    ready.insert(child_key);
                                }
                            }
                        }
                        if dependencies.is_empty() {
                            ready.insert(key);
                        } else {
                            blocked.insert(key, dependencies);
                        }
                    }
                }
                continue;
            }

            // Ordinary cycles are not coinductive. Mark the least blocked
            // obligation unsatisfied, then deterministically wake dependents.
            let cyclic = blocked
                .keys()
                .next()
                .cloned()
                .expect("an unfinished root is either ready or blocked");
            self.memo
                .insert(cyclic.clone(), TraitSolveResult::Unsatisfied);
            blocked.remove(&cyclic);
            wake_ready(&mut blocked, &self.memo, &mut ready);
        }
        Ok(self.memo.get(&root).expect("root was solved").clone())
    }

    fn evaluate(
        &self,
        obligation: &CanonicalTraitObligation,
    ) -> Result<Evaluation, TraitSolveError> {
        if let Some(witness) = exact_bound_witness(obligation) {
            return Ok(Evaluation::Ready(TraitSolveResult::Selected(
                TraitEvidence::BoundWitness(Box::new(witness)),
            )));
        }
        if let Some(evidence) = sealed_evidence(obligation)? {
            return Ok(Evaluation::Ready(TraitSolveResult::Selected(evidence)));
        }

        let mut viable = Vec::new();
        let mut pending = false;
        let mut blocked = Vec::new();
        let mut inherited_ambiguity = false;

        for candidate in self.candidates.values() {
            if candidate.head.trait_key != obligation.predicate.trait_key {
                continue;
            }
            let substitution = match unify_head(candidate, &obligation.predicate)? {
                HeadMatch::NoMatch => continue,
                HeadMatch::NeedsCtfe => {
                    pending = true;
                    continue;
                }
                HeadMatch::Match(substitution) => substitution,
            };
            match self.evaluate_candidate(candidate, substitution, obligation)? {
                CandidateEvaluation::Viable(selection) => viable.push(selection),
                CandidateEvaluation::NotViable => {}
                CandidateEvaluation::NeedsCtfe => pending = true,
                CandidateEvaluation::Blocked(children) => blocked.extend(children),
                CandidateEvaluation::AmbiguousDependency => inherited_ambiguity = true,
            }
        }

        if !blocked.is_empty() {
            blocked.sort_by(|left, right| left.canonical.cmp(&right.canonical));
            blocked.dedup_by(|left, right| left.canonical == right.canonical);
            return Ok(Evaluation::Blocked(blocked));
        }
        if pending {
            return Ok(Evaluation::Ready(TraitSolveResult::NeedsCtfe(
                needs_ctfe_for(obligation)?,
            )));
        }
        if inherited_ambiguity {
            return Ok(Evaluation::Ready(TraitSolveResult::Ambiguous(
                viable
                    .into_iter()
                    .map(|selection| selection.implementation)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            )));
        }
        if viable.is_empty() {
            return Ok(Evaluation::Ready(TraitSolveResult::Unsatisfied));
        }

        let mut maxima = Vec::new();
        'candidate: for selection in &viable {
            let selected = &self.candidates[&selection.implementation];
            for other in &viable {
                if selection.implementation == other.implementation {
                    continue;
                }
                let other_candidate = &self.candidates[&other.implementation];
                if is_strict_specialization(other_candidate, selected)? {
                    continue 'candidate;
                }
            }
            maxima.push(selection.clone());
        }
        maxima.sort_by(|left, right| left.implementation.cmp(&right.implementation));
        if maxima.len() == 1 {
            return Ok(Evaluation::Ready(TraitSolveResult::Selected(
                TraitEvidence::Ordinary(Box::new(maxima.remove(0))),
            )));
        }
        Ok(Evaluation::Ready(TraitSolveResult::Ambiguous(
            maxima
                .into_iter()
                .map(|selection| selection.implementation)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )))
    }

    fn evaluate_candidate(
        &self,
        candidate: &OrdinaryImplCandidate,
        substitution: TraitSubstitution,
        parent: &CanonicalTraitObligation,
    ) -> Result<CandidateEvaluation, TraitSolveError> {
        let predicates = substitute_environment(&candidate.predicates, &substitution)?;
        let mut evidence = Vec::new();
        let mut children = Vec::new();
        let mut pending = false;
        let mut ambiguous = false;

        for predicate in predicates.predicates() {
            if let Some((environment_index, exact)) = parent.environment.exact(predicate) {
                if let Some(trait_predicate) = exact.as_trait() {
                    if trait_predicate.contains_bound_input() {
                        evidence.push(PredicateEvidence::Trait(Box::new(
                            TraitEvidence::BoundWitness(Box::new(BoundWitness {
                                obligation: trait_predicate.clone(),
                                environment_index,
                            })),
                        )));
                        continue;
                    }
                } else {
                    evidence.push(PredicateEvidence::ExactEnvironment {
                        predicate: Box::new(exact.clone()),
                        environment_index,
                    });
                    continue;
                }
            }

            let Some(trait_predicate) = predicate.as_trait() else {
                return Err(TraitSolveError::UnsupportedPredicateEntailment(Box::new(
                    predicate.clone(),
                )));
            };
            let child =
                CanonicalTraitObligation::new(trait_predicate.clone(), parent.environment.clone())?;
            let Some(result) = self.memo.get(child.canonical_bytes()) else {
                children.push(child);
                continue;
            };
            match result {
                TraitSolveResult::Selected(child_evidence) => {
                    evidence.push(PredicateEvidence::Trait(Box::new(child_evidence.clone())))
                }
                TraitSolveResult::Unsatisfied => return Ok(CandidateEvaluation::NotViable),
                TraitSolveResult::Ambiguous(_) => ambiguous = true,
                TraitSolveResult::NeedsCtfe(_) => pending = true,
            }
        }
        if !children.is_empty() {
            return Ok(CandidateEvaluation::Blocked(children));
        }
        if pending {
            return Ok(CandidateEvaluation::NeedsCtfe);
        }
        if ambiguous {
            return Ok(CandidateEvaluation::AmbiguousDependency);
        }
        Ok(CandidateEvaluation::Viable(OrdinaryImplSelection {
            implementation: candidate.key.clone(),
            substitution,
            predicate_evidence: evidence.into_boxed_slice(),
        }))
    }

    #[cfg(test)]
    fn for_test(candidates: Vec<OrdinaryImplCandidate>) -> Result<Self, TraitSolverBuildError> {
        Self::from_candidates(candidates)
    }
}

#[derive(Debug)]
enum Evaluation {
    Ready(TraitSolveResult),
    Blocked(Vec<CanonicalTraitObligation>),
}

#[derive(Debug)]
enum CandidateEvaluation {
    Viable(OrdinaryImplSelection),
    NotViable,
    NeedsCtfe,
    Blocked(Vec<CanonicalTraitObligation>),
    AmbiguousDependency,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraitModelError {
    ExpectedTraitDeclaration,
    EmptySemanticDefinitionKey,
    WrongCompilerTraitArity {
        trait_kind: CompilerTraitKind,
        expected: u8,
        actual: usize,
    },
    NonTypeCompilerTraitArgument {
        trait_kind: CompilerTraitKind,
        index: usize,
    },
    CompilerTraitDesignatedSelfMismatch(CompilerTraitKind),
    DuplicateEnvironmentPredicate,
    LengthOverflow,
    ShapeEncoding(ShapeEncodingError),
}

impl fmt::Display for TraitModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid C2 trait model: {self:?}")
    }
}

impl std::error::Error for TraitModelError {}

impl From<ShapeEncodingError> for TraitModelError {
    fn from(error: ShapeEncodingError) -> Self {
        Self::ShapeEncoding(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraitSolverBuildError {
    ExpectedImplDeclaration,
    DuplicateImplDescriptor(OrdinarySemanticImplKey),
    OrphanRuleViolation {
        implementation: OrdinarySemanticImplKey,
        trait_owner: PackageId,
        outermost_nominal_owner: Option<PackageId>,
    },
    CompilerTraitUserImplForbidden(CompilerTraitKind),
    InvalidNominalPackageName(String),
    UnconstrainedGenericParameter {
        implementation_trait: SemanticTraitKey,
        index: u64,
    },
    InvalidGenericHead(GenericFormationError),
    ExactImplOverlap {
        first: OrdinarySemanticImplKey,
        second: OrdinarySemanticImplKey,
    },
    IllegalImplOverlap {
        first: OrdinarySemanticImplKey,
        second: OrdinarySemanticImplKey,
    },
}

impl fmt::Display for TraitSolverBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid C2 impl universe: {self:?}")
    }
}

impl std::error::Error for TraitSolverBuildError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraitSolveError {
    Model(TraitModelError),
    GenericFormation(GenericFormationError),
    InvalidCandidateGenericUse,
    UnsupportedConstUnification,
    UnsupportedPredicateEntailment(Box<SemanticPredicate>),
    UnsupportedSpecializationEntailment {
        parent: OrdinarySemanticImplKey,
        child: OrdinarySemanticImplKey,
    },
}

impl fmt::Display for TraitSolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "C2 trait selection failed closed: {self:?}")
    }
}

impl std::error::Error for TraitSolveError {}

impl From<TraitModelError> for TraitSolveError {
    fn from(error: TraitModelError) -> Self {
        Self::Model(error)
    }
}

impl From<GenericFormationError> for TraitSolveError {
    fn from(error: GenericFormationError) -> Self {
        Self::GenericFormation(error)
    }
}

fn validate_compiler_trait_application(
    key: &SemanticTraitKey,
    self_type: &SymbolicType,
    arguments: &[GenericArgumentShape],
) -> Result<(), TraitModelError> {
    let Some(authority) = key.compiler_authority() else {
        return Ok(());
    };
    if arguments.len() != usize::from(authority.explicit_generic_arity) {
        return Err(TraitModelError::WrongCompilerTraitArity {
            trait_kind: authority.kind,
            expected: authority.explicit_generic_arity,
            actual: arguments.len(),
        });
    }
    let mut type_arguments = Vec::with_capacity(arguments.len());
    for (index, argument) in arguments.iter().enumerate() {
        let GenericArgumentShape::Type(ty) = argument else {
            return Err(TraitModelError::NonTypeCompilerTraitArgument {
                trait_kind: authority.kind,
                index,
            });
        };
        type_arguments.push(ty);
    }
    let designated = match authority.designated_self {
        CompilerTraitSelfRelation::OperatedType | CompilerTraitSelfRelation::CallableType => {
            self_type
        }
        CompilerTraitSelfRelation::Target(parameter)
        | CompilerTraitSelfRelation::LeftHandSide(parameter)
        | CompilerTraitSelfRelation::Input(parameter)
        | CompilerTraitSelfRelation::Source(parameter)
        | CompilerTraitSelfRelation::Iterator(parameter) => type_arguments
            .get(usize::from(parameter.index()))
            .copied()
            .ok_or(TraitModelError::WrongCompilerTraitArity {
                trait_kind: authority.kind,
                expected: authority.explicit_generic_arity,
                actual: arguments.len(),
            })?,
    };
    if designated != self_type {
        return Err(TraitModelError::CompilerTraitDesignatedSelfMismatch(
            authority.kind,
        ));
    }
    Ok(())
}

fn encode_trait_predicate(
    key: &SemanticTraitKey,
    self_type: &SymbolicType,
    arguments: &[GenericArgumentShape],
) -> Result<Vec<u8>, TraitModelError> {
    let mut output = vec![1];
    push_bytes(&mut output, key.canonical_bytes())?;
    push_bytes(&mut output, &encode_symbolic_type(self_type)?)?;
    push_bytes(&mut output, &encode_generic_arguments(arguments)?)?;
    Ok(output)
}

fn push_bytes(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), TraitModelError> {
    let length = u64::try_from(bytes.len()).map_err(|_| TraitModelError::LengthOverflow)?;
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

fn push_count(output: &mut Vec<u8>, count: usize) -> Result<(), TraitModelError> {
    output.extend_from_slice(
        &u64::try_from(count)
            .map_err(|_| TraitModelError::LengthOverflow)?
            .to_le_bytes(),
    );
    Ok(())
}

fn exact_bound_witness(obligation: &CanonicalTraitObligation) -> Option<BoundWitness> {
    if !obligation.predicate.contains_bound_input() {
        return None;
    }
    let needle = SemanticPredicate::trait_bound(obligation.predicate.clone());
    let (environment_index, exact) = obligation.environment.exact(&needle)?;
    let exact = exact.as_trait()?;
    Some(BoundWitness {
        obligation: exact.clone(),
        environment_index,
    })
}

fn sealed_evidence(
    obligation: &CanonicalTraitObligation,
) -> Result<Option<TraitEvidence>, TraitSolveError> {
    let Some(kind) = obligation.predicate.trait_key.compiler_kind() else {
        return Ok(None);
    };
    if kind == CompilerTraitKind::Copy {
        return Ok(derive_sealed_copy(&obligation.predicate.self_type)
            .map(Box::new)
            .map(TraitEvidence::SealedCopy));
    }
    if let Some(operator) = primitive_operator_kind(kind) {
        if let Some(evidence) = select_sealed_primitive_operator(
            operator,
            &obligation.predicate.self_type,
            &obligation.predicate.arguments,
        ) {
            return Ok(Some(TraitEvidence::SealedPrimitiveOperator(Box::new(
                evidence,
            ))));
        }
    }
    if matches!(kind, CompilerTraitKind::Eq | CompilerTraitKind::Ord) {
        return Ok(ecs_key_comparison_continuation(obligation));
    }
    Ok(None)
}

fn primitive_operator_kind(kind: CompilerTraitKind) -> Option<PrimitiveOperatorTrait> {
    Some(match kind {
        CompilerTraitKind::Neg => PrimitiveOperatorTrait::Neg,
        CompilerTraitKind::LogicalNot => PrimitiveOperatorTrait::LogicalNot,
        CompilerTraitKind::BitNot => PrimitiveOperatorTrait::BitNot,
        CompilerTraitKind::Add => PrimitiveOperatorTrait::Add,
        CompilerTraitKind::Sub => PrimitiveOperatorTrait::Sub,
        CompilerTraitKind::Mul => PrimitiveOperatorTrait::Mul,
        CompilerTraitKind::Div => PrimitiveOperatorTrait::Div,
        CompilerTraitKind::Rem => PrimitiveOperatorTrait::Rem,
        CompilerTraitKind::ShiftLeft => PrimitiveOperatorTrait::ShiftLeft,
        CompilerTraitKind::ShiftRight => PrimitiveOperatorTrait::ShiftRight,
        CompilerTraitKind::BitAnd => PrimitiveOperatorTrait::BitAnd,
        CompilerTraitKind::BitXor => PrimitiveOperatorTrait::BitXor,
        CompilerTraitKind::BitOr => PrimitiveOperatorTrait::BitOr,
        CompilerTraitKind::Eq => PrimitiveOperatorTrait::Eq,
        CompilerTraitKind::Ord => PrimitiveOperatorTrait::Ord,
        CompilerTraitKind::Clone
        | CompilerTraitKind::Copy
        | CompilerTraitKind::Drop
        | CompilerTraitKind::EcsKey
        | CompilerTraitKind::EcsValue
        | CompilerTraitKind::Fn
        | CompilerTraitKind::FnMut
        | CompilerTraitKind::FnOnce
        | CompilerTraitKind::From
        | CompilerTraitKind::IntoIterator
        | CompilerTraitKind::Iterator
        | CompilerTraitKind::Send
        | CompilerTraitKind::Sync
        | CompilerTraitKind::TryFrom
        | CompilerTraitKind::Unpin
        | CompilerTraitKind::UnwindPayload => return None,
    })
}

fn ecs_key_comparison_continuation(obligation: &CanonicalTraitObligation) -> Option<TraitEvidence> {
    let key_type = match obligation.predicate.arguments() {
        [GenericArgumentShape::Type(left), GenericArgumentShape::Type(right)]
            if left == obligation.predicate.self_type() && right == left =>
        {
            left
        }
        _ => return None,
    };
    let comparison_authority = obligation.predicate.trait_key.compiler_authority()?;
    for (index, member) in obligation.environment.predicates().iter().enumerate() {
        let Some(bound) = member.as_trait() else {
            continue;
        };
        let Some(bound_authority) = bound.trait_key.compiler_authority() else {
            continue;
        };
        if bound_authority.kind != CompilerTraitKind::EcsKey
            || bound_authority.interface_version != comparison_authority.interface_version
            || bound_authority.interface_digest != comparison_authority.interface_digest
            || bound.self_type != *key_type
            || !bound.arguments.is_empty()
            || !bound.contains_bound_input()
        {
            continue;
        }
        let environment_index = u64::try_from(index).ok()?;
        return Some(TraitEvidence::PendingC4EcsKeyComparison(Box::new(
            PendingC4EcsKeyComparison {
                obligation: obligation.predicate.clone(),
                key_type: key_type.clone(),
                ecs_key_witness: BoundWitness {
                    obligation: bound.clone(),
                    environment_index,
                },
            },
        )));
    }
    None
}

fn needs_ctfe_for(
    obligation: &CanonicalTraitObligation,
) -> Result<NeedsCtfeObligations, TraitSolveError> {
    let dependency = NeedsCtfeObligation::from_canonical_bytes(obligation.canonical.to_vec())
        .expect("a canonical trait obligation is nonempty");
    Ok(NeedsCtfeObligations::from_unsorted(vec![dependency])
        .expect("the retained CTFE set is nonempty"))
}

fn wake_ready(
    blocked: &mut BTreeMap<Box<[u8]>, BTreeSet<Box<[u8]>>>,
    memo: &BTreeMap<Box<[u8]>, TraitSolveResult>,
    ready: &mut BTreeSet<Box<[u8]>>,
) {
    let awakened = blocked
        .iter()
        .filter(|(_, dependencies)| dependencies.iter().all(|key| memo.contains_key(key)))
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    for key in awakened {
        blocked.remove(&key);
        ready.insert(key);
    }
}

#[derive(Debug)]
enum HeadMatch {
    Match(TraitSubstitution),
    NoMatch,
    NeedsCtfe,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MatchState {
    Match,
    NoMatch,
    NeedsCtfe,
}

struct HeadUnifier<'a> {
    formals: &'a [GenericParameterKind],
    bindings: Vec<Option<GenericArgumentShape>>,
}

impl<'a> HeadUnifier<'a> {
    fn new(formals: &'a [GenericParameterKind]) -> Self {
        Self {
            formals,
            bindings: vec![None; formals.len()],
        }
    }

    fn finish(self) -> Result<TraitSubstitution, TraitSolveError> {
        let arguments = self
            .bindings
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or(TraitSolveError::InvalidCandidateGenericUse)?;
        Ok(TraitSubstitution {
            formals: self.formals.to_vec().into_boxed_slice(),
            arguments: arguments.into_boxed_slice(),
        })
    }

    fn bind(
        &mut self,
        index: u64,
        expected: &GenericParameterKind,
        value: GenericArgumentShape,
    ) -> Result<MatchState, TraitSolveError> {
        let index = usize::try_from(index)
            .ok()
            .filter(|index| self.formals.get(*index) == Some(expected))
            .ok_or(TraitSolveError::InvalidCandidateGenericUse)?;
        match &self.bindings[index] {
            None => {
                self.bindings[index] = Some(value);
                Ok(MatchState::Match)
            }
            Some(existing) if existing == &value => Ok(MatchState::Match),
            Some(existing)
                if generic_argument_contains_ctfe(existing)
                    || generic_argument_contains_ctfe(&value) =>
            {
                Ok(MatchState::NeedsCtfe)
            }
            Some(_) => Ok(MatchState::NoMatch),
        }
    }
}

fn unify_head(
    candidate: &OrdinaryImplCandidate,
    obligation: &TraitPredicate,
) -> Result<HeadMatch, TraitSolveError> {
    if candidate.head.trait_key != obligation.trait_key
        || candidate.head.arguments.len() != obligation.arguments.len()
    {
        return Ok(HeadMatch::NoMatch);
    }
    let mut unifier = HeadUnifier::new(&candidate.generic_parameters);
    let mut state = unify_type(
        &candidate.head.self_type,
        &obligation.self_type,
        &mut unifier,
    )?;
    for (pattern, actual) in candidate
        .head
        .arguments
        .iter()
        .zip(obligation.arguments.iter())
    {
        state = combine_match(state, unify_argument(pattern, actual, &mut unifier)?);
        if state == MatchState::NoMatch {
            return Ok(HeadMatch::NoMatch);
        }
    }
    match state {
        MatchState::Match => Ok(HeadMatch::Match(unifier.finish()?)),
        MatchState::NoMatch => Ok(HeadMatch::NoMatch),
        MatchState::NeedsCtfe => Ok(HeadMatch::NeedsCtfe),
    }
}

fn unify_type(
    pattern: &SymbolicType,
    actual: &SymbolicType,
    unifier: &mut HeadUnifier<'_>,
) -> Result<MatchState, TraitSolveError> {
    if pattern == actual {
        return Ok(MatchState::Match);
    }
    if let SymbolicType::BoundType { depth: 0, index } = pattern {
        return unifier.bind(
            *index,
            &GenericParameterKind::Type,
            GenericArgumentShape::Type(actual.clone()),
        );
    }
    let state = match (pattern, actual) {
        (SymbolicType::Slice(left), SymbolicType::Slice(right)) => {
            unify_type(left, right, unifier)?
        }
        (
            SymbolicType::Array {
                element: left_element,
                length: left_length,
            },
            SymbolicType::Array {
                element: right_element,
                length: right_length,
            },
        ) => combine_match(
            unify_type(left_element, right_element, unifier)?,
            unify_const(left_length, right_length, unifier)?,
        ),
        (SymbolicType::Tuple(left), SymbolicType::Tuple(right)) if left.len() == right.len() => {
            unify_types(left, right, unifier)?
        }
        (
            SymbolicType::Reference {
                mutability: left_mutability,
                lifetime: left_lifetime,
                pointee: left_pointee,
            },
            SymbolicType::Reference {
                mutability: right_mutability,
                lifetime: right_lifetime,
                pointee: right_pointee,
            },
        ) if left_mutability == right_mutability => combine_match(
            unify_lifetime(left_lifetime, right_lifetime, unifier)?,
            unify_type(left_pointee, right_pointee, unifier)?,
        ),
        (
            SymbolicType::RawPointer {
                mutability: left_mutability,
                pointee: left_pointee,
            },
            SymbolicType::RawPointer {
                mutability: right_mutability,
                pointee: right_pointee,
            },
        ) if left_mutability == right_mutability => {
            unify_type(left_pointee, right_pointee, unifier)?
        }
        (
            SymbolicType::NominalPath {
                declaration: left_declaration,
                arguments: left_arguments,
            },
            SymbolicType::NominalPath {
                declaration: right_declaration,
                arguments: right_arguments,
            },
        ) if left_declaration == right_declaration
            && left_arguments.len() == right_arguments.len() =>
        {
            unify_arguments(left_arguments, right_arguments, unifier)?
        }
        (
            SymbolicType::FunctionPointer {
                unsafe_: left_unsafe,
                parameters: left_parameters,
                result: left_result,
                requires: left_requires,
                throws: left_throws,
            },
            SymbolicType::FunctionPointer {
                unsafe_: right_unsafe,
                parameters: right_parameters,
                result: right_result,
                requires: right_requires,
                throws: right_throws,
            },
        ) if left_unsafe == right_unsafe
            && left_parameters.len() == right_parameters.len()
            && left_requires.members().len() == right_requires.members().len()
            && left_throws.members().len() == right_throws.members().len() =>
        {
            let mut state = unify_types(left_parameters, right_parameters, unifier)?;
            state = combine_match(state, unify_type(left_result, right_result, unifier)?);
            state = combine_match(
                state,
                unify_types(left_requires.members(), right_requires.members(), unifier)?,
            );
            combine_match(
                state,
                unify_types(left_throws.members(), right_throws.members(), unifier)?,
            )
        }
        _ => MatchState::NoMatch,
    };
    Ok(state)
}

fn unify_types(
    patterns: &[SymbolicType],
    actuals: &[SymbolicType],
    unifier: &mut HeadUnifier<'_>,
) -> Result<MatchState, TraitSolveError> {
    if patterns.len() != actuals.len() {
        return Ok(MatchState::NoMatch);
    }
    let mut state = MatchState::Match;
    for (pattern, actual) in patterns.iter().zip(actuals) {
        state = combine_match(state, unify_type(pattern, actual, unifier)?);
    }
    Ok(state)
}

fn unify_arguments(
    patterns: &[GenericArgumentShape],
    actuals: &[GenericArgumentShape],
    unifier: &mut HeadUnifier<'_>,
) -> Result<MatchState, TraitSolveError> {
    if patterns.len() != actuals.len() {
        return Ok(MatchState::NoMatch);
    }
    let mut state = MatchState::Match;
    for (pattern, actual) in patterns.iter().zip(actuals) {
        state = combine_match(state, unify_argument(pattern, actual, unifier)?);
    }
    Ok(state)
}

fn unify_argument(
    pattern: &GenericArgumentShape,
    actual: &GenericArgumentShape,
    unifier: &mut HeadUnifier<'_>,
) -> Result<MatchState, TraitSolveError> {
    Ok(match (pattern, actual) {
        (GenericArgumentShape::Type(left), GenericArgumentShape::Type(right)) => {
            unify_type(left, right, unifier)?
        }
        (GenericArgumentShape::Lifetime(left), GenericArgumentShape::Lifetime(right)) => {
            unify_lifetime(left, right, unifier)?
        }
        (GenericArgumentShape::IntegerConst(left), GenericArgumentShape::IntegerConst(right)) => {
            unify_const(left, right, unifier)?
        }
        _ => MatchState::NoMatch,
    })
}

fn unify_lifetime(
    pattern: &SymbolicLifetime,
    actual: &SymbolicLifetime,
    unifier: &mut HeadUnifier<'_>,
) -> Result<MatchState, TraitSolveError> {
    if pattern == actual {
        return Ok(MatchState::Match);
    }
    if let SymbolicLifetime::Bound { depth: 0, index } = pattern {
        return unifier.bind(
            *index,
            &GenericParameterKind::Lifetime,
            GenericArgumentShape::Lifetime(actual.clone()),
        );
    }
    Ok(MatchState::NoMatch)
}

fn unify_const(
    pattern: &SymbolicConstExpression,
    actual: &SymbolicConstExpression,
    unifier: &mut HeadUnifier<'_>,
) -> Result<MatchState, TraitSolveError> {
    if pattern == actual {
        return Ok(MatchState::Match);
    }
    if pattern.integer_type != actual.integer_type {
        return Ok(MatchState::NoMatch);
    }
    if let SymbolicConstNode::Bound { depth: 0, index } = pattern.node {
        return unifier.bind(
            index,
            &GenericParameterKind::IntegerConst(pattern.integer_type),
            GenericArgumentShape::IntegerConst(actual.clone()),
        );
    }
    if const_contains_bound(pattern) {
        return Err(TraitSolveError::UnsupportedConstUnification);
    }
    if const_contains_ctfe(pattern) || const_contains_ctfe(actual) {
        return Ok(MatchState::NeedsCtfe);
    }
    Ok(MatchState::NoMatch)
}

const fn combine_match(left: MatchState, right: MatchState) -> MatchState {
    match (left, right) {
        (MatchState::NoMatch, _) | (_, MatchState::NoMatch) => MatchState::NoMatch,
        (MatchState::NeedsCtfe, _) | (_, MatchState::NeedsCtfe) => MatchState::NeedsCtfe,
        (MatchState::Match, MatchState::Match) => MatchState::Match,
    }
}

fn substitute_environment(
    environment: &TraitEnvironment,
    substitution: &TraitSubstitution,
) -> Result<TraitEnvironment, TraitSolveError> {
    let frame = substitution.frame()?;
    let mut predicates = Vec::with_capacity(environment.predicates().len());
    for predicate in environment.predicates() {
        predicates.push(substitute_predicate(predicate, &frame)?);
    }
    Ok(TraitEnvironment::new(predicates)?)
}

fn substitute_predicate(
    predicate: &SemanticPredicate,
    frame: &TraitFrameSubstitution,
) -> Result<SemanticPredicate, TraitSolveError> {
    let frontend = match &predicate.kind {
        SemanticPredicateKind::Trait(bound) => SymbolicPredicate::Trait {
            trait_path: arche_frontend::SemanticDeclarationPath {
                registry_origin: String::new(),
                package_name: String::new(),
                target: arche_frontend::TargetRoot::Library,
                modules: Vec::new(),
                kind: DeclarationKind::Trait,
                name: String::new(),
            },
            self_type: bound.self_type.clone(),
            arguments: bound.arguments.to_vec(),
        },
        SemanticPredicateKind::LifetimeOutlives { longer, shorter } => {
            SymbolicPredicate::LifetimeOutlives {
                longer: longer.clone(),
                shorter: shorter.clone(),
            }
        }
        SemanticPredicateKind::TypeOutlives { ty, lifetime } => SymbolicPredicate::TypeOutlives {
            ty: ty.clone(),
            lifetime: lifetime.clone(),
        },
    };
    match (frame.substitute_predicate(&frontend, 0)?, &predicate.kind) {
        (
            SymbolicPredicate::Trait {
                self_type,
                arguments,
                ..
            },
            SemanticPredicateKind::Trait(bound),
        ) => Ok(SemanticPredicate::trait_bound(TraitPredicate::new(
            bound.trait_key.clone(),
            self_type,
            arguments,
        )?)),
        (
            SymbolicPredicate::LifetimeOutlives { longer, shorter },
            SemanticPredicateKind::LifetimeOutlives { .. },
        ) => Ok(SemanticPredicate::lifetime_outlives(longer, shorter)?),
        (
            SymbolicPredicate::TypeOutlives { ty, lifetime },
            SemanticPredicateKind::TypeOutlives { .. },
        ) => Ok(SemanticPredicate::type_outlives(ty, lifetime)?),
        _ => Err(TraitSolveError::InvalidCandidateGenericUse),
    }
}

fn validate_candidate_formals(
    formals: &[GenericParameterKind],
    head: &TraitPredicate,
) -> Result<(), TraitSolverBuildError> {
    let arguments = formals
        .iter()
        .enumerate()
        .map(|(index, kind)| dummy_argument(index, kind))
        .collect::<Vec<_>>();
    let frame = TraitFrameSubstitution::new(formals.to_vec(), arguments, SymbolicType::Unit)
        .map_err(TraitSolverBuildError::InvalidGenericHead)?;
    let frontend = SymbolicPredicate::Trait {
        trait_path: arche_frontend::SemanticDeclarationPath {
            registry_origin: String::new(),
            package_name: String::new(),
            target: arche_frontend::TargetRoot::Library,
            modules: Vec::new(),
            kind: DeclarationKind::Trait,
            name: String::new(),
        },
        self_type: head.self_type.clone(),
        arguments: head.arguments.to_vec(),
    };
    frame
        .substitute_predicate(&frontend, 0)
        .map_err(TraitSolverBuildError::InvalidGenericHead)?;
    let mut used = BTreeSet::new();
    collect_head_formals(head, &mut used);
    for index in 0..formals.len() {
        let index_u64 = u64::try_from(index).expect("generic index fits u64");
        if !used.contains(&index_u64) {
            return Err(TraitSolverBuildError::UnconstrainedGenericParameter {
                implementation_trait: head.trait_key.clone(),
                index: index_u64,
            });
        }
    }
    Ok(())
}

fn dummy_argument(index: usize, kind: &GenericParameterKind) -> GenericArgumentShape {
    let index = u64::try_from(index).expect("generic index fits u64");
    match kind {
        GenericParameterKind::Type => GenericArgumentShape::Type(SymbolicType::BoundType {
            depth: u64::MAX,
            index,
        }),
        GenericParameterKind::Lifetime => GenericArgumentShape::Lifetime(SymbolicLifetime::Bound {
            depth: u64::MAX,
            index,
        }),
        GenericParameterKind::IntegerConst(integer_type) => {
            GenericArgumentShape::IntegerConst(SymbolicConstExpression {
                integer_type: *integer_type,
                node: SymbolicConstNode::Bound {
                    depth: u64::MAX,
                    index,
                },
            })
        }
    }
}

fn collect_head_formals(head: &TraitPredicate, used: &mut BTreeSet<u64>) {
    collect_type_formals(&head.self_type, used);
    for argument in &head.arguments {
        collect_argument_formals(argument, used);
    }
}

fn collect_argument_formals(argument: &GenericArgumentShape, used: &mut BTreeSet<u64>) {
    match argument {
        GenericArgumentShape::Type(ty) => collect_type_formals(ty, used),
        GenericArgumentShape::Lifetime(SymbolicLifetime::Bound { depth: 0, index }) => {
            used.insert(*index);
        }
        GenericArgumentShape::IntegerConst(value) => collect_const_formals(value, used),
        GenericArgumentShape::Lifetime(
            SymbolicLifetime::Static
            | SymbolicLifetime::ErasedLocal
            | SymbolicLifetime::Bound { .. },
        ) => {}
    }
}

fn collect_type_formals(ty: &SymbolicType, used: &mut BTreeSet<u64>) {
    match ty {
        SymbolicType::BoundType { depth: 0, index } => {
            used.insert(*index);
        }
        SymbolicType::Slice(element) => collect_type_formals(element, used),
        SymbolicType::Array { element, length } => {
            collect_type_formals(element, used);
            collect_const_formals(length, used);
        }
        SymbolicType::Tuple(elements) => {
            for element in elements {
                collect_type_formals(element, used);
            }
        }
        SymbolicType::Reference {
            lifetime, pointee, ..
        } => {
            if let SymbolicLifetime::Bound { depth: 0, index } = lifetime {
                used.insert(*index);
            }
            collect_type_formals(pointee, used);
        }
        SymbolicType::RawPointer { pointee, .. } => collect_type_formals(pointee, used),
        SymbolicType::NominalPath { arguments, .. } => {
            for argument in arguments {
                collect_argument_formals(argument, used);
            }
        }
        SymbolicType::FunctionPointer {
            parameters,
            result,
            requires,
            throws,
            ..
        } => {
            for ty in parameters
                .iter()
                .chain(std::iter::once(result.as_ref()))
                .chain(requires.members())
                .chain(throws.members())
            {
                collect_type_formals(ty, used);
            }
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
        | SymbolicType::BoundType { .. }
        | SymbolicType::Closure { .. }
        | SymbolicType::Generator { .. }
        | SymbolicType::JoinHandle { .. }
        | SymbolicType::GeneratorFactory { .. } => {}
    }
}

fn collect_const_formals(value: &SymbolicConstExpression, used: &mut BTreeSet<u64>) {
    fn visit(node_value: &SymbolicConstNode, used: &mut BTreeSet<u64>) {
        match node_value {
            SymbolicConstNode::Bound { depth: 0, index } => {
                used.insert(*index);
            }
            SymbolicConstNode::WrappingNeg(value) | SymbolicConstNode::BitNot(value) => {
                visit(&value.node, used);
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
                visit(&left.node, used);
                visit(&right.node, used);
            }
            SymbolicConstNode::IntegerLiteral(_)
            | SymbolicConstNode::ConstDefinitionPath(_)
            | SymbolicConstNode::Bound { .. } => {}
        }
    }
    visit(&value.node, used);
}

fn validate_coherence(
    candidates: &BTreeMap<OrdinarySemanticImplKey, OrdinaryImplCandidate>,
) -> Result<(), TraitSolverBuildError> {
    let rows = candidates.values().collect::<Vec<_>>();
    for (index, left) in rows.iter().enumerate() {
        for right in rows.iter().skip(index + 1) {
            if left.head.trait_key != right.head.trait_key || !heads_may_overlap(left, right) {
                continue;
            }
            if left.head.canonical == right.head.canonical && left.predicates == right.predicates {
                return Err(TraitSolverBuildError::ExactImplOverlap {
                    first: left.key.clone(),
                    second: right.key.clone(),
                });
            }
            let left_child = is_strict_specialization_build(left, right)?;
            let right_child = is_strict_specialization_build(right, left)?;
            let legal = (right.is_default && left_child) || (left.is_default && right_child);
            if !legal {
                return Err(TraitSolverBuildError::IllegalImplOverlap {
                    first: left.key.clone(),
                    second: right.key.clone(),
                });
            }
        }
    }
    Ok(())
}

fn is_strict_specialization_build(
    child: &OrdinaryImplCandidate,
    parent: &OrdinaryImplCandidate,
) -> Result<bool, TraitSolverBuildError> {
    match strict_specialization_relation(child, parent) {
        Ok(value) => Ok(value),
        Err(_) => Err(TraitSolverBuildError::IllegalImplOverlap {
            first: child.key.clone(),
            second: parent.key.clone(),
        }),
    }
}

fn is_strict_specialization(
    child: &OrdinaryImplCandidate,
    parent: &OrdinaryImplCandidate,
) -> Result<bool, TraitSolveError> {
    strict_specialization_relation(child, parent).map_err(|_| {
        TraitSolveError::UnsupportedSpecializationEntailment {
            parent: parent.key.clone(),
            child: child.key.clone(),
        }
    })
}

fn strict_specialization_relation(
    child: &OrdinaryImplCandidate,
    parent: &OrdinaryImplCandidate,
) -> Result<bool, ()> {
    if !parent.is_default {
        return Ok(false);
    }
    if !candidate_subsumes(parent, child)? {
        return Ok(false);
    }
    Ok(!candidate_subsumes(child, parent)?)
}

fn candidate_subsumes(
    general: &OrdinaryImplCandidate,
    specific: &OrdinaryImplCandidate,
) -> Result<bool, ()> {
    let rigid = rigidify_candidate(specific).map_err(|_| ())?;
    let substitution = match unify_head(general, &rigid.head).map_err(|_| ())? {
        HeadMatch::Match(substitution) => substitution,
        HeadMatch::NoMatch => return Ok(false),
        HeadMatch::NeedsCtfe => return Err(()),
    };
    let general_predicates =
        substitute_environment(&general.predicates, &substitution).map_err(|_| ())?;
    if general_predicates.predicates().is_empty() {
        return Ok(true);
    }
    let specific_predicates = rigid.predicates;
    for predicate in general_predicates.predicates() {
        if specific_predicates.exact(predicate).is_none() {
            return Err(());
        }
    }
    Ok(true)
}

fn rigidify_candidate(
    candidate: &OrdinaryImplCandidate,
) -> Result<OrdinaryImplCandidate, TraitSolveError> {
    let arguments = candidate
        .generic_parameters
        .iter()
        .enumerate()
        .map(|(index, kind)| dummy_argument(index, kind))
        .collect::<Vec<_>>();
    let substitution = TraitSubstitution {
        formals: candidate.generic_parameters.clone(),
        arguments: arguments.into_boxed_slice(),
    };
    let head_environment =
        TraitEnvironment::new(vec![SemanticPredicate::trait_bound(candidate.head.clone())])?;
    let head = substitute_environment(&head_environment, &substitution)?
        .predicates()
        .first()
        .and_then(SemanticPredicate::as_trait)
        .cloned()
        .ok_or(TraitSolveError::InvalidCandidateGenericUse)?;
    Ok(OrdinaryImplCandidate {
        key: candidate.key.clone(),
        is_default: candidate.is_default,
        generic_parameters: Box::new([]),
        head,
        predicates: substitute_environment(&candidate.predicates, &substitution)?,
    })
}

fn heads_may_overlap(left: &OrdinaryImplCandidate, right: &OrdinaryImplCandidate) -> bool {
    patterns_compatible_type(&left.head.self_type, &right.head.self_type)
        && left.head.arguments.len() == right.head.arguments.len()
        && left
            .head
            .arguments
            .iter()
            .zip(right.head.arguments.iter())
            .all(|(left, right)| patterns_compatible_argument(left, right))
}

fn patterns_compatible_argument(left: &GenericArgumentShape, right: &GenericArgumentShape) -> bool {
    match (left, right) {
        (GenericArgumentShape::Type(left), GenericArgumentShape::Type(right)) => {
            patterns_compatible_type(left, right)
        }
        (GenericArgumentShape::Lifetime(left), GenericArgumentShape::Lifetime(right)) => {
            matches!(left, SymbolicLifetime::Bound { depth: 0, .. })
                || matches!(right, SymbolicLifetime::Bound { depth: 0, .. })
                || left == right
        }
        (GenericArgumentShape::IntegerConst(left), GenericArgumentShape::IntegerConst(right)) => {
            left.integer_type == right.integer_type
                && (matches!(left.node, SymbolicConstNode::Bound { depth: 0, .. })
                    || matches!(right.node, SymbolicConstNode::Bound { depth: 0, .. })
                    || const_contains_ctfe(left)
                    || const_contains_ctfe(right)
                    || left == right)
        }
        _ => false,
    }
}

fn patterns_compatible_type(left: &SymbolicType, right: &SymbolicType) -> bool {
    if left == right
        || matches!(left, SymbolicType::BoundType { depth: 0, .. })
        || matches!(right, SymbolicType::BoundType { depth: 0, .. })
    {
        return true;
    }
    match (left, right) {
        (SymbolicType::Slice(left), SymbolicType::Slice(right)) => {
            patterns_compatible_type(left, right)
        }
        (
            SymbolicType::Array {
                element: left_element,
                length: left_length,
            },
            SymbolicType::Array {
                element: right_element,
                length: right_length,
            },
        ) => {
            patterns_compatible_type(left_element, right_element)
                && patterns_compatible_argument(
                    &GenericArgumentShape::IntegerConst(left_length.clone()),
                    &GenericArgumentShape::IntegerConst(right_length.clone()),
                )
        }
        (SymbolicType::Tuple(left), SymbolicType::Tuple(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| patterns_compatible_type(left, right))
        }
        (
            SymbolicType::Reference {
                mutability: left_mutability,
                pointee: left_pointee,
                ..
            },
            SymbolicType::Reference {
                mutability: right_mutability,
                pointee: right_pointee,
                ..
            },
        ) => {
            left_mutability == right_mutability
                && patterns_compatible_type(left_pointee, right_pointee)
        }
        (
            SymbolicType::RawPointer {
                mutability: left_mutability,
                pointee: left_pointee,
            },
            SymbolicType::RawPointer {
                mutability: right_mutability,
                pointee: right_pointee,
            },
        ) => {
            left_mutability == right_mutability
                && patterns_compatible_type(left_pointee, right_pointee)
        }
        (
            SymbolicType::NominalPath {
                declaration: left_declaration,
                arguments: left_arguments,
            },
            SymbolicType::NominalPath {
                declaration: right_declaration,
                arguments: right_arguments,
            },
        ) => {
            left_declaration == right_declaration
                && left_arguments.len() == right_arguments.len()
                && left_arguments
                    .iter()
                    .zip(right_arguments)
                    .all(|(left, right)| patterns_compatible_argument(left, right))
        }
        _ => false,
    }
}

fn type_contains_bound(ty: &SymbolicType) -> bool {
    match ty {
        SymbolicType::BoundType { .. } => true,
        SymbolicType::Slice(element) => type_contains_bound(element),
        SymbolicType::Array { element, length } => {
            type_contains_bound(element) || const_contains_bound(length)
        }
        SymbolicType::Tuple(elements) => elements.iter().any(type_contains_bound),
        SymbolicType::Reference {
            lifetime, pointee, ..
        } => lifetime_contains_bound(lifetime) || type_contains_bound(pointee),
        SymbolicType::RawPointer { pointee, .. } => type_contains_bound(pointee),
        SymbolicType::NominalPath { arguments, .. } => {
            arguments.iter().any(generic_argument_contains_bound)
        }
        SymbolicType::FunctionPointer {
            parameters,
            result,
            requires,
            throws,
            ..
        } => {
            parameters.iter().any(type_contains_bound)
                || type_contains_bound(result)
                || requires.members().iter().any(type_contains_bound)
                || throws.members().iter().any(type_contains_bound)
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
        | SymbolicType::Closure { .. }
        | SymbolicType::Generator { .. }
        | SymbolicType::JoinHandle { .. }
        | SymbolicType::GeneratorFactory { .. } => false,
    }
}

fn generic_argument_contains_bound(argument: &GenericArgumentShape) -> bool {
    match argument {
        GenericArgumentShape::Type(ty) => type_contains_bound(ty),
        GenericArgumentShape::Lifetime(lifetime) => lifetime_contains_bound(lifetime),
        GenericArgumentShape::IntegerConst(value) => const_contains_bound(value),
    }
}

fn lifetime_contains_bound(lifetime: &SymbolicLifetime) -> bool {
    matches!(lifetime, SymbolicLifetime::Bound { .. })
}

fn const_contains_bound(value: &SymbolicConstExpression) -> bool {
    fn node(value: &SymbolicConstNode) -> bool {
        match value {
            SymbolicConstNode::Bound { .. } => true,
            SymbolicConstNode::WrappingNeg(value) | SymbolicConstNode::BitNot(value) => {
                node(&value.node)
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
            | SymbolicConstNode::BitOr(left, right) => node(&left.node) || node(&right.node),
            SymbolicConstNode::IntegerLiteral(_) | SymbolicConstNode::ConstDefinitionPath(_) => {
                false
            }
        }
    }
    node(&value.node)
}

fn type_contains_ctfe(ty: &SymbolicType) -> bool {
    match ty {
        SymbolicType::Slice(element) => type_contains_ctfe(element),
        SymbolicType::Array { element, length } => {
            type_contains_ctfe(element) || const_contains_ctfe(length)
        }
        SymbolicType::Tuple(elements) => elements.iter().any(type_contains_ctfe),
        SymbolicType::Reference { pointee, .. } | SymbolicType::RawPointer { pointee, .. } => {
            type_contains_ctfe(pointee)
        }
        SymbolicType::NominalPath { arguments, .. } => {
            arguments.iter().any(generic_argument_contains_ctfe)
        }
        SymbolicType::FunctionPointer {
            parameters,
            result,
            requires,
            throws,
            ..
        } => {
            parameters.iter().any(type_contains_ctfe)
                || type_contains_ctfe(result)
                || requires.members().iter().any(type_contains_ctfe)
                || throws.members().iter().any(type_contains_ctfe)
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
        | SymbolicType::BoundType { .. }
        | SymbolicType::Closure { .. }
        | SymbolicType::Generator { .. }
        | SymbolicType::JoinHandle { .. }
        | SymbolicType::GeneratorFactory { .. } => false,
    }
}

fn generic_argument_contains_ctfe(argument: &GenericArgumentShape) -> bool {
    match argument {
        GenericArgumentShape::Type(ty) => type_contains_ctfe(ty),
        GenericArgumentShape::Lifetime(_) => false,
        GenericArgumentShape::IntegerConst(value) => const_contains_ctfe(value),
    }
}

fn const_contains_ctfe(value: &SymbolicConstExpression) -> bool {
    fn node(value: &SymbolicConstNode) -> bool {
        match value {
            SymbolicConstNode::ConstDefinitionPath(_) => true,
            SymbolicConstNode::WrappingNeg(value) | SymbolicConstNode::BitNot(value) => {
                node(&value.node)
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
            | SymbolicConstNode::BitOr(left, right) => node(&left.node) || node(&right.node),
            SymbolicConstNode::IntegerLiteral(_) | SymbolicConstNode::Bound { .. } => false,
        }
    }
    node(&value.node)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use arche_frontend::embedded_core::verified_embedded_core_authority;
    use arche_frontend::{
        check_workspace_c1, DependencyKind, IntegerType, Mutability, SemanticDeclarationPath,
        TargetRoot,
    };
    use arche_package::{load_workspace, resolve, ManifestRequest, RegistrySnapshot};

    use super::*;
    use crate::{C2Handoff, DeclarationTable};

    fn package(byte: u8) -> PackageId {
        PackageId::from_bytes([byte; 16])
    }

    fn core() -> Arc<VerifiedEmbeddedCoreAuthority> {
        verified_embedded_core_authority().unwrap()
    }

    fn compiler_key(
        core: &VerifiedEmbeddedCoreAuthority,
        kind: CompilerTraitKind,
    ) -> SemanticTraitKey {
        SemanticTraitKey::from_verified_embedded_core(core, kind)
    }

    fn type_arguments(types: &[SymbolicType]) -> Vec<GenericArgumentShape> {
        types
            .iter()
            .cloned()
            .map(GenericArgumentShape::Type)
            .collect()
    }

    fn compiler_predicate(
        core: &VerifiedEmbeddedCoreAuthority,
        kind: CompilerTraitKind,
        self_type: SymbolicType,
        arguments: Vec<GenericArgumentShape>,
    ) -> TraitPredicate {
        TraitPredicate::new(compiler_key(core, kind), self_type, arguments).unwrap()
    }

    fn obligation(
        predicate: TraitPredicate,
        environment: TraitEnvironment,
    ) -> CanonicalTraitObligation {
        CanonicalTraitObligation::new(predicate, environment).unwrap()
    }

    fn empty_environment() -> TraitEnvironment {
        TraitEnvironment::default()
    }

    fn solve_compiler(
        kind: CompilerTraitKind,
        self_type: SymbolicType,
        arguments: Vec<GenericArgumentShape>,
    ) -> TraitSolveResult {
        let core = core();
        let predicate = compiler_predicate(&core, kind, self_type, arguments);
        TraitSolver::default()
            .solve(obligation(predicate, empty_environment()))
            .unwrap()
    }

    #[test]
    fn key_bytes_use_complete_ordinary_bytes_and_raw_compiler_definition_id() {
        let ordinary = SemanticTraitKey::ordinary_for_test(package(1), b"complete-key");
        let mut expected = vec![1];
        expected.extend_from_slice(&12_u64.to_le_bytes());
        expected.extend_from_slice(b"complete-key");
        assert_eq!(ordinary.canonical_bytes(), expected);

        let core = core();
        let definition = core
            .typed_c2()
            .compiler_trait(CompilerTraitKind::Add)
            .definition_id();
        let compiler = SemanticTraitKey::from_verified_embedded_core(&core, CompilerTraitKind::Add);
        let mut expected = vec![2];
        expected.extend_from_slice(&core.interface_version().to_le_bytes());
        expected.extend_from_slice(core.interface_digest());
        expected.extend_from_slice(definition.as_bytes());
        assert_eq!(compiler.canonical_bytes(), expected);
        assert_eq!(compiler.canonical_bytes().len(), 53);
    }

    #[test]
    fn every_compiler_trait_enforces_arity_and_designated_self() {
        let core = core();
        for row in core.typed_c2().compiler_traits() {
            let key = compiler_key(&core, row.kind());
            let arguments = vec![
                GenericArgumentShape::Type(SymbolicType::I32);
                usize::from(row.explicit_generic_arity())
            ];
            TraitPredicate::new(key.clone(), SymbolicType::I32, arguments.clone()).unwrap();
            let mut wrong_arity = arguments.clone();
            wrong_arity.push(GenericArgumentShape::Type(SymbolicType::I32));
            assert!(matches!(
                TraitPredicate::new(key.clone(), SymbolicType::I32, wrong_arity),
                Err(TraitModelError::WrongCompilerTraitArity { .. })
            ));
            if !matches!(
                row.designated_self(),
                CompilerTraitSelfRelation::OperatedType | CompilerTraitSelfRelation::CallableType
            ) {
                assert_eq!(
                    TraitPredicate::new(key, SymbolicType::U32, arguments).unwrap_err(),
                    TraitModelError::CompilerTraitDesignatedSelfMismatch(row.kind())
                );
            }
        }
    }

    #[test]
    fn primitive_matrix_has_exact_rows_and_no_conversion_clone_or_iterator_fallbacks() {
        assert!(matches!(
            solve_compiler(
                CompilerTraitKind::Add,
                SymbolicType::I32,
                type_arguments(&[SymbolicType::I32, SymbolicType::I32, SymbolicType::I32]),
            ),
            TraitSolveResult::Selected(TraitEvidence::SealedPrimitiveOperator(_))
        ));
        assert_eq!(
            solve_compiler(
                CompilerTraitKind::Add,
                SymbolicType::I32,
                type_arguments(&[SymbolicType::I32, SymbolicType::U32, SymbolicType::I32]),
            ),
            TraitSolveResult::Unsatisfied
        );
        assert_eq!(
            solve_compiler(
                CompilerTraitKind::LogicalNot,
                SymbolicType::I32,
                type_arguments(&[SymbolicType::I32, SymbolicType::I32]),
            ),
            TraitSolveResult::Unsatisfied
        );

        for (kind, arguments) in [
            (
                CompilerTraitKind::From,
                type_arguments(&[SymbolicType::I32, SymbolicType::I32]),
            ),
            (
                CompilerTraitKind::TryFrom,
                type_arguments(&[SymbolicType::I32, SymbolicType::I32, SymbolicType::Unit]),
            ),
            (CompilerTraitKind::Clone, Vec::new()),
            (
                CompilerTraitKind::IntoIterator,
                type_arguments(&[SymbolicType::I32, SymbolicType::I32]),
            ),
            (
                CompilerTraitKind::Iterator,
                type_arguments(&[SymbolicType::I32, SymbolicType::I32]),
            ),
        ] {
            assert_eq!(
                solve_compiler(kind, SymbolicType::I32, arguments),
                TraitSolveResult::Unsatisfied,
                "{kind:?} must not gain a compiler candidate"
            );
        }
    }

    #[test]
    fn sealed_copy_and_exact_bound_witness_are_distinct_evidence() {
        assert!(matches!(
            solve_compiler(CompilerTraitKind::Copy, SymbolicType::I32, Vec::new()),
            TraitSolveResult::Selected(TraitEvidence::SealedCopy(_))
        ));
        assert_eq!(
            solve_compiler(
                CompilerTraitKind::Copy,
                SymbolicType::Reference {
                    mutability: Mutability::Mutable,
                    lifetime: SymbolicLifetime::Static,
                    pointee: Box::new(SymbolicType::I32),
                },
                Vec::new(),
            ),
            TraitSolveResult::Unsatisfied
        );

        let core = core();
        let bound = compiler_predicate(
            &core,
            CompilerTraitKind::Clone,
            SymbolicType::BoundType { depth: 0, index: 0 },
            Vec::new(),
        );
        let environment =
            TraitEnvironment::new(vec![SemanticPredicate::trait_bound(bound.clone())]).unwrap();
        let result = TraitSolver::default()
            .solve(obligation(bound.clone(), environment))
            .unwrap();
        let TraitSolveResult::Selected(TraitEvidence::BoundWitness(witness)) = result else {
            panic!("expected an exact bound witness")
        };
        assert_eq!(witness.environment_index(), 0);
        assert_eq!(
            witness.obligation().canonical_bytes(),
            bound.canonical_bytes()
        );

        let concrete_environment =
            TraitEnvironment::new(vec![SemanticPredicate::trait_bound(compiler_predicate(
                &core,
                CompilerTraitKind::Clone,
                SymbolicType::I32,
                Vec::new(),
            ))])
            .unwrap();
        assert_eq!(
            TraitSolver::default()
                .solve(obligation(
                    compiler_predicate(
                        &core,
                        CompilerTraitKind::Clone,
                        SymbolicType::I32,
                        Vec::new(),
                    ),
                    concrete_environment,
                ))
                .unwrap(),
            TraitSolveResult::Unsatisfied
        );
    }

    fn ordinary_candidate(
        impl_byte: u8,
        impl_package: PackageId,
        trait_key: SemanticTraitKey,
        is_default: bool,
        formals: Vec<GenericParameterKind>,
        self_type: SymbolicType,
    ) -> Result<OrdinaryImplCandidate, TraitSolverBuildError> {
        OrdinaryImplCandidate::from_parts(
            OrdinarySemanticImplKey {
                owner_package: impl_package,
                definition_key: vec![impl_byte].into_boxed_slice(),
            },
            OrdinaryImplCandidateSpec::new(
                is_default,
                formals,
                TraitPredicate::new(trait_key, self_type, Vec::new()).unwrap(),
                TraitEnvironment::default(),
            ),
        )
    }

    #[test]
    fn orphan_exact_overlap_and_default_chain_are_validated_before_selection() {
        let owner = package(1);
        let foreign = package(2);
        let trait_key = SemanticTraitKey::ordinary_for_test(owner, b"Trait");
        assert!(matches!(
            ordinary_candidate(
                1,
                foreign,
                trait_key.clone(),
                false,
                Vec::new(),
                SymbolicType::I32,
            ),
            Err(TraitSolverBuildError::OrphanRuleViolation { .. })
        ));

        let nominal_package = PackageName::from_str("fixtures/owned").unwrap();
        let nominal_owner = canonical_package_id(&nominal_package);
        let owned_nominal = SymbolicType::NominalPath {
            declaration: SemanticDeclarationPath {
                registry_origin: "workspace".to_owned(),
                package_name: nominal_package.as_str().to_owned(),
                target: TargetRoot::Library,
                modules: vec!["types".to_owned()],
                kind: DeclarationKind::Struct,
                name: "Owned".to_owned(),
            },
            arguments: Vec::new(),
        };
        ordinary_candidate(
            3,
            nominal_owner,
            trait_key.clone(),
            false,
            Vec::new(),
            owned_nominal.clone(),
        )
        .unwrap();
        assert!(matches!(
            ordinary_candidate(
                4,
                foreign,
                trait_key.clone(),
                false,
                Vec::new(),
                owned_nominal,
            ),
            Err(TraitSolverBuildError::OrphanRuleViolation { .. })
        ));

        let duplicate_a = ordinary_candidate(
            1,
            owner,
            trait_key.clone(),
            false,
            Vec::new(),
            SymbolicType::I32,
        )
        .unwrap();
        let duplicate_b = ordinary_candidate(
            2,
            owner,
            trait_key.clone(),
            true,
            Vec::new(),
            SymbolicType::I32,
        )
        .unwrap();
        assert!(matches!(
            TraitSolver::for_test(vec![duplicate_a, duplicate_b]),
            Err(TraitSolverBuildError::ExactImplOverlap { .. })
        ));

        let parent = ordinary_candidate(
            1,
            owner,
            trait_key.clone(),
            true,
            vec![GenericParameterKind::Type],
            SymbolicType::BoundType { depth: 0, index: 0 },
        )
        .unwrap();
        let child = ordinary_candidate(
            2,
            owner,
            trait_key.clone(),
            false,
            Vec::new(),
            SymbolicType::I32,
        )
        .unwrap();
        let mut forward = TraitSolver::for_test(vec![parent.clone(), child.clone()]).unwrap();
        let mut reverse = TraitSolver::for_test(vec![child, parent]).unwrap();
        let i32_obligation = obligation(
            TraitPredicate::new(trait_key.clone(), SymbolicType::I32, Vec::new()).unwrap(),
            empty_environment(),
        );
        let forward_result = forward.solve(i32_obligation.clone()).unwrap();
        let reverse_result = reverse.solve(i32_obligation).unwrap();
        assert_eq!(forward_result, reverse_result);
        let TraitSolveResult::Selected(TraitEvidence::Ordinary(selection)) = forward_result else {
            panic!("the concrete child must be the unique maximal candidate")
        };
        assert_eq!(selection.implementation().definition_key_bytes(), &[2]);

        let generic_result = forward
            .solve(obligation(
                TraitPredicate::new(trait_key, SymbolicType::U32, Vec::new()).unwrap(),
                empty_environment(),
            ))
            .unwrap();
        let TraitSolveResult::Selected(TraitEvidence::Ordinary(selection)) = generic_result else {
            panic!("the default parent must cover the remaining domain")
        };
        assert_eq!(selection.implementation().definition_key_bytes(), &[1]);
    }

    #[test]
    fn potentially_viable_const_candidate_defers_instead_of_disappearing() {
        let owner = package(1);
        let trait_key = SemanticTraitKey::ordinary_for_test(owner, b"ConstTrait");
        let path = SemanticDeclarationPath {
            registry_origin: "workspace".to_owned(),
            package_name: "fixture".to_owned(),
            target: TargetRoot::Library,
            modules: vec!["constants".to_owned()],
            kind: DeclarationKind::Const,
            name: "WIDTH".to_owned(),
        };
        let candidate_type = SymbolicType::Array {
            element: Box::new(SymbolicType::U8),
            length: SymbolicConstExpression {
                integer_type: IntegerType::Usize,
                node: SymbolicConstNode::ConstDefinitionPath(path),
            },
        };
        let candidate = ordinary_candidate(
            1,
            owner,
            trait_key.clone(),
            false,
            Vec::new(),
            candidate_type,
        )
        .unwrap();
        let actual_type = SymbolicType::Array {
            element: Box::new(SymbolicType::U8),
            length: SymbolicConstExpression {
                integer_type: IntegerType::Usize,
                node: SymbolicConstNode::IntegerLiteral(4_u64.to_le_bytes().to_vec()),
            },
        };
        let mut solver = TraitSolver::for_test(vec![candidate]).unwrap();
        let result = solver
            .solve(obligation(
                TraitPredicate::new(trait_key, actual_type, Vec::new()).unwrap(),
                empty_environment(),
            ))
            .unwrap();
        let TraitSolveResult::NeedsCtfe(dependencies) = result else {
            panic!("the potentially viable const candidate must defer")
        };
        assert_eq!(dependencies.len(), 1);
        assert!(!dependencies.as_slice()[0].canonical_bytes().is_empty());
    }

    #[test]
    fn ecs_key_bound_furnishes_only_the_exact_pending_c4_comparison() {
        let core = core();
        let key_type = SymbolicType::BoundType { depth: 0, index: 0 };
        let ecs_key = compiler_predicate(
            &core,
            CompilerTraitKind::EcsKey,
            key_type.clone(),
            Vec::new(),
        );
        let environment =
            TraitEnvironment::new(vec![SemanticPredicate::trait_bound(ecs_key.clone())]).unwrap();
        let eq = compiler_predicate(
            &core,
            CompilerTraitKind::Eq,
            key_type.clone(),
            type_arguments(&[key_type.clone(), key_type.clone()]),
        );
        let result = TraitSolver::default()
            .solve(obligation(eq.clone(), environment))
            .unwrap();
        let TraitSolveResult::Selected(TraitEvidence::PendingC4EcsKeyComparison(continuation)) =
            result
        else {
            panic!("the EcsKey entailment must retain a typed C4 continuation")
        };
        assert_eq!(
            continuation.obligation().canonical_bytes(),
            eq.canonical_bytes()
        );
        assert_eq!(continuation.key_type(), &key_type);
        assert_eq!(continuation.ecs_key_witness().obligation(), &ecs_key);
        assert_eq!(continuation.ecs_key_witness().environment_index(), 0);
    }

    #[test]
    fn solver_ingests_only_the_declaration_universes_current_and_normal_rows() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../tests/m27c2/v1/language-game");
        let workspace = load_workspace(&ManifestRequest::discover_from(&root)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        let handoff =
            C2Handoff::begin(check_workspace_c1(&workspace, &graph, &[]).unwrap()).unwrap();
        let table = DeclarationTable::build(&handoff).unwrap();
        let package = handoff
            .frontend()
            .hir()
            .packages
            .iter()
            .find(|package| {
                package
                    .targets
                    .iter()
                    .any(|target| matches!(target.target, TargetRoot::Binary(_)))
            })
            .unwrap();
        let binary = package
            .targets
            .iter()
            .find(|target| matches!(target.target, TargetRoot::Binary(_)))
            .unwrap();
        let library = package
            .targets
            .iter()
            .find(|target| target.target == TargetRoot::Library)
            .unwrap();
        let development = handoff
            .frontend()
            .inventory()
            .packages
            .iter()
            .find(|candidate| candidate.package == package.package)
            .unwrap()
            .provenance
            .dependencies
            .iter()
            .find(|dependency| dependency.kind == DependencyKind::Development)
            .unwrap()
            .package;

        let binary_universe = table
            .ordinary_impl_candidates(package.package, binary.id)
            .unwrap();
        let library_universe = table
            .ordinary_impl_candidates(package.package, library.id)
            .unwrap();
        assert!(library_universe.candidates().any(|candidate| {
            candidate.package() == package.package && candidate.target() == library.id
        }));
        assert!(binary_universe
            .candidates()
            .all(|candidate| candidate.package() != package.package
                || candidate.target() == binary.id));
        assert!(binary_universe
            .candidates()
            .all(|candidate| candidate.package() != development));

        // The solver's only public ingestion path consumes this already-exact
        // universe, so neither excluded sibling/dev row can be reintroduced.
        let solver = TraitSolver::from_universe(&binary_universe, |_| Ok(None)).unwrap();
        assert!(solver.candidates.is_empty());
    }
}
