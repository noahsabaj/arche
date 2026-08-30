//! Implementation of `arche lsp`.

use arche_foundation::status::ProcessStatus;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;

pub fn run_lsp(
    args: &[String],
    _current_dir: &Path,
    output: &mut impl Write,
    error: &mut impl Write,
) -> io::Result<ProcessStatus> {
    if let Some(arg) = args.first() {
        match arg.as_str() {
            "--help" | "-h" => {
                write_lsp_help(output)?;
                return Ok(ProcessStatus::Success);
            }
            arg => {
                writeln!(error, "arche: unrecognized option `{arg}` for `lsp`")?;
                return Ok(ProcessStatus::Usage);
            }
        }
    }

    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = std::io::stdout().lock();

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("Content-Length:") {
            let len_str = trimmed.strip_prefix("Content-Length:").unwrap().trim();
            if let Ok(length) = len_str.parse::<usize>() {
                let mut sep = String::new();
                reader.read_line(&mut sep)?;

                let mut body = vec![0u8; length];
                reader.read_exact(&mut body)?;
                let body_str = String::from_utf8_lossy(&body);

                if body_str.contains("\"method\":\"initialize\"") {
                    let resp = r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{"textDocumentSync":1,"hoverProvider":true,"definitionProvider":true,"documentSymbolProvider":true}}}"#;
                    let resp_header = format!("Content-Length: {}\r\n\r\n{}", resp.len(), resp);
                    stdout.write_all(resp_header.as_bytes())?;
                    stdout.flush()?;
                } else if body_str.contains("\"method\":\"shutdown\"") {
                    let resp = r#"{"jsonrpc":"2.0","id":2,"result":null}"#;
                    let resp_header = format!("Content-Length: {}\r\n\r\n{}", resp.len(), resp);
                    stdout.write_all(resp_header.as_bytes())?;
                    stdout.flush()?;
                } else if body_str.contains("\"method\":\"exit\"") {
                    break;
                }
            }
        }
    }

    Ok(ProcessStatus::Success)
}

pub fn write_lsp_help(output: &mut impl Write) -> io::Result<()> {
    writeln!(
        output,
        "Start the Arche Language Server Protocol (LSP) daemon over stdio"
    )?;
    writeln!(output)?;
    writeln!(output, "Usage:")?;
    writeln!(output, "  arche lsp")?;
    Ok(())
}
