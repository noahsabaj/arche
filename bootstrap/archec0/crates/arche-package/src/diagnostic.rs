use crate::manifest::ManifestSpan;
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
    IdentityInvalid,
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
            Self::IdentityInvalid => "IDENTITY001",
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
    pub start_line: Option<u64>,
    pub start_column: Option<u64>,
    pub end_line: Option<u64>,
    pub end_column: Option<u64>,
}

impl SourceLabel {
    pub fn new(path: impl Into<PathBuf>, start: u64, end: u64) -> Self {
        Self {
            path: path.into(),
            start,
            end,
            start_line: None,
            start_column: None,
            end_line: None,
            end_column: None,
        }
    }

    pub fn source_span(path: impl Into<PathBuf>, span: ManifestSpan) -> Self {
        Self {
            path: path.into(),
            start: span.start_byte,
            end: span.end_byte,
            start_line: Some(span.start_line),
            start_column: Some(span.start_column),
            end_line: Some(span.end_line),
            end_column: Some(span.end_column),
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

    pub fn at_source_span(mut self, path: impl AsRef<Path>, span: ManifestSpan) -> Self {
        self.primary = Some(SourceLabel::source_span(path.as_ref(), span));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_span_remains_byte_only() {
        let diagnostic =
            Diagnostic::new(DiagnosticCode::ManifestSyntax, "invalid").at_span("Arche.toml", 4, 9);
        let label = diagnostic.primary.expect("primary label");

        assert_eq!(label.start, 4);
        assert_eq!(label.end, 9);
        assert_eq!(label.start_line, None);
        assert_eq!(label.start_column, None);
        assert_eq!(label.end_line, None);
        assert_eq!(label.end_column, None);
    }

    #[test]
    fn exact_source_span_preserves_u64_coordinates() {
        let start_byte = u64::from(u32::MAX) + 1;
        let diagnostic = Diagnostic::new(DiagnosticCode::IdentityInvalid, "exhausted")
            .at_source_span(
                "Arche.toml",
                ManifestSpan {
                    start_byte,
                    end_byte: u64::MAX,
                    start_line: 7,
                    start_column: 11,
                    end_line: 8,
                    end_column: 3,
                },
            );
        let label = diagnostic.primary.expect("primary label");

        assert_eq!(label.start, start_byte);
        assert_eq!(label.end, u64::MAX);
        assert_eq!(label.start_line, Some(7));
        assert_eq!(label.start_column, Some(11));
        assert_eq!(label.end_line, Some(8));
        assert_eq!(label.end_column, Some(3));
    }
}
