//! Implementation of `arche fmt`.

use arche_foundation::status::ProcessStatus;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct FmtOptions {
    pub check: bool,
    pub manifest_path: Option<PathBuf>,
}

pub fn run_fmt(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    let mut options = FmtOptions::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                write_fmt_help(output)?;
                return Ok(ProcessStatus::Success);
            }
            "--check" => options.check = true,
            "--manifest-path" => {
                i += 1;
                if i >= args.len() {
                    writeln!(error, "arche: missing path for `--manifest-path`")?;
                    return Ok(ProcessStatus::Usage);
                }
                options.manifest_path = Some(PathBuf::from(&args[i]));
            }
            arg if arg.starts_with('-') => {
                writeln!(error, "arche: unrecognized option `{arg}` for `fmt`")?;
                return Ok(ProcessStatus::Usage);
            }
            arg => {
                writeln!(error, "arche: unexpected argument `{arg}` for `fmt`")?;
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

    let mut arc_files = Vec::new();
    collect_arc_files(&workspace.root, &mut arc_files)?;

    let mut unformatted_count = 0;
    for file in &arc_files {
        let content = std::fs::read_to_string(file)?;
        let formatted = format_arche_source(&content);

        if content != formatted {
            unformatted_count += 1;
            if options.check {
                writeln!(output, "Diff in {}", file.display())?;
            } else {
                std::fs::write(file, &formatted)?;
                writeln!(output, "Formatted {}", file.display())?;
            }
        }
    }

    if options.check && unformatted_count > 0 {
        writeln!(
            error,
            "arche fmt: {unformatted_count} file(s) require formatting"
        )?;
        Ok(ProcessStatus::Failure)
    } else {
        Ok(ProcessStatus::Success)
    }
}

fn collect_arc_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if name != "target" && name != ".git" {
                collect_arc_files(&path, files)?;
            }
        } else if path.extension().is_some_and(|ext| ext == "arc") {
            files.push(path);
        }
    }
    Ok(())
}

pub fn format_arche_source(source: &str) -> String {
    let mut result = String::new();
    let mut indent_level = 0usize;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            result.push('\n');
            continue;
        }

        // Adjust indent for closing braces
        let closing_count = trimmed
            .chars()
            .take_while(|&c| c == '}' || c == ')' || c == ']')
            .count();
        if closing_count > 0 && indent_level >= closing_count {
            indent_level -= closing_count;
        }

        for _ in 0..indent_level {
            result.push_str("    ");
        }
        result.push_str(trimmed);
        result.push('\n');

        // Count new open braces minus already adjusted closing braces
        let opens = trimmed.chars().filter(|&c| c == '{').count();
        let closes = trimmed.chars().filter(|&c| c == '}').count();
        if opens > closes {
            indent_level += opens - closes;
        }
    }

    // Ensure single trailing newline
    while result.ends_with("\n\n") {
        result.pop();
    }
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

pub fn write_fmt_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(
        output,
        "Format Arche source files in a package or workspace"
    )?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche fmt [options]")?;
    writeln!(output)?;
    writeln!(output, "Options:")?;
    writeln!(
        output,
        "  --check                   Check formatting without writing changes"
    )?;
    writeln!(output, "  --manifest-path <PATH>    Path to Arche.toml")?;
    writeln!(output, "  -h, --help                Print help information")?;
    Ok(())
}
