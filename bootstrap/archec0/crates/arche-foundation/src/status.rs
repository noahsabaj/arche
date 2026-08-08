//! Stable process-status taxonomy shared by compiler and public tooling.

/// Process outcomes reserved by the Arche platform contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum ProcessStatus {
    Success = 0,
    Failure = 1,
    Usage = 2,
    TrainerProtocolViolation = 64,
    TrapOrPanic = 70,
    UncaughtException = 71,
    EnvironmentInvariant = 72,
    Abort = 134,
}

impl ProcessStatus {
    pub const fn code(self) -> i32 {
        self as i32
    }

    pub const fn code_u8(self) -> u8 {
        self as u8
    }
}

/// Applies the language contract for a source-directed `main` return value.
pub const fn source_exit_code(value: i32) -> u8 {
    value.to_le_bytes()[0]
}

/// The subset of process outcomes that a compiler invocation may produce.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum CompilerStatus {
    Success = ProcessStatus::Success as i32,
    Failure = ProcessStatus::Failure as i32,
    Usage = ProcessStatus::Usage as i32,
}

impl CompilerStatus {
    pub const fn code(self) -> i32 {
        self as i32
    }
}

impl From<CompilerStatus> for ProcessStatus {
    fn from(status: CompilerStatus) -> Self {
        match status {
            CompilerStatus::Success => Self::Success,
            CompilerStatus::Failure => Self::Failure,
            CompilerStatus::Usage => Self::Usage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_status_codes_are_the_public_contract() {
        assert_eq!(
            [
                ProcessStatus::Success.code(),
                ProcessStatus::Failure.code(),
                ProcessStatus::Usage.code(),
                ProcessStatus::TrainerProtocolViolation.code(),
                ProcessStatus::TrapOrPanic.code(),
                ProcessStatus::UncaughtException.code(),
                ProcessStatus::EnvironmentInvariant.code(),
                ProcessStatus::Abort.code(),
            ],
            [0, 1, 2, 64, 70, 71, 72, 134]
        );
    }

    #[test]
    fn compiler_statuses_map_to_the_shared_process_taxonomy() {
        for (compiler, process) in [
            (CompilerStatus::Success, ProcessStatus::Success),
            (CompilerStatus::Failure, ProcessStatus::Failure),
            (CompilerStatus::Usage, ProcessStatus::Usage),
        ] {
            assert_eq!(compiler.code(), process.code());
            assert_eq!(ProcessStatus::from(compiler), process);
        }
    }

    #[test]
    fn source_returns_use_the_low_eight_bits() {
        assert_eq!(source_exit_code(0), 0);
        assert_eq!(source_exit_code(47), 47);
        assert_eq!(source_exit_code(70), 70);
        assert_eq!(source_exit_code(256), 0);
        assert_eq!(source_exit_code(-1), 255);
        assert_eq!(source_exit_code(i32::MIN), 0);
    }
}
