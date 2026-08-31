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

fn assert_terminal_rejection(case: ExpectedCase) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../tests/m27c2/v1/negative")
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
