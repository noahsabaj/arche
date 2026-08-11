/// Library half of the deterministic environment corpus.
pub mod shared;

pub struct EnvironmentLibraryMarker {
    pub version: u32,
}

struct UnreferencedEnvironmentPrivate;
