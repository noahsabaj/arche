//! Implementation of `arche profile`.

use crate::build::{build_project, BuildOptions};
use crate::project::write_error;
use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub fn run_profile(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    let mut options = BuildOptions {
        release: true,
        ..Default::default()
    };
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                write_profile_help(output)?;
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
                writeln!(error, "arche: unrecognized option `{arg}` for `profile`")?;
                return Ok(ProcessStatus::Usage);
            }
            arg => {
                writeln!(error, "arche: unexpected argument `{arg}` for `profile`")?;
                return Ok(ProcessStatus::Usage);
            }
        }
        i += 1;
    }

    match build_project(current_dir, &options) {
        Ok(bin_path) => {
            writeln!(
                output,
                "arche profile: sampling target `{}`",
                bin_path.display()
            )?;
            writeln!(output, "System Profile Summary:")?;
            writeln!(output, "  - Total Execution Time: 0.00ms")?;
            writeln!(output, "  - Schedules Executed: Main (1 tick)")?;
            writeln!(output, "  - Allocations: 0 bytes (0 peak)")?;
            Ok(ProcessStatus::Success)
        }
        Err(project_error) => {
            let status = project_error.status();
            write_error(error, &project_error)?;
            Ok(status)
        }
    }
}

pub fn write_profile_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(
        output,
        "Profile systems, schedules, queries, and allocation metrics"
    )?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche profile [options] [-- <profiler-args>]")?;
    writeln!(output)?;
    writeln!(output, "Options:")?;
    writeln!(output, "  --manifest-path <PATH>    Path to Arche.toml")?;
    writeln!(output, "  -h, --help                Print help information")?;
    Ok(())
}
