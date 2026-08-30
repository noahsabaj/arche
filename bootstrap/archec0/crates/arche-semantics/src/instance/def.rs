//! Core data definitions for monomorphic instances and data layouts (M27-D).

use crate::mir::MirBody;
use arche_foundation::identity::{DefinitionId, InstanceId, TypeId};
use arche_frontend::Span;

/// Field offset metadata within a struct or tuple layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FieldOffset {
    pub offset_bytes: u64,
    pub size_bytes: u64,
    pub align_bytes: u64,
}

/// Target memory layout for a concrete type on x86-64.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeLayout {
    pub size_bytes: u64,
    pub align_bytes: u64,
    pub field_offsets: Vec<FieldOffset>,
}

impl TypeLayout {
    /// Zero-sized type layout (size 0, align 1).
    #[must_use]
    pub const fn zst() -> Self {
        Self {
            size_bytes: 0,
            align_bytes: 1,
            field_offsets: Vec::new(),
        }
    }

    /// Creates a scalar layout with exact size and alignment.
    #[must_use]
    pub const fn scalar(size: u64, align: u64) -> Self {
        Self {
            size_bytes: size,
            align_bytes: align,
            field_offsets: Vec::new(),
        }
    }
}

/// Kind of specialized instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceKind {
    /// Concrete monomorphic function.
    Function,
    /// Concrete monomorphic closure body.
    Closure,
    /// Concrete monomorphic generator body.
    Generator,
    /// Static VTable descriptor.
    VTable,
}

/// A fully monomorphized instance body ready for code generation.
#[derive(Clone, Debug, PartialEq)]
pub struct InstanceBody {
    pub instance_id: InstanceId,
    pub definition_id: DefinitionId,
    pub type_arguments: Vec<TypeId>,
    pub kind: InstanceKind,
    pub body: MirBody,
    pub span: Option<Span>,
}

/// Relocation kind within an ARCHEOBJ package object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelocKind {
    /// Reference to a local or imported InstanceId (64-bit address).
    InstanceRef(InstanceId),
    /// Reference to a content-addressed constant in the `.consts` section.
    ConstRef { offset: u64, size: u64 },
}

/// A relocation entry inside the `.relocs` section of ARCHEOBJ.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelocEntry {
    pub target_section: String,
    pub offset_in_section: u64,
    pub kind: RelocKind,
}
