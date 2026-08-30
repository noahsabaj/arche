//! Implementation of `arche doc`.

use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct DocOptions {
    pub manifest_path: Option<PathBuf>,
    pub open: bool,
}

pub fn run_doc(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    let mut options = DocOptions::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                write_doc_help(output)?;
                return Ok(ProcessStatus::Success);
            }
            "--open" => options.open = true,
            "--manifest-path" => {
                i += 1;
                if i >= args.len() {
                    writeln!(error, "arche: missing path for `--manifest-path`")?;
                    return Ok(ProcessStatus::Usage);
                }
                options.manifest_path = Some(PathBuf::from(&args[i]));
            }
            arg if arg.starts_with('-') => {
                writeln!(error, "arche: unrecognized option `{arg}` for `doc`")?;
                return Ok(ProcessStatus::Usage);
            }
            arg => {
                writeln!(error, "arche: unexpected argument `{arg}` for `doc`")?;
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

    let doc_root = workspace.root.join("target").join("doc");
    std::fs::create_dir_all(&doc_root)?;

    for member in &workspace.members {
        let pkg_name = member
            .manifest
            .package
            .as_ref()
            .map(|p| p.name.leaf())
            .unwrap_or("app");
        let pkg_doc_dir = doc_root.join(pkg_name);
        std::fs::create_dir_all(&pkg_doc_dir)?;

        let html_content = format!(
            "<!DOCTYPE html>\n<html><head><title>{}</title></head><body><h1>Package {}</h1><p>Arche documentation</p></body></html>\n",
            pkg_name, pkg_name
        );
        let index_path = pkg_doc_dir.join("index.html");
        std::fs::write(&index_path, html_content)?;
        writeln!(
            output,
            "arche: generated documentation in target/doc/{pkg_name}/index.html"
        )?;
    }

    Ok(ProcessStatus::Success)
}

pub fn write_doc_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(output, "Build documentation for an Arche package")?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche doc [options]")?;
    writeln!(output)?;
    writeln!(output, "Options:")?;
    writeln!(
        output,
        "  --open                    Open documentation in default web browser"
    )?;
    writeln!(output, "  --manifest-path <PATH>    Path to Arche.toml")?;
    writeln!(output, "  -h, --help                Print help information")?;
    Ok(())
}
