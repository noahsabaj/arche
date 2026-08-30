use std::io::{self, Write};

use crate::core::{CoreComponentKind, CoreScheduleItem};
use crate::core_verify::VerifiedExecutableCore;
use crate::execution_package_build::canonical_core_ids;

/// Formats the generalized AOT input for an already-verified executable Core program.
///
/// Keeping the verifier brand in this interface is intentional: Machine IR is a
/// diagnostic view of the AOT lowering input, not an independently admitted
/// representation of source semantics.
pub fn write_machine(output: &mut impl Write, core: &VerifiedExecutableCore) -> io::Result<()> {
    let program = core.program();
    let ids = canonical_core_ids(core).map_err(io::Error::other)?;
    write!(
        output,
        "machine x86_64-linux-static-pie\nauthority verified-executable-core\nworld {}",
        program.world.name
    )?;

    for schema in &program.components {
        let kind = match schema.kind {
            CoreComponentKind::Component => "component",
            CoreComponentKind::Tag => "tag",
        };
        write!(
            output,
            "\nlink schema core-id @{} schema-id {} {kind} {}",
            schema.id,
            ids.schema(schema.id).ok_or_else(|| io::Error::other(
                "verified Core schema has no canonical identifier"
            ))?,
            schema.name
        )?;
    }
    for resource in &program.resources {
        write!(
            output,
            "\nlink schema core-id @{} schema-id {} resource {}",
            resource.id,
            ids.schema(resource.id).ok_or_else(|| io::Error::other(
                "verified Core resource has no canonical identifier"
            ))?,
            resource.name
        )?;
    }
    for system in &program.systems {
        let system_id = ids
            .system(system.id)
            .ok_or_else(|| io::Error::other("verified Core system has no canonical identifier"))?;
        write!(
            output,
            "\nlink function core-id @{} system-id {} {}",
            system.id, system_id, system.name
        )?;
        for parameter in &system.params {
            if !matches!(
                parameter.kind,
                crate::core::CoreSystemParamKind::Query { .. }
            ) {
                continue;
            }
            let query_id = ids.query(system.id, &parameter.name).ok_or_else(|| {
                io::Error::other("verified Core query has no canonical identifier")
            })?;
            write!(
                output,
                "\nlink query system-core-id @{} parameter {} query-id {} {}.{}.{}",
                system.id,
                parameter.name,
                query_id,
                program.world.name,
                system.name,
                parameter.name
            )?;
        }
    }
    for schedule in &program.schedules {
        write!(
            output,
            "\nlink schedule core-id @{} schedule-id {} {} {{",
            schedule.id,
            ids.schedule(schedule.id).ok_or_else(|| io::Error::other(
                "verified Core schedule has no canonical identifier"
            ))?,
            schedule.name
        )?;
        for item in &schedule.items {
            let CoreScheduleItem::Run {
                system_id,
                system_name,
            } = item;
            write!(
                output,
                "\n  dispatch system-core-id @{system_id} system-id {} {system_name}",
                ids.system(*system_id).ok_or_else(|| io::Error::other(
                    "verified Core dispatch target has no canonical identifier"
                ))?
            )?;
        }
        write!(output, "\n}}")?;
    }

    write!(output, "\n\nlower verified-core {{\n  ")?;
    {
        let mut indented = IndentedWriter::new(output, b"  ");
        crate::core_format::write_core_program(&mut indented, program)?;
    }
    write!(output, "\n}}")
}

struct IndentedWriter<'a, W> {
    output: &'a mut W,
    prefix: &'static [u8],
    at_line_start: bool,
}

impl<'a, W> IndentedWriter<'a, W> {
    fn new(output: &'a mut W, prefix: &'static [u8]) -> Self {
        Self {
            output,
            prefix,
            at_line_start: false,
        }
    }
}

impl<W: Write> Write for IndentedWriter<'_, W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let mut start = 0usize;
        while start < buffer.len() {
            if self.at_line_start && buffer[start] != b'\n' {
                self.output.write_all(self.prefix)?;
                self.at_line_start = false;
            }
            let end = buffer[start..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(buffer.len(), |relative| start + relative + 1);
            self.output.write_all(&buffer[start..end])?;
            self.at_line_start = buffer[end - 1] == b'\n';
            start = end;
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

#[cfg(test)]
pub fn emit_machine(core: &VerifiedExecutableCore) -> String {
    let mut output = Vec::new();
    write_machine(&mut output, core).expect("in-memory Machine formatting succeeds");
    String::from_utf8(output).expect("Machine formatting is UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified(source: &str) -> VerifiedExecutableCore {
        let tokens = crate::lexer::lex(source).expect("fixture lexes");
        let program = crate::parser::parse_program(&tokens).expect("fixture parses");
        crate::checker::check_program(&program).expect("fixture checks as executable");
        let core = crate::core_lower::lower_program_to_core(&program).expect("fixture lowers");
        crate::core_verify::verify_executable_core(core).expect("fixture Core verifies")
    }

    #[test]
    fn emits_startup_cfg_only_from_verified_core() {
        let core = verified(
            "world Main startup {
                let value: i32 = (1 + 2) * 3
                exit value
            }",
        );

        let output = emit_machine(&core);
        assert!(output
            .starts_with("machine x86_64-linux-static-pie\nauthority verified-executable-core"));
        assert!(output.contains("%2 = i32.add %0, %1"));
        assert!(output.contains("%4 = i32.mul %2, %3"));
        assert!(output.contains("local.store value, %4"));
        assert!(output.contains("exit %5"));
        assert!(!output.contains("unsupported"));
    }

    #[test]
    fn emits_generalized_m26_systems_schedules_queries_and_startup_effects() {
        let core = verified(include_str!("../../../examples/m26_closure.arc"));
        let output = emit_machine(&core);
        let expected =
            include_str!("../../../tests/golden/m26_closure.machine").replace("\r\n", "\n");

        assert_eq!(output, expected.trim_end());
        assert!(output.contains(
            "link schema core-id @0 schema-id DD73DA45122DA6C43B963101BF8427BA component M26Closure.Position"
        ));
        assert!(output.contains(
            "link schema core-id @3 schema-id 315BAD85FC991F92EB5E0327087C7CBB tag M26Closure.Enemy"
        ));
        assert!(output.contains(
            "link function core-id @0 system-id B5774A60450757A71EB9432EF8ADA480 Advance"
        ));
        assert!(output.contains(
            "link query system-core-id @0 parameter moving query-id 19C5FAFEBEF407EE5F046E6BCEC0B0CA M26Closure.Advance.moving"
        ));
        assert!(output.contains(
            "link schedule core-id @0 schedule-id D96D851EFBBFE2A9F1C8AB4837B716A5 Warmup {"
        ));
        assert!(output.contains(
            "dispatch system-core-id @1 system-id 76A5E6FCA69842B77F9FEE429E7BEBD9 M26Closure.Normalize"
        ));
        assert_eq!(
            output
                .matches(
                    "dispatch system-core-id @0 system-id B5774A60450757A71EB9432EF8ADA480 M26Closure.Advance"
                )
                .count(),
            3
        );
        assert!(output.contains("while i32.lt pass, config.step"));
        assert!(output.contains("for moving {"));
        assert!(output.contains("add_assign position.x"));
        assert!(output.contains("resource M26Closure.Config core-id"));
        assert!(output.contains("spawn"));
        assert!(output.contains("run M26Closure.Reverse core-id"));
        assert!(!output.contains("unsupported"));
    }
}
