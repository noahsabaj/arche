//! Separate compilation, generic monomorphization, layouts, and ARCHEOBJ v1 for M27-D.

pub mod archeobj;
pub mod def;
pub mod layout;
pub mod monomorphize;

pub use archeobj::{ArcheObjFile, ArcheObjSection, ARCHEOBJ_MAGIC, ARCHEOBJ_VERSION};
pub use def::{FieldOffset, InstanceBody, InstanceKind, RelocEntry, RelocKind, TypeLayout};
pub use layout::{align_to, compute_type_layout};
pub use monomorphize::{mint_instance_id, MonomorphizationTable};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mir::MirBody;
    use arche_foundation::identity::{DefinitionId, TypeId};
    use arche_frontend::SymbolicType;

    #[test]
    fn full_monomorphization_to_archeobj_pipeline() {
        let def_id = DefinitionId::from_bytes([1; 16]);
        let arg_type_id = TypeId::from_bytes([2; 16]);
        let mut table = MonomorphizationTable::new();

        let body = MirBody::new(SymbolicType::I32);
        let inst_id =
            table.register_instance(def_id, vec![arg_type_id], InstanceKind::Function, body);

        assert_eq!(table.instances.len(), 1);
        assert!(table.instances.contains_key(&inst_id));

        // Package into ARCHEOBJ v1
        let mut obj = ArcheObjFile::new();
        obj.add_section(".instances", 0, vec![1, 2, 3, 4]);
        obj.add_section(".layouts", 0, vec![5, 6, 7, 8]);
        let bytes = obj.write_to_bytes();

        assert_eq!(&bytes[0..8], ARCHEOBJ_MAGIC);
    }
}
