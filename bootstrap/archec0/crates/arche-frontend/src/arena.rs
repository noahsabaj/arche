//! Checked session-arena allocation for M27-C HIR construction.
//!
//! These counters deliberately do not mint any stable identity. They allocate
//! only workspace-session IDs and fail before wrapping.

use std::fmt;

use arche_package::PackageNodeId;

use super::{HirBodyId, HirItemId, HirModuleId, TargetId};

pub const IDENTITY_ERROR_CODE: &str = "IDENTITY001";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArenaExhausted {
    arena: &'static str,
}

impl ArenaExhausted {
    pub const fn code(&self) -> &'static str {
        IDENTITY_ERROR_CODE
    }
}

impl fmt::Display for ArenaExhausted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {} session ID allocator is exhausted",
            IDENTITY_ERROR_CODE, self.arena
        )
    }
}

impl std::error::Error for ArenaExhausted {}

#[derive(Clone, Debug)]
struct CheckedArena {
    arena: &'static str,
    next: u64,
    last_assignable: u64,
    exhausted: bool,
}

impl CheckedArena {
    const fn new(arena: &'static str, last_assignable: u64) -> Self {
        Self {
            arena,
            next: 0,
            last_assignable,
            exhausted: false,
        }
    }

    fn allocate(&mut self) -> Result<u64, ArenaExhausted> {
        if self.exhausted || self.next > self.last_assignable {
            return Err(ArenaExhausted { arena: self.arena });
        }
        let id = self.next;
        if id == self.last_assignable {
            self.exhausted = true;
        } else {
            self.next = id
                .checked_add(1)
                .ok_or(ArenaExhausted { arena: self.arena })?;
        }
        Ok(id)
    }

    #[cfg(test)]
    const fn starting_at(arena: &'static str, next: u64, last_assignable: u64) -> Self {
        Self {
            arena,
            next,
            last_assignable,
            exhausted: false,
        }
    }
}

/// The globally unique item and body arenas owned by one `FrontendOutput`.
///
/// `FileId` authority belongs exclusively to `SourceDatabaseBuilder`. Item and
/// body arenas may allocate the full `u64` range once, then report checked
/// exhaustion.
#[derive(Clone, Debug)]
pub struct HirArenaAllocators {
    items: CheckedArena,
    bodies: CheckedArena,
}

impl Default for HirArenaAllocators {
    fn default() -> Self {
        Self {
            items: CheckedArena::new("HirItemId", u64::MAX),
            bodies: CheckedArena::new("HirBodyId", u64::MAX),
        }
    }
}

impl HirArenaAllocators {
    pub fn next_item(&mut self) -> Result<HirItemId, ArenaExhausted> {
        self.items.allocate().map(HirItemId)
    }

    pub fn next_body(&mut self) -> Result<HirBodyId, ArenaExhausted> {
        self.bodies.allocate().map(HirBodyId)
    }

    #[cfg(test)]
    fn near_exhaustion() -> Self {
        Self {
            items: CheckedArena::starting_at("HirItemId", u64::MAX, u64::MAX),
            bodies: CheckedArena::starting_at("HirBodyId", u64::MAX, u64::MAX),
        }
    }
}

/// Dense module-local arena for one `(PackageNodeId, TargetId)` pair.
#[derive(Clone, Debug)]
pub struct HirModuleIdAllocator {
    package: PackageNodeId,
    target: TargetId,
    modules: CheckedArena,
}

impl HirModuleIdAllocator {
    pub const fn new(package: PackageNodeId, target: TargetId) -> Self {
        Self {
            package,
            target,
            modules: CheckedArena::new("HirModuleId", u64::MAX),
        }
    }

    pub fn next_module(&mut self) -> Result<HirModuleId, ArenaExhausted> {
        self.modules
            .allocate()
            .map(|local| HirModuleId::new(self.package, self.target, local))
    }

    #[cfg(test)]
    pub(crate) const fn near_exhaustion(package: PackageNodeId, target: TargetId) -> Self {
        Self {
            package,
            target,
            modules: CheckedArena::starting_at("HirModuleId", u64::MAX, u64::MAX),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arenas_start_at_zero_and_remain_distinct() {
        let mut arenas = HirArenaAllocators::default();
        assert_eq!(arenas.next_item().unwrap(), HirItemId(0));
        assert_eq!(arenas.next_body().unwrap(), HirBodyId(0));
        assert_eq!(arenas.next_item().unwrap(), HirItemId(1));
        assert_eq!(arenas.next_body().unwrap(), HirBodyId(1));
    }

    #[test]
    fn each_last_value_is_assigned_once_then_identity001_is_reported() {
        let mut arenas = HirArenaAllocators::near_exhaustion();
        assert_eq!(arenas.next_item().unwrap(), HirItemId(u64::MAX));
        assert_eq!(arenas.next_body().unwrap(), HirBodyId(u64::MAX));

        for error in [
            arenas.next_item().unwrap_err(),
            arenas.next_body().unwrap_err(),
        ] {
            assert_eq!(error.code(), "IDENTITY001");
        }
    }

    #[test]
    fn module_arena_is_package_target_qualified_and_fails_closed() {
        let package = PackageNodeId::new(7);
        let target = TargetId(3);
        let mut modules = HirModuleIdAllocator::near_exhaustion(package, target);

        assert_eq!(
            modules.next_module().unwrap(),
            HirModuleId::new(package, target, u64::MAX)
        );
        let error = modules.next_module().unwrap_err();
        assert_eq!(error.code(), "IDENTITY001");
        assert!(error.to_string().contains("HirModuleId"));
        assert_eq!(modules.next_module().unwrap_err(), error);
    }
}
