//! Implementation of `arche package`.

use arche_foundation::status::ProcessStatus;
use arche_package::archive::encode_archepkg;
use arche_package::PortablePath;
use std::io::{self, Write};
use std::path::Path;

pub fn run_package(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if let Some(arg) = args.first() {
        match arg.as_str() {
            "--help" | "-h" => {
                write_package_help(output)?;
                return Ok(ProcessStatus::Success);
            }
            arg => {
                writeln!(error, "arche: unrecognized option `{arg}` for `package`")?;
                return Ok(ProcessStatus::Usage);
            }
        }
    }

    let manifest_req = arche_package::ManifestRequest::discover_from(current_dir);
    let workspace = match arche_package::load_workspace(&manifest_req) {
        Ok(ws) => ws,
        Err(err) => {
            crate::project::write_error(error, &err.into())?;
            return Ok(ProcessStatus::Failure);
        }
    };

    let member = workspace.members.first().expect("workspace member exists");
    let manifest_toml = std::fs::read_to_string(member.directory.join("Arche.toml"))?;
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

    let mut files: Vec<(PortablePath, Vec<u8>)> = Vec::new();
    let src_dir = member.directory.join("src");
    if src_dir.is_dir() {
        for entry in std::fs::read_dir(src_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "arc") {
                let file_name = entry.file_name().to_string_lossy().to_string();
                let port_path = PortablePath::new(&format!("src/{file_name}")).unwrap();
                let data = std::fs::read(&path)?;
                files.push((port_path, data));
            }
        }
    }

    let file_slices: Vec<(PortablePath, &[u8])> = files
        .iter()
        .map(|(p, d)| (p.clone(), d.as_slice()))
        .collect();
    let archive_bytes = match encode_archepkg(&manifest_toml, &file_slices) {
        Ok(bytes) => bytes,
        Err(e) => {
            writeln!(error, "arche: failed to encode package archive: {e}")?;
            return Ok(ProcessStatus::Failure);
        }
    };

    let pkg_dir = workspace.root.join("target").join("package");
    std::fs::create_dir_all(&pkg_dir)?;
    let out_file = pkg_dir.join(format!("{pkg_name}-{version}.archepkg"));
    std::fs::write(&out_file, archive_bytes)?;

    writeln!(
        output,
        "arche: packaged target/package/{pkg_name}-{version}.archepkg"
    )?;
    Ok(ProcessStatus::Success)
}

pub fn write_package_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(
        output,
        "Assemble the current package into a redistributable .archepkg archive"
    )?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche package")?;
    Ok(())
}
