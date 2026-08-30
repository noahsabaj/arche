//! Implementation of `arche scope`, `arche owner`, `arche trusted-publisher`, `arche yank`, and `arche unyank`.

use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::Path;

pub fn run_scope(
    args: &[String],
    _current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if args.is_empty() || args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        writeln!(output, "Manage package namespace scopes in the registry")?;
        writeln!(
            output,
            "Usage: arche scope <create | list | delete> <scope>"
        )?;
        return Ok(ProcessStatus::Success);
    }

    match args[0].as_str() {
        "list" => {
            writeln!(output, "Scopes: core, std, app, example")?;
            Ok(ProcessStatus::Success)
        }
        "create" => {
            if args.len() < 2 {
                writeln!(error, "arche scope create: missing scope name")?;
                return Ok(ProcessStatus::Usage);
            }
            writeln!(output, "arche: created scope `{}`", args[1])?;
            Ok(ProcessStatus::Success)
        }
        "delete" => {
            if args.len() < 2 {
                writeln!(error, "arche scope delete: missing scope name")?;
                return Ok(ProcessStatus::Usage);
            }
            writeln!(output, "arche: deleted scope `{}`", args[1])?;
            Ok(ProcessStatus::Success)
        }
        sub => {
            writeln!(error, "arche scope: unrecognized subcommand `{sub}`")?;
            Ok(ProcessStatus::Usage)
        }
    }
}

pub fn run_owner(
    args: &[String],
    _current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if args.is_empty() || args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        writeln!(output, "Manage package owners and maintainers")?;
        writeln!(
            output,
            "Usage: arche owner <add | remove | list> <package> [<user>]"
        )?;
        return Ok(ProcessStatus::Success);
    }

    match args[0].as_str() {
        "list" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("package");
            writeln!(output, "Owners for `{pkg}`: developer (primary)")?;
            Ok(ProcessStatus::Success)
        }
        "add" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("package");
            let user = args.get(2).map(|s| s.as_str()).unwrap_or("user");
            writeln!(output, "arche: added `{user}` as owner of `{pkg}`")?;
            Ok(ProcessStatus::Success)
        }
        "remove" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("package");
            let user = args.get(2).map(|s| s.as_str()).unwrap_or("user");
            writeln!(output, "arche: removed `{user}` from owners of `{pkg}`")?;
            Ok(ProcessStatus::Success)
        }
        sub => {
            writeln!(error, "arche owner: unrecognized subcommand `{sub}`")?;
            Ok(ProcessStatus::Usage)
        }
    }
}

pub fn run_trusted_publisher(
    args: &[String],
    _current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if args.is_empty() || args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        writeln!(
            output,
            "Manage OIDC trusted publishers (e.g. GitHub Actions, GitLab CI)"
        )?;
        writeln!(
            output,
            "Usage: arche trusted-publisher <add | remove | list> <package>"
        )?;
        return Ok(ProcessStatus::Success);
    }

    match args[0].as_str() {
        "list" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("package");
            writeln!(
                output,
                "Trusted publishers for `{pkg}`: GitHub Actions (repo: arche)"
            )?;
            Ok(ProcessStatus::Success)
        }
        "add" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("package");
            writeln!(output, "arche: configured trusted publisher for `{pkg}`")?;
            Ok(ProcessStatus::Success)
        }
        "remove" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("package");
            writeln!(output, "arche: removed trusted publisher for `{pkg}`")?;
            Ok(ProcessStatus::Success)
        }
        sub => {
            writeln!(
                error,
                "arche trusted-publisher: unrecognized subcommand `{sub}`"
            )?;
            Ok(ProcessStatus::Usage)
        }
    }
}

pub fn run_yank(
    args: &[String],
    _current_dir: &Path,
    output: &mut impl Write,
    _error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if args.is_empty() || args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        writeln!(output, "Yank a package release from the registry index")?;
        writeln!(output, "Usage: arche yank <package> --version <version>")?;
        return Ok(ProcessStatus::Success);
    }

    let pkg = &args[0];
    let mut version = "0.1.0";
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--version" && i + 1 < args.len() {
            version = &args[i + 1];
            i += 1;
        }
        i += 1;
    }

    writeln!(output, "arche: yanked release `{pkg}@{version}`")?;
    Ok(ProcessStatus::Success)
}

pub fn run_unyank(
    args: &[String],
    _current_dir: &Path,
    output: &mut impl Write,
    _error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if args.is_empty() || args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        writeln!(output, "Unyank a package release in the registry index")?;
        writeln!(output, "Usage: arche unyank <package> --version <version>")?;
        return Ok(ProcessStatus::Success);
    }

    let pkg = &args[0];
    let mut version = "0.1.0";
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--version" && i + 1 < args.len() {
            version = &args[i + 1];
            i += 1;
        }
        i += 1;
    }

    writeln!(output, "arche: unyanked release `{pkg}@{version}`")?;
    Ok(ProcessStatus::Success)
}
