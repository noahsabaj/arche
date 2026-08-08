mod aot_v2;
mod checker;
mod component_inspect;
mod core;
mod core_format;
mod core_lower;
mod core_verify;
mod diagnostics;
mod elf64;
pub mod execution_package_build;
mod identifier;
mod layout;
mod lexer;
mod machine;
mod native_runtime_v2;
mod output;
mod parser;
pub mod reference_executor_v2;
mod source_snapshot;

pub use archec0::scalar_v2;

use arche_foundation::status::CompilerStatus;
use std::env;
use std::path::Path;
use std::process;

#[cfg(all(test, target_os = "linux"))]
pub(crate) fn lock_linux_test_artifact_execution() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn main() {
    initialize_compiler_floating_point_environment();
    let args: Vec<String> = env::args().skip(1).collect();

    match args.as_slice() {
        [arg] if arg == "--help" || arg == "-h" => print_help(),
        [arg] if arg == "--version" => println!("archec0 {}", env!("CARGO_PKG_VERSION")),
        [source_path, flag] if flag == "--emit-tokens" => emit_tokens(source_path),
        [source_path, flag] if flag == "--emit-ast" => emit_ast(source_path),
        [source_path, flag] if flag == "--check" => check_program(source_path),
        [source_path, flag] if flag == "--emit-machine" => emit_machine(source_path),
        [source_path, flag] if flag == "--emit-core" => emit_core(source_path),
        [source_path, flag] if flag == "--inspect-components" => inspect_components(source_path),
        [source_path, flag, output_path] if flag == "-o" || flag == "--output" => {
            write_output(source_path, output_path)
        }
        [source_path] => check_program(source_path),
        [] => {
            eprintln!("archec0: no input provided");
            eprintln!("run `archec0 --help` for usage");
            process::exit(CompilerStatus::Usage.code());
        }
        _ => {
            eprintln!("archec0: command not implemented yet");
            eprintln!("run `archec0 --help` for usage");
            process::exit(CompilerStatus::Usage.code());
        }
    }
}

fn initialize_compiler_floating_point_environment() {
    scalar_v2::initialize_floating_point_environment();
}

fn print_help() {
    println!(
        "\
archec0 - Arche bootstrap compiler

Usage:
  archec0 --help
  archec0 -h
  archec0 --version
  archec0 <source.arc>
  archec0 <source.arc> --emit-tokens
  archec0 <source.arc> --emit-ast
  archec0 <source.arc> --check
  archec0 <source.arc> --emit-machine
  archec0 <source.arc> --emit-core
  archec0 <source.arc> --inspect-components
  archec0 <source.arc> -o <output>

Source-only invocation is equivalent to --check.
"
    );
}

fn emit_tokens(source_path: &str) {
    let path = Path::new(source_path);
    let source = read_source(path);
    let reader = source_reader(path, &source);
    let mut lexer = lexer::Lexer::new(reader);
    let stdout = std::io::stdout();
    let mut output = stdout.lock();

    loop {
        let token = match lexer.next_token() {
            Ok(token) => token,
            Err(error) => report_lexer_failure(path, error),
        };
        let eof = token.kind == lexer::TokenKind::Eof;
        if let Err(error) = lexer::write_token(&mut output, &token) {
            eprintln!("archec0: could not emit tokens: {error}");
            process::exit(CompilerStatus::Failure.code());
        }
        if eof {
            break;
        }
    }
    if let Err(error) = std::io::Write::flush(&mut output) {
        eprintln!("archec0: could not flush tokens: {error}");
        process::exit(CompilerStatus::Failure.code());
    }
}

fn emit_ast(source_path: &str) {
    let path = Path::new(source_path);
    let source = read_source(path);
    let program = parse_source(path, &source);

    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    if let Err(error) = parser::write_program(&mut output, &program)
        .and_then(|()| std::io::Write::write_all(&mut output, b"\n"))
        .and_then(|()| std::io::Write::flush(&mut output))
    {
        eprintln!("archec0: could not emit AST: {error}");
        process::exit(CompilerStatus::Failure.code());
    }
}

fn check_program(source_path: &str) {
    let path = Path::new(source_path);
    let source = read_source(path);
    let program = parse_source(path, &source);

    let _executable = build_executable(path, &program);

    println!("archec0: check passed {}", path.display());
}

fn emit_machine(source_path: &str) {
    let path = Path::new(source_path);
    let source = read_source(path);
    let program = parse_source(path, &source);

    let executable = build_executable(path, &program);

    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    if let Err(error) = machine::write_machine(&mut output, &executable.core)
        .and_then(|()| std::io::Write::write_all(&mut output, b"\n"))
        .and_then(|()| std::io::Write::flush(&mut output))
    {
        eprintln!("archec0: could not emit Machine IR: {error}");
        process::exit(CompilerStatus::Failure.code());
    }
}

fn emit_core(source_path: &str) {
    let path = Path::new(source_path);
    let source = read_source(path);
    let program = parse_source(path, &source);

    let executable = build_executable(path, &program);

    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    if let Err(error) = core_format::write_verified_core_program(&mut output, &executable.core)
        .and_then(|()| std::io::Write::write_all(&mut output, b"\n"))
        .and_then(|()| std::io::Write::flush(&mut output))
    {
        eprintln!("archec0: could not emit Core: {error}");
        process::exit(CompilerStatus::Failure.code());
    }
}

fn inspect_components(source_path: &str) {
    let path = Path::new(source_path);
    let source = read_source(path);
    let program = parse_source(path, &source);

    if let Err(error) = checker::check_declarations(&program) {
        eprintln!(
            "{}",
            diagnostics::format_check_error(path, error.span.start, &error)
        );
        process::exit(CompilerStatus::Failure.code());
    }

    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    if let Err(error) = component_inspect::write_components(&mut output, &program) {
        eprintln!("archec0: could not inspect components: {error}");
        process::exit(CompilerStatus::Failure.code());
    }
    if let Err(error) = std::io::Write::flush(&mut output) {
        eprintln!("archec0: could not flush component inspection: {error}");
        process::exit(CompilerStatus::Failure.code());
    }
}

fn read_source(path: &Path) -> source_snapshot::SourceSnapshot {
    if !path.is_file() {
        eprintln!("archec0: source file not found: {}", path.display());
        process::exit(CompilerStatus::Usage.code());
    }

    match source_snapshot::SourceSnapshot::capture(path) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!(
                "archec0: could not snapshot source {}: {}",
                path.display(),
                error
            );
            process::exit(CompilerStatus::Failure.code());
        }
    }
}

fn source_reader(
    path: &Path,
    source: &source_snapshot::SourceSnapshot,
) -> std::io::BufReader<std::fs::File> {
    match source.reader() {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!(
                "archec0: could not read source {}: {}",
                path.display(),
                error
            );
            process::exit(CompilerStatus::Failure.code());
        }
    }
}

fn report_lexer_failure(path: &Path, error: lexer::LexerFailure) -> ! {
    match error {
        lexer::LexerFailure::Lex(error) => {
            eprintln!(
                "{}",
                diagnostics::format_lex_error(path, error.span.start, &error)
            );
        }
        lexer::LexerFailure::Read(error) => {
            eprintln!(
                "archec0: could not read source {}: {}",
                path.display(),
                error
            );
        }
    }
    process::exit(CompilerStatus::Failure.code());
}

fn parse_source(path: &Path, source: &source_snapshot::SourceSnapshot) -> parser::Program {
    let lexer = lexer::Lexer::new(source_reader(path, source));
    match parser::parse_lexer(lexer) {
        Ok(program) => program,
        Err(parser::ParseStreamError::Parse(error)) => {
            eprintln!(
                "{}",
                diagnostics::format_parse_error(path, error.span.start, &error)
            );
            process::exit(CompilerStatus::Failure.code());
        }
        Err(parser::ParseStreamError::Lex(error)) => {
            eprintln!(
                "{}",
                diagnostics::format_lex_error(path, error.span.start, &error)
            );
            process::exit(CompilerStatus::Failure.code());
        }
        Err(parser::ParseStreamError::Read(error)) => {
            eprintln!(
                "archec0: could not read source {}: {}",
                path.display(),
                error
            );
            process::exit(CompilerStatus::Failure.code());
        }
    }
}

struct ExecutableBuild {
    core: core_verify::VerifiedExecutableCore,
    image: aot_v2::AotImage,
}

fn build_executable(path: &Path, program: &parser::Program) -> ExecutableBuild {
    if let Err(error) = checker::check_program(program) {
        eprintln!(
            "{}",
            diagnostics::format_check_error(path, error.span.start, &error)
        );
        process::exit(CompilerStatus::Failure.code());
    }

    let core = match core_lower::lower_program_to_core(program) {
        Ok(core) => core,
        Err(error) => {
            eprintln!("archec0: could not lower Core: {}", error.message);
            process::exit(CompilerStatus::Failure.code());
        }
    };
    let core = verify_executable_core(core);
    let plan = match aot_v2::plan_native(&core) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("archec0: could not plan native executable: {error}");
            process::exit(CompilerStatus::Failure.code());
        }
    };
    let source_name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("source.arc"))
        .to_string_lossy();
    let package = match execution_package_build::build_execution_package(
        &core,
        &source_name,
        plan.native_code_layout(),
    ) {
        Ok(package) => package,
        Err(error) => {
            eprintln!("archec0: could not build ARCHEECS v2 package: {error}");
            process::exit(CompilerStatus::Failure.code());
        }
    };
    let image = match aot_v2::finalize_native(plan, &core, &package) {
        Ok(image) => image,
        Err(error) => {
            eprintln!("archec0: could not link native executable: {error}");
            process::exit(CompilerStatus::Failure.code());
        }
    };

    ExecutableBuild { core, image }
}

fn verify_executable_core(core: core::CoreProgram) -> core_verify::VerifiedExecutableCore {
    match core_verify::verify_executable_core(core) {
        Ok(core) => core,
        Err(error) => {
            eprintln!("archec0: invalid executable Core: {}", error.message);
            process::exit(CompilerStatus::Failure.code());
        }
    }
}

fn write_output(source_path: &str, output_path: &str) {
    let source = Path::new(source_path);
    let output = Path::new(output_path);

    let source_snapshot = read_source(source);
    let program = parse_source(source, &source_snapshot);
    let executable = build_executable(source, &program);

    match output::publish_with(source_snapshot.identity(), output, |temporary| {
        executable
            .image
            .write_static_pie(temporary, 0)
            .map(|_| ())
            .map_err(std::io::Error::other)
    }) {
        Ok(()) => {}
        Err(output::PublishError::SourceOutputAlias) => {
            eprintln!(
                "archec0: refusing to overwrite input source with output {}",
                output.display()
            );
            process::exit(CompilerStatus::Usage.code());
        }
        Err(error) => {
            eprintln!(
                "archec0: could not write output {}: {}",
                output.display(),
                error
            );
            process::exit(CompilerStatus::Failure.code());
        }
    }

    println!("archec0: accepted source {}", source.display());
    println!("archec0: wrote ELF64 executable {}", output.display());
}

#[cfg(test)]
mod compiler_entry_tests {
    use super::*;

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn entry_initializer_controls_metadata_authoritative_f32_folding() {
        let perturbed_mxcsr = 0x0000_ffc0_u32;
        // SAFETY: the value uses only architecturally defined low MXCSR bits
        // and the instruction reads from a valid four-byte stack location.
        unsafe {
            std::arch::asm!(
                "ldmxcsr [{mxcsr}]",
                mxcsr = in(reg) &perturbed_mxcsr,
                options(nostack, preserves_flags, readonly),
            );
        }
        initialize_compiler_floating_point_environment();

        let source = "world EntryFp
resource Result { value: f32 }
startup {
  resource Result {
    value: 0.000000000000000000000000000000000000000000001
      + 0.000000000000000000000000000000000000000000001
  }
  exit 0
}";
        let tokens = lexer::lex(source).expect("entry f32 fixture lexes");
        let program = parser::parse_program(&tokens).expect("entry f32 fixture parses");
        checker::check_program(&program).expect("entry f32 fixture checks");
        let core = core_lower::lower_program_to_core(&program).expect("entry f32 fixture lowers");
        let fields = core.functions[0].blocks[0]
            .instructions
            .iter()
            .find_map(|instruction| match instruction {
                core::CoreInstruction::InitializeResource { fields, .. } => Some(fields),
                _ => None,
            })
            .expect("folded resource payload exists");
        assert_eq!(
            fields[0].value,
            core::CoreSpawnFieldValue::F32Bits(2),
            "FTZ or DAZ would collapse the two minimum subnormals to zero"
        );
    }
}
