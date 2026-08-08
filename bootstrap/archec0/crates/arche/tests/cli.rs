use arche_foundation::status::ProcessStatus;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_PROJECT_ORDINAL: AtomicU64 = AtomicU64::new(0);

struct TempProject {
    root: PathBuf,
}

impl TempProject {
    fn copy_fixture(name: &str) -> Self {
        let fixture = repository_root().join("tests/m27b").join(name);
        let ordinal = TEMP_PROJECT_ORDINAL.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("arche-m27b-cli-{}-{ordinal}", std::process::id()));
        assert!(!root.exists(), "temporary project path already exists");
        copy_directory(&fixture, &root);
        Self { root }
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let expected_prefix = format!("arche-m27b-cli-{}-", std::process::id());
        let safe_name = self
            .root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(&expected_prefix));
        if safe_name && self.root.starts_with(std::env::temp_dir()) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

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
    assert!(stdout.contains("M27 commands:\n"));
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
        b"arche: `build` is reserved but not implemented yet\n"
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

#[test]
fn public_cli_exposes_the_m27_b_check_shape() {
    let output = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["check", "--help"])
        .output()
        .expect("public Arche CLI starts");

    assert_eq!(output.status.code(), Some(ProcessStatus::Success.code()));
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        concat!(
            "Check an Arche package or workspace\n",
            "\n",
            "Usage:\n",
            "  arche check\n",
            "  arche check --manifest-path <Arche.toml>\n",
        )
        .as_bytes()
    );
}

#[test]
fn public_check_discovers_from_a_nested_directory_and_only_publishes_the_lock() {
    let project = TempProject::copy_fixture("mixed-workspace");
    let nested = project.root.join("nested/working-directory");
    fs::create_dir_all(&nested).expect("nested working directory is created");
    let before = relative_files(&project.root);

    let first = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("check")
        .current_dir(&nested)
        .output()
        .expect("public Arche CLI starts");
    assert_eq!(first.status.code(), Some(ProcessStatus::Success.code()));
    assert!(first.stderr.is_empty());
    assert_eq!(
        first.stdout,
        b"arche: resolved packages=1 targets=3 modules=5\n"
    );

    let after = relative_files(&project.root);
    let created = after.difference(&before).cloned().collect::<Vec<_>>();
    assert_eq!(created, [PathBuf::from("Arche.lock")]);
    let lock_path = project.root.join("Arche.lock");
    let lock = fs::read(&lock_path).expect("canonical lock is readable");
    assert!(!lock.is_empty());
    assert!(!lock.contains(&b'\r'));

    fs::write(&lock_path, b"incomplete lock bytes\n")
        .expect("pre-existing lock can be replaced during the proof");

    let explicit = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args([
            "check",
            "--manifest-path",
            project
                .root
                .join("Arche.toml")
                .to_str()
                .expect("UTF-8 path"),
        ])
        .current_dir(repository_root())
        .output()
        .expect("public Arche CLI starts");
    assert_eq!(explicit.status.code(), Some(ProcessStatus::Success.code()));
    assert!(explicit.stderr.is_empty());
    assert_eq!(explicit.stdout, first.stdout);
    assert_eq!(
        fs::read(&lock_path).expect("canonical lock remains readable"),
        lock
    );
    assert_eq!(relative_files(&project.root), after);
    assert_no_lock_temporaries(&project.root);
}

#[test]
fn public_check_resolves_a_multi_member_workspace_path_dependency() {
    let project = TempProject::copy_fixture("path-workspace");
    let nested = project.root.join("packages/shared/src");
    let before = relative_files(&project.root);
    let output = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("check")
        .current_dir(&nested)
        .output()
        .expect("public Arche CLI starts");

    assert_eq!(output.status.code(), Some(ProcessStatus::Success.code()));
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        b"arche: resolved packages=2 targets=2 modules=2\n"
    );

    let after = relative_files(&project.root);
    let created = after.difference(&before).cloned().collect::<Vec<_>>();
    assert_eq!(created, [PathBuf::from("Arche.lock")]);
    let lock_path = project.root.join("Arche.lock");
    let lock = fs::read_to_string(&lock_path).expect("multi-member lock is canonical UTF-8");
    assert!(lock.contains("[workspace]\nsource-digest = \"sha256:"));
    assert!(lock.contains("name = \"example/app\"\n"));
    assert!(lock.contains("name = \"example/shared\"\n"));
    assert!(lock.contains("alias = \"shared\", package = \"example/shared\""));

    let manifest_path = project.root.join("Arche.toml");
    let manifest = fs::read_to_string(&manifest_path).expect("workspace manifest is readable");
    let changed_manifest = manifest.replace(
        "default-members = [\"packages/app\", \"packages/shared\"]",
        "default-members = [\"packages/app\"]",
    );
    assert_ne!(changed_manifest, manifest);
    fs::write(&manifest_path, changed_manifest).expect("workspace defaults are changed");
    let changed = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("check")
        .current_dir(&nested)
        .output()
        .expect("public Arche CLI starts");
    assert_eq!(changed.status.code(), Some(ProcessStatus::Success.code()));
    assert!(changed.stderr.is_empty());
    assert_eq!(changed.stdout, output.stdout);
    assert_ne!(
        fs::read_to_string(lock_path).expect("updated lock is canonical UTF-8"),
        lock,
        "virtual workspace authority changes must replace the lock"
    );
    assert_eq!(relative_files(&project.root), after);
    assert_no_lock_temporaries(&project.root);
}

#[test]
fn public_check_hard_cuts_m26_startup_without_publishing_a_lock() {
    let project = TempProject::copy_fixture("legacy-startup");
    let lock_path = project.root.join("Arche.lock");
    let original_lock = b"previous complete lock\n";
    fs::write(&lock_path, original_lock).expect("existing lock is seeded");
    let before = relative_files(&project.root);
    let output = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("check")
        .current_dir(&project.root)
        .output()
        .expect("public Arche CLI starts");

    assert_eq!(output.status.code(), Some(ProcessStatus::Failure.code()));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("diagnostic is UTF-8");
    assert!(diagnostic.contains("error[MIGRATE001]"));
    assert!(diagnostic.contains("M26 `world Name`"));
    assert_eq!(relative_files(&project.root), before);
    assert_eq!(
        fs::read(lock_path).expect("failed check preserves the existing lock"),
        original_lock
    );
    assert_no_lock_temporaries(&project.root);
}

#[test]
fn public_check_fails_closed_when_registry_source_acquisition_is_unavailable() {
    let project = TempProject::copy_fixture("registry-unavailable");
    let before = relative_files(&project.root);
    let output = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("check")
        .current_dir(&project.root)
        .output()
        .expect("public Arche CLI starts");

    assert_eq!(output.status.code(), Some(ProcessStatus::Failure.code()));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("diagnostic is UTF-8");
    assert!(diagnostic.contains("error[DEPENDENCY001]"));
    assert!(diagnostic.contains("example/remote"));
    assert_eq!(relative_files(&project.root), before);
}

#[test]
fn public_check_rejects_an_incompatible_toolchain_without_mutating_the_lock() {
    let project = TempProject::copy_fixture("toolchain-mismatch");
    let lock_path = project.root.join("Arche.lock");
    let original_lock = b"previous complete lock\n";
    fs::write(&lock_path, original_lock).expect("existing lock is seeded");
    let before = relative_files(&project.root);
    let output = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("check")
        .current_dir(&project.root)
        .output()
        .expect("public Arche CLI starts");

    assert_eq!(output.status.code(), Some(ProcessStatus::Usage.code()));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("diagnostic is UTF-8");
    assert!(diagnostic.contains("error[MANIFEST004]"));
    assert!(diagnostic.contains(
        "package `example/future` requires Arche `>=1.0.0`, but selected toolchain is `0.0.0`"
    ));
    assert_eq!(relative_files(&project.root), before);
    assert_eq!(
        fs::read(lock_path).expect("toolchain rejection preserves the existing lock"),
        original_lock
    );
    assert_no_lock_temporaries(&project.root);
}

#[test]
fn public_check_classifies_a_malformed_manifest_as_usage() {
    let project = TempProject::copy_fixture("malformed-manifest");
    let before = relative_files(&project.root);
    let output = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("check")
        .current_dir(&project.root)
        .output()
        .expect("public Arche CLI starts");

    assert_eq!(output.status.code(), Some(ProcessStatus::Usage.code()));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).expect("diagnostic is UTF-8");
    assert!(diagnostic.contains("error[MANIFEST002]"));
    assert!(diagnostic.contains("unsupported Arche.toml schema 2; expected schema 1"));
    assert_eq!(relative_files(&project.root), before);
}

#[test]
fn malformed_check_arguments_are_process_usage_errors() {
    let output = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["check", "--manifest-path"])
        .output()
        .expect("public Arche CLI starts");

    assert_eq!(output.status.code(), Some(ProcessStatus::Usage.code()));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        concat!(
            "arche: invalid arguments for `check`\n",
            "usage: arche check [--manifest-path <Arche.toml>]\n",
        )
        .as_bytes()
    );
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("arche crate is nested below the repository root")
        .to_owned()
}

fn copy_directory(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("fixture destination is created");
    let mut entries = fs::read_dir(source)
        .expect("fixture directory is readable")
        .collect::<Result<Vec<_>, _>>()
        .expect("fixture entries are readable");
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry
            .file_type()
            .expect("fixture entry type is readable")
            .is_dir()
        {
            copy_directory(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path).expect("fixture file is copied");
        }
    }
}

fn relative_files(root: &Path) -> BTreeSet<PathBuf> {
    fn visit(root: &Path, directory: &Path, files: &mut BTreeSet<PathBuf>) {
        let mut entries = fs::read_dir(directory)
            .expect("project directory is readable")
            .collect::<Result<Vec<_>, _>>()
            .expect("project entries are readable");
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            if entry
                .file_type()
                .expect("project entry type is readable")
                .is_dir()
            {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root)
                        .expect("project file is below its root")
                        .to_owned(),
                );
            }
        }
    }

    let mut files = BTreeSet::new();
    visit(root, root, &mut files);
    files
}

fn assert_no_lock_temporaries(root: &Path) {
    assert!(relative_files(root).iter().all(|path| !path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains(".Arche.lock.arche-tmp-"))));
}
