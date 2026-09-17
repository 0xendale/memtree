//! Findings: the problems `check` reports.

use std::fmt;

/// How serious a finding is. Errors fail `check`; warnings do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Fails `check`.
    Error,
    /// Reported without failing `check`.
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Error => "error",
            Self::Warning => "warning",
        })
    }
}

/// What kind of problem a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// A note or the index is not valid UTF-8.
    Encoding,
    /// A note does not start with a frontmatter block.
    FrontmatterMissing,
    /// A frontmatter block has no closing delimiter.
    FrontmatterUnterminated,
    /// A frontmatter line is outside the accepted syntax.
    FrontmatterSyntax,
    /// The frontmatter has no `name`, or it is empty.
    NameMissing,
    /// The `name` is not a kebab-case name.
    NameInvalid,
    /// The frontmatter has no `description`, or it is blank.
    DescriptionMissing,
    /// `metadata` is a plain value instead of nested keys.
    MetadataInvalid,
    /// `metadata.type` is not a memory type.
    TypeInvalid,
    /// Two or more notes share a name.
    DuplicateName,
    /// The store root has no index.
    IndexMissing,
    /// A note is not listed in the index.
    Orphan,
    /// An index entry points at no note.
    StaleIndexEntry,
    /// A note is listed in the index more than once.
    DuplicateIndexEntry,
    /// A note's path cannot be written as an index entry.
    PathUnindexable,
    /// A link names no note.
    DanglingLink,
    /// A symbolic link was not followed.
    Symlink,
}

impl Code {
    /// The code as printed.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Encoding => "encoding",
            Self::FrontmatterMissing => "frontmatter-missing",
            Self::FrontmatterUnterminated => "frontmatter-unterminated",
            Self::FrontmatterSyntax => "frontmatter-syntax",
            Self::NameMissing => "name-missing",
            Self::NameInvalid => "name-invalid",
            Self::DescriptionMissing => "description-missing",
            Self::MetadataInvalid => "metadata-invalid",
            Self::TypeInvalid => "type-invalid",
            Self::DuplicateName => "duplicate-name",
            Self::IndexMissing => "index-missing",
            Self::Orphan => "orphan",
            Self::StaleIndexEntry => "stale-index-entry",
            Self::DuplicateIndexEntry => "duplicate-index-entry",
            Self::PathUnindexable => "path-unindexable",
            Self::DanglingLink => "dangling-link",
            Self::Symlink => "symlink",
        }
    }

    /// Whether findings with this code are errors or warnings.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::DanglingLink | Self::Symlink => Severity::Warning,
            _ => Severity::Error,
        }
    }
}

/// One problem, in a store-relative file and optionally at a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Store-relative path, `/`-separated.
    pub path: String,
    /// 1-based line, or `None` for a problem with the whole file.
    pub line: Option<usize>,
    /// What kind of problem it is.
    pub code: Code,
    /// What is wrong, for a person to read.
    pub message: String,
}

impl Finding {
    /// Creates a finding.
    #[must_use]
    pub fn new(
        path: impl Into<String>,
        line: Option<usize>,
        code: Code,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            line,
            code,
            message: message.into(),
        }
    }

    /// The finding's severity, which its code decides.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.code.severity()
    }
}

/// `path:line: severity: code: message`, without `:line` for a whole-file finding.
impl fmt::Display for Finding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.path)?;
        if let Some(line) = self.line {
            write!(formatter, ":{line}")?;
        }
        write!(
            formatter,
            ": {}: {}: {}",
            self.severity(),
            self.code.as_str(),
            self.message
        )
    }
}

/// Sorts findings by path, then line (whole-file findings first), code, and message.
pub fn sort(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        (a.path.as_str(), a.line, a.code.as_str(), a.message.as_str()).cmp(&(
            b.path.as_str(),
            b.line,
            b.code.as_str(),
            b.message.as_str(),
        ))
    });
}
