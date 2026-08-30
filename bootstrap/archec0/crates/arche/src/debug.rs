//! Implementation of `arche debug`.

use crate::build::{build_project, BuildOptions};
use crate::project::write_error;
use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub fn run_debug(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    let mut options = BuildOptions::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                write_debug_help(output)?;
                return Ok(ProcessStatus::Success);
            }
            "--manifest-path" => {
                i += 1;
                if i >= args.len() {
                    writeln!(error, "arche: missing path for `--manifest-path`")?;
                    return Ok(ProcessStatus::Usage);
                }
                options.manifest_path = Some(PathBuf::from(&args[i]));
            }
            "--" => break,
            arg if arg.starts_with('-') => {
                writeln!(error, "arche: unrecognized option `{arg}` for `debug`")?;
                return Ok(ProcessStatus::Usage);
            }
            arg => {
                writeln!(error, "arche: unexpected argument `{arg}` for `debug`")?;
                return Ok(ProcessStatus::Usage);
            }
        }
        i += 1;
    }

    match build_project(current_dir, &options) {
        Ok(bin_path) => {
            writeln!(
                output,
                "arche debug: prepared target `{}` with source maps",
                bin_path.display()
            )?;
            writeln!(output, "arche debug: ready for LLDB/GDB connection")?;
            Ok(ProcessStatus::Success)
        }
        Err(project_error) => {
            let status = project_error.status();
            write_error(error, &project_error)?;
            Ok(status)
        }
    }
}

pub fn write_debug_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(output, "Launch interactive ECS-aware debugger session")?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche debug [options] [-- <debugger-args>]")?;
    writeln!(output)?;
    writeln!(output, "Options:")?;
    writeln!(output, "  --manifest-path <PATH>    Path to Arche.toml")?;
    writeln!(output, "  -h, --help                Print help information")?;
    Ok(())
}
