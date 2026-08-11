/// Library root for the general-purpose language-game corpus.
pub mod shared;
mod hidden;

pub use self::hidden::PublicApi;
pub use self::hidden::api;

pub struct LibraryMarker {
    pub code: i32,
    pub(package) internal: u64,
    private: bool,
}

struct UnreferencedPrivate;
