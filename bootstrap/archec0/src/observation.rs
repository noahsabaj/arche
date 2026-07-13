use std::collections::HashSet;
use std::fmt::Write;

use crate::layout::ComponentId;
use crate::native_world_plan::NativeWorldStoragePlan;
use crate::runtime::{ArcheWorld, ArchetypeKey};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ObservationError {
    pub(crate) message: String,
}

pub(crate) fn serialize_world_observation(
    world: &ArcheWorld,
    plan: &NativeWorldStoragePlan,
) -> Result<Vec<u8>, ObservationError> {
    if world.archetype_count() != plan.tables.len() {
        return Err(observation_error(format!(
            "world contains {} archetypes, but the native plan contains {}",
            world.archetype_count(),
            plan.tables.len()
        )));
    }

    let mut output = String::from("ARCHEOBS1\n");
    let mut observed_keys = HashSet::new();
    for table in &plan.tables {
        let key_ids = table
            .key
            .iter()
            .copied()
            .map(ComponentId)
            .collect::<Vec<_>>();
        let key = ArchetypeKey::new(key_ids);
        if !observed_keys.insert(table.key.to_vec()) {
            return Err(observation_error(
                "native observation plan contains a duplicate archetype key",
            ));
        }
        if table.columns.len() != table.key.len()
            || !table
                .columns
                .iter()
                .zip(table.key.iter())
                .all(|(column, component_id)| column.schema.id == *component_id)
        {
            return Err(observation_error(
                "native observation plan columns do not match the canonical table key",
            ));
        }

        let world_table = world.archetype(&key).ok_or_else(|| {
            observation_error("native observation plan references a missing world archetype")
        })?;
        let live_row_count = world_table.entity_count();
        if live_row_count > table.rows.len() {
            return Err(observation_error(format!(
                "world archetype has {live_row_count} live rows, but the native plan has only {} row identities",
                table.rows.len()
            )));
        }
        for column in &table.columns {
            let world_column = world_table
                .column(ComponentId(column.schema.id))
                .ok_or_else(|| {
                    observation_error(format!(
                        "world archetype is missing component 0x{:016X}",
                        column.schema.id
                    ))
                })?;
            if world_column.row_capacity() != table.capacity as usize {
                return Err(observation_error(format!(
                    "world component 0x{:016X} has capacity {}, expected {}",
                    column.schema.id,
                    world_column.row_capacity(),
                    table.capacity
                )));
            }
        }

        write!(&mut output, "T {}", table.key.len()).expect("writing to a String cannot fail");
        for component_id in table.key.iter() {
            write!(&mut output, " {component_id:016X}").expect("writing to a String cannot fail");
        }
        writeln!(&mut output, " {live_row_count} {}", table.capacity)
            .expect("writing to a String cannot fail");

        for row_index in 0..live_row_count {
            if world_table.entity(row_index).is_none() {
                return Err(observation_error(format!(
                    "world archetype row {row_index} has no live entity"
                )));
            }
            let planned_row = table.rows.get(row_index).ok_or_else(|| {
                observation_error(format!(
                    "native observation row {row_index} has no spawn identity"
                ))
            })?;
            write!(
                &mut output,
                "R {row_index} {} {}",
                planned_row.spawn_ordinal,
                table.columns.len()
            )
            .expect("writing to a String cannot fail");

            for column in &table.columns {
                let payload = world_table
                    .column(ComponentId(column.schema.id))
                    .and_then(|column| column.row_bytes(row_index))
                    .ok_or_else(|| {
                        observation_error(format!(
                            "world archetype row {row_index} is missing component 0x{:016X}",
                            column.schema.id
                        ))
                    })?;
                if payload.len() != column.schema.size as usize {
                    return Err(observation_error(format!(
                        "world component 0x{:016X} row {row_index} has {} bytes, expected {}",
                        column.schema.id,
                        payload.len(),
                        column.schema.size
                    )));
                }
                write!(&mut output, " {:016X} {} ", column.schema.id, payload.len())
                    .expect("writing to a String cannot fail");
                write_upper_hex(&mut output, payload);
            }
            output.push('\n');
        }
    }

    Ok(output.into_bytes())
}

fn write_upper_hex(output: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    output.reserve(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
}

fn observation_error(message: impl Into<String>) -> ObservationError {
    ObservationError {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_world_plan::{derive_native_world_storage_plan, NATIVE_STORAGE_BASE_OFFSET};
    use crate::runtime_assembly::{
        self, execute_startup_resource_payload_operation, execute_startup_spawn_operation,
        register_assembly_descriptors_into_world, StartupOperation,
    };
    use crate::{checker, core_lower, core_verify, lexer, parser};

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct ParsedObservation {
        tables: Vec<ParsedTable>,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct ParsedTable {
        key: Vec<u64>,
        live_row_count: usize,
        capacity: usize,
        rows: Vec<ParsedRow>,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct ParsedRow {
        row_index: usize,
        spawn_ordinal: u32,
        components: Vec<(u64, Vec<u8>)>,
    }

    #[test]
    fn serializes_reference_observation_canonically() {
        for (source, expected_tables, expected_rows) in [
            (
                include_str!("../../../examples/move_system_two_rows.arc"),
                1,
                2,
            ),
            (include_str!("../../../examples/arena_recovery.arc"), 2, 5),
        ] {
            let (world, plan) = startup_world(source);
            let serialized = serialize_world_observation(&world, &plan)
                .expect("committed startup world serializes");
            assert!(serialized.starts_with(b"ARCHEOBS1\n"));
            assert!(serialized
                .iter()
                .all(|byte| byte.is_ascii() && !byte.is_ascii_lowercase()));

            let parsed = parse_observation(&serialized);
            assert_eq!(parsed.tables.len(), expected_tables);
            assert_eq!(
                parsed
                    .tables
                    .iter()
                    .map(|table| table.rows.len())
                    .sum::<usize>(),
                expected_rows
            );
            for table in &parsed.tables {
                assert_eq!(table.live_row_count, table.rows.len());
                assert!(table.capacity >= table.live_row_count);
                assert!(table
                    .rows
                    .iter()
                    .enumerate()
                    .all(|(index, row)| index == row.row_index));
                assert!(table.rows.iter().all(|row| row
                    .components
                    .iter()
                    .map(|item| item.0)
                    .eq(table.key.iter().copied())));
            }
        }
    }

    #[test]
    fn parser_observes_arena_integer_payloads_and_spawn_ordinals() {
        let (world, plan) = startup_world(include_str!("../../../examples/arena_recovery.arc"));
        let serialized =
            serialize_world_observation(&world, &plan).expect("Arena startup world serializes");
        let parsed = parse_observation(&serialized);
        assert_eq!(
            parsed
                .tables
                .iter()
                .map(|table| (table.live_row_count, table.capacity))
                .collect::<Vec<_>>(),
            [(3, 4), (2, 2)]
        );
        let faction_id = crate::layout::stable_component_id("Arena", "Faction").0;
        let factions = parsed
            .tables
            .iter()
            .flat_map(|table| &table.rows)
            .map(|row| {
                let bytes = &row
                    .components
                    .iter()
                    .find(|(component_id, _)| *component_id == faction_id)
                    .expect("every Arena row contains Faction")
                    .1;
                (
                    row.spawn_ordinal,
                    i32::from_le_bytes(bytes.as_slice().try_into().expect("Faction is four bytes")),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(factions, [(0, 1), (1, 2), (2, 3), (3, 4), (4, 5)]);
    }

    fn startup_world(source: &str) -> (ArcheWorld, NativeWorldStoragePlan) {
        let tokens = lexer::lex(source).expect("fixture lexes");
        let program = parser::parse_program(&tokens).expect("fixture parses");
        checker::check_program(&program).expect("fixture checks");
        let core = core_lower::lower_program_to_core(&program).expect("fixture Core lowers");
        core_verify::verify_core_program(&core).expect("fixture Core verifies");
        let assembly =
            runtime_assembly::assemble_runtime_program_from_verified_core(&program, &core)
                .expect("fixture runtime assembly builds");
        let plan = derive_native_world_storage_plan(&core, &assembly, NATIVE_STORAGE_BASE_OFFSET)
            .expect("fixture native plan derives");
        let mut world = ArcheWorld::create();
        register_assembly_descriptors_into_world(&assembly, &mut world)
            .expect("fixture descriptors register");
        for operation in &assembly.startup_operations {
            match operation {
                StartupOperation::ResourcePayload { .. } => {
                    execute_startup_resource_payload_operation(operation, &mut world)
                        .expect("resource startup executes");
                }
                StartupOperation::Spawn { .. } => {
                    execute_startup_spawn_operation(operation, &mut world)
                        .expect("spawn startup executes");
                }
                StartupOperation::RunSchedule { .. } => {}
            }
        }
        (world, plan)
    }

    fn parse_observation(bytes: &[u8]) -> ParsedObservation {
        let text = std::str::from_utf8(bytes).expect("observation is UTF-8 ASCII");
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("ARCHEOBS1"));
        let mut tables: Vec<ParsedTable> = Vec::new();
        for line in lines {
            let mut fields = line.split_ascii_whitespace();
            match fields.next().expect("observation line has a record kind") {
                "T" => {
                    let key_count = parse_usize(fields.next());
                    let key = (0..key_count)
                        .map(|_| parse_hex_u64(fields.next()))
                        .collect::<Vec<_>>();
                    let live_row_count = parse_usize(fields.next());
                    let capacity = parse_usize(fields.next());
                    assert!(fields.next().is_none());
                    tables.push(ParsedTable {
                        key,
                        live_row_count,
                        capacity,
                        rows: Vec::new(),
                    });
                }
                "R" => {
                    let row_index = parse_usize(fields.next());
                    let spawn_ordinal = fields
                        .next()
                        .expect("row has spawn ordinal")
                        .parse::<u32>()
                        .expect("spawn ordinal is decimal u32");
                    let component_count = parse_usize(fields.next());
                    let components = (0..component_count)
                        .map(|_| {
                            let component_id = parse_hex_u64(fields.next());
                            let byte_len = parse_usize(fields.next());
                            let payload =
                                parse_upper_hex(fields.next().expect("component has a payload"));
                            assert_eq!(payload.len(), byte_len);
                            (component_id, payload)
                        })
                        .collect::<Vec<_>>();
                    assert!(fields.next().is_none());
                    tables
                        .last_mut()
                        .expect("row follows a table record")
                        .rows
                        .push(ParsedRow {
                            row_index,
                            spawn_ordinal,
                            components,
                        });
                }
                kind => panic!("unknown observation record `{kind}`"),
            }
        }
        ParsedObservation { tables }
    }

    fn parse_usize(field: Option<&str>) -> usize {
        field
            .expect("observation has decimal field")
            .parse::<usize>()
            .expect("observation decimal field is valid")
    }

    fn parse_hex_u64(field: Option<&str>) -> u64 {
        let field = field.expect("observation has component ID");
        assert_eq!(field.len(), 16);
        assert!(field
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte)));
        u64::from_str_radix(field, 16).expect("component ID is hexadecimal")
    }

    fn parse_upper_hex(field: &str) -> Vec<u8> {
        assert_eq!(field.len() % 2, 0);
        assert!(field
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte)));
        field
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let pair = std::str::from_utf8(pair).expect("hex pair is ASCII");
                u8::from_str_radix(pair, 16).expect("payload byte is hexadecimal")
            })
            .collect()
    }
}
