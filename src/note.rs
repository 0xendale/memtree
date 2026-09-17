//! One note: its frontmatter fields, its links, and the problems it has on its own.

use crate::finding::{Code, Finding, Severity};
use crate::frontmatter::{self, Entry, Frontmatter, FrontmatterError, Value};
use crate::links::{self, Link};

/// The memory types `metadata.type` may name.
pub const TYPES: [&str; 4] = ["user", "feedback", "project", "reference"];

/// A parsed note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Store-relative path, `/`-separated.
    pub path: String,
    /// The `name` and its line, whenever it is a non-empty single value, even an invalid one.
    pub name: Option<(String, usize)>,
    /// The `description`, when it is a single value that is not blank.
    pub description: Option<String>,
    /// Links in the body. A note without frontmatter is all body; an unterminated block has none.
    pub links: Vec<Link>,
    /// Problems with this note alone, all of them errors.
    pub findings: Vec<Finding>,
}

impl Note {
    /// Parses the note at store-relative `path` whose contents are `text`.
    #[must_use]
    pub fn parse(path: &str, text: &str) -> Self {
        let mut note = Self::empty(path);
        match frontmatter::parse(text) {
            Ok(frontmatter) => {
                note.read_fields(&frontmatter);
                note.links = links::extract(text, frontmatter.body_start);
            }
            Err(FrontmatterError::Missing) => {
                note.error(
                    None,
                    Code::FrontmatterMissing,
                    "note does not start with a `---` frontmatter block",
                );
                note.links = links::extract(text, 0);
            }
            Err(FrontmatterError::Unterminated) => note.error(
                Some(1),
                Code::FrontmatterUnterminated,
                "frontmatter block has no closing `---`",
            ),
            Err(FrontmatterError::Syntax {
                line,
                message,
                body_start,
            }) => {
                note.error(Some(line), Code::FrontmatterSyntax, message);
                note.links = links::extract(text, body_start);
            }
        }
        note
    }

    /// A note whose bytes are not UTF-8, so nothing in it can be read.
    #[must_use]
    pub fn not_utf8(path: &str) -> Self {
        let mut note = Self::empty(path);
        note.error(None, Code::Encoding, "note is not valid UTF-8");
        note
    }

    /// Whether the note has no errors.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.findings
            .iter()
            .all(|finding| finding.severity() != Severity::Error)
    }

    fn empty(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            name: None,
            description: None,
            links: Vec::new(),
            findings: Vec::new(),
        }
    }

    fn error(&mut self, line: Option<usize>, code: Code, message: impl Into<String>) {
        self.findings
            .push(Finding::new(self.path.as_str(), line, code, message));
    }

    fn read_fields(&mut self, frontmatter: &Frontmatter) {
        match frontmatter.entries.get("name") {
            None => self.error(None, Code::NameMissing, "frontmatter has no `name`"),
            Some(Entry {
                line,
                value: Value::Map(_),
            }) => self.error(
                Some(*line),
                Code::NameInvalid,
                "`name` must be a single value",
            ),
            Some(Entry {
                line,
                value: Value::Scalar(name),
            }) => {
                if name.is_empty() {
                    self.error(Some(*line), Code::NameMissing, "`name` is empty");
                } else {
                    if !is_kebab_case(name) {
                        self.error(
                            Some(*line),
                            Code::NameInvalid,
                            format!(
                                "`{name}` is not a kebab-case name (lowercase letters and digits joined by single hyphens)"
                            ),
                        );
                    }
                    self.name = Some((name.clone(), *line));
                }
            }
        }

        match frontmatter.entries.get("description") {
            None => self.error(
                None,
                Code::DescriptionMissing,
                "frontmatter has no `description`",
            ),
            Some(Entry {
                line,
                value: Value::Map(_),
            }) => self.error(
                Some(*line),
                Code::DescriptionMissing,
                "`description` must be a single value",
            ),
            Some(Entry {
                line,
                value: Value::Scalar(description),
            }) => {
                if description.trim().is_empty() {
                    self.error(
                        Some(*line),
                        Code::DescriptionMissing,
                        "`description` is empty",
                    );
                } else {
                    self.description = Some(description.clone());
                }
            }
        }

        match frontmatter.entries.get("metadata") {
            Some(Entry {
                line,
                value: Value::Scalar(value),
            }) if !value.is_empty() => self.error(
                Some(*line),
                Code::MetadataInvalid,
                "`metadata` must hold indented keys such as `type`",
            ),
            Some(Entry {
                value: Value::Map(metadata),
                ..
            }) => {
                if let Some((line, kind)) = metadata.get("type") {
                    let expected = TYPES.join(", ");
                    if kind.is_empty() {
                        self.error(
                            Some(*line),
                            Code::TypeInvalid,
                            format!("`metadata.type` is empty; expected one of {expected}"),
                        );
                    } else if !TYPES.contains(&kind.as_str()) {
                        self.error(
                            Some(*line),
                            Code::TypeInvalid,
                            format!("`{kind}` is not a memory type; expected one of {expected}"),
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

/// Lowercase ASCII letters and digits, in groups joined by single hyphens.
fn is_kebab_case(name: &str) -> bool {
    name.split('-').all(|group| {
        !group.is_empty()
            && group
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}
