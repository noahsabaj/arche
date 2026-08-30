use crate::ids_v2::SchemaId;
use crate::runtime_v2::{ResourceState, RuntimeWorldV2};
use std::io::{self, Write};

pub const MAGIC: &str = "ARCHEOBS2";

/// Streams the complete semantic world state in canonical ARCHEOBS2 order.
///
/// `RuntimeWorldV2` stores resources, tables, table keys, and row columns in
/// canonical schema-ID order. Rows retain committed spawn order. Keeping that
/// ordering invariant in the world lets this formatter write directly to the
/// caller without constructing a second observation buffer.
pub fn write_observation<W: Write>(world: &RuntimeWorldV2, output: &mut W) -> io::Result<()> {
    writeln!(output, "{MAGIC}")?;

    let mut prior_resource = None;
    for resource in world.resources() {
        let id = resource.schema().id();
        debug_assert!(prior_resource.is_none_or(|prior| prior < id));
        prior_resource = Some(id);
        match resource.state() {
            ResourceState::Uninitialized => {
                writeln!(output, "RESOURCE {id} UNINITIALIZED")?;
            }
            ResourceState::Initialized(bytes) => {
                write!(
                    output,
                    "RESOURCE {id} INITIALIZED {} ",
                    wire_len(bytes.len(), "resource payload")?
                )?;
                write_payload(output, bytes)?;
                writeln!(output)?;
            }
        }
    }

    let mut prior_table_key: Option<&[SchemaId]> = None;
    for table in world.tables() {
        debug_assert!(prior_table_key.is_none_or(|prior| prior < table.key()));
        prior_table_key = Some(table.key());

        write!(
            output,
            "TABLE {}",
            wire_len(table.key().len(), "table key")?
        )?;
        for id in table.key() {
            write!(output, " {id}")?;
        }
        writeln!(output, " {}", wire_len(table.rows().len(), "table rows")?)?;

        for (row_index, row) in table.rows().iter().enumerate() {
            writeln!(
                output,
                "ROW {} {} {}",
                wire_len(row_index, "row index")?,
                row.spawn_ordinal(),
                wire_len(row.columns().len(), "row columns")?
            )?;
            for column in row.columns() {
                write!(
                    output,
                    "COLUMN {} {} ",
                    column.schema_id(),
                    wire_len(column.bytes().len(), "column payload")?
                )?;
                write_payload(output, column.bytes())?;
                writeln!(output)?;
            }
        }
    }

    writeln!(output, "END")?;
    output.flush()
}

fn wire_len(value: usize, context: &'static str) -> io::Result<u64> {
    u64::try_from(value).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{context} length does not fit the ARCHEOBS2 u64 format"),
        )
    })
}

fn write_payload<W: Write>(output: &mut W, bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() {
        return output.write_all(b"-");
    }
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in bytes {
        output.write_all(&[HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0x0f)]])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_package_v2::{
        ExecutionPackage, FieldRecord, FunctionLinkRecord, FunctionTarget, PayloadRecord,
        PayloadRef, SchemaFlags, SchemaRecord, SchemaRef, StartupOperationKind,
        StartupOperationRecord, StringRef, WorldRecord,
    };
    use crate::ids_v2::{AbiHasher, BodyHasher, PrimitiveType, SchemaField, SchemaKind};
    use crate::runtime_v2::{StartupExecutionError, SystemDispatcher, SystemInvocation};
    use std::convert::Infallible;

    struct NoSystems;

    impl SystemDispatcher for NoSystems {
        type Error = Infallible;

        fn dispatch(
            &mut self,
            _world: &mut RuntimeWorldV2,
            _invocation: SystemInvocation,
        ) -> Result<(), Self::Error> {
            unreachable!("the observation fixture has no scheduled systems")
        }
    }

    fn observation_world() -> RuntimeWorldV2 {
        let strings = [
            "Counter",
            "Demo",
            "EmptyResource",
            "Enemy",
            "Flag",
            "_arche_Demo_startup",
            "n",
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();

        let counter_id = crate::ids_v2::SchemaId::derive(
            SchemaKind::Resource,
            "Demo",
            "Counter",
            &[SchemaField {
                name: "n",
                primitive: PrimitiveType::I32,
            }],
        );
        let empty_resource_id =
            crate::ids_v2::SchemaId::derive(SchemaKind::Resource, "Demo", "EmptyResource", &[]);
        let enemy_id = crate::ids_v2::SchemaId::derive(SchemaKind::Tag, "Demo", "Enemy", &[]);
        let flag_id = crate::ids_v2::SchemaId::derive(
            SchemaKind::Component,
            "Demo",
            "Flag",
            &[SchemaField {
                name: "n",
                primitive: PrimitiveType::Bool,
            }],
        );

        let mut schema_specs = [
            (counter_id, SchemaKind::Resource, 0_u64, 4_u64, 4_u64),
            (empty_resource_id, SchemaKind::Resource, 2_u64, 0_u64, 1_u64),
            (enemy_id, SchemaKind::Tag, 3_u64, 0_u64, 1_u64),
            (flag_id, SchemaKind::Component, 4_u64, 1_u64, 1_u64),
        ];
        schema_specs.sort_unstable_by_key(|(id, ..)| *id);
        let schemas = schema_specs
            .iter()
            .map(|(id, kind, name, byte_size, alignment)| SchemaRecord {
                id: *id,
                kind: *kind,
                flags: SchemaFlags::for_kind(*kind),
                name: StringRef::new(*name),
                byte_size: *byte_size,
                alignment: *alignment,
                source_span: None,
            })
            .collect::<Vec<_>>();
        let schema_ref = |id| {
            SchemaRef::new(
                u64::try_from(
                    schemas
                        .binary_search_by_key(&id, |schema| schema.id)
                        .expect("fixture schema exists"),
                )
                .expect("fixture schema index fits u64"),
            )
        };
        let counter = schema_ref(counter_id);
        let enemy = schema_ref(enemy_id);
        let flag = schema_ref(flag_id);

        let mut fields = vec![
            FieldRecord {
                schema: counter,
                name: StringRef::new(6),
                primitive: PrimitiveType::I32,
                byte_offset: 0,
                source_span: None,
            },
            FieldRecord {
                schema: flag,
                name: StringRef::new(6),
                primitive: PrimitiveType::Bool,
                byte_offset: 0,
                source_span: None,
            },
        ];
        fields.sort_unstable_by_key(|field| field.schema.index());

        let mut spawn_payloads = vec![
            PayloadRecord {
                schema: enemy,
                bytes: Vec::new(),
            },
            PayloadRecord {
                schema: flag,
                bytes: vec![1],
            },
        ];
        spawn_payloads.sort_unstable_by_key(|payload| payload.schema.index());
        let mut payloads = vec![PayloadRecord {
            schema: counter,
            bytes: 7_i32.to_le_bytes().to_vec(),
        }];
        payloads.extend(spawn_payloads);

        let mut startup_abi = AbiHasher::new();
        startup_abi.append_string("startup");
        let startup_abi_hash = startup_abi.finalize();
        let mut startup_body = BodyHasher::new();
        startup_body.append_string("startup").append_u64(0);
        let startup_body_hash = startup_body.finalize();

        let package = ExecutionPackage {
            strings,
            world: WorldRecord {
                name: StringRef::new(1),
                source_span: None,
                startup_abi_hash,
                startup_body_hash,
            },
            schemas,
            fields,
            systems: Vec::new(),
            parameters: Vec::new(),
            queries: Vec::new(),
            terms: Vec::new(),
            schedules: Vec::new(),
            schedule_items: Vec::new(),
            startup_operations: vec![
                StartupOperationRecord {
                    kind: StartupOperationKind::ResourcePayload {
                        resource: counter,
                        payload: PayloadRef::new(0),
                    },
                    source_span: None,
                },
                StartupOperationRecord {
                    kind: StartupOperationKind::Spawn {
                        first_payload: PayloadRef::new(1),
                        payload_count: 2,
                    },
                    source_span: None,
                },
                StartupOperationRecord {
                    kind: StartupOperationKind::Spawn {
                        first_payload: PayloadRef::new(3),
                        payload_count: 0,
                    },
                    source_span: None,
                },
            ],
            payloads,
            function_links: vec![FunctionLinkRecord {
                target: FunctionTarget::Startup,
                symbol_name: StringRef::new(5),
                abi_hash: startup_abi_hash,
                body_hash: startup_body_hash,
                code_offset: 0,
                code_byte_len: 1,
                source_span: None,
                first_body_span: None,
                body_span_count: 0,
            }],
            source_spans: Vec::new(),
        };

        let mut world = RuntimeWorldV2::from_package(package).expect("fixture package is valid");
        let result: Result<(), StartupExecutionError<Infallible>> =
            world.execute_startup(&mut NoSystems);
        result.expect("fixture startup executes");
        world
    }

    #[test]
    fn streams_resources_zsts_and_empty_archetype_rows_canonically() {
        let world = observation_world();
        let mut output = Vec::new();
        write_observation(&world, &mut output).expect("observation writes");
        let actual = String::from_utf8(output).expect("observation is ASCII");

        let mut expected = String::from("ARCHEOBS2\n");
        for resource in world.resources() {
            match resource.state() {
                ResourceState::Uninitialized => expected.push_str(&format!(
                    "RESOURCE {} UNINITIALIZED\n",
                    resource.schema().id()
                )),
                ResourceState::Initialized(bytes) => expected.push_str(&format!(
                    "RESOURCE {} INITIALIZED {} {}\n",
                    resource.schema().id(),
                    bytes.len(),
                    payload_text(bytes)
                )),
            }
        }
        for table in world.tables() {
            expected.push_str(&format!("TABLE {}", table.key().len()));
            for id in table.key() {
                expected.push_str(&format!(" {id}"));
            }
            expected.push_str(&format!(" {}\n", table.rows().len()));
            for (row_index, row) in table.rows().iter().enumerate() {
                expected.push_str(&format!(
                    "ROW {row_index} {} {}\n",
                    row.spawn_ordinal(),
                    row.columns().len()
                ));
                for column in row.columns() {
                    expected.push_str(&format!(
                        "COLUMN {} {} {}\n",
                        column.schema_id(),
                        column.bytes().len(),
                        payload_text(column.bytes())
                    ));
                }
            }
        }
        expected.push_str("END\n");

        assert_eq!(actual, expected);
        assert!(world.tables()[0].key().is_empty());
        assert_eq!(world.tables()[0].rows()[0].spawn_ordinal(), 1);
        assert!(actual.contains("TABLE 0 1\nROW 0 1 0\n"));
        assert!(actual.contains(" 0 -\n"));
        assert!(!actual.contains("capacity"));
    }

    fn payload_text(bytes: &[u8]) -> String {
        if bytes.is_empty() {
            return "-".to_string();
        }
        let mut text = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write as _;
            write!(&mut text, "{byte:02X}").expect("writing to a String cannot fail");
        }
        text
    }

    #[test]
    fn propagates_observation_io_failures_without_claiming_end() {
        struct FailsAfter {
            remaining: usize,
            bytes: Vec<u8>,
        }

        impl Write for FailsAfter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.remaining == 0 {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
                }
                let count = bytes.len().min(self.remaining);
                self.bytes.extend_from_slice(&bytes[..count]);
                self.remaining -= count;
                Ok(count)
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let world = observation_world();
        let mut output = FailsAfter {
            remaining: 20,
            bytes: Vec::new(),
        };
        let error = write_observation(&world, &mut output).expect_err("writer fails");

        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert!(!output.bytes.ends_with(b"END\n"));
    }
}
