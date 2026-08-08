use arche_foundation::status::ProcessStatus;
use std::process::Command;

#[test]
fn public_cli_help_runs_as_a_real_process() {
    let output = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("--help")
        .output()
        .expect("public Arche CLI starts");

    assert_eq!(output.status.code(), Some(ProcessStatus::Success.code()));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("help is UTF-8");
    assert!(stdout.starts_with("arche - the Arche language toolchain\n"));
    assert!(stdout.contains("  build\n"));
    assert!(stdout.contains("  toolchain\n"));
}

#[test]
fn public_cli_distinguishes_reserved_and_unknown_commands() {
    let reserved = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("build")
        .output()
        .expect("public Arche CLI starts");
    assert_eq!(reserved.status.code(), Some(ProcessStatus::Usage.code()));
    assert_eq!(
        reserved.stderr,
        b"arche: `build` is reserved but not implemented in M27-A\n"
    );

    let unknown = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("wat")
        .output()
        .expect("public Arche CLI starts");
    assert_eq!(unknown.status.code(), Some(ProcessStatus::Usage.code()));
    assert!(unknown
        .stderr
        .starts_with(b"arche: unknown command `wat`\n"));
}
