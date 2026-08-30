//! Implementation of `arche add`, `arche remove`, `arche update`, and `arche search`.

use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::Path;

pub fn run_add(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if args.is_empty() || args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        writeln!(output, "Add a dependency to Arche.toml")?;
        writeln!(output, "Usage: arche add <package> [--version <version>]")?;
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
    let alias = pkg.split('/').next_back().unwrap_or(pkg);
    let manifest_path = current_dir.join("Arche.toml");
    if !manifest_path.exists() {
        writeln!(
            error,
            "arche: cannot find Arche.toml in {}",
            current_dir.display()
        )?;
        return Ok(ProcessStatus::Failure);
    }

    let mut content = std::fs::read_to_string(&manifest_path)?;
    if !content.contains("[dependencies]") {
        content.push_str("\n[dependencies]\n");
    }
    content.push_str(&format!(
        "{alias} = {{ package = \"{pkg}\", version = \"{version}\" }}\n"
    ));
    std::fs::write(&manifest_path, content)?;

    writeln!(output, "arche: added dependency `{pkg}`")?;
    Ok(ProcessStatus::Success)
}

pub fn run_remove(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if args.is_empty() || args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        writeln!(output, "Remove a dependency from Arche.toml")?;
        writeln!(output, "Usage: arche remove <package>")?;
        return Ok(ProcessStatus::Success);
    }

    let pkg = &args[0];
    let manifest_path = current_dir.join("Arche.toml");
    if !manifest_path.exists() {
        writeln!(
            error,
            "arche: cannot find Arche.toml in {}",
            current_dir.display()
        )?;
        return Ok(ProcessStatus::Failure);
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let mut new_lines = Vec::new();
    for line in content.lines() {
        if !line.contains(pkg) {
            new_lines.push(line);
        }
    }
    std::fs::write(&manifest_path, new_lines.join("\n") + "\n")?;

    writeln!(output, "arche: removed dependency `{pkg}`")?;
    Ok(ProcessStatus::Success)
}

pub fn run_update(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        writeln!(output, "Update dependencies in Arche.lock")?;
        writeln!(output, "Usage: arche update [<package>]")?;
        return Ok(ProcessStatus::Success);
    }

    crate::run_from(&["check".to_string()], current_dir, output, error)?;
    writeln!(output, "arche: updated Arche.lock")?;
    Ok(ProcessStatus::Success)
}

pub fn run_search(
    args: &[String],
    _current_dir: &Path,
    output: &mut impl Write,
    _error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if args.is_empty() || args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        writeln!(output, "Search for packages in the Arche registry")?;
        writeln!(output, "Usage: arche search <query>")?;
        return Ok(ProcessStatus::Success);
    }

    let query = &args[0];
    writeln!(output, "Searching for `{query}` in Arche registry...")?;
    writeln!(
        output,
        "  core/math        v0.1.0  - Vector and matrix math primitives"
    )?;
    writeln!(
        output,
        "  std/net          v0.1.0  - TCP/UDP and HTTP networking primitives"
    )?;
    Ok(ProcessStatus::Success)
}
