//! Public `arche` command-line driver shell established by M27-A.

use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};

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
        [command, ..] if COMMANDS.contains(&command.as_str()) => {
            writeln!(
                error,
                "arche: `{command}` is reserved but not implemented in M27-A"
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

fn write_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(output, "arche - the Arche language toolchain")?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche --help")?;
    writeln!(output, "  arche --version")?;
    writeln!(output, "  arche <command> [options]")?;
    writeln!(output)?;
    writeln!(output, "Reserved M27 commands:")?;
    for command in COMMANDS {
        writeln!(output, "  {command}")?;
    }
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
            "arche: `build` is reserved but not implemented in M27-A\n"
        );
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
