//! Implementation of rche run.

use crate::build::{build_project, BuildOptions};
use crate::project::write_error;
use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run_run(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    let mut options = BuildOptions::default();
    let mut extra_args = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                write_run_help(output)?;
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
                extra_args.extend_from_slice(&args[i + 1..]);
                break;
            }
            arg if arg.starts_with('-') => {
                writeln!(error, "arche: unrecognized option `{arg}` for `run`")?;
                return Ok(ProcessStatus::Usage);
            }
            arg => {
                writeln!(error, "arche: unexpected argument `{arg}` for `run`")?;
                return Ok(ProcessStatus::Usage);
            }
        }
        i += 1;
    }

    let binary_path = match build_project(current_dir, &options) {
        Ok(path) => path,
        Err(project_error) => {
            let status = project_error.status();
            write_error(error, &project_error)?;
            return Ok(status);
        }
    };

    // Execute target
    execute_target(&binary_path, &extra_args, output, error)
}

fn execute_target(
    binary_path: &Path,
    args: &[String],
    _output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    #[cfg(unix)]
    {
        let status = Command::new(binary_path).args(args).status();

        match status {
            Ok(exit) => {
                if exit.success() {
                    Ok(ProcessStatus::Success)
                } else if exit.code() == Some(70) {
                    writeln!(error, "arche: process terminated with integer trap")?;
                    Ok(ProcessStatus::Failure)
                } else {
                    Ok(ProcessStatus::Failure)
                }
            }
            Err(e) => {
                writeln!(
                    error,
                    "arche: failed to execute binary {}: {e}",
                    binary_path.display()
                )?;
                Ok(ProcessStatus::Failure)
            }
        }
    }

    #[cfg(windows)]
    {
        // On Windows, bridge execution via WSL
        let wsl_path = windows_to_wsl_path(binary_path);
        let mut cmd = Command::new("wsl.exe");
        cmd.arg(&wsl_path);
        for arg in args {
            cmd.arg(arg);
        }

        match cmd.status() {
            Ok(exit) => {
                if exit.success() {
                    Ok(ProcessStatus::Success)
                } else if exit.code() == Some(70) {
                    writeln!(error, "arche: process terminated with integer trap")?;
                    Ok(ProcessStatus::Failure)
                } else {
                    Ok(ProcessStatus::Failure)
                }
            }
            Err(e) => {
                writeln!(
                    error,
                    "arche: running ELF binaries on Windows requires WSL (Windows Subsystem for Linux); failed to spawn wsl.exe: {e}"
                )?;
                Ok(ProcessStatus::Failure)
            }
        }
    }
}

#[cfg(windows)]
fn windows_to_wsl_path(path: &Path) -> String {
    let canonical = path.to_string_lossy().to_string();
    if canonical.len() >= 2 && canonical.as_bytes()[1] == b':' {
        let drive = canonical.chars().next().unwrap().to_ascii_lowercase();
        let rest = &canonical[2..].replace('\\', "/");
        format!("/mnt/{drive}{rest}")
    } else {
        canonical.replace('\\', "/")
    }
}

pub fn write_run_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(output, "Build and execute the default binary target")?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche run [options] [-- <args>...]")?;
    writeln!(output)?;
    writeln!(output, "Options:")?;
    writeln!(
        output,
        "  --release                 Build and run in release mode"
    )?;
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
