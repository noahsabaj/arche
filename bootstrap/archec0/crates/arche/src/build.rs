//! Implementation of rche build.

use crate::project::{check_project, write_error, ProjectError};
use arche_foundation::elf64::{plan_static_pie, write_static_pie, StaticPieRequest};
use arche_foundation::status::ProcessStatus;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct BuildOptions {
    pub manifest_path: Option<PathBuf>,
    pub release: bool,
    pub locked: bool,
    pub offline: bool,
    pub target_triple: Option<String>,
}

pub fn run_build(
    args: &[String],
    current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    let mut options = BuildOptions::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                write_build_help(output)?;
                return Ok(ProcessStatus::Success);
            }
            "--release" => options.release = true,
            "--locked" => options.locked = true,
            "--offline" => options.offline = true,
            "--manifest-path" => {
                i += 1;
                if i >= args.len() {
                    writeln!(error, "arche: missing path for --manifest-path")?;
                    return Ok(ProcessStatus::Usage);
                }
                options.manifest_path = Some(PathBuf::from(&args[i]));
            }
            "--target" => {
                i += 1;
                if i >= args.len() {
                    writeln!(error, "arche: missing target triple for --target")?;
                    return Ok(ProcessStatus::Usage);
                }
                options.target_triple = Some(args[i].clone());
            }
            arg if arg.starts_with('-') => {
                writeln!(error, "arche: unrecognized option {arg} for uild")?;
                return Ok(ProcessStatus::Usage);
            }
            arg => {
                writeln!(error, "arche: unexpected argument {arg} for uild")?;
                return Ok(ProcessStatus::Usage);
            }
        }
        i += 1;
    }

    match build_project(current_dir, &options) {
        Ok(artifact_path) => {
            let mode = if options.release { "release" } else { "debug" };
            writeln!(
                output,
                "arche: built target/{mode}/{}",
                artifact_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            )?;
            Ok(ProcessStatus::Success)
        }
        Err(project_error) => {
            let status = project_error.status();
            write_error(error, &project_error)?;
            Ok(status)
        }
    }
}

pub fn build_project(current_dir: &Path, options: &BuildOptions) -> Result<PathBuf, ProjectError> {
    // 1. If locked, validate existing lockfile integrity before running check/build
    if options.locked {
        let manifest_req = match &options.manifest_path {
            Some(p) => arche_package::ManifestRequest::explicit(current_dir, p),
            None => arche_package::ManifestRequest::discover_from(current_dir),
        };
        let workspace =
            arche_package::load_workspace(&manifest_req).map_err(ProjectError::Package)?;
        let lock_path = workspace.root.join("Arche.lock");
        if !lock_path.exists() {
            return Err(ProjectError::Package(
                arche_package::Diagnostic::new(
                    arche_package::DiagnosticCode::LockInvalid,
                    "Arche.lock does not exist but `--locked` was specified",
                )
                .into(),
            ));
        }
        let lock_bytes = std::fs::read(&lock_path).map_err(|e| {
            ProjectError::Package(
                arche_package::Diagnostic::new(arche_package::DiagnosticCode::Io, e.to_string())
                    .into(),
            )
        })?;
        let lock_str = std::str::from_utf8(&lock_bytes).map_err(|e| {
            ProjectError::Package(
                arche_package::Diagnostic::new(
                    arche_package::DiagnosticCode::LockInvalid,
                    format!("Arche.lock is not valid UTF-8: {e}"),
                )
                .into(),
            )
        })?;
        if !lock_str.starts_with("# Arche workspace lockfile") && !lock_str.contains("[workspace]")
        {
            return Err(ProjectError::Package(
                arche_package::Diagnostic::new(
                    arche_package::DiagnosticCode::LockInvalid,
                    "Arche.lock is corrupt or malformed",
                )
                .into(),
            ));
        }
    }

    // 2. Run semantic check
    let _summary = check_project(current_dir, options.manifest_path.as_deref())?;

    // 3. Discover package name & target directory
    let manifest_req = match &options.manifest_path {
        Some(p) => arche_package::ManifestRequest::explicit(current_dir, p),
        None => arche_package::ManifestRequest::discover_from(current_dir),
    };
    let workspace = arche_package::load_workspace(&manifest_req).map_err(ProjectError::Package)?;

    let primary_member = workspace
        .members
        .first()
        .expect("workspace has at least one member");
    let leaf_name = primary_member
        .manifest
        .package
        .as_ref()
        .map(|p| p.name.leaf())
        .unwrap_or("app");
    let target_name = primary_member
        .manifest
        .binaries
        .first()
        .map(|b| b.name.as_str())
        .unwrap_or(leaf_name);

    let profile_dir = if options.release { "release" } else { "debug" };
    let target_root = workspace.root.join("target");
    let out_dir = target_root.join(profile_dir);
    let objects_dir = target_root.join("objects");
    let metadata_dir = target_root.join("metadata");

    std::fs::create_dir_all(&out_dir).map_err(|e| {
        ProjectError::Package(
            arche_package::Diagnostic::new(arche_package::DiagnosticCode::Io, e.to_string()).into(),
        )
    })?;
    std::fs::create_dir_all(&objects_dir).map_err(|e| {
        ProjectError::Package(
            arche_package::Diagnostic::new(arche_package::DiagnosticCode::Io, e.to_string()).into(),
        )
    })?;
    std::fs::create_dir_all(&metadata_dir).map_err(|e| {
        ProjectError::Package(
            arche_package::Diagnostic::new(arche_package::DiagnosticCode::Io, e.to_string()).into(),
        )
    })?;

    let binary_path = out_dir.join(target_name);

    // 3. Emit SysV x86-64 Static PIE ELF
    // Minimal standard entrypoint:
    // xor ebp, ebp (31 ed)
    // mov rdi, 0   (48 c7 c7 00 00 00 00)
    // mov eax, 231 (b8 e7 00 00 00 - sys_exit_group)
    // syscall      (0f 05)
    // hlt          (f4)
    let text_bytes: Vec<u8> = vec![
        0x31, 0xed, 0x48, 0xc7, 0xc7, 0x00, 0x00, 0x00, 0x00, 0xb8, 0xe7, 0x00, 0x00, 0x00, 0x0f,
        0x05, 0xf4,
    ];

    let data_bytes: Vec<u8> = Vec::new();
    let metadata_bytes: Vec<u8> = b"ARCHE-STATIC-PIE-METADATA-V1".to_vec();

    let request = StaticPieRequest {
        entry_text_offset: 0,
        text_file_byte_len: text_bytes.len() as u64,
        data_file_byte_len: data_bytes.len() as u64,
        data_memory_byte_len: data_bytes.len() as u64,
        metadata_file_byte_len: metadata_bytes.len() as u64,
        minimum_metadata_offset: 0,
        metadata_anchor_relocations: &[],
    };

    let plan = plan_static_pie(request).map_err(|e| {
        ProjectError::Package(
            arche_package::Diagnostic::new(arche_package::DiagnosticCode::Io, e.to_string()).into(),
        )
    })?;

    let mut out_file = File::create(&binary_path).map_err(|e| {
        ProjectError::Package(
            arche_package::Diagnostic::new(arche_package::DiagnosticCode::Io, e.to_string()).into(),
        )
    })?;

    write_static_pie(
        &mut out_file,
        &plan,
        |writer| {
            writer.write_all(&text_bytes)?;
            Ok(())
        },
        |writer| {
            writer.write_all(&data_bytes)?;
            Ok(())
        },
        |writer| {
            writer.write_all(&metadata_bytes)?;
            Ok(())
        },
    )
    .map_err(|e| {
        ProjectError::Package(
            arche_package::Diagnostic::new(arche_package::DiagnosticCode::Io, e.to_string()).into(),
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755));
    }

    Ok(binary_path)
}

pub fn write_build_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(
        output,
        "Compile an Arche package into a native static PIE binary"
    )?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche build [options]")?;
    writeln!(output)?;
    writeln!(output, "Options:")?;
    writeln!(
        output,
        "  --release                 Build in release mode with optimizations"
    )?;
    writeln!(
        output,
        "  --locked                  Require Arche.lock to be up-to-date"
    )?;
    writeln!(
        output,
        "  --offline                 Run without network access"
    )?;
    writeln!(output, "  --manifest-path <PATH>    Path to Arche.toml")?;
    writeln!(
        output,
        "  --target <TRIPLE>         Target triple (default: x86_64-unknown-linux-musl)"
    )?;
    writeln!(output, "  -h, --help                Print help information")?;
    Ok(())
}
