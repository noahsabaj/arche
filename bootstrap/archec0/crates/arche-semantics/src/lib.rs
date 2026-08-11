//! Private M27-C2 semantic-checking boundary.
//!
//! This crate owns the handoff from C1, but it does not yet expose a workspace
//! checker or claim a successfully checked C2 workspace. The opaque indexes in
//! this module are valid only while their owning handoff/result is alive; they
//! are not stable semantic identities and must never be serialized.

use std::collections::BTreeMap;
use std::sync::Arc;

use arche_frontend::{
    Diagnostic, FrontendOutput, HirItemId, ResolvedSymbolicWorkspaceHir, SourceDatabase,
    WorkspaceInventorySkeleton,
};

/// Dense item index owned by one C2 checking session.
///
/// Its private owner brand and offset prevent consumers from forging an index
/// or reusing one with a different workspace session. This is an ephemeral
/// table coordinate, not a stable semantic identity.
#[derive(Clone, Debug)]
pub struct SessionItemIndex {
    owner: Arc<SessionOwner>,
    offset: u64,
}

impl SessionItemIndex {
    /// Returns the session-local dense offset for diagnostics and testing.
    pub const fn offset(&self) -> u64 {
        self.offset
    }
}

/// Dense checked-type index owned by one C2 checking session.
///
/// C2 does not mint these until a type has actually been checked. The initial
/// handoff therefore owns an empty type table. This index is not a stable type
/// identity and cannot cross a checking-session boundary.
#[derive(Clone, Debug)]
pub struct SessionTypeIndex {
    owner: Arc<SessionOwner>,
    offset: u64,
}

impl SessionTypeIndex {
    /// Returns the session-local dense offset for diagnostics and testing.
    pub const fn offset(&self) -> u64 {
        self.offset
    }
}

/// Error raised while constructing the deterministic C2 session indexes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionIndexError {
    diagnostic: Diagnostic,
}

impl SessionIndexError {
    /// Returns the exact diagnostic that rejected index construction.
    pub const fn diagnostic(&self) -> &Diagnostic {
        &self.diagnostic
    }
}

/// Deterministic dense indexes derived from one retained C1 result.
///
/// Item rows follow the already-deterministic package, target, and HIR item
/// orders. The checked-type table intentionally starts empty: populating it is
/// semantic work, not part of this handoff scaffold.
#[derive(Debug)]
pub struct SessionIndexTables {
    owner: Arc<SessionOwner>,
    item_rows: Vec<HirItemId>,
    item_lookup: BTreeMap<HirItemId, u64>,
    checked_type_rows: Vec<CheckedTypeRow>,
}

#[derive(Debug)]
struct SessionOwner;

#[derive(Debug)]
struct CheckedTypeRow;

impl SessionIndexTables {
    fn empty() -> Self {
        Self {
            owner: Arc::new(SessionOwner),
            item_rows: Vec::new(),
            item_lookup: BTreeMap::new(),
            checked_type_rows: Vec::new(),
        }
    }

    fn from_hir(hir: &ResolvedSymbolicWorkspaceHir) -> Result<Self, SessionIndexError> {
        let mut tables = Self::empty();
        for package in &hir.packages {
            for target in &package.targets {
                for item in &target.items {
                    let offset =
                        u64::try_from(tables.item_rows.len()).map_err(|_| SessionIndexError {
                            diagnostic: Diagnostic::path(
                                "IDENTITY001",
                                "C2 session item index space is exhausted",
                            ),
                        })?;
                    if let Some(previous) = tables.item_lookup.insert(item.id, offset) {
                        return Err(SessionIndexError {
                            diagnostic: Diagnostic::at(
                                "IDENTITY001",
                                item.span,
                                format!(
                                    "HIR item {} appears twice in the C2 session index (first at {})",
                                    item.id.0,
                                    previous
                                ),
                            ),
                        });
                    }
                    tables.item_rows.push(item.id);
                }
            }
        }
        Ok(tables)
    }

    /// Returns the number of indexed C1 HIR items.
    pub fn item_count(&self) -> usize {
        self.item_rows.len()
    }

    /// Returns the number of types checked by C2.
    ///
    /// This remains zero until the real type checker populates this table.
    pub fn checked_type_count(&self) -> usize {
        self.checked_type_rows.len()
    }

    /// Looks up the session-local index for a retained HIR item.
    pub fn item_index(&self, item: HirItemId) -> Option<SessionItemIndex> {
        self.item_lookup.get(&item).map(|&offset| SessionItemIndex {
            owner: Arc::clone(&self.owner),
            offset,
        })
    }

    /// Resolves an opaque item index after validating its owner and offset.
    pub fn hir_item(&self, index: &SessionItemIndex) -> Option<HirItemId> {
        if !Arc::ptr_eq(&self.owner, &index.owner) {
            return None;
        }
        usize::try_from(index.offset)
            .ok()
            .and_then(|offset| self.item_rows.get(offset))
            .copied()
    }

    /// Validates an opaque checked-type index's owner and offset.
    pub fn contains_checked_type(&self, index: &SessionTypeIndex) -> bool {
        Arc::ptr_eq(&self.owner, &index.owner)
            && usize::try_from(index.offset)
                .ok()
                .is_some_and(|offset| offset < self.checked_type_rows.len())
    }
}

/// Consuming handoff from C1 into the still-private C2 implementation.
///
/// Constructing this value only transfers ownership and validates the session
/// index skeleton. It is not a proof and is not evidence that any C2 semantic
/// rule has run.
#[derive(Debug)]
pub struct C2Handoff {
    frontend: FrontendOutput,
    indexes: SessionIndexTables,
}

impl C2Handoff {
    /// Takes ownership of a C1 result and constructs its deterministic indexes.
    ///
    /// If index construction fails, the rejection still owns the complete C1
    /// result, so no immutable source authority is lost or accidentally reused.
    pub fn begin(frontend: FrontendOutput) -> Result<Self, Box<C2RejectedWorkspace>> {
        match SessionIndexTables::from_hir(frontend.hir()) {
            Ok(indexes) => Ok(Self { frontend, indexes }),
            Err(error) => Err(Box::new(C2RejectedWorkspace {
                frontend,
                diagnostics: vec![error.diagnostic],
            })),
        }
    }

    /// Returns the retained C1 authority owned by this handoff.
    pub const fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }

    /// Returns the session-local C2 index skeleton.
    pub const fn indexes(&self) -> &SessionIndexTables {
        &self.indexes
    }

    /// Consumes this handoff into a rejected result with at least one
    /// diagnostic while preserving ownership of the retained C1 authority.
    pub fn reject(
        self,
        primary: Diagnostic,
        mut additional: Vec<Diagnostic>,
    ) -> C2RejectedWorkspace {
        let mut diagnostics = Vec::with_capacity(additional.len() + 1);
        diagnostics.push(primary);
        diagnostics.append(&mut additional);
        C2RejectedWorkspace {
            frontend: self.frontend,
            diagnostics,
        }
    }
}

/// A future fully checked C2 result.
///
/// No public constructor or checker can create this value yet. Its existence
/// pins the consuming success boundary without claiming that C2 is complete.
#[derive(Debug)]
pub struct C2CheckedWorkspace {
    frontend: FrontendOutput,
    indexes: SessionIndexTables,
}

impl C2CheckedWorkspace {
    /// Returns the retained C1 authority owned by the checked result.
    pub const fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }

    /// Returns the checked result's session-local indexes.
    pub const fn indexes(&self) -> &SessionIndexTables {
        &self.indexes
    }
}

/// A non-proof rejection wrapper preserving the complete retained C1 authority.
///
/// This wrapper records an ordered failure and ownership transfer only; it does
/// not imply that the incomplete C2 checker reached any particular phase.
#[derive(Debug)]
pub struct C2RejectedWorkspace {
    frontend: FrontendOutput,
    diagnostics: Vec<Diagnostic>,
}

impl C2RejectedWorkspace {
    /// Returns the retained C1 authority owned by the rejection.
    pub const fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }

    /// Returns the nonempty ordered diagnostic sequence.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

/// Read-only view shared by C2 handoff and terminal result wrappers.
pub trait RetainedFrontend {
    /// Returns the retained C1 authority.
    fn frontend(&self) -> &FrontendOutput;

    /// Returns the retained symbolic HIR.
    fn hir(&self) -> &ResolvedSymbolicWorkspaceHir {
        self.frontend().hir()
    }

    /// Returns the retained immutable source database.
    fn sources(&self) -> &std::sync::Arc<SourceDatabase> {
        self.frontend().sources()
    }

    /// Returns the retained unverified inventory skeleton.
    fn inventory(&self) -> &WorkspaceInventorySkeleton {
        self.frontend().inventory()
    }
}

impl RetainedFrontend for C2Handoff {
    fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }
}

impl RetainedFrontend for C2CheckedWorkspace {
    fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }
}

impl RetainedFrontend for C2RejectedWorkspace {
    fn frontend(&self) -> &FrontendOutput {
        &self.frontend
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use arche_frontend::check_workspace_c1;
    use arche_package::{load_workspace, resolve, ManifestRequest, RegistrySnapshot};

    use super::*;

    fn corpus_frontend(name: &str) -> FrontendOutput {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../tests/m27c1")
            .join(name);
        let workspace = load_workspace(&ManifestRequest::discover_from(&root)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        check_workspace_c1(&workspace, &graph, &[]).unwrap()
    }

    #[test]
    fn handoff_owns_frontend_and_builds_deterministic_item_indexes() {
        let handoff = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let expected = handoff
            .hir()
            .packages
            .iter()
            .flat_map(|package| &package.targets)
            .flat_map(|target| &target.items)
            .map(|item| item.id)
            .collect::<Vec<_>>();

        assert_eq!(handoff.indexes().item_count(), expected.len());
        assert_eq!(handoff.indexes().checked_type_count(), 0);
        for (offset, item) in expected.into_iter().enumerate() {
            let index = handoff.indexes().item_index(item).unwrap();
            assert_eq!(index.offset(), u64::try_from(offset).unwrap());
            assert_eq!(handoff.indexes().hir_item(&index), Some(item));
        }
    }

    #[test]
    fn opaque_indexes_are_validated_against_the_owning_table() {
        let handoff = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let out_of_range_item = SessionItemIndex {
            owner: Arc::clone(&handoff.indexes().owner),
            offset: u64::try_from(handoff.indexes().item_count()).unwrap(),
        };
        assert_eq!(handoff.indexes().hir_item(&out_of_range_item), None);
        assert!(!handoff.indexes().contains_checked_type(&SessionTypeIndex {
            owner: Arc::clone(&handoff.indexes().owner),
            offset: 0,
        }));
        assert!(!handoff.indexes().contains_checked_type(&SessionTypeIndex {
            owner: Arc::clone(&handoff.indexes().owner),
            offset: u64::MAX,
        }));
    }

    #[test]
    fn duplicate_hir_item_is_identity001_at_the_duplicate_span() {
        let frontend = corpus_frontend("language-game");
        let mut hir = frontend.hir().clone();
        let mut next_offset = 0_u64;
        let mut selected = None;
        'packages: for (package_index, package) in hir.packages.iter().enumerate() {
            for (target_index, target) in package.targets.iter().enumerate() {
                for item in &target.items {
                    if next_offset != 0 {
                        selected = Some((package_index, target_index, item.clone(), next_offset));
                        break 'packages;
                    }
                    next_offset += 1;
                }
            }
        }
        let (package_index, target_index, duplicate, first_offset) =
            selected.expect("the mandatory corpus retains at least two HIR items");
        assert_ne!(first_offset, 0);
        let duplicate_id = duplicate.id;
        let duplicate_span = duplicate.span;
        hir.packages[package_index].targets[target_index]
            .items
            .push(duplicate);

        let error = SessionIndexTables::from_hir(&hir).unwrap_err();
        let diagnostic = error.diagnostic();
        let expected_message = format!(
            "HIR item {} appears twice in the C2 session index (first at {first_offset})",
            duplicate_id.0
        );
        assert_eq!(diagnostic.code, "IDENTITY001");
        assert_eq!(diagnostic.message, expected_message);
        assert_eq!(diagnostic.primary.span, Some(duplicate_span));
        assert_eq!(diagnostic.primary.message, expected_message);
        assert!(diagnostic.secondary.is_empty());
        assert!(diagnostic.notes.is_empty());
    }

    #[test]
    fn distinct_workspace_sessions_reject_each_others_in_range_indexes() {
        let mut game = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let mut environment = C2Handoff::begin(corpus_frontend("language-environment")).unwrap();

        let game_item = game.indexes().item_rows[0];
        let environment_item = environment.indexes().item_rows[0];
        let game_index = game.indexes().item_index(game_item).unwrap();
        let environment_index = environment.indexes().item_index(environment_item).unwrap();
        assert_eq!(game_index.offset(), environment_index.offset());
        assert_eq!(game.indexes().hir_item(&game_index), Some(game_item));
        assert_eq!(
            environment.indexes().hir_item(&environment_index),
            Some(environment_item)
        );
        assert_eq!(game.indexes().hir_item(&environment_index), None);
        assert_eq!(environment.indexes().hir_item(&game_index), None);

        // Placeholder rows exercise owner validation only; they do not claim
        // that a type has passed any C2 semantic rule.
        game.indexes.checked_type_rows.push(CheckedTypeRow);
        environment.indexes.checked_type_rows.push(CheckedTypeRow);
        let game_type = SessionTypeIndex {
            owner: Arc::clone(&game.indexes.owner),
            offset: 0,
        };
        let environment_type = SessionTypeIndex {
            owner: Arc::clone(&environment.indexes.owner),
            offset: 0,
        };
        assert!(game.indexes().contains_checked_type(&game_type));
        assert!(environment
            .indexes()
            .contains_checked_type(&environment_type));
        assert!(!game.indexes().contains_checked_type(&environment_type));
        assert!(!environment.indexes().contains_checked_type(&game_type));
    }

    #[test]
    fn rejection_consumes_handoff_and_preserves_frontend_authority() {
        let handoff = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let source_count = handoff.sources().files().len();
        let package_count = handoff.hir().packages.len();
        let rejection = handoff.reject(
            Diagnostic::path("TYPE001", "deliberate scaffold rejection")
                .with_note("C2 does not expose a successful checker yet"),
            vec![Diagnostic::path("TRAIT001", "secondary semantic failure")],
        );

        assert_eq!(rejection.diagnostics().len(), 2);
        assert_eq!(rejection.sources().files().len(), source_count);
        assert_eq!(rejection.hir().packages.len(), package_count);
    }

    #[test]
    fn checked_boundary_would_consume_the_same_frontend_authority() {
        let handoff = C2Handoff::begin(corpus_frontend("language-game")).unwrap();
        let source_count = handoff.sources().files().len();
        let item_count = handoff.indexes().item_count();

        // This direct construction is available only to this in-crate test.
        // Production has no successful C2 checker or public proof constructor.
        let checked = C2CheckedWorkspace {
            frontend: handoff.frontend,
            indexes: handoff.indexes,
        };

        assert_eq!(checked.sources().files().len(), source_count);
        assert_eq!(checked.indexes().item_count(), item_count);
    }
}
