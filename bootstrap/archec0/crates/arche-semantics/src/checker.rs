//! Consuming C2 workspace orchestration.
//!
//! A source rejection and a compiler-authority blocker are deliberately
//! different terminal states. Until every C2 authority is present, this
//! module retains the complete C1 handoff and partial checked facts rather
//! than manufacturing a successful result or blaming valid source.

use arche_frontend::{FrontendOutput, SourceDatabase};

use crate::body_check::{check_workspace_bodies_c2, C2BodyCheckFailure, C2BodyTable};
use crate::declaration_check::{
    check_declarations_c2, CheckedDeclarationFacts, DeclarationCheckFailure,
};
use crate::declarations::{DeclarationTable, DeclarationTableError};
use crate::diagnostic::{NonEmptySemanticDiagnostics, SemanticDiagnostic};
use crate::model::{
    C2CheckedWorkspace, C2Handoff, C2ModelError, C2RejectedWorkspace, RetainedFrontend,
    SessionIndexFailure, SessionIndexTables,
};

/// Runs the complete C2 authority currently available for one consumed C1
/// frontend result.
///
/// Success is minted only after exact-session terminal aggregation validates
/// complete declaration/body producer coverage and derives all checked-type
/// and target gates from those semantic facts.
pub fn check_workspace_c2(frontend: FrontendOutput) -> Result<C2CheckedWorkspace, C2CheckFailure> {
    let handoff = C2Handoff::begin(frontend).map_err(C2CheckFailure::SessionIndex)?;
    let declarations = match DeclarationTable::build(&handoff) {
        Ok(declarations) => declarations,
        Err(error) => {
            return Err(C2CheckFailure::Blocked(Box::new(C2BlockedWorkspace {
                handoff,
                stage: C2BlockStage::DeclarationTable,
                declaration_table_error: Some(error),
                model_error: None,
                declarations: None,
                declaration_result: None,
                body_result: None,
            })));
        }
    };

    let declaration_result = check_declarations_c2(&handoff, &declarations);
    let checked_declarations = match &declaration_result {
        Ok(facts) => facts,
        Err(failure) => failure.partial(),
    };
    let body_result = check_workspace_bodies_c2(&handoff, &declarations, checked_declarations);
    // Missing-judgment blockers forbid success but do not cast doubt on the
    // diagnostics implemented checks produced; only genuine authority gaps
    // suppress source rejection.
    let authority_blocked = declaration_result
        .as_ref()
        .is_err_and(DeclarationCheckFailure::suppresses_source_diagnostics)
        || body_result
            .as_ref()
            .is_err_and(|failure| !failure.incompleteness().is_empty());
    if !authority_blocked {
        let diagnostics = collect_diagnostics(&declaration_result, &body_result);
        if let Ok(diagnostics) = NonEmptySemanticDiagnostics::from_unsorted(diagnostics) {
            return Err(C2CheckFailure::Rejected(Box::new(
                handoff.into_rejected(diagnostics),
            )));
        }
    }

    if authority_blocked || declaration_result.is_err() || body_result.is_err() {
        return Err(C2CheckFailure::Blocked(Box::new(C2BlockedWorkspace {
            handoff,
            stage: C2BlockStage::DeclarationOrBodyAuthority,
            declaration_table_error: None,
            model_error: None,
            declarations: Some(declarations),
            declaration_result: Some(declaration_result),
            body_result: Some(body_result),
        })));
    }

    let (Ok(checked_declarations), Ok(checked_bodies)) = (declaration_result, body_result) else {
        unreachable!("all non-success declaration/body results returned above")
    };
    match handoff.aggregate_checked(checked_declarations, checked_bodies) {
        Ok(checked) => Ok(checked),
        Err(failure) => {
            let (handoff, checked_declarations, checked_bodies, model_error) =
                (*failure).into_parts();
            Err(C2CheckFailure::Blocked(Box::new(C2BlockedWorkspace {
                handoff,
                stage: C2BlockStage::CheckedTypeAggregation,
                declaration_table_error: None,
                model_error: Some(model_error),
                declarations: Some(declarations),
                declaration_result: Some(Ok(checked_declarations)),
                body_result: Some(Ok(checked_bodies)),
            })))
        }
    }
}

fn collect_diagnostics(
    declarations: &Result<CheckedDeclarationFacts, DeclarationCheckFailure>,
    bodies: &Result<C2BodyTable, C2BodyCheckFailure>,
) -> Vec<SemanticDiagnostic> {
    let mut diagnostics = Vec::new();
    if let Err(failure) = declarations {
        if let Some(rows) = failure.diagnostics() {
            diagnostics.extend(rows.as_slice().iter().cloned());
        }
    }
    if let Err(failure) = bodies {
        if let Some(rows) = failure.diagnostics() {
            diagnostics.extend(rows.as_slice().iter().cloned());
        }
    }
    diagnostics
}

/// Failure from the consuming C2 entry point.
#[derive(Debug)]
pub enum C2CheckFailure {
    /// C1 HIR could not form one owner-branded dense C2 session.
    SessionIndex(Box<SessionIndexFailure>),
    /// Source semantics were invalid; diagnostics are canonical and nonempty.
    Rejected(Box<C2RejectedWorkspace>),
    /// Valid retained input reached a missing or corrupt compiler authority.
    Blocked(Box<C2BlockedWorkspace>),
}

/// The earliest compiler-authority stage that prevented C2 success.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C2BlockStage {
    DeclarationTable,
    DeclarationOrBodyAuthority,
    CheckedTypeAggregation,
}

/// Retained fail-closed state for a valid or not-yet-diagnosed workspace.
#[derive(Debug)]
pub struct C2BlockedWorkspace {
    handoff: C2Handoff,
    stage: C2BlockStage,
    declaration_table_error: Option<DeclarationTableError>,
    model_error: Option<C2ModelError>,
    declarations: Option<DeclarationTable>,
    declaration_result: Option<Result<CheckedDeclarationFacts, DeclarationCheckFailure>>,
    body_result: Option<Result<C2BodyTable, C2BodyCheckFailure>>,
}

impl C2BlockedWorkspace {
    pub const fn stage(&self) -> C2BlockStage {
        self.stage
    }

    pub const fn indexes(&self) -> &SessionIndexTables {
        self.handoff.indexes()
    }

    pub const fn declaration_table_error(&self) -> Option<&DeclarationTableError> {
        self.declaration_table_error.as_ref()
    }

    /// Returns a stable compiler-internal category when exact terminal
    /// aggregation, rather than declaration/body checking, blocked success.
    pub fn aggregation_error_code(&self) -> Option<&'static str> {
        self.model_error.map(C2ModelError::code)
    }

    pub const fn declarations(&self) -> Option<&DeclarationTable> {
        self.declarations.as_ref()
    }

    pub fn declaration_result(
        &self,
    ) -> Option<Result<&CheckedDeclarationFacts, &DeclarationCheckFailure>> {
        self.declaration_result.as_ref().map(Result::as_ref)
    }

    pub fn body_result(&self) -> Option<Result<&C2BodyTable, &C2BodyCheckFailure>> {
        self.body_result.as_ref().map(Result::as_ref)
    }
}

impl RetainedFrontend for C2BlockedWorkspace {
    fn frontend(&self) -> &FrontendOutput {
        self.handoff.frontend()
    }

    fn sources(&self) -> &std::sync::Arc<SourceDatabase> {
        self.handoff.frontend().sources()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use arche_frontend::check_workspace_c1;
    use arche_package::{load_workspace, resolve, ManifestRequest, RegistrySnapshot};

    use super::*;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct TemporaryWorkspace(PathBuf);

    impl Drop for TemporaryWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn corpus_frontend(name: &str) -> FrontendOutput {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../tests/m27c2/v1")
            .join(name);
        let workspace = load_workspace(&ManifestRequest::discover_from(&root)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        check_workspace_c1(&workspace, &graph, &[]).unwrap()
    }

    fn empty_frontend() -> FrontendOutput {
        let ordinal = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let fixture = TemporaryWorkspace(
            std::env::temp_dir().join(format!("arche-c2-checker-{}-{ordinal}", std::process::id())),
        );
        fs::create_dir_all(fixture.0.join("src")).unwrap();
        fs::write(
            fixture.0.join("Arche.toml"),
            concat!(
                "schema = 1\n\n",
                "[package]\n",
                "name = \"example/checker-empty\"\n",
                "version = \"0.1.0\"\n",
                "edition = \"2026\"\n",
                "arche = \">=0.0.0\"\n",
                "publish = false\n\n",
                "[lib]\n",
                "path = \"src/lib.arc\"\n",
            ),
        )
        .unwrap();
        fs::write(fixture.0.join("src/lib.arc"), "// Intentionally empty.\n").unwrap();
        let workspace = load_workspace(&ManifestRequest::discover_from(&fixture.0)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        check_workspace_c1(&workspace, &graph, &[]).unwrap()
    }

    #[test]
    fn real_v1_input_blocks_without_fabricating_a_source_rejection() {
        for corpus in ["language-game", "language-environment"] {
            let failure = check_workspace_c2(corpus_frontend(corpus)).unwrap_err();
            let C2CheckFailure::Blocked(blocked) = failure else {
                panic!("{corpus} must remain an internal C2 authority blocker");
            };
            assert_eq!(blocked.stage(), C2BlockStage::DeclarationOrBodyAuthority);
            assert!(blocked.declarations().is_some());
            assert!(blocked.declaration_result().is_some());
            assert!(blocked.body_result().is_some());
        }
    }

    #[test]
    fn empty_target_mints_an_exact_terminal_workspace_and_retains_semantic_facts() {
        let checked = check_workspace_c2(empty_frontend()).unwrap();
        assert_eq!(checked.indexes().checked_type_count(), 0);
        assert_eq!(checked.target_count(), 1);
        assert!(checked.declarations().is_empty());
        assert!(checked.bodies().is_empty());
        let target = checked.targets().next().unwrap();
        assert_eq!(target.resolution(), &crate::model::C2Resolution::Complete);
        assert!(target.pending_c4().is_empty());
    }
}
