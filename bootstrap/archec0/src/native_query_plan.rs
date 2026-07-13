#![allow(dead_code)]

use crate::core::{
    CoreProgram, CoreQueryAccess, CoreQueryLoop, CoreSystem, CoreSystemParamKind,
    CoreSystemStatement,
};
use crate::core_verify;
use crate::native_world_plan::{NativeCatalogColumnSlots, NativeSlot, NativeWorldStoragePlan};
use crate::runtime::{self, ComponentFieldDescriptor};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeQueryPlanError {
    pub(crate) message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeQueryBindingPlan {
    pub(crate) queries: Vec<NativeBoundQuery>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeBoundQuery {
    pub(crate) system_id: u64,
    pub(crate) system_name: String,
    pub(crate) query_id: u64,
    pub(crate) query_param: String,
    pub(crate) terms: Vec<NativeBoundQueryTerm>,
    pub(crate) scan_blocks: Vec<NativeQueryScanBlock>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeBoundQueryTerm {
    pub(crate) binding_name: String,
    pub(crate) component_id: u64,
    pub(crate) component_name: String,
    pub(crate) access: CoreQueryAccess,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeQueryScanBlock {
    pub(crate) table_index: usize,
    pub(crate) table_key: Box<[u64]>,
    pub(crate) catalog_row_count_address_slot: NativeSlot,
    pub(crate) capacity: u32,
    pub(crate) columns: Vec<NativeBoundQueryColumn>,
    pub(crate) row_cases: Vec<NativeQueryRowCase>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeBoundQueryColumn {
    pub(crate) binding_name: String,
    pub(crate) component_id: u64,
    pub(crate) component_name: String,
    pub(crate) access: CoreQueryAccess,
    pub(crate) element_size: u32,
    pub(crate) element_align: u32,
    pub(crate) fields: Vec<ComponentFieldDescriptor>,
    pub(crate) catalog: NativeCatalogColumnSlots,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeQueryRowCase {
    pub(crate) row_index: u32,
    pub(crate) guard: NativeQueryLiveRowGuard,
    pub(crate) planned_terms: Vec<NativePlannedQueryTermAddress>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeQueryLiveRowGuard {
    pub(crate) row_index: u32,
    pub(crate) catalog_row_count_address_slot: NativeSlot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativePlannedQueryTermAddress {
    pub(crate) binding_name: String,
    pub(crate) component_id: u64,
    pub(crate) access: CoreQueryAccess,
    pub(crate) payload_base_address_slot: NativeSlot,
    pub(crate) element_size: u32,
    pub(crate) row_index: u32,
    pub(crate) byte_offset: u32,
}

pub(crate) fn derive_native_query_binding_plan(
    core: &CoreProgram,
    storage: &NativeWorldStoragePlan,
) -> Result<NativeQueryBindingPlan, NativeQueryPlanError> {
    core_verify::verify_core_program(core).map_err(|error| {
        query_plan_error(format!(
            "cannot derive native query bindings from invalid Core: {}",
            error.message
        ))
    })?;

    let mut queries = Vec::new();
    for system in &core.systems {
        for statement in &system.body.statements {
            let CoreSystemStatement::QueryLoop(query_loop) = statement else {
                continue;
            };
            queries.push(bind_query_loop(core, system, query_loop, storage)?);
        }
    }

    Ok(NativeQueryBindingPlan { queries })
}

fn bind_query_loop(
    core: &CoreProgram,
    system: &CoreSystem,
    query_loop: &CoreQueryLoop,
    storage: &NativeWorldStoragePlan,
) -> Result<NativeBoundQuery, NativeQueryPlanError> {
    let query_param = system
        .params
        .iter()
        .find(|param| param.name == query_loop.query_param)
        .ok_or_else(|| {
            query_plan_error(format!(
                "verified Core query parameter `{}` disappeared during native binding",
                query_loop.query_param
            ))
        })?;
    let CoreSystemParamKind::Query { terms } = &query_param.kind else {
        return Err(query_plan_error(format!(
            "verified Core parameter `{}` is not a query",
            query_loop.query_param
        )));
    };

    let bound_terms = query_loop
        .bindings
        .iter()
        .zip(terms)
        .map(|(binding, term)| NativeBoundQueryTerm {
            binding_name: binding.name.clone(),
            component_id: term.component_id,
            component_name: term.name.clone(),
            access: term.access,
        })
        .collect::<Vec<_>>();

    let mut scan_blocks = Vec::new();
    for (table_index, table) in storage.tables.iter().enumerate() {
        if !bound_terms.iter().all(|term| {
            table
                .columns
                .iter()
                .any(|column| column.schema.id == term.component_id)
        }) {
            continue;
        }

        let columns = bound_terms
            .iter()
            .map(|term| {
                let column = table
                    .columns
                    .iter()
                    .find(|column| column.schema.id == term.component_id)
                    .expect("matching table contains every query component");
                NativeBoundQueryColumn {
                    binding_name: term.binding_name.clone(),
                    component_id: term.component_id,
                    component_name: term.component_name.clone(),
                    access: term.access,
                    element_size: column.schema.size,
                    element_align: column.schema.align,
                    fields: column.schema.fields.clone(),
                    catalog: column.catalog,
                }
            })
            .collect::<Vec<_>>();
        let row_cases =
            derive_capacity_row_cases(table.capacity, table.catalog.row_count_address, &columns)?;

        scan_blocks.push(NativeQueryScanBlock {
            table_index,
            table_key: table.key.clone(),
            catalog_row_count_address_slot: table.catalog.row_count_address,
            capacity: table.capacity,
            columns,
            row_cases,
        });
    }

    Ok(NativeBoundQuery {
        system_id: system.id,
        system_name: system.name.clone(),
        query_id: runtime::stable_query_id(&core.world.name, &system.name, &query_loop.query_param)
            .0,
        query_param: query_loop.query_param.clone(),
        terms: bound_terms,
        scan_blocks,
    })
}

fn derive_capacity_row_cases(
    capacity: u32,
    catalog_row_count_address_slot: NativeSlot,
    columns: &[NativeBoundQueryColumn],
) -> Result<Vec<NativeQueryRowCase>, NativeQueryPlanError> {
    let mut row_cases = Vec::with_capacity(capacity as usize);
    for row_index in 0..capacity {
        let planned_terms = columns
            .iter()
            .map(|column| {
                let byte_offset = row_index.checked_mul(column.element_size).ok_or_else(|| {
                    query_plan_error(format!(
                        "native query row {row_index} byte offset overflows component `{}`",
                        column.component_name
                    ))
                })?;
                Ok(NativePlannedQueryTermAddress {
                    binding_name: column.binding_name.clone(),
                    component_id: column.component_id,
                    access: column.access,
                    payload_base_address_slot: column.catalog.payload_base_address,
                    element_size: column.element_size,
                    row_index,
                    byte_offset,
                })
            })
            .collect::<Result<Vec<_>, NativeQueryPlanError>>()?;
        row_cases.push(NativeQueryRowCase {
            row_index,
            guard: NativeQueryLiveRowGuard {
                row_index,
                catalog_row_count_address_slot,
            },
            planned_terms,
        });
    }
    Ok(row_cases)
}

fn query_plan_error(message: impl Into<String>) -> NativeQueryPlanError {
    NativeQueryPlanError {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_world_plan::{derive_native_world_storage_plan, NATIVE_STORAGE_BASE_OFFSET};
    use crate::{checker, core_lower, lexer, parser, runtime_assembly};

    #[test]
    fn matches_archetypes_and_binds_query_terms_by_component_id() {
        let (demo_core, demo_storage) =
            lower_fixture(include_str!("../../../examples/move_system_two_rows.arc"));
        let demo = derive_native_query_binding_plan(&demo_core, &demo_storage)
            .expect("Demo query bindings derive");
        let [demo_query] = demo.queries.as_slice() else {
            panic!("Demo has exactly one compiled query loop");
        };
        assert_eq!(demo_query.system_name, "Move");
        assert_eq!(demo_query.query_param, "movers");
        assert_eq!(
            demo_query
                .terms
                .iter()
                .map(|term| (
                    term.binding_name.as_str(),
                    term.component_name.as_str(),
                    term.access
                ))
                .collect::<Vec<_>>(),
            [
                ("pos", "Demo.Position", CoreQueryAccess::Mut),
                ("vel", "Demo.Velocity", CoreQueryAccess::Read),
            ]
        );
        let [demo_block] = demo_query.scan_blocks.as_slice() else {
            panic!("Demo query matches exactly one table");
        };
        assert_eq!(demo_block.capacity, 2);
        assert_eq!(demo_block.row_cases.len(), 2);
        assert_eq!(
            demo_block.catalog_row_count_address_slot,
            demo_storage.tables[0].catalog.row_count_address
        );
        assert!(demo_block.row_cases.iter().all(|row_case| {
            row_case.guard.catalog_row_count_address_slot
                == demo_storage.tables[0].catalog.row_count_address
        }));
        assert_eq!(
            demo_block.row_cases[1]
                .planned_terms
                .iter()
                .map(|term| term.byte_offset)
                .collect::<Vec<_>>(),
            [8, 8]
        );

        let (arena_core, arena_storage) =
            lower_fixture(include_str!("../../../examples/arena_recovery.arc"));
        assert_eq!(arena_storage.tables.len(), 2);
        assert_eq!(arena_storage.tables[0].rows.len(), 3);
        assert_eq!(arena_storage.tables[0].capacity, 4);
        assert_eq!(arena_storage.tables[1].rows.len(), 2);
        let arena = derive_native_query_binding_plan(&arena_core, &arena_storage)
            .expect("Arena query bindings derive");
        let [arena_query] = arena.queries.as_slice() else {
            panic!("Arena has exactly one compiled query loop");
        };
        assert_eq!(arena_query.system_name, "Recover");
        assert_eq!(arena_query.query_param, "units");
        assert_eq!(
            arena_query
                .terms
                .iter()
                .map(|term| (
                    term.binding_name.as_str(),
                    term.component_name.as_str(),
                    term.access
                ))
                .collect::<Vec<_>>(),
            [
                ("vitality", "Arena.Vitality", CoreQueryAccess::Mut),
                ("regeneration", "Arena.Regeneration", CoreQueryAccess::Read,),
            ]
        );

        let [arena_block] = arena_query.scan_blocks.as_slice() else {
            panic!("Arena query matches only the full archetype table");
        };
        assert_eq!(arena_block.table_index, 0);
        assert_eq!(arena_block.table_key, arena_storage.tables[0].key);
        assert_eq!(arena_block.capacity, 4);
        assert_eq!(arena_block.row_cases.len(), 4);
        assert_eq!(
            arena_block.catalog_row_count_address_slot,
            arena_storage.tables[0].catalog.row_count_address
        );
        assert_eq!(
            arena_block
                .columns
                .iter()
                .map(|column| {
                    (
                        column.component_name.as_str(),
                        column.element_size,
                        column.access,
                    )
                })
                .collect::<Vec<_>>(),
            [
                ("Arena.Vitality", 8, CoreQueryAccess::Mut),
                ("Arena.Regeneration", 12, CoreQueryAccess::Read),
            ]
        );
        assert_eq!(
            arena_block.columns[0]
                .fields
                .iter()
                .map(|field| (field.name.as_str(), field.offset))
                .collect::<Vec<_>>(),
            [("current", 0), ("reserve", 4)]
        );
        assert_eq!(
            arena_block.columns[1]
                .fields
                .iter()
                .map(|field| (field.name.as_str(), field.offset))
                .collect::<Vec<_>>(),
            [("current_rate", 0), ("reserve_rate", 4), ("cap", 8),]
        );
        let final_case = &arena_block.row_cases[3];
        assert_eq!(final_case.row_index, 3);
        assert_eq!(final_case.guard.row_index, 3);
        assert!(arena_block.row_cases.iter().all(|row_case| {
            row_case.guard.catalog_row_count_address_slot
                == arena_storage.tables[0].catalog.row_count_address
        }));
        assert_eq!(
            final_case
                .planned_terms
                .iter()
                .map(|term| (term.row_index, term.element_size, term.byte_offset))
                .collect::<Vec<_>>(),
            [(3, 8, 24), (3, 12, 36)]
        );
        assert!(arena_storage.tables[1]
            .columns
            .iter()
            .all(|column| column.schema.name != "Arena.Regeneration"));

        let arena_with_unrelated_source = include_str!("../../../examples/arena_recovery.arc")
            .replacen(
                "resource Tick",
                "component Tag {\n    id: i32\n}\n\nresource Tick",
                1,
            )
            .replacen(
                "    run Step",
                "    spawn {\n        Tag { id: 99 }\n    }\n\n    run Step",
                1,
            );
        let (arena_with_unrelated_core, arena_with_unrelated_storage) =
            lower_fixture(&arena_with_unrelated_source);
        assert_eq!(arena_with_unrelated_storage.tables.len(), 3);
        let arena_with_unrelated = derive_native_query_binding_plan(
            &arena_with_unrelated_core,
            &arena_with_unrelated_storage,
        )
        .expect("an unrelated archetype does not affect query planning");
        let [arena_with_unrelated_query] = arena_with_unrelated.queries.as_slice() else {
            panic!("extended Arena still has one query loop");
        };
        let [arena_with_unrelated_block] = arena_with_unrelated_query.scan_blocks.as_slice() else {
            panic!("partial and unrelated Arena tables are both excluded");
        };
        assert_eq!(arena_with_unrelated_block.capacity, 4);
        assert_eq!(arena_with_unrelated_block.row_cases.len(), 4);
        assert!(arena_with_unrelated_storage.tables.iter().any(|table| {
            table.columns.len() == 1 && table.columns[0].schema.name == "Arena.Tag"
        }));

        let arena_with_two_matches_source = arena_with_unrelated_source.replacen(
            "        Faction { id: 1 }",
            "        Faction { id: 1 }\n        Tag { id: 11 }",
            1,
        );
        let (arena_with_two_matches_core, arena_with_two_matches_storage) =
            lower_fixture(&arena_with_two_matches_source);
        let arena_with_two_matches = derive_native_query_binding_plan(
            &arena_with_two_matches_core,
            &arena_with_two_matches_storage,
        )
        .expect("every table containing the required component subset binds");
        let [arena_with_two_matches_query] = arena_with_two_matches.queries.as_slice() else {
            panic!("extended Arena still has one query loop");
        };
        assert_eq!(arena_with_two_matches_query.scan_blocks.len(), 2);
        assert!(arena_with_two_matches_query
            .scan_blocks
            .iter()
            .all(|block| block.columns.len() == 2));
        assert_eq!(
            arena_with_two_matches_query
                .scan_blocks
                .iter()
                .map(|block| block.row_cases.len())
                .sum::<usize>(),
            3,
            "two live-full tables have capacity-derived cases 2 + 1"
        );

        let mut reordered_core = arena_core.clone();
        reordered_core.components.reverse();
        let mut reordered_storage = arena_storage.clone();
        for table in &mut reordered_storage.tables {
            table.columns.reverse();
        }
        assert_eq!(
            derive_native_query_binding_plan(&reordered_core, &reordered_storage)
                .expect("declaration and column order do not affect query bindings"),
            arena
        );
    }

    #[test]
    fn permits_repeated_read_only_terms_and_rejects_mutable_aliases() {
        let source = r#"
world Repeat

component Sample {
    value: f32
}

system Observe(items: query[Sample, Sample]) {
    for (left, right) in items {
        left.value
        right.value
    }
}

startup {
    spawn {
        Sample { value: 1.0 }
    }
    exit 0
}
"#;
        let (core, storage) = lower_fixture(source);
        let plan = derive_native_query_binding_plan(&core, &storage)
            .expect("repeated read-only query terms remain legal");
        let [query] = plan.queries.as_slice() else {
            panic!("Repeat has one query loop");
        };
        assert_eq!(query.terms.len(), 2);
        assert_eq!(query.terms[0].component_id, query.terms[1].component_id);
        assert_eq!(query.terms[0].access, CoreQueryAccess::Read);
        assert_eq!(query.terms[1].access, CoreQueryAccess::Read);
        let [block] = query.scan_blocks.as_slice() else {
            panic!("repeated read-only terms match the one-component table");
        };
        assert_eq!(block.columns.len(), 2);
        assert_eq!(
            block.columns[0].catalog.payload_base_address,
            block.columns[1].catalog.payload_base_address
        );

        let mut invalid_core = core;
        let query_param = invalid_core.systems[0]
            .params
            .iter_mut()
            .find(|param| param.name == "items")
            .expect("items parameter exists");
        let CoreSystemParamKind::Query { terms } = &mut query_param.kind else {
            panic!("items is a query");
        };
        terms[0].access = CoreQueryAccess::Mut;
        let CoreSystemStatement::QueryLoop(query_loop) =
            &mut invalid_core.systems[0].body.statements[0]
        else {
            panic!("Observe contains a query loop");
        };
        query_loop.bindings[0].access = CoreQueryAccess::Mut;
        let error = derive_native_query_binding_plan(&invalid_core, &storage)
            .expect_err("mutable aliases must be rejected before native binding");
        assert!(error.message.contains("conflicting Core query access"));
    }

    fn lower_fixture(source: &str) -> (CoreProgram, NativeWorldStoragePlan) {
        let tokens = lexer::lex(source).expect("fixture lexes");
        let program = parser::parse_program(&tokens).expect("fixture parses");
        checker::check_program(&program).expect("fixture checks");
        let core = core_lower::lower_program_to_core(&program).expect("fixture Core lowers");
        let assembly =
            runtime_assembly::assemble_runtime_program_from_verified_core(&program, &core)
                .expect("fixture runtime assembly builds");
        let storage =
            derive_native_world_storage_plan(&core, &assembly, NATIVE_STORAGE_BASE_OFFSET)
                .expect("fixture native storage plan derives");
        (core, storage)
    }
}
