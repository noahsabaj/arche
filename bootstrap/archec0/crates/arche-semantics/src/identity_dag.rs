//! Topological 128-bit identity DAG construction and semantic inventory sealing for M27-C4.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use arche_foundation::identity::{DefinitionId, TypeId};
use arche_frontend::{
    encode_symbolic_type, SemanticDeclarationPath, Span, SymbolicType, TargetRoot,
};

/// Semantic identity DAG construction error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentityDagError {
    CyclicTypeDependency {
        type_id: TypeId,
        span: Option<Span>,
    },
    MissingChildDependency {
        parent: TypeId,
        missing_child: TypeId,
        span: Option<Span>,
    },
    DuplicateDefinitionId {
        id: DefinitionId,
        path: String,
    },
}

impl fmt::Display for IdentityDagError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CyclicTypeDependency { type_id, .. } => {
                write!(
                    formatter,
                    "IDENTITY001: cyclic type dependency detected for {}",
                    type_id
                )
            }
            Self::MissingChildDependency {
                parent,
                missing_child,
                ..
            } => {
                write!(
                    formatter,
                    "IDENTITY001: type {} references missing child {}",
                    parent, missing_child
                )
            }
            Self::DuplicateDefinitionId { id, path } => {
                write!(
                    formatter,
                    "IDENTITY001: duplicate definition ID {} minted for {}",
                    id, path
                )
            }
        }
    }
}

impl std::error::Error for IdentityDagError {}

/// Mints a stable 128-bit `DefinitionId` from canonical declaration path components.
#[must_use]
pub fn mint_definition_id(declaration: &SemanticDeclarationPath, shape_tag: &str) -> DefinitionId {
    let mut preimage = Vec::new();
    preimage.extend_from_slice(declaration.registry_origin.as_bytes());
    preimage.push(0);
    preimage.extend_from_slice(declaration.package_name.as_bytes());
    preimage.push(0);
    match &declaration.target {
        TargetRoot::Library => {
            preimage.extend_from_slice(b"lib");
        }
        TargetRoot::Binary(name) => {
            preimage.extend_from_slice(b"bin:");
            preimage.extend_from_slice(name.as_bytes());
        }
        TargetRoot::Environment(name) => {
            preimage.extend_from_slice(b"env:");
            preimage.extend_from_slice(name.as_bytes());
        }
    }
    preimage.push(0);
    for module in &declaration.modules {
        preimage.extend_from_slice(module.as_bytes());
        preimage.push(b'/');
    }
    preimage.push(0);
    preimage.extend_from_slice(declaration.name.as_bytes());
    preimage.push(0);
    preimage.extend_from_slice(shape_tag.as_bytes());

    DefinitionId::from_canonical_preimage(&preimage)
}

/// Mints a stable 128-bit `TypeId` from a symbolic type tree.
#[must_use]
pub fn mint_type_id(ty: &SymbolicType) -> TypeId {
    let preimage = encode_symbolic_type(ty).unwrap_or_default();
    TypeId::from_canonical_preimage(&preimage)
}

/// Verifies that a collection of minted type identities forms an acyclic topological DAG.
pub fn verify_type_dag_acyclicity(
    type_children: &BTreeMap<TypeId, Vec<TypeId>>,
) -> Result<(), IdentityDagError> {
    let mut visited = BTreeSet::new();
    let mut in_stack = BTreeSet::new();

    for &type_id in type_children.keys() {
        if !visited.contains(&type_id) {
            check_cycle_dfs(type_id, type_children, &mut visited, &mut in_stack)?;
        }
    }

    Ok(())
}

fn check_cycle_dfs(
    current: TypeId,
    graph: &BTreeMap<TypeId, Vec<TypeId>>,
    visited: &mut BTreeSet<TypeId>,
    in_stack: &mut BTreeSet<TypeId>,
) -> Result<(), IdentityDagError> {
    visited.insert(current);
    in_stack.insert(current);

    if let Some(children) = graph.get(&current) {
        for &child in children {
            if in_stack.contains(&child) {
                return Err(IdentityDagError::CyclicTypeDependency {
                    type_id: child,
                    span: None,
                });
            }
            if !visited.contains(&child) {
                check_cycle_dfs(child, graph, visited, in_stack)?;
            }
        }
    }

    in_stack.remove(&current);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arche_frontend::DeclarationKind;

    #[test]
    fn definition_id_is_deterministic_and_unique() {
        let decl_a = SemanticDeclarationPath {
            registry_origin: "workspace".into(),
            package_name: "pkg".into(),
            target: TargetRoot::Library,
            modules: vec!["mod_a".into()],
            kind: DeclarationKind::Struct,
            name: "ItemA".into(),
        };
        let decl_b = SemanticDeclarationPath {
            registry_origin: "workspace".into(),
            package_name: "pkg".into(),
            target: TargetRoot::Library,
            modules: vec!["mod_a".into()],
            kind: DeclarationKind::Struct,
            name: "ItemB".into(),
        };

        let id_a1 = mint_definition_id(&decl_a, "struct");
        let id_a2 = mint_definition_id(&decl_a, "struct");
        let id_b = mint_definition_id(&decl_b, "struct");

        assert_eq!(id_a1, id_a2);
        assert_ne!(id_a1, id_b);
    }

    #[test]
    fn type_id_is_deterministic_and_structural() {
        let ty_i32 = SymbolicType::I32;
        let id1 = mint_type_id(&ty_i32);
        let id2 = mint_type_id(&ty_i32);
        assert_eq!(id1, id2);

        let ty_tuple = SymbolicType::Tuple(vec![SymbolicType::I32, SymbolicType::Bool]);
        let id_tuple = mint_type_id(&ty_tuple);
        assert_ne!(id1, id_tuple);
    }

    #[test]
    fn type_dag_detects_cycles() {
        let t1 = mint_type_id(&SymbolicType::I32);
        let t2 = mint_type_id(&SymbolicType::U32);

        let mut graph = BTreeMap::new();
        graph.insert(t1, vec![t2]);
        graph.insert(t2, vec![t1]); // Cycle: t1 -> t2 -> t1

        let err = verify_type_dag_acyclicity(&graph).unwrap_err();
        assert!(matches!(err, IdentityDagError::CyclicTypeDependency { .. }));
    }
}
