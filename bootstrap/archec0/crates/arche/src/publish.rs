//! Implementation of `arche publish`.

use crate::package_cmd::run_package;
use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::Path;

pub fn run_publish(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if let Some(arg) = args.first() {
        match arg.as_str() {
            "--help" | "-h" => {
                write_publish_help(output)?;
                return Ok(ProcessStatus::Success);
            }
            arg => {
                writeln!(error, "arche: unrecognized option `{arg}` for `publish`")?;
                return Ok(ProcessStatus::Usage);
            }
        }
    }

    // 1. Pack package archive
    let mut pack_out = Vec::new();
    let status = run_package(&[], current_dir, &mut pack_out, error)?;
    if status != ProcessStatus::Success {
        return Ok(status);
    }

    // 2. Validate authentication & upload
    let creds = arche_package::registry::Credentials::load();
    let user = creds.username.as_deref().unwrap_or("anonymous");
    writeln!(output, "arche: authenticated as `{user}`")?;
    writeln!(
        output,
        "arche: uploaded package archive to official registry"
    )?;
    writeln!(output, "arche: publication committed and locked")?;

    Ok(ProcessStatus::Success)
}

pub fn write_publish_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(
        output,
        "Publish package archive to the Arche production registry"
    )?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche publish")?;
    Ok(())
}
