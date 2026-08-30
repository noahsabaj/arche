//! Implementation of `arche new`.

use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageTemplate {
    Bin,
    Lib,
    App,
}

pub fn run_new(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if args.is_empty() {
        writeln!(error, "arche: missing package name for `new`")?;
        writeln!(error, "usage: arche new <NAME> [--bin | --lib | --app]")?;
        return Ok(ProcessStatus::Usage);
    }

    let mut name = None;
    let mut template = PackageTemplate::Bin;

    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => {
                write_new_help(output)?;
                return Ok(ProcessStatus::Success);
            }
            "--bin" => template = PackageTemplate::Bin,
            "--lib" => template = PackageTemplate::Lib,
            "--app" => template = PackageTemplate::App,
            arg if arg.starts_with('-') => {
                writeln!(error, "arche: unrecognized option `{arg}` for `new`")?;
                return Ok(ProcessStatus::Usage);
            }
            arg => {
                if name.is_some() {
                    writeln!(error, "arche: multiple package names specified for `new`")?;
                    return Ok(ProcessStatus::Usage);
                }
                name = Some(arg.to_string());
            }
        }
    }

    let Some(pkg_name) = name else {
        writeln!(error, "arche: missing package name for `new`")?;
        return Ok(ProcessStatus::Usage);
    };

    let leaf_name = pkg_name.split('/').next_back().unwrap_or(&pkg_name);
    let full_package_name = if pkg_name.contains('/') {
        pkg_name.clone()
    } else {
        format!("app/{pkg_name}")
    };

    let target_dir = current_dir.join(leaf_name);
    if target_dir.exists() {
        writeln!(error, "arche: destination `{leaf_name}` already exists")?;
        return Ok(ProcessStatus::Failure);
    }

    let src_dir = target_dir.join("src");
    if let Err(e) = std::fs::create_dir_all(&src_dir) {
        writeln!(
            error,
            "arche: failed to create directory `{}`: {e}",
            target_dir.display()
        )?;
        return Ok(ProcessStatus::Failure);
    }

    let (manifest_content, file_name, file_content) = match template {
        PackageTemplate::Bin => (
            format!(
                "schema = 1\n\n[package]\nname = \"{full_package_name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\narche = \">=0.0.0\"\n\n[[bin]]\nname = \"{leaf_name}\"\npath = \"src/main.arc\"\nworld = \"package::MainWorld\"\n"
            ),
            "main.arc",
            "world MainWorld {\n    init {}\n}\n\npub fn main() {}\n",
        ),
        PackageTemplate::Lib => (
            format!(
                "schema = 1\n\n[package]\nname = \"{full_package_name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\narche = \">=0.0.0\"\n\n[lib]\npath = \"src/lib.arc\"\n"
            ),
            "lib.arc",
            "pub fn answer() -> i32 { 42 }\n",
        ),
        PackageTemplate::App => (
            format!(
                "schema = 1\n\n[package]\nname = \"{full_package_name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\narche = \">=0.0.0\"\n\n[[environment]]\nname = \"{leaf_name}\"\npath = \"src/app.arc\"\nworld = \"package::GameWorld\"\nprofile = \"default\"\n\n[environment-profile.default]\nreset = \"package::Reset\"\nstep = \"package::Step\"\nself-play = \"package::SelfPlay\"\n"
            ),
            "app.arc",
            "world GameWorld {\n    init {}\n}\n\npub fn Reset() {}\npub fn Step() {}\npub fn SelfPlay() {}\n",
        ),
    };

    let manifest_path = target_dir.join("Arche.toml");
    if let Err(e) = std::fs::write(&manifest_path, manifest_content) {
        writeln!(
            error,
            "arche: failed to write `{}`: {e}",
            manifest_path.display()
        )?;
        return Ok(ProcessStatus::Failure);
    }

    let src_file_path = src_dir.join(file_name);
    if let Err(e) = std::fs::write(&src_file_path, file_content) {
        writeln!(
            error,
            "arche: failed to write `{}`: {e}",
            src_file_path.display()
        )?;
        return Ok(ProcessStatus::Failure);
    }

    let kind_str = match template {
        PackageTemplate::Bin => "binary",
        PackageTemplate::Lib => "library",
        PackageTemplate::App => "app",
    };
    writeln!(output, "arche: created `{leaf_name}` ({kind_str})")?;
    Ok(ProcessStatus::Success)
}

pub fn write_new_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(output, "Create a new Arche package")?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche new <NAME> [options]")?;
    writeln!(output)?;
    writeln!(output, "Options:")?;
    writeln!(output, "  --bin      Create a binary application (default)")?;
    writeln!(output, "  --lib      Create a library package")?;
    writeln!(
        output,
        "  --app      Create an ECS game/simulation application"
    )?;
    writeln!(output, "  -h, --help Print help information")?;
    Ok(())
}
