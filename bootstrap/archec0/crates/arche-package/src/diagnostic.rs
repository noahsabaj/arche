use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DiagnosticCode {
    ManifestSyntax,
    ManifestSchema,
    ManifestUnknown,
    ManifestValue,
    ManifestTarget,
    WorkspaceDiscovery,
    WorkspacePath,
    WorkspaceMember,
    DependencyConflict,
    DependencyCycle,
    RegistryInvalid,
    LockInvalid,
    Io,
}

impl DiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ManifestSyntax => "MANIFEST001",
            Self::ManifestSchema => "MANIFEST002",
            Self::ManifestUnknown => "MANIFEST003",
            Self::ManifestValue => "MANIFEST004",
            Self::ManifestTarget => "MANIFEST005",
            Self::WorkspaceDiscovery => "WORKSPACE001",
            Self::WorkspacePath => "WORKSPACE002",
            Self::WorkspaceMember => "WORKSPACE003",
            Self::DependencyConflict => "DEPENDENCY001",
            Self::DependencyCycle => "DEPENDENCY002",
            Self::RegistryInvalid => "DEPENDENCY003",
            Self::LockInvalid => "LOCK001",
            Self::Io => "PACKAGEIO001",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceLabel {
    pub path: PathBuf,
    pub start: u64,
    pub end: u64,
}

impl SourceLabel {
    pub fn new(path: impl Into<PathBuf>, start: u64, end: u64) -> Self {
        Self {
            path: path.into(),
            start,
            end,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: String,
    pub primary: Option<SourceLabel>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn new(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            primary: None,
            notes: Vec::new(),
        }
    }

    pub fn at_path(mut self, path: impl AsRef<Path>) -> Self {
        self.primary = Some(SourceLabel::new(path.as_ref(), 0, 0));
        self
    }

    pub fn at_span(mut self, path: impl AsRef<Path>, start: usize, end: usize) -> Self {
        self.primary = Some(SourceLabel::new(
            path.as_ref(),
            u64::try_from(start).unwrap_or(u64::MAX),
            u64::try_from(end).unwrap_or(u64::MAX),
        ));
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.message)?;
        if let Some(label) = &self.primary {
            write!(
                formatter,
                " [{} bytes {}..{}]",
                label.path.display(),
                label.start,
                label.end
            )?;
        }
        for note in &self.notes {
            write!(formatter, "\nnote: {note}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostics {
    entries: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn one(diagnostic: Diagnostic) -> Self {
        Self {
            entries: vec![diagnostic],
        }
    }

    pub fn new(entries: Vec<Diagnostic>) -> Self {
        debug_assert!(!entries.is_empty());
        Self { entries }
    }

    pub fn entries(&self) -> &[Diagnostic] {
        &self.entries
    }

    pub fn into_entries(self) -> Vec<Diagnostic> {
        self.entries
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.entries.iter().enumerate() {
            if index != 0 {
                formatter.write_str("\n")?;
            }
            diagnostic.fmt(formatter)?;
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostics {}

impl From<Diagnostic> for Diagnostics {
    fn from(value: Diagnostic) -> Self {
        Self::one(value)
    }
}

pub(crate) fn io_diagnostic(path: &Path, action: &str, error: &std::io::Error) -> Diagnostics {
    Diagnostic::new(
        DiagnosticCode::Io,
        format!("could not {action} `{}`: {error}", path.display()),
    )
    .at_path(path)
    .into()
}
