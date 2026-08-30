//! Target memory layout engine for x86-64 (M27-D).

use crate::instance::def::{FieldOffset, TypeLayout};
use arche_frontend::{SymbolicConstNode, SymbolicType};

/// Aligns an offset upward to the given power-of-two alignment.
#[must_use]
pub const fn align_to(offset: u64, align: u64) -> u64 {
    if align <= 1 {
        offset
    } else {
        (offset + align - 1) & !(align - 1)
    }
}

/// Computes the x86-64 target memory layout for a concrete symbolic type.
#[must_use]
pub fn compute_type_layout(ty: &SymbolicType) -> TypeLayout {
    match ty {
        SymbolicType::I8 | SymbolicType::U8 | SymbolicType::Bool => TypeLayout::scalar(1, 1),
        SymbolicType::I16 | SymbolicType::U16 => TypeLayout::scalar(2, 2),
        SymbolicType::I32 | SymbolicType::U32 | SymbolicType::F32 | SymbolicType::Char => {
            TypeLayout::scalar(4, 4)
        }
        SymbolicType::I64
        | SymbolicType::U64
        | SymbolicType::Isize
        | SymbolicType::Usize
        | SymbolicType::F64
        | SymbolicType::Entity
        | SymbolicType::Reference { .. }
        | SymbolicType::RawPointer { .. }
        | SymbolicType::FunctionPointer { .. }
        | SymbolicType::JoinHandle { .. } => TypeLayout::scalar(8, 8),
        SymbolicType::Unit | SymbolicType::Never => TypeLayout::zst(),
        SymbolicType::Str | SymbolicType::Slice(_) => TypeLayout::scalar(16, 8), // Fat pointer: (ptr: 8, len: 8)
        SymbolicType::Array { element, length } => {
            let elem_layout = compute_type_layout(element);
            let elem_stride = align_to(elem_layout.size_bytes, elem_layout.align_bytes);
            let count = match &length.node {
                SymbolicConstNode::IntegerLiteral(bytes) => {
                    let s = std::str::from_utf8(bytes).unwrap_or("0");
                    s.parse::<u64>().unwrap_or(0)
                }
                _ => 1,
            };
            let total_size = elem_stride * count;
            TypeLayout {
                size_bytes: total_size,
                align_bytes: elem_layout.align_bytes,
                field_offsets: Vec::new(),
            }
        }
        SymbolicType::Tuple(fields) => {
            if fields.is_empty() {
                return TypeLayout::zst();
            }
            let mut current_offset = 0u64;
            let mut max_align = 1u64;
            let mut field_offsets = Vec::with_capacity(fields.len());

            for field in fields {
                let field_layout = compute_type_layout(field);
                if field_layout.size_bytes == 0 {
                    // ZSTs do not advance offset
                    field_offsets.push(FieldOffset {
                        offset_bytes: current_offset,
                        size_bytes: 0,
                        align_bytes: field_layout.align_bytes,
                    });
                } else {
                    current_offset = align_to(current_offset, field_layout.align_bytes);
                    field_offsets.push(FieldOffset {
                        offset_bytes: current_offset,
                        size_bytes: field_layout.size_bytes,
                        align_bytes: field_layout.align_bytes,
                    });
                    current_offset += field_layout.size_bytes;
                }
                max_align = max_align.max(field_layout.align_bytes);
            }

            let total_size = align_to(current_offset, max_align);
            TypeLayout {
                size_bytes: total_size,
                align_bytes: max_align,
                field_offsets,
            }
        }
        _ => TypeLayout::scalar(8, 8), // Default nominal pointer/handle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitive_layouts() {
        assert_eq!(
            compute_type_layout(&SymbolicType::I32),
            TypeLayout::scalar(4, 4)
        );
        assert_eq!(
            compute_type_layout(&SymbolicType::U64),
            TypeLayout::scalar(8, 8)
        );
        assert_eq!(
            compute_type_layout(&SymbolicType::Bool),
            TypeLayout::scalar(1, 1)
        );
        assert_eq!(compute_type_layout(&SymbolicType::Unit), TypeLayout::zst());
    }

    #[test]
    fn tuple_layout_with_alignment_padding() {
        // (u8, u32, u8) -> u8 at 0, padding 1..4, u32 at 4, u8 at 8, padding 9..12 -> size 12, align 4
        let ty = SymbolicType::Tuple(vec![SymbolicType::U8, SymbolicType::U32, SymbolicType::U8]);
        let layout = compute_type_layout(&ty);
        assert_eq!(layout.size_bytes, 12);
        assert_eq!(layout.align_bytes, 4);
        assert_eq!(layout.field_offsets.len(), 3);
        assert_eq!(layout.field_offsets[0].offset_bytes, 0);
        assert_eq!(layout.field_offsets[1].offset_bytes, 4);
        assert_eq!(layout.field_offsets[2].offset_bytes, 8);
    }

    #[test]
    fn zst_fields_do_not_advance_offset() {
        // (u32, (), u32) -> u32 at 0, () at 4, u32 at 4 -> size 8, align 4
        let ty = SymbolicType::Tuple(vec![
            SymbolicType::U32,
            SymbolicType::Unit,
            SymbolicType::U32,
        ]);
        let layout = compute_type_layout(&ty);
        assert_eq!(layout.size_bytes, 8);
        assert_eq!(layout.align_bytes, 4);
        assert_eq!(layout.field_offsets[1].offset_bytes, 4);
        assert_eq!(layout.field_offsets[1].size_bytes, 0);
        assert_eq!(layout.field_offsets[2].offset_bytes, 4);
    }
}
