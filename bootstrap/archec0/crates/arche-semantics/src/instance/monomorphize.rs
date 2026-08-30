//! Monomorphization graph and InstanceId minting for M27-D.

use std::collections::BTreeMap;

use crate::instance::def::{InstanceBody, InstanceKind};
use crate::mir::MirBody;
use arche_foundation::identity::{DefinitionId, InstanceId, TypeId};

/// Mints a stable 128-bit `InstanceId` from a generic `DefinitionId` and concrete argument `TypeId`s.
#[must_use]
pub fn mint_instance_id(definition_id: DefinitionId, type_args: &[TypeId]) -> InstanceId {
    let mut preimage = Vec::new();
    preimage.extend_from_slice(definition_id.as_bytes());
    preimage.extend_from_slice(&(type_args.len() as u32).to_le_bytes());
    for arg in type_args {
        preimage.extend_from_slice(arg.as_bytes());
    }
    InstanceId::from_canonical_preimage(&preimage)
}

/// A collection of monomorphized instance bodies for a package target.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonomorphizationTable {
    pub instances: BTreeMap<InstanceId, InstanceBody>,
}

impl MonomorphizationTable {
    /// Creates an empty monomorphization table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a specialized instance body.
    pub fn register_instance(
        &mut self,
        definition_id: DefinitionId,
        type_arguments: Vec<TypeId>,
        kind: InstanceKind,
        body: MirBody,
    ) -> InstanceId {
        let instance_id = mint_instance_id(definition_id, &type_arguments);
        self.instances.insert(
            instance_id,
            InstanceBody {
                instance_id,
                definition_id,
                type_arguments,
                kind,
                body,
                span: None,
            },
        );
        instance_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_id_is_deterministic_and_depends_on_type_args() {
        let def_id = DefinitionId::from_bytes([1; 16]);
        let t1 = TypeId::from_bytes([2; 16]);
        let t2 = TypeId::from_bytes([3; 16]);

        let inst1 = mint_instance_id(def_id, &[t1]);
        let inst1_dup = mint_instance_id(def_id, &[t1]);
        let inst2 = mint_instance_id(def_id, &[t2]);

        assert_eq!(inst1, inst1_dup);
        assert_ne!(inst1, inst2);
    }
}
