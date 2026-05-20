mod checker;
mod codegen;
mod component_inspect;
mod component_metadata;
mod core;
mod core_format;
mod core_lower;
mod core_verify;
mod diagnostics;
mod ecs_metadata;
mod elf64;
mod layout;
mod lexer;
mod machine;
mod parser;
mod runtime;
mod runtime_assembly;

use std::env;
use std::fs;
use std::path::Path;
use std::process;

fn main() {
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
        [source_path] => check_source_path(source_path),
        [] => {
            eprintln!("archec0: no input provided");
            eprintln!("run `archec0 --help` for usage");
            process::exit(2);
        }
        _ => {
            eprintln!("archec0: command not implemented yet");
            eprintln!("run `archec0 --help` for usage");
            process::exit(2);
        }
    }
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

This seed executable currently proves that the bootstrap compiler can be invoked.
"
    );
}

fn check_source_path(source_path: &str) {
    let path = Path::new(source_path);

    if !path.is_file() {
        eprintln!("archec0: source file not found: {}", path.display());
        process::exit(2);
    }

    println!("archec0: accepted source {}", path.display());
    println!("archec0: compilation is not implemented yet");
}

fn emit_tokens(source_path: &str) {
    let path = Path::new(source_path);

    let source = read_source(path);
    let tokens = lex_source(path, &source);

    for token in tokens {
        let _span = token.span;
        println!("{}", token.kind);
    }
}

fn emit_ast(source_path: &str) {
    let path = Path::new(source_path);
    let source = read_source(path);
    let program = parse_source(path, &source);

    println!("{}", program);
}

fn check_program(source_path: &str) {
    let path = Path::new(source_path);
    let source = read_source(path);
    let program = parse_source(path, &source);

    if let Err(error) = checker::check_program(&program) {
        eprintln!("{}", diagnostics::format_check_error(path, &source, &error));
        process::exit(1);
    }

    println!("archec0: check passed {}", path.display());
}

fn emit_machine(source_path: &str) {
    let path = Path::new(source_path);
    let source = read_source(path);
    let program = parse_source(path, &source);

    if let Err(error) = checker::check_program(&program) {
        eprintln!("{}", diagnostics::format_check_error(path, &source, &error));
        process::exit(1);
    }

    println!("{}", machine::emit_machine(&program));
}

fn emit_core(source_path: &str) {
    let path = Path::new(source_path);
    let source = read_source(path);
    let program = parse_source(path, &source);

    if let Err(error) = checker::check_program(&program) {
        eprintln!("{}", diagnostics::format_check_error(path, &source, &error));
        process::exit(1);
    }

    let core = match core_lower::lower_program_to_core(&program) {
        Ok(core) => core,
        Err(error) => {
            eprintln!("archec0: could not lower Core: {}", error.message);
            process::exit(1);
        }
    };

    if let Err(error) = core_verify::verify_core_program(&core) {
        eprintln!("archec0: invalid Core: {}", error.message);
        process::exit(1);
    }

    println!("{}", core_format::format_core_program(&core));
}

fn inspect_components(source_path: &str) {
    let path = Path::new(source_path);
    let source = read_source(path);
    let program = parse_source(path, &source);

    let output = match component_inspect::format_components(&program) {
        Ok(output) => output,
        Err(error) => {
            eprintln!("archec0: could not inspect components: {}", error.message);
            process::exit(1);
        }
    };

    if !output.is_empty() {
        println!("{output}");
    }
}

fn read_source(path: &Path) -> String {
    if !path.is_file() {
        eprintln!("archec0: source file not found: {}", path.display());
        process::exit(2);
    }

    match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!(
                "archec0: could not read source {}: {}",
                path.display(),
                error
            );
            process::exit(1);
        }
    }
}

fn lex_source(path: &Path, source: &str) -> Vec<lexer::Token> {
    match lexer::lex(source) {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("{}", diagnostics::format_lex_error(path, source, &error));
            process::exit(1);
        }
    }
}

fn parse_source(path: &Path, source: &str) -> parser::Program {
    let tokens = lex_source(path, source);
    match parser::parse_program(&tokens) {
        Ok(program) => program,
        Err(error) => {
            eprintln!("{}", diagnostics::format_parse_error(path, source, &error));
            process::exit(1);
        }
    }
}

fn write_output(source_path: &str, output_path: &str) {
    let source = Path::new(source_path);
    let output = Path::new(output_path);

    let source_text = read_source(source);
    let program = parse_source(source, &source_text);
    let assembly = match runtime_assembly::assemble_runtime_program_from_source(&program) {
        Ok(assembly) => assembly,
        Err(error) => {
            eprintln!(
                "archec0: could not assemble runtime metadata: {}",
                error.message
            );
            process::exit(1);
        }
    };

    let (text_payload, metadata_payload) = if assembly.requires_ecs_metadata() {
        if let Err(error) = checker::check_ecs_declarations(&program) {
            eprintln!(
                "{}",
                diagnostics::format_check_error(source, &source_text, &error)
            );
            process::exit(1);
        }

        let text_payload = match codegen::metadata_carrier_text_payload(&program) {
            Ok(text_payload) => text_payload,
            Err(error) => {
                eprintln!("archec0: {}", error.message);
                process::exit(1);
            }
        };
        let metadata_payload = match ecs_metadata::encode_ecs_metadata(&assembly) {
            Ok(metadata_payload) => metadata_payload,
            Err(error) => {
                eprintln!("archec0: could not encode ECS metadata: {}", error.message);
                process::exit(1);
            }
        };

        (text_payload, metadata_payload)
    } else {
        if let Err(error) = checker::check_program(&program) {
            eprintln!(
                "{}",
                diagnostics::format_check_error(source, &source_text, &error)
            );
            process::exit(1);
        }

        let text_payload = match codegen::startup_text_payload(&program) {
            Ok(text_payload) => text_payload,
            Err(error) => {
                eprintln!("archec0: {}", error.message);
                process::exit(1);
            }
        };
        let metadata_payload = match component_metadata::encode_component_metadata(&program) {
            Ok(metadata_payload) => metadata_payload,
            Err(error) => {
                eprintln!(
                    "archec0: could not encode component metadata: {}",
                    error.message
                );
                process::exit(1);
            }
        };

        (text_payload, metadata_payload)
    };

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(error) = fs::create_dir_all(parent) {
                eprintln!(
                    "archec0: could not create output directory {}: {}",
                    parent.display(),
                    error
                );
                process::exit(1);
            }
        }
    }

    if let Err(error) =
        elf64::write_executable_with_metadata(output, &text_payload, &metadata_payload)
    {
        eprintln!(
            "archec0: could not write output {}: {}",
            output.display(),
            error
        );
        process::exit(1);
    }

    println!("archec0: accepted source {}", source.display());
    println!("archec0: wrote ELF64 executable {}", output.display());
}
