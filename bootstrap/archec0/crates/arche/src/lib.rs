//! Public `arche` command-line driver.

pub mod auth;
pub mod build;
pub mod clean;
pub mod debug;
pub mod deps;
pub mod doc;
pub mod fmt;
pub mod governance;
pub mod inspect;
pub mod lsp;
pub mod new;
pub mod package_cmd;
pub mod profile;
mod project;
pub mod publish;
pub mod run;
pub mod test;
pub mod toolchain;

use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const COMMANDS: &[&str] = &[
    "new",
    "check",
    "build",
    "run",
    "test",
    "clean",
    "inspect",
    "fmt",
    "doc",
    "lsp",
    "debug",
    "profile",
    "add",
    "remove",
    "update",
    "search",
    "package",
    "publish",
    "login",
    "logout",
    "whoami",
    "scope",
    "owner",
    "trusted-publisher",
    "yank",
    "unyank",
    "toolchain",
];

pub fn run<I, S, O, E>(args: I, output: &mut O, error: &mut E) -> io::Result<ProcessStatus>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    O: Write,
    E: Write,
{
    let current_dir = std::env::current_dir()?;
    run_from(args, &current_dir, output, error)
}

/// Runs the public driver relative to an explicit working directory.
pub fn run_from<I, S, O, E>(
    args: I,
    current_dir: &Path,
    output: &mut O,
    error: &mut E,
) -> io::Result<ProcessStatus>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    O: Write,
    E: Write,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();
    match args.as_slice() {
        [] => {
            write_help(output)?;
            Ok(ProcessStatus::Success)
        }
        [arg] if arg == "--help" || arg == "-h" => {
            write_help(output)?;
            Ok(ProcessStatus::Success)
        }
        [arg] if arg == "--version" => {
            writeln!(output, "arche {}", env!("CARGO_PKG_VERSION"))?;
            Ok(ProcessStatus::Success)
        }
        [command, rest @ ..] if command == "new" => new::run_new(rest, current_dir, output, error),
        [command, rest @ ..] if command == "clean" => {
            clean::run_clean(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "check" => run_check(rest, current_dir, output, error),
        [command, rest @ ..] if command == "build" => {
            build::run_build(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "run" => run::run_run(rest, current_dir, output, error),
        [command, rest @ ..] if command == "test" => {
            test::run_test(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "fmt" => fmt::run_fmt(rest, current_dir, output, error),
        [command, rest @ ..] if command == "doc" => doc::run_doc(rest, current_dir, output, error),
        [command, rest @ ..] if command == "lsp" => lsp::run_lsp(rest, current_dir, output, error),
        [command, rest @ ..] if command == "inspect" => {
            inspect::run_inspect(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "debug" => {
            debug::run_debug(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "profile" => {
            profile::run_profile(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "add" => deps::run_add(rest, current_dir, output, error),
        [command, rest @ ..] if command == "remove" => {
            deps::run_remove(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "update" => {
            deps::run_update(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "search" => {
            deps::run_search(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "package" => {
            package_cmd::run_package(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "publish" => {
            publish::run_publish(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "login" => {
            auth::run_login(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "logout" => {
            auth::run_logout(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "whoami" => {
            auth::run_whoami(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "scope" => {
            governance::run_scope(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "owner" => {
            governance::run_owner(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "trusted-publisher" => {
            governance::run_trusted_publisher(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "yank" => {
            governance::run_yank(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "unyank" => {
            governance::run_unyank(rest, current_dir, output, error)
        }
        [command, rest @ ..] if command == "toolchain" => {
            toolchain::run_toolchain(rest, current_dir, output, error)
        }
        [command, ..] => {
            writeln!(error, "arche: unknown command `{command}`")?;
            writeln!(error, "run `arche --help` for usage")?;
            Ok(ProcessStatus::Usage)
        }
    }
}

fn run_check(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    let mut manifest_path = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                write_check_help(output)?;
                return Ok(ProcessStatus::Success);
            }
            "--release" | "--locked" | "--offline" => {
                // Accepted standard options
            }
            "--manifest-path" => {
                i += 1;
                if i >= args.len() {
                    writeln!(error, "arche: invalid arguments for `check`")?;
                    writeln!(error, "usage: arche check [--manifest-path <Arche.toml>]")?;
                    return Ok(ProcessStatus::Usage);
                }
                manifest_path = Some(PathBuf::from(&args[i]));
            }
            _ => {
                writeln!(error, "arche: invalid arguments for `check`")?;
                writeln!(error, "usage: arche check [--manifest-path <Arche.toml>]")?;
                return Ok(ProcessStatus::Usage);
            }
        }
        i += 1;
    }
    run_project_check(current_dir, manifest_path.as_deref(), output, error)
}

fn run_project_check(
    current_dir: &Path,
    manifest_path: Option<&Path>,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    match project::check_project(current_dir, manifest_path) {
        Ok(summary) => {
            writeln!(
                output,
                "arche: resolved packages={} targets={} modules={}",
                summary.packages, summary.targets, summary.modules
            )?;
            Ok(ProcessStatus::Success)
        }
        Err(project_error) => {
            let status = project_error.status();
            project::write_error(error, &project_error)?;
            Ok(status)
        }
    }
}

fn write_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(output, "arche - the Arche language toolchain")?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche --help")?;
    writeln!(output, "  arche --version")?;
    writeln!(output, "  arche <command> [options]")?;
    writeln!(output)?;
    writeln!(output, "Available commands:")?;
    for command in COMMANDS {
        writeln!(output, "  {command}")?;
    }
    writeln!(output)?;
    writeln!(output, "M27 commands:")?;
    for command in COMMANDS {
        writeln!(output, "  {command}")?;
    }
    Ok(())
}

fn write_check_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(output, "Check an Arche package or workspace")?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche check [options]")?;
    writeln!(output)?;
    writeln!(output, "Options:")?;
    writeln!(output, "  --release                 Check in release mode")?;
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
