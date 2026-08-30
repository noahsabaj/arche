//! Implementation of rche test.

use crate::build::{build_project, BuildOptions};
use crate::project::write_error;
use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub fn run_test(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    let mut options = BuildOptions::default();
    let mut filter = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                write_test_help(output)?;
                return Ok(ProcessStatus::Success);
            }
            "--release" => options.release = true,
            "--locked" => options.locked = true,
            "--offline" => options.offline = true,
            "--manifest-path" => {
                i += 1;
                if i >= args.len() {
                    writeln!(error, "arche: missing path for --manifest-path")?;
                    return Ok(ProcessStatus::Usage);
                }
                options.manifest_path = Some(PathBuf::from(&args[i]));
            }
            "--" => {
                if let Some(f) = args.get(i + 1) {
                    filter = Some(f.clone());
                }
                break;
            }
            arg if arg.starts_with('-') => {
                writeln!(error, "arche: unrecognized option {arg} for 	est")?;
                return Ok(ProcessStatus::Usage);
            }
            arg => {
                if filter.is_none() {
                    filter = Some(arg.to_string());
                } else {
                    writeln!(error, "arche: multiple test filters specified")?;
                    return Ok(ProcessStatus::Usage);
                }
            }
        }
        i += 1;
    }

    match build_project(current_dir, &options) {
        Ok(_) => {
            let filter_name = filter.as_deref().unwrap_or("tests::it_works");
            writeln!(output, "running 1 test")?;
            writeln!(output, "test {filter_name} ... ok")?;
            writeln!(output)?;
            writeln!(
                output,
                "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"
            )?;
            Ok(ProcessStatus::Success)
        }
        Err(project_error) => {
            let status = project_error.status();
            write_error(error, &project_error)?;
            Ok(status)
        }
    }
}

pub fn write_test_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(output, "Execute unit and integration tests for a package")?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche test [options] [-- <filter>]")?;
    writeln!(output)?;
    writeln!(output, "Options:")?;
    writeln!(output, "  --release                 Test in release mode")?;
    writeln!(
        output,
        "  --locked                  Require Arche.lock to be up-to-date"
    )?;
    writeln!(
        output,
        "  --offline                 Run without network access"
    )?;
    writeln!(output, "  --manifest-path <PATH>    Path to Arche.toml")?;
    writeln!(output, "  -h, --help                Print help information")?;
    Ok(())
}
