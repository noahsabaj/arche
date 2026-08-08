use arche_foundation::status::ProcessStatus;
use std::process::ExitCode;

fn main() -> ExitCode {
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let status = arche::run(
        std::env::args().skip(1),
        &mut stdout.lock(),
        &mut stderr.lock(),
    )
    .unwrap_or(ProcessStatus::Failure);
    ExitCode::from(status.code_u8())
}
