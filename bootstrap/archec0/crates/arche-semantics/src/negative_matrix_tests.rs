use std::path::PathBuf;

use arche_frontend::{check_workspace_c1, FileId, SourcePosition, Span, TargetId};
use arche_package::{load_workspace, resolve, ManifestRequest, RegistrySnapshot};

use crate::{check_workspace_c2, C2CheckFailure, CompilationPhase};

#[derive(Clone, Copy)]
struct ExpectedCase {
    fixture: &'static str,
    package: &'static str,
    phase: CompilationPhase,
    code: &'static str,
    message: &'static str,
    start_byte: u64,
    start_line: u64,
    start_column: u64,
    end_byte: u64,
    end_line: u64,
    end_column: u64,
}

#[derive(Clone, Copy)]
struct ExpectedDiagnostic {
    code: &'static str,
    message: &'static str,
    start_byte: u64,
    start_line: u64,
    start_column: u64,
    end_byte: u64,
    end_line: u64,
    end_column: u64,
}

fn assert_terminal_rejection_sequence(
    fixture: &'static str,
    package: &'static str,
    phase: CompilationPhase,
    expected: &[ExpectedDiagnostic],
) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../tests/m27c2/v1/vectors")
        .join(fixture);
    let workspace = load_workspace(&ManifestRequest::discover_from(&root))
        .unwrap_or_else(|error| panic!("{fixture} must load as a standalone package: {error}"));
    let graph = resolve(&workspace, &RegistrySnapshot::empty())
        .unwrap_or_else(|error| panic!("{fixture} must resolve without a registry: {error}"));
    let frontend = check_workspace_c1(&workspace, &graph, &[])
        .unwrap_or_else(|error| panic!("{fixture} must reach the C2 boundary: {error}"));
    let rejected = match check_workspace_c2(frontend) {
        Err(C2CheckFailure::Rejected(rejected)) => rejected,
        other => panic!("{fixture} must terminate as a source rejection: {other:#?}"),
    };
    let diagnostics = rejected.diagnostics().as_slice();
    assert_eq!(
        diagnostics.len(),
        expected.len(),
        "{fixture} diagnostic count: {diagnostics:#?}"
    );
    for (semantic, case) in diagnostics.iter().zip(expected) {
        assert_eq!(semantic.phase(), phase, "{fixture} phase");
        assert_eq!(
            semantic.package().as_bytes(),
            package.as_bytes(),
            "{fixture} package scope"
        );
        assert_eq!(semantic.target(), TargetId(0), "{fixture} target");
        assert_eq!(semantic.path().as_str(), "src/lib.arc", "{fixture} path");
        let diagnostic = semantic.diagnostic();
        assert_eq!(
            diagnostic.code, case.code,
            "{fixture} code in {diagnostic:#?}"
        );
        assert_eq!(
            diagnostic.message, case.message,
            "{fixture} message in {diagnostic:#?}"
        );
        assert_eq!(
            diagnostic.primary.span,
            Some(Span {
                file: FileId(0),
                start: SourcePosition {
                    byte: case.start_byte,
                    line: case.start_line,
                    column: case.start_column,
                },
                end: SourcePosition {
                    byte: case.end_byte,
                    line: case.end_line,
                    column: case.end_column,
                },
            }),
            "{fixture} primary span"
        );
        assert_eq!(
            diagnostic.primary.message, case.message,
            "{fixture} primary label"
        );
    }
}

fn assert_terminal_rejection(case: ExpectedCase) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../tests/m27c2/v1/vectors")
        .join(case.fixture);
    let workspace =
        load_workspace(&ManifestRequest::discover_from(&root)).unwrap_or_else(|error| {
            panic!(
                "{} must load as a standalone package: {error}",
                case.fixture
            )
        });
    let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap_or_else(|error| {
        panic!("{} must resolve without a registry: {error}", case.fixture)
    });
    let frontend = check_workspace_c1(&workspace, &graph, &[])
        .unwrap_or_else(|error| panic!("{} must reach the C2 boundary: {error}", case.fixture));

    let rejected = match check_workspace_c2(frontend) {
        Err(C2CheckFailure::Rejected(rejected)) => rejected,
        Err(C2CheckFailure::Blocked(blocked)) => panic!(
            "{} is invalid source and must never freeze an internal C2 blocker: {blocked:#?}",
            case.fixture
        ),
        Err(C2CheckFailure::SessionIndex(error)) => panic!(
            "{} must form a valid C2 session index before rejection: {error:#?}",
            case.fixture
        ),
        Ok(checked) => panic!(
            "{} is invalid source but minted a checked C2 workspace: {checked:#?}",
            case.fixture
        ),
    };

    let diagnostics = rejected.diagnostics().as_slice();
    assert_eq!(
        diagnostics.len(),
        1,
        "{} must produce exactly one canonical diagnostic: {diagnostics:#?}",
        case.fixture
    );
    let semantic = &diagnostics[0];
    assert_eq!(semantic.phase(), case.phase, "{} phase", case.fixture);
    assert_eq!(
        semantic.package().as_bytes(),
        case.package.as_bytes(),
        "{} package scope",
        case.fixture
    );
    assert_eq!(semantic.target(), TargetId(0), "{} target", case.fixture);
    assert_eq!(
        semantic.path().as_str(),
        "src/lib.arc",
        "{} portable path",
        case.fixture
    );

    let diagnostic = semantic.diagnostic();
    assert_eq!(
        diagnostic.code, case.code,
        "{} code in {diagnostic:#?}",
        case.fixture
    );
    assert_eq!(
        diagnostic.message, case.message,
        "{} message in {diagnostic:#?}",
        case.fixture
    );
    assert_eq!(
        diagnostic.primary.span,
        Some(Span {
            file: FileId(0),
            start: SourcePosition {
                byte: case.start_byte,
                line: case.start_line,
                column: case.start_column,
            },
            end: SourcePosition {
                byte: case.end_byte,
                line: case.end_line,
                column: case.end_column,
            },
        }),
        "{} primary span and FileId",
        case.fixture
    );
    assert_eq!(
        diagnostic.primary.message, case.message,
        "{} primary label",
        case.fixture
    );
    assert!(
        diagnostic.secondary.is_empty(),
        "{} secondary labels: {:#?}",
        case.fixture,
        diagnostic.secondary
    );
    assert!(
        semantic.secondary_paths().is_empty(),
        "{} secondary paths: {:#?}",
        case.fixture,
        semantic.secondary_paths()
    );
    assert!(
        diagnostic.notes.is_empty(),
        "{} notes: {:#?}",
        case.fixture,
        diagnostic.notes
    );
}

#[test]
fn pattern001_duplicate_or_alternative_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "pattern001-duplicate-or-alternative",
        package: "fixtures/m27c2-negative-pattern001-duplicate-or-alternative",
        phase: CompilationPhase::BodyCallOperatorPattern,
        code: "PATTERN001",
        message: "or-pattern contains a duplicate alternative",
        start_byte: 62,
        start_line: 3,
        start_column: 9,
        end_byte: 75,
        end_line: 3,
        end_column: 22,
    });
}

#[test]
fn pattern001_unreachable_fallback_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "pattern001-unreachable-fallback",
        package: "fixtures/m27c2-negative-pattern001-unreachable-fallback",
        phase: CompilationPhase::BodyCallOperatorPattern,
        code: "PATTERN001",
        message: "arm pattern is unreachable",
        start_byte: 86,
        start_line: 4,
        start_column: 9,
        end_byte: 87,
        end_line: 4,
        end_column: 10,
    });
}

#[test]
fn trait001_trait_impl_method_visibility_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "trait001-trait-impl-method-visibility",
        package: "fixtures/m27c2-negative-trait001-trait-impl-method-visibility",
        phase: CompilationPhase::DeclarationTypeTraitCoherence,
        code: "TRAIT001",
        message: "trait-impl methods cannot spell visibility",
        start_byte: 127,
        start_line: 8,
        start_column: 5,
        end_byte: 130,
        end_line: 8,
        end_column: 8,
    });
}

#[test]
fn trait002_integer_logical_not_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "trait002-integer-logical-not",
        package: "fixtures/m27c2-negative-trait002-integer-logical-not",
        phase: CompilationPhase::BodyCallOperatorPattern,
        code: "TRAIT002",
        message: "no primitive logical-not selection for left=u32, right=none, result=u32",
        start_byte: 25,
        start_line: 1,
        start_column: 26,
        end_byte: 30,
        end_line: 1,
        end_column: 31,
    });
}

#[test]
fn trait002_mixed_step_scalars_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "trait002-mixed-step-scalars",
        package: "fixtures/m27c2-negative-trait002-mixed-step-scalars",
        phase: CompilationPhase::BodyCallOperatorPattern,
        code: "TRAIT002",
        message: "no primitive add-assignment selection for left=i64, right=u32, result=i64",
        start_byte: 47,
        start_line: 2,
        start_column: 5,
        end_byte: 60,
        end_line: 2,
        end_column: 18,
    });
}

#[test]
fn type001_untyped_empty_array_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "type001-untyped-empty-array",
        package: "fixtures/m27c2-negative-type001-untyped-empty-array",
        phase: CompilationPhase::BodyCallOperatorPattern,
        code: "TYPE001",
        message: "type inference left an unresolved variable",
        start_byte: 36,
        start_line: 2,
        start_column: 18,
        end_byte: 38,
        end_line: 2,
        end_column: 20,
    });
}

#[test]
fn type002_choice_payload_mismatch_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "type002-choice-payload-mismatch",
        package: "fixtures/m27c2-negative-type002-choice-payload-mismatch",
        phase: CompilationPhase::BodyCallOperatorPattern,
        code: "TYPE002",
        message: "expected i32, found u32",
        start_byte: 138,
        start_line: 6,
        start_column: 39,
        end_byte: 143,
        end_line: 6,
        end_column: 44,
    });
}

#[test]
fn type002_array_as_vec_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "type002-array-as-vec",
        package: "fixtures/m27c2-negative-type002-array-as-vec",
        phase: CompilationPhase::BodyCallOperatorPattern,
        code: "TYPE002",
        message: "array expression cannot satisfy the expected non-array type",
        start_byte: 106,
        start_line: 6,
        start_column: 22,
        end_byte: 108,
        end_line: 6,
        end_column: 24,
    });
}

#[test]
fn type002_reference_address_cast_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "type002-reference-address-cast",
        package: "fixtures/m27c2-negative-type002-reference-address-cast",
        phase: CompilationPhase::BodyCallOperatorPattern,
        code: "TYPE002",
        message: "`as` supports only raw-pointer/address reconstruction, not (reference mutable (bound-lifetime 0 0) u8) to usize",
        start_byte: 70,
        start_line: 3,
        start_column: 9,
        end_byte: 84,
        end_line: 3,
        end_column: 23,
    });
}

#[test]
fn type002_map_remove_owned_key_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "type002-map-remove-owned-key",
        package: "fixtures/m27c2-negative-type002-map-remove-owned-key",
        phase: CompilationPhase::BodyCallOperatorPattern,
        code: "TYPE002",
        message:
            "expected (reference shared erased-local (bound-type 0 0)), found (bound-type 0 0)",
        start_byte: 98,
        start_line: 3,
        start_column: 16,
        end_byte: 101,
        end_line: 3,
        end_column: 19,
    });
}

#[test]
fn type003_u8_overflow_literal_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "type003-u8-overflow-literal",
        package: "fixtures/m27c2-negative-type003-u8-overflow-literal",
        phase: CompilationPhase::BodyCallOperatorPattern,
        code: "TYPE003",
        message: "invalid integer literal: the positive value is out of range for u8",
        start_byte: 24,
        start_line: 1,
        start_column: 25,
        end_byte: 37,
        end_line: 3,
        end_column: 2,
    });
}

#[test]
fn pattern002_nonexhaustive_match_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "pattern002-nonexhaustive-match",
        package: "fixtures/m27c2-negative-pattern002-nonexhaustive-match",
        phase: CompilationPhase::BodyCallOperatorPattern,
        code: "PATTERN002",
        message: "match is not exhaustive",
        start_byte: 87,
        start_line: 6,
        start_column: 11,
        end_byte: 91,
        end_line: 6,
        end_column: 15,
    });
}

#[test]
fn coherence001_orphan_impl_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "coherence001-orphan-impl",
        package: "fixtures/m27c2-negative-coherence001-orphan-impl",
        phase: CompilationPhase::DeclarationTypeTraitCoherence,
        code: "COHERENCE001",
        message: "impl of `Clone` violates the orphan rule: the package owns neither the trait nor the outermost nominal target",
        start_byte: 0,
        start_line: 1,
        start_column: 1,
        end_byte: 71,
        end_line: 5,
        end_column: 2,
    });
}

#[test]
fn coherence002_nondefault_overlap_is_a_terminal_c2_rejection() {
    assert_terminal_rejection_sequence(
        "coherence002-nondefault-overlap",
        "fixtures/m27c2-negative-coherence002-nondefault-overlap",
        CompilationPhase::DeclarationTypeTraitCoherence,
        &[
            ExpectedDiagnostic {
                code: "COHERENCE002",
                message: "impls of `One` have overlapping match sets and neither is a default specialization parent",
                start_byte: 82,
                start_line: 7,
                start_column: 1,
                end_byte: 151,
                end_line: 11,
                end_column: 2,
            },
            ExpectedDiagnostic {
                code: "COHERENCE002",
                message: "impls of `One` have overlapping match sets and neither is a default specialization parent",
                start_byte: 152,
                start_line: 12,
                start_column: 1,
                end_byte: 221,
                end_line: 16,
                end_column: 2,
            },
        ],
    );
}

#[test]
fn trait001_duplicate_inherent_method_is_a_terminal_c2_rejection() {
    assert_terminal_rejection_sequence(
        "trait001-duplicate-inherent-method",
        "fixtures/m27c2-negative-trait001-duplicate-inherent-method",
        CompilationPhase::DeclarationTypeTraitCoherence,
        &[
            ExpectedDiagnostic {
                code: "TRAIT001",
                message: "inherent method `value` is declared more than once under one byte-identical canonical impl head",
                start_byte: 38,
                start_line: 4,
                start_column: 1,
                end_byte: 107,
                end_line: 8,
                end_column: 2,
            },
            ExpectedDiagnostic {
                code: "TRAIT001",
                message: "inherent method `value` is declared more than once under one byte-identical canonical impl head",
                start_byte: 108,
                start_line: 9,
                start_column: 1,
                end_byte: 177,
                end_line: 13,
                end_column: 2,
            },
        ],
    );
}

#[test]
fn type001_direct_sized_recursion_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "type001-direct-sized-recursion",
        package: "fixtures/m27c2-negative-type001-direct-sized-recursion",
        phase: CompilationPhase::DeclarationTypeTraitCoherence,
        code: "TYPE001",
        message: "`Nest` participates in a direct recursive storage cycle with no approved sized indirection",
        start_byte: 4,
        start_line: 1,
        start_column: 5,
        end_byte: 40,
        end_line: 3,
        end_column: 2,
    });
}

#[test]
fn type002_nonidentical_float_comparison_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "type002-nonidentical-float-comparison",
        package: "fixtures/m27c2-negative-type002-nonidentical-float-comparison",
        phase: CompilationPhase::BodyCallOperatorPattern,
        code: "TYPE002",
        message: "expected f32, found f64",
        start_byte: 46,
        start_line: 1,
        start_column: 47,
        end_byte: 67,
        end_line: 3,
        end_column: 2,
    });
}

#[test]
fn trait002_map_float_key_is_a_terminal_c2_rejection() {
    assert_terminal_rejection(ExpectedCase {
        fixture: "trait002-map-float-key",
        package: "fixtures/m27c2-negative-trait002-map-float-key",
        phase: CompilationPhase::DeclarationTypeTraitCoherence,
        code: "TRAIT002",
        message: "float map keys are categorically ineligible: exact same-type float comparison is a syntax-only primitive exception and furnishes no Eq/Ord selection",
        start_byte: 4,
        start_line: 1,
        start_column: 5,
        end_byte: 51,
        end_line: 3,
        end_column: 2,
    });
}

#[test]
fn frozen_corpus_bytes_agree_across_git_surfaces() {
    use std::process::Command;
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let toplevel_probe = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&manifest)
        .output()
        .expect("git is available");
    assert!(toplevel_probe.status.success(), "git rev-parse failed");
    let toplevel = PathBuf::from(String::from_utf8(toplevel_probe.stdout).unwrap().trim());
    // Every corpus pathspec is repo-relative, so every command runs from the
    // repository toplevel.
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(&toplevel)
            .output()
            .expect("git is available");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };

    let status = git(&["status", "--porcelain", "--", "tests/m27c2"]);
    assert!(
        status.is_empty(),
        "the frozen corpus differs between worktree and index:\n{}",
        String::from_utf8_lossy(&status)
    );

    let head_listing =
        String::from_utf8(git(&["ls-tree", "-r", "HEAD", "--", "tests/m27c2"])).unwrap();
    let index_listing = String::from_utf8(git(&["ls-files", "-s", "--", "tests/m27c2"])).unwrap();
    let head_rows: Vec<(String, String)> = head_listing
        .lines()
        .map(|line| {
            let mut parts = line.split_whitespace();
            let _mode = parts.next().unwrap();
            let _kind = parts.next().unwrap();
            let blob = parts.next().unwrap().to_owned();
            let path = line.split('\t').nth(1).unwrap().to_owned();
            (path, blob)
        })
        .collect();
    let index_rows: Vec<(String, String)> = index_listing
        .lines()
        .map(|line| {
            let mut parts = line.split_whitespace();
            let _mode = parts.next().unwrap();
            let blob = parts.next().unwrap().to_owned();
            let _stage = parts.next().unwrap();
            let path = line.split('\t').nth(1).unwrap().to_owned();
            (path, blob)
        })
        .collect();
    assert!(!head_rows.is_empty(), "the frozen corpus is tracked");
    assert_eq!(
        head_rows, index_rows,
        "HEAD and index disagree over tests/m27c2"
    );

    for (path, blob) in &head_rows {
        let blob_bytes = git(&["cat-file", "blob", blob]);
        let worktree_bytes = std::fs::read(toplevel.join(path))
            .unwrap_or_else(|error| panic!("{path} must exist in the worktree: {error}"));
        assert_eq!(
            blob_bytes, worktree_bytes,
            "{path} bytes differ between the Git blob and the worktree"
        );
    }

    let fresh = std::env::temp_dir().join(format!("arche-corpus-freeze-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&fresh);
    git(&[
        "worktree",
        "add",
        "--detach",
        fresh.to_str().unwrap(),
        "HEAD",
    ]);
    for (path, _) in &head_rows {
        let fresh_bytes = std::fs::read(fresh.join(path))
            .unwrap_or_else(|error| panic!("{path} must exist in a fresh checkout: {error}"));
        let worktree_bytes = std::fs::read(toplevel.join(path)).unwrap();
        assert_eq!(
            fresh_bytes, worktree_bytes,
            "{path} bytes differ in a fresh detached checkout"
        );
    }
    git(&["worktree", "remove", "--force", fresh.to_str().unwrap()]);
}
