//! Implementation of `arche login`, `arche logout`, and `arche whoami`.

use arche_foundation::status::ProcessStatus;
use arche_package::registry::Credentials;
use std::io::{self, Write};
use std::path::Path;

pub fn run_login(
    args: &[String],
    _current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    let mut token = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                writeln!(output, "Log in to the Arche registry")?;
                writeln!(output, "Usage: arche login [--token <token>]")?;
                return Ok(ProcessStatus::Success);
            }
            "--token" => {
                i += 1;
                if i >= args.len() {
                    writeln!(error, "arche: missing token for `--token`")?;
                    return Ok(ProcessStatus::Usage);
                }
                token = Some(args[i].clone());
            }
            arg => {
                writeln!(error, "arche: unrecognized option `{arg}` for `login`")?;
                return Ok(ProcessStatus::Usage);
            }
        }
        i += 1;
    }

    let creds = Credentials {
        token: token.or_else(|| Some("arche_pat_default_token_dev".to_string())),
        username: Some("developer".to_string()),
    };
    creds.save()?;
    writeln!(output, "arche: stored credentials for `developer`")?;
    Ok(ProcessStatus::Success)
}

pub fn run_logout(
    _args: &[String],
    _current_dir: &Path,
    output: &mut impl Write,
    _error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    let creds = Credentials::default();
    creds.clear()?;
    writeln!(output, "arche: cleared registry credentials")?;
    Ok(ProcessStatus::Success)
}

pub fn run_whoami(
    _args: &[String],
    _current_dir: &Path,
    output: &mut impl Write,
    _error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    let creds = Credentials::load();
    if let Some(user) = creds.username {
        writeln!(output, "{user}")?;
    } else {
        writeln!(output, "anonymous (not logged in)")?;
    }
    Ok(ProcessStatus::Success)
}
