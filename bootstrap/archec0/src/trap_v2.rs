use crate::observation_v2;
use crate::runtime_v2::RuntimeWorldV2;
use crate::scalar_v2::TrapKind;
use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};

pub const TRAP_EXIT_STATUS: i32 = ProcessStatus::TrapOrPanic.code();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrapSite<'a> {
    pub basename: &'a str,
    pub line: u64,
    pub column: u64,
    pub start_byte: u64,
    pub end_byte: u64,
}

/// Emits the canonical committed-state snapshot before the trap diagnostic.
///
/// If observation writing or flushing fails, this returns immediately and no
/// trap diagnostic is written. The caller must then use the ordinary runtime
/// failure status rather than claiming a complete semantic trap observation.
pub fn emit_trap<WOut: Write, WErr: Write>(
    world: &RuntimeWorldV2,
    stdout: &mut WOut,
    stderr: &mut WErr,
    kind: TrapKind,
    site: TrapSite<'_>,
) -> io::Result<()> {
    observation_v2::write_observation(world, stdout)?;
    write_trap_diagnostic(stderr, kind, site)?;
    stderr.flush()
}

pub fn write_trap_diagnostic<W: Write>(
    stderr: &mut W,
    kind: TrapKind,
    site: TrapSite<'_>,
) -> io::Result<()> {
    writeln!(
        stderr,
        "arche: trap[{}] {}:{}:{} bytes {}..{}",
        kind.diagnostic_name(),
        site.basename,
        site.line,
        site.column,
        site.start_byte,
        site.end_byte
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trap_diagnostic_has_the_exact_source_span_grammar() {
        let mut stderr = Vec::new();
        write_trap_diagnostic(
            &mut stderr,
            TrapKind::I32DivideByZero,
            TrapSite {
                basename: "trap.arc",
                line: 12,
                column: 9,
                start_byte: 144,
                end_byte: 149,
            },
        )
        .expect("diagnostic writes");

        assert_eq!(
            stderr,
            b"arche: trap[I32_DIVIDE_BY_ZERO] trap.arc:12:9 bytes 144..149\n"
        );
        assert_eq!(TRAP_EXIT_STATUS, 70);
    }

    #[test]
    fn diagnostic_io_failure_is_reported() {
        struct Closed;

        impl Write for Closed {
            fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let error = write_trap_diagnostic(
            &mut Closed,
            TrapKind::I32RemainderOverflow,
            TrapSite {
                basename: "overflow.arc",
                line: 1,
                column: 1,
                start_byte: 0,
                end_byte: 1,
            },
        )
        .expect_err("closed writer fails");

        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
}
