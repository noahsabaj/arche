//! Implementation of `arche clean`.

use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::Path;

pub fn run_clean(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if let Some(arg) = args.first() {
        match arg.as_str() {
            "--help" | "-h" => {
                write_clean_help(output)?;
                return Ok(ProcessStatus::Success);
            }
            arg => {
                writeln!(error, "arche: unrecognized option `{arg}` for `clean`")?;
                return Ok(ProcessStatus::Usage);
            }
        }
    }

    let target_dir = current_dir.join("target");
    if target_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&target_dir) {
            writeln!(
                error,
                "arche: failed to clean `{}`: {e}",
                target_dir.display()
            )?;
            return Ok(ProcessStatus::Failure);
        }
        writeln!(output, "arche: cleaned target/")?;
    } else {
        writeln!(output, "arche: nothing to clean")?;
    }

    Ok(ProcessStatus::Success)
}

pub fn write_clean_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(output, "Remove build artifacts from target directory")?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche clean")?;
    Ok(())
}
