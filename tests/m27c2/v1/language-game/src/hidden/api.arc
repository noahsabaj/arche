pub tag ReexportedModuleMarker;

use super::PublicApi;
use super::super::shared::Position;

pub(super) struct ParentVisible;
pub(in package::hidden) struct AncestorVisible;
pub(in self) struct SelfVisible;
