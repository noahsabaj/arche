#![allow(
    dead_code,
    reason = "crate-private diagnostic constructors are reserved for C2 checker modules"
)]

use std::cmp::Ordering;

use arche_frontend::{Diagnostic, Label, Span, TargetId};
use arche_package::PortablePath;

/// Normative compilation-phase order used by M27-C diagnostics.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CompilationPhase {
    /// Manifest, workspace, dependency, and lock validation.
    ManifestWorkspaceDependencyLock,
    /// Lexing and parsing.
    LexParse,
    /// Module and name resolution.
    ModuleNameResolution,
    /// Declaration, type, trait, and coherence checking.
    DeclarationTypeTraitCoherence,
    /// Body, call, operator, and pattern checking.
    BodyCallOperatorPattern,
    /// Move, borrow, lifetime, unsafe, and drop checking.
    MoveBorrowLifetimeUnsafeDrop,
    /// Effects, capabilities, closures, generators, threads, and ECS checking.
    EffectCapabilityClosureGeneratorThreadEcs,
    /// Immutable include acquisition and input validation.
    IncludeAcquisitionInputValidation,
    /// Dependency-ready RootSlice construction and verification.
    RootSliceCore,
    /// CTFE execution and result promotion.
    Ctfe,
    /// Stable semantic identity finalization.
    IdentityFinalization,
    /// CompleteWorkspace construction and verification.
    CompleteWorkspaceCore,
}

impl CompilationPhase {
    /// Every phase in the normative diagnostic order.
    pub const ALL: [Self; 12] = [
        Self::ManifestWorkspaceDependencyLock,
        Self::LexParse,
        Self::ModuleNameResolution,
        Self::DeclarationTypeTraitCoherence,
        Self::BodyCallOperatorPattern,
        Self::MoveBorrowLifetimeUnsafeDrop,
        Self::EffectCapabilityClosureGeneratorThreadEcs,
        Self::IncludeAcquisitionInputValidation,
        Self::RootSliceCore,
        Self::Ctfe,
        Self::IdentityFinalization,
        Self::CompleteWorkspaceCore,
    ];
}

/// Canonical UTF-8 bytes of one scoped package name.
///
/// C1 already validates the package-name grammar. C2 keeps the exact bytes as
/// the diagnostic scope so a target-local `FileId` spelling can never stand in
/// for package authority.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ScopedPackageBytes(Box<[u8]>);

impl ScopedPackageBytes {
    pub(crate) fn from_canonical_name(name: &str) -> Option<Self> {
        (!name.is_empty()).then(|| Self(name.as_bytes().into()))
    }

    /// Returns the canonical scoped-package bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// One fully scoped M27-C diagnostic in canonical secondary-label order.
///
/// Construction is crate-private because callers must pair every secondary
/// label with its package-portable path. Public consumers can only inspect the
/// exact frontend diagnostic and its explicit semantic scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticDiagnostic {
    phase: CompilationPhase,
    package: ScopedPackageBytes,
    target: TargetId,
    path: PortablePath,
    diagnostic: Diagnostic,
    secondary_paths: Box<[PortablePath]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SemanticDiagnosticError {
    SecondaryPathCount,
}

impl SemanticDiagnostic {
    pub(crate) fn new(
        phase: CompilationPhase,
        package: ScopedPackageBytes,
        target: TargetId,
        path: PortablePath,
        mut diagnostic: Diagnostic,
        secondary_paths: Vec<PortablePath>,
    ) -> Result<Self, SemanticDiagnosticError> {
        if diagnostic.secondary.len() != secondary_paths.len() {
            return Err(SemanticDiagnosticError::SecondaryPathCount);
        }

        let mut secondary = diagnostic
            .secondary
            .drain(..)
            .zip(secondary_paths)
            .collect::<Vec<_>>();
        secondary.sort_by(|(left_label, left_path), (right_label, right_path)| {
            compare_secondary(left_path, left_label, right_path, right_label)
        });
        let (labels, paths): (Vec<_>, Vec<_>) = secondary.into_iter().unzip();
        diagnostic.secondary = labels;

        Ok(Self {
            phase,
            package,
            target,
            path,
            diagnostic,
            secondary_paths: paths.into_boxed_slice(),
        })
    }

    /// Returns the compilation phase that emitted this diagnostic.
    pub const fn phase(&self) -> CompilationPhase {
        self.phase
    }

    /// Returns the canonical scoped-package bytes.
    pub const fn package(&self) -> &ScopedPackageBytes {
        &self.package
    }

    /// Returns the package-local manifest target ID.
    pub const fn target(&self) -> TargetId {
        self.target
    }

    /// Returns the package-portable primary source path.
    pub const fn path(&self) -> &PortablePath {
        &self.path
    }

    /// Returns the exact diagnostic with canonically ordered secondary labels.
    pub const fn diagnostic(&self) -> &Diagnostic {
        &self.diagnostic
    }

    /// Returns paths aligned one-for-one with the canonical secondary labels.
    pub fn secondary_paths(&self) -> &[PortablePath] {
        &self.secondary_paths
    }
}

impl Ord for SemanticDiagnostic {
    fn cmp(&self, other: &Self) -> Ordering {
        self.phase
            .cmp(&other.phase)
            .then_with(|| self.package.cmp(&other.package))
            .then_with(|| self.target.cmp(&other.target))
            .then_with(|| compare_primary(self, other))
            .then_with(|| {
                self.diagnostic
                    .code
                    .as_bytes()
                    .cmp(other.diagnostic.code.as_bytes())
            })
            .then_with(|| {
                self.diagnostic
                    .message
                    .as_bytes()
                    .cmp(other.diagnostic.message.as_bytes())
            })
            .then_with(|| compare_label_exact(&self.diagnostic.primary, &other.diagnostic.primary))
            .then_with(|| compare_secondary_sequences(self, other))
            .then_with(|| compare_string_sequences(&self.diagnostic.notes, &other.diagnostic.notes))
    }
}

impl PartialOrd for SemanticDiagnostic {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Nonempty, sorted, exact-deduplicated semantic diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptySemanticDiagnostics(Box<[SemanticDiagnostic]>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EmptySemanticDiagnostics;

impl NonEmptySemanticDiagnostics {
    pub(crate) fn from_unsorted(
        mut diagnostics: Vec<SemanticDiagnostic>,
    ) -> Result<Self, EmptySemanticDiagnostics> {
        if diagnostics.is_empty() {
            return Err(EmptySemanticDiagnostics);
        }
        diagnostics.sort();
        diagnostics.dedup();
        Ok(Self(diagnostics.into_boxed_slice()))
    }

    /// Returns the canonical nonempty diagnostic sequence.
    pub fn as_slice(&self) -> &[SemanticDiagnostic] {
        &self.0
    }

    /// Returns the number of distinct diagnostics.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns false; construction enforces the nonempty invariant.
    pub const fn is_empty(&self) -> bool {
        false
    }
}

fn compare_primary(left: &SemanticDiagnostic, right: &SemanticDiagnostic) -> Ordering {
    match (left.diagnostic.primary.span, right.diagnostic.primary.span) {
        (Some(left_span), Some(right_span)) => left
            .path
            .as_str()
            .as_bytes()
            .cmp(right.path.as_str().as_bytes())
            .then_with(|| left_span.start.byte.cmp(&right_span.start.byte))
            .then_with(|| left_span.end.byte.cmp(&right_span.end.byte)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left
            .path
            .as_str()
            .as_bytes()
            .cmp(right.path.as_str().as_bytes()),
    }
}

fn compare_secondary(
    left_path: &PortablePath,
    left: &Label,
    right_path: &PortablePath,
    right: &Label,
) -> Ordering {
    left_path
        .as_str()
        .as_bytes()
        .cmp(right_path.as_str().as_bytes())
        .then_with(|| compare_optional_span_location(left.span, right.span))
        .then_with(|| left.message.as_bytes().cmp(right.message.as_bytes()))
        .then_with(|| compare_optional_span_exact(left.span, right.span))
}

fn compare_secondary_sequences(left: &SemanticDiagnostic, right: &SemanticDiagnostic) -> Ordering {
    let mut left_rows = left
        .diagnostic
        .secondary
        .iter()
        .zip(left.secondary_paths.iter());
    let mut right_rows = right
        .diagnostic
        .secondary
        .iter()
        .zip(right.secondary_paths.iter());
    loop {
        match (left_rows.next(), right_rows.next()) {
            (Some((left_label, left_path)), Some((right_label, right_path))) => {
                let ordering = compare_secondary(left_path, left_label, right_path, right_label);
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }
}

fn compare_label_exact(left: &Label, right: &Label) -> Ordering {
    compare_optional_span_exact(left.span, right.span)
        .then_with(|| left.message.as_bytes().cmp(right.message.as_bytes()))
}

fn compare_optional_span_location(left: Option<Span>, right: Option<Span>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left
            .start
            .byte
            .cmp(&right.start.byte)
            .then_with(|| left.end.byte.cmp(&right.end.byte)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_optional_span_exact(left: Option<Span>, right: Option<Span>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left
            .file
            .cmp(&right.file)
            .then_with(|| left.start.byte.cmp(&right.start.byte))
            .then_with(|| left.end.byte.cmp(&right.end.byte))
            .then_with(|| left.start.line.cmp(&right.start.line))
            .then_with(|| left.start.column.cmp(&right.start.column))
            .then_with(|| left.end.line.cmp(&right.end.line))
            .then_with(|| left.end.column.cmp(&right.end.column)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_string_sequences(left: &[String], right: &[String]) -> Ordering {
    let mut left = left.iter();
    let mut right = right.iter();
    loop {
        match (left.next(), right.next()) {
            (Some(left), Some(right)) => {
                let ordering = left.as_bytes().cmp(right.as_bytes());
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }
}

#[cfg(test)]
mod tests {
    use arche_frontend::{FileId, SourcePosition};

    use super::*;

    fn package(name: &str) -> ScopedPackageBytes {
        ScopedPackageBytes::from_canonical_name(name).unwrap()
    }

    fn path(value: &str) -> PortablePath {
        PortablePath::new(value).unwrap()
    }

    fn span(file: u64, start: u64, end: u64) -> Span {
        Span {
            file: FileId(file),
            start: SourcePosition {
                byte: start,
                line: start + 1,
                column: 1,
            },
            end: SourcePosition {
                byte: end,
                line: end + 1,
                column: 1,
            },
        }
    }

    fn scoped(
        phase: CompilationPhase,
        package_name: &str,
        target: u64,
        source_path: &str,
        primary: Span,
        code: &'static str,
        message: &str,
    ) -> SemanticDiagnostic {
        SemanticDiagnostic::new(
            phase,
            package(package_name),
            TargetId(target),
            path(source_path),
            Diagnostic::at(code, primary, message),
            Vec::new(),
        )
        .unwrap()
    }

    #[test]
    fn total_order_follows_phase_package_target_path_span_code_then_message() {
        let base = scoped(
            CompilationPhase::DeclarationTypeTraitCoherence,
            "alpha/pkg",
            1,
            "src/a.arc",
            span(7, 10, 11),
            "TYPE001",
            "alpha",
        );
        assert!(
            scoped(
                CompilationPhase::ModuleNameResolution,
                "zeta/pkg",
                9,
                "src/z.arc",
                span(7, 90, 91),
                "TYPE999",
                "zeta",
            ) < base
        );
        assert!(
            base < scoped(
                base.phase(),
                "beta/pkg",
                0,
                "src/a.arc",
                span(7, 1, 2),
                "TYPE001",
                "alpha",
            )
        );
        assert!(
            base < scoped(
                base.phase(),
                "alpha/pkg",
                2,
                "src/a.arc",
                span(7, 1, 2),
                "TYPE001",
                "alpha",
            )
        );
        assert!(
            base < scoped(
                base.phase(),
                "alpha/pkg",
                1,
                "src/b.arc",
                span(7, 1, 2),
                "TYPE001",
                "alpha",
            )
        );
        assert!(
            base < scoped(
                base.phase(),
                "alpha/pkg",
                1,
                "src/a.arc",
                span(7, 10, 12),
                "TYPE001",
                "alpha",
            )
        );
        assert!(
            base < scoped(
                base.phase(),
                "alpha/pkg",
                1,
                "src/a.arc",
                span(7, 11, 12),
                "TYPE001",
                "alpha",
            )
        );
        assert!(
            base < scoped(
                base.phase(),
                "alpha/pkg",
                1,
                "src/a.arc",
                span(7, 10, 11),
                "TYPE002",
                "alpha",
            )
        );
        assert!(
            base < scoped(
                base.phase(),
                "alpha/pkg",
                1,
                "src/a.arc",
                span(7, 10, 11),
                "TYPE001",
                "beta",
            )
        );

        let spanless = SemanticDiagnostic::new(
            base.phase(),
            package("alpha/pkg"),
            TargetId(1),
            path("src/0.arc"),
            Diagnostic::path("TYPE001", "spanless"),
            Vec::new(),
        )
        .unwrap();
        assert!(base < spanless);
    }

    #[test]
    fn every_compilation_phase_has_the_frozen_relative_order() {
        for pair in CompilationPhase::ALL.windows(2) {
            assert!(pair[0] < pair[1]);
        }
    }

    #[test]
    fn target_and_path_scope_disambiguate_a_shared_file_id() {
        let first = scoped(
            CompilationPhase::BodyCallOperatorPattern,
            "example/shared",
            0,
            "src/bin.arc",
            span(3, 4, 5),
            "TYPE001",
            "same",
        );
        let second = scoped(
            CompilationPhase::BodyCallOperatorPattern,
            "example/shared",
            1,
            "src/env.arc",
            span(3, 4, 5),
            "TYPE001",
            "same",
        );
        let diagnostics =
            NonEmptySemanticDiagnostics::from_unsorted(vec![second.clone(), first.clone()])
                .unwrap();
        assert_eq!(diagnostics.as_slice(), [first, second]);
    }

    #[test]
    fn secondary_permutations_canonicalize_and_exactly_deduplicate() {
        let primary = span(1, 1, 2);
        let first = Diagnostic::at("TRAIT001", primary, "failure")
            .with_secondary(span(2, 20, 21), "later")
            .with_secondary(span(3, 10, 11), "earlier")
            .with_note("semantic note");
        let second = Diagnostic::at("TRAIT001", primary, "failure")
            .with_secondary(span(3, 10, 11), "earlier")
            .with_secondary(span(2, 20, 21), "later")
            .with_note("semantic note");
        let first = SemanticDiagnostic::new(
            CompilationPhase::DeclarationTypeTraitCoherence,
            package("example/pkg"),
            TargetId(0),
            path("src/lib.arc"),
            first,
            vec![path("src/z.arc"), path("src/a.arc")],
        )
        .unwrap();
        let second = SemanticDiagnostic::new(
            CompilationPhase::DeclarationTypeTraitCoherence,
            package("example/pkg"),
            TargetId(0),
            path("src/lib.arc"),
            second,
            vec![path("src/a.arc"), path("src/z.arc")],
        )
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.secondary_paths()[0].as_str(), "src/a.arc");
        assert_eq!(first.diagnostic().secondary[0].message, "earlier");

        let diagnostics = NonEmptySemanticDiagnostics::from_unsorted(vec![second, first]).unwrap();
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn exact_dedup_includes_secondary_labels_and_note_order() {
        let base = scoped(
            CompilationPhase::BodyCallOperatorPattern,
            "example/pkg",
            0,
            "src/lib.arc",
            span(0, 2, 3),
            "PATTERN001",
            "failure",
        );
        let mut with_note = base.clone();
        with_note.diagnostic.notes.push("note".to_owned());
        let mut reversed_notes = base.clone();
        reversed_notes.diagnostic.notes = vec!["z".to_owned(), "a".to_owned()];
        let mut sorted_notes = base.clone();
        sorted_notes.diagnostic.notes = vec!["a".to_owned(), "z".to_owned()];
        let diagnostics = NonEmptySemanticDiagnostics::from_unsorted(vec![
            reversed_notes.clone(),
            base.clone(),
            with_note,
            sorted_notes.clone(),
            base,
        ])
        .unwrap();
        assert_eq!(diagnostics.len(), 4);
        assert_eq!(reversed_notes.diagnostic().notes, ["z", "a"]);
        assert_eq!(sorted_notes.diagnostic().notes, ["a", "z"]);
        assert_ne!(reversed_notes, sorted_notes);
    }

    #[test]
    fn secondary_content_participates_in_exact_dedup() {
        let primary = span(1, 1, 2);
        let diagnostic = |secondary_message: &str| {
            SemanticDiagnostic::new(
                CompilationPhase::BodyCallOperatorPattern,
                package("example/pkg"),
                TargetId(0),
                path("src/lib.arc"),
                Diagnostic::at("TYPE001", primary, "failure")
                    .with_secondary(span(2, 3, 4), secondary_message),
                vec![path("src/other.arc")],
            )
            .unwrap()
        };
        let first = diagnostic("first");
        let duplicate = first.clone();
        let distinct = diagnostic("second");
        let diagnostics =
            NonEmptySemanticDiagnostics::from_unsorted(vec![distinct, duplicate, first]).unwrap();
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn secondary_path_count_is_an_invariant() {
        let error = SemanticDiagnostic::new(
            CompilationPhase::BodyCallOperatorPattern,
            package("example/pkg"),
            TargetId(0),
            path("src/lib.arc"),
            Diagnostic::at("TYPE001", span(0, 0, 1), "failure")
                .with_secondary(span(0, 2, 3), "secondary"),
            Vec::new(),
        )
        .unwrap_err();
        assert_eq!(error, SemanticDiagnosticError::SecondaryPathCount);
        assert!(NonEmptySemanticDiagnostics::from_unsorted(Vec::new()).is_err());
    }
}
