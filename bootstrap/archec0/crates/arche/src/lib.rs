//! Public `arche` command-line driver.

mod project;

use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::Path;

pub const COMMANDS: &[&str] = &[
    "new",
    "check",
    "build",
    "run",
    "test",
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
///
/// Keeping the working directory injectable makes project discovery testable
/// without changing process-global state.
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
        [command, rest @ ..] if command == "check" => run_check(rest, current_dir, output, error),
        [command, ..] if COMMANDS.contains(&command.as_str()) => {
            writeln!(
                error,
                "arche: `{command}` is reserved but not implemented yet"
            )?;
            Ok(ProcessStatus::Usage)
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
    match args {
        [arg] if arg == "--help" || arg == "-h" => {
            write_check_help(output)?;
            Ok(ProcessStatus::Success)
        }
        [] => run_project_check(current_dir, None, output, error),
        [flag, manifest_path] if flag == "--manifest-path" && !manifest_path.is_empty() => {
            run_project_check(current_dir, Some(Path::new(manifest_path)), output, error)
        }
        _ => {
            writeln!(error, "arche: invalid arguments for `check`")?;
            writeln!(error, "usage: arche check [--manifest-path <Arche.toml>]")?;
            Ok(ProcessStatus::Usage)
        }
    }
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
    writeln!(output, "  arche check")?;
    writeln!(output, "  arche check --manifest-path <Arche.toml>")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_exposes_the_reserved_public_command_surface() {
        let mut output = Vec::new();
        let mut error = Vec::new();
        let status = run(["--help"], &mut output, &mut error).expect("help writes");
        let output = String::from_utf8(output).expect("help is UTF-8");

        assert_eq!(status, ProcessStatus::Success);
        assert!(error.is_empty());
        for command in COMMANDS {
            assert!(output.lines().any(|line| line.trim() == *command));
        }
    }

    #[test]
    fn reserved_commands_fail_without_claiming_implementation() {
        let mut output = Vec::new();
        let mut error = Vec::new();
        let status = run(["build"], &mut output, &mut error).expect("diagnostic writes");

        assert_eq!(status, ProcessStatus::Usage);
        assert!(output.is_empty());
        assert_eq!(
            String::from_utf8(error).expect("diagnostic is UTF-8"),
            "arche: `build` is reserved but not implemented yet\n"
        );
    }

    #[test]
    fn check_help_exposes_only_the_m27_b_surface() {
        let mut output = Vec::new();
        let mut error = Vec::new();
        let status = run_from(
            ["check", "--help"],
            Path::new("unused"),
            &mut output,
            &mut error,
        )
        .expect("help writes");

        assert_eq!(status, ProcessStatus::Success);
        assert!(error.is_empty());
        assert_eq!(
            String::from_utf8(output).expect("help is UTF-8"),
            concat!(
                "Check an Arche package or workspace\n",
                "\n",
                "Usage:\n",
                "  arche check\n",
                "  arche check --manifest-path <Arche.toml>\n",
            )
        );
    }

    #[test]
    fn invalid_check_arguments_are_usage_errors() {
        for args in [
            vec!["check", "--manifest-path"],
            vec!["check", "--manifest-path", ""],
            vec!["check", "--manifest-path", "Arche.toml", "extra"],
            vec!["check", "--offline"],
        ] {
            let mut output = Vec::new();
            let mut error = Vec::new();
            let status = run_from(args, Path::new("unused"), &mut output, &mut error)
                .expect("diagnostic writes");

            assert_eq!(status, ProcessStatus::Usage);
            assert!(output.is_empty());
            assert_eq!(
                String::from_utf8(error).expect("diagnostic is UTF-8"),
                concat!(
                    "arche: invalid arguments for `check`\n",
                    "usage: arche check [--manifest-path <Arche.toml>]\n",
                )
            );
        }
    }

    #[test]
    fn unknown_commands_are_usage_errors() {
        let mut output = Vec::new();
        let mut error = Vec::new();
        let status = run(["wat"], &mut output, &mut error).expect("diagnostic writes");

        assert_eq!(status, ProcessStatus::Usage);
        assert!(output.is_empty());
        assert!(String::from_utf8(error)
            .expect("diagnostic is UTF-8")
            .starts_with("arche: unknown command `wat`\n"));
    }
}
