use std::path::Path;

use crate::checker::CheckError;
use crate::lexer::LexError;
use crate::parser::ParseError;
use crate::source_snapshot::SourcePosition;

pub fn format_lex_error(path: &Path, position: SourcePosition, error: &LexError) -> String {
    format_error(
        path,
        position,
        "LEX001",
        &format!("unexpected character '{}'", error.character),
    )
}

pub fn format_parse_error(path: &Path, position: SourcePosition, error: &ParseError) -> String {
    format_error(path, position, "PARSE001", &error.message)
}

pub fn format_check_error(path: &Path, position: SourcePosition, error: &CheckError) -> String {
    format_error(path, position, "CHECK001", &error.message)
}

fn format_error(path: &Path, position: SourcePosition, code: &str, message: &str) -> String {
    format!(
        "{}:{}:{}: error[{}]: {}",
        path.display(),
        position.line,
        position.column,
        code,
        message
    )
}
