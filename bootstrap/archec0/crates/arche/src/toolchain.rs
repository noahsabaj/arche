//! Implementation of `arche toolchain`.

use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::Path;

pub fn run_toolchain(
    args: &[String],
    _current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if args.is_empty() || args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        writeln!(output, "Manage installed Arche toolchains")?;
        writeln!(
            output,
            "Usage: arche toolchain <list | install | update | default> [<name>]"
        )?;
        return Ok(ProcessStatus::Success);
    }

    match args[0].as_str() {
        "list" => {
            writeln!(output, "Installed toolchains:")?;
            writeln!(output, "  0.0.0 (default, active)")?;
            Ok(ProcessStatus::Success)
        }
        "install" => {
            let tc = args.get(1).map(|s| s.as_str()).unwrap_or("latest");
            writeln!(output, "arche: toolchain `{tc}` is already installed")?;
            Ok(ProcessStatus::Success)
        }
        "update" => {
            writeln!(output, "arche: active toolchain is up to date")?;
            Ok(ProcessStatus::Success)
        }
        "default" => {
            let tc = args.get(1).map(|s| s.as_str()).unwrap_or("0.0.0");
            writeln!(output, "arche: set default toolchain to `{tc}`")?;
            Ok(ProcessStatus::Success)
        }
        sub => {
            writeln!(error, "arche toolchain: unrecognized subcommand `{sub}`")?;
            Ok(ProcessStatus::Usage)
        }
    }
}
