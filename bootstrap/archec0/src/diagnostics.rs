use std::path::Path;

use crate::checker::CheckError;
use crate::lexer::{LexError, Span};
use crate::parser::ParseError;

pub fn format_lex_error(path: &Path, source: &str, error: &LexError) -> String {
    format_error(
        path,
        source,
        Span {
            start: error.offset,
            end: error.offset,
        },
        "LEX001",
        &format!("unexpected character '{}'", error.character),
    )
}

pub fn format_parse_error(path: &Path, source: &str, error: &ParseError) -> String {
    format_error(path, source, error.span, "PARSE001", &error.message)
}

pub fn format_check_error(path: &Path, source: &str, error: &CheckError) -> String {
    format_error(path, source, error.span, "CHECK001", &error.message)
}

fn format_error(path: &Path, source: &str, span: Span, code: &str, message: &str) -> String {
    let location = line_column(source, span.start);
    format!(
        "{}:{}:{}: error[{}]: {}",
        path.display(),
        location.line,
        location.column,
        code,
        message
    )
}

fn line_column(source: &str, offset: usize) -> Location {
    let mut line = 1;
    let mut column = 1;

    for (index, character) in source.char_indices() {
        if index >= offset {
            break;
        }

        if character == '\n' {
            line += 1;
            column = 1;
        } else if character != '\r' {
            column += 1;
        }
    }

    Location { line, column }
}

struct Location {
    line: usize,
    column: usize,
}
