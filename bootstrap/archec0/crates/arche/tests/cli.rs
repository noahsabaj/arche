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

    fn new_temp(prefix: &str) -> Self {
        let ordinal = TEMP_PROJECT_ORDINAL.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("arche-{prefix}-{}-{ordinal}", std::process::id()));
        assert!(!root.exists(), "temporary project path already exists");
        fs::create_dir_all(&root).expect("temp dir created");
        Self { root }
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let safe_name = self
            .root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("arche-"));
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
    assert!(stdout.contains("Available commands:\n"));
    assert!(stdout.contains("  build\n"));
    assert!(stdout.contains("  toolchain\n"));
}

#[test]
fn public_cli_rejects_unknown_commands() {
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
    let stdout = String::from_utf8(output.stdout).expect("help is UTF-8");
    assert!(stdout.starts_with("Check an Arche package or workspace\n"));
    assert!(stdout.contains("Usage:\n  arche check"));
}

#[test]
fn public_cli_new_clean_build_run_test_workflow() {
    let temp = TempProject::new_temp("workflow");

    // 1. arche new my_pkg
    let new_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["new", "my_pkg", "--bin"])
        .current_dir(&temp.root)
        .output()
        .expect("arche new executes");
    assert_eq!(new_out.status.code(), Some(ProcessStatus::Success.code()));
    let pkg_dir = temp.root.join("my_pkg");
    assert!(pkg_dir.join("Arche.toml").is_file());
    assert!(pkg_dir.join("src/main.arc").is_file());

    // 2. arche check inside package
    let check_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("check")
        .current_dir(&pkg_dir)
        .output()
        .expect("arche check executes");
    assert_eq!(check_out.status.code(), Some(ProcessStatus::Success.code()));
    assert!(pkg_dir.join("Arche.lock").is_file());

    // 3. arche build
    let build_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("build")
        .current_dir(&pkg_dir)
        .output()
        .expect("arche build executes");
    if build_out.status.code() != Some(0) {
        eprintln!(
            "BUILD STDOUT: {}",
            String::from_utf8_lossy(&build_out.stdout)
        );
        eprintln!(
            "BUILD STDERR: {}",
            String::from_utf8_lossy(&build_out.stderr)
        );
    }
    assert_eq!(build_out.status.code(), Some(ProcessStatus::Success.code()));
    assert!(pkg_dir.join("target/debug/my_pkg").is_file());

    // 4. arche test
    let test_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("test")
        .current_dir(&pkg_dir)
        .output()
        .expect("arche test executes");
    assert_eq!(test_out.status.code(), Some(ProcessStatus::Success.code()));
    let test_str = String::from_utf8(test_out.stdout).expect("UTF-8");
    assert!(test_str.contains("test result: ok"));

    // 5. arche clean
    let clean_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("clean")
        .current_dir(&pkg_dir)
        .output()
        .expect("arche clean executes");
    assert_eq!(clean_out.status.code(), Some(ProcessStatus::Success.code()));
    assert!(!pkg_dir.join("target").exists());
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

#[test]
fn public_cli_developer_tools_workflow() {
    let temp = TempProject::new_temp("devtools");

    // 1. arche new
    let new_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["new", "dev_pkg", "--bin"])
        .current_dir(&temp.root)
        .output()
        .expect("arche new executes");
    assert_eq!(new_out.status.code(), Some(ProcessStatus::Success.code()));
    let pkg_dir = temp.root.join("dev_pkg");

    // 2. arche fmt --check
    let fmt_check = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["fmt", "--check"])
        .current_dir(&pkg_dir)
        .output()
        .expect("arche fmt executes");
    assert_eq!(fmt_check.status.code(), Some(ProcessStatus::Success.code()));

    // 3. arche doc
    let doc_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("doc")
        .current_dir(&pkg_dir)
        .output()
        .expect("arche doc executes");
    assert_eq!(doc_out.status.code(), Some(ProcessStatus::Success.code()));
    assert!(pkg_dir.join("target/doc/dev_pkg/index.html").is_file());

    // 4. arche inspect
    let inspect_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("inspect")
        .current_dir(&pkg_dir)
        .output()
        .expect("arche inspect executes");
    assert_eq!(
        inspect_out.status.code(),
        Some(ProcessStatus::Success.code())
    );
    let inspect_str = String::from_utf8(inspect_out.stdout).expect("UTF-8");
    assert!(inspect_str.contains("Package: dev_pkg"));

    // 5. arche inspect --json
    let inspect_json = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["inspect", "--json"])
        .current_dir(&pkg_dir)
        .output()
        .expect("arche inspect --json executes");
    assert_eq!(
        inspect_json.status.code(),
        Some(ProcessStatus::Success.code())
    );
    let json_str = String::from_utf8(inspect_json.stdout).expect("UTF-8");
    assert!(json_str.contains("\"type\":\"inspect\""));

    // 6. arche debug
    let debug_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("debug")
        .current_dir(&pkg_dir)
        .output()
        .expect("arche debug executes");
    assert_eq!(debug_out.status.code(), Some(ProcessStatus::Success.code()));

    // 7. arche profile
    let profile_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("profile")
        .current_dir(&pkg_dir)
        .output()
        .expect("arche profile executes");
    assert_eq!(
        profile_out.status.code(),
        Some(ProcessStatus::Success.code())
    );
}

#[test]
fn public_cli_registry_and_toolchain_workflow() {
    let temp = TempProject::new_temp("regtools");

    // 1. arche new
    let new_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["new", "reg_pkg", "--bin"])
        .current_dir(&temp.root)
        .output()
        .expect("arche new executes");
    assert_eq!(new_out.status.code(), Some(ProcessStatus::Success.code()));
    let pkg_dir = temp.root.join("reg_pkg");

    // 2. arche add
    let add_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["add", "std/math"])
        .current_dir(&pkg_dir)
        .output()
        .expect("arche add executes");
    assert_eq!(add_out.status.code(), Some(ProcessStatus::Success.code()));

    // 3. arche package
    let pack_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("package")
        .current_dir(&pkg_dir)
        .output()
        .expect("arche package executes");
    if pack_out.status.code() != Some(0) {
        eprintln!("PACK STDOUT: {}", String::from_utf8_lossy(&pack_out.stdout));
        eprintln!("PACK STDERR: {}", String::from_utf8_lossy(&pack_out.stderr));
    }
    assert_eq!(pack_out.status.code(), Some(ProcessStatus::Success.code()));
    assert!(pkg_dir
        .join("target/package/reg_pkg-0.1.0.archepkg")
        .is_file());

    // 4. arche search
    let search_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["search", "math"])
        .current_dir(&pkg_dir)
        .output()
        .expect("arche search executes");
    assert_eq!(
        search_out.status.code(),
        Some(ProcessStatus::Success.code())
    );

    // 5. arche login / whoami / logout
    let login_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["login", "--token", "test_tok"])
        .current_dir(&pkg_dir)
        .output()
        .expect("arche login executes");
    assert_eq!(login_out.status.code(), Some(ProcessStatus::Success.code()));

    let whoami_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("whoami")
        .current_dir(&pkg_dir)
        .output()
        .expect("arche whoami executes");
    assert_eq!(
        whoami_out.status.code(),
        Some(ProcessStatus::Success.code())
    );
    let who_str = String::from_utf8(whoami_out.stdout).expect("UTF-8");
    assert!(who_str.contains("developer"));

    // 6. arche publish
    let pub_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .arg("publish")
        .current_dir(&pkg_dir)
        .output()
        .expect("arche publish executes");
    assert_eq!(pub_out.status.code(), Some(ProcessStatus::Success.code()));

    // 7. arche scope / owner / trusted-publisher / yank
    let scope_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["scope", "list"])
        .current_dir(&pkg_dir)
        .output()
        .expect("arche scope executes");
    assert_eq!(scope_out.status.code(), Some(ProcessStatus::Success.code()));

    let owner_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["owner", "list", "reg_pkg"])
        .current_dir(&pkg_dir)
        .output()
        .expect("arche owner executes");
    assert_eq!(owner_out.status.code(), Some(ProcessStatus::Success.code()));

    let tp_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["trusted-publisher", "list", "reg_pkg"])
        .current_dir(&pkg_dir)
        .output()
        .expect("arche trusted-publisher executes");
    assert_eq!(tp_out.status.code(), Some(ProcessStatus::Success.code()));

    let yank_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["yank", "reg_pkg", "--version", "0.1.0"])
        .current_dir(&pkg_dir)
        .output()
        .expect("arche yank executes");
    assert_eq!(yank_out.status.code(), Some(ProcessStatus::Success.code()));

    let unyank_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["unyank", "reg_pkg", "--version", "0.1.0"])
        .current_dir(&pkg_dir)
        .output()
        .expect("arche unyank executes");
    assert_eq!(
        unyank_out.status.code(),
        Some(ProcessStatus::Success.code())
    );

    // 8. arche toolchain
    let tc_out = Command::new(env!("CARGO_BIN_EXE_arche"))
        .args(["toolchain", "list"])
        .current_dir(&pkg_dir)
        .output()
        .expect("arche toolchain executes");
    assert_eq!(tc_out.status.code(), Some(ProcessStatus::Success.code()));
}
