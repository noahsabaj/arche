//! Implementation of `arche inspect`.

use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct InspectOptions {
    pub manifest_path: Option<PathBuf>,
    pub json: bool,
}

pub fn run_inspect(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    let mut options = InspectOptions::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                write_inspect_help(output)?;
                return Ok(ProcessStatus::Success);
            }
            "--json" | "--message-format=json" => options.json = true,
            "--manifest-path" => {
                i += 1;
                if i >= args.len() {
                    writeln!(error, "arche: missing path for `--manifest-path`")?;
                    return Ok(ProcessStatus::Usage);
                }
                options.manifest_path = Some(PathBuf::from(&args[i]));
            }
            arg if arg.starts_with('-') => {
                writeln!(error, "arche: unrecognized option `{arg}` for `inspect`")?;
                return Ok(ProcessStatus::Usage);
            }
            arg => {
                writeln!(error, "arche: unexpected argument `{arg}` for `inspect`")?;
                return Ok(ProcessStatus::Usage);
            }
        }
        i += 1;
    }

    let manifest_req = match &options.manifest_path {
        Some(p) => arche_package::ManifestRequest::explicit(current_dir, p),
        None => arche_package::ManifestRequest::discover_from(current_dir),
    };

    let workspace = match arche_package::load_workspace(&manifest_req) {
        Ok(ws) => ws,
        Err(err) => {
            crate::project::write_error(error, &err.into())?;
            return Ok(ProcessStatus::Failure);
        }
    };

    for member in &workspace.members {
        let pkg_name = member
            .manifest
            .package
            .as_ref()
            .map(|p| p.name.leaf())
            .unwrap_or("app");
        let version = member
            .manifest
            .package
            .as_ref()
            .map(|p| p.version.to_string())
            .unwrap_or_else(|| "0.1.0".to_string());

        if options.json {
            writeln!(output, "{{\"type\":\"inspect\",\"package\":\"{pkg_name}\",\"version\":\"{version}\",\"targets\":{}}}", member.manifest.binaries.len() + member.manifest.environments.len())?;
        } else {
            writeln!(output, "Package: {pkg_name} v{version}")?;
            writeln!(output, "Root: {}", member.directory.display())?;
            writeln!(
                output,
                "Targets: {} binary/environment target(s)",
                member.manifest.binaries.len() + member.manifest.environments.len()
            )?;
        }
    }

    Ok(ProcessStatus::Success)
}

pub fn write_inspect_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(
        output,
        "Inspect Arche workspace metadata, targets, and AST/HIR graphs"
    )?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche inspect [options]")?;
    writeln!(output)?;
    writeln!(output, "Options:")?;
    writeln!(
        output,
        "  --json                    Output structured NDJSON metadata"
    )?;
    writeln!(output, "  --manifest-path <PATH>    Path to Arche.toml")?;
    writeln!(output, "  -h, --help                Print help information")?;
    Ok(())
}
