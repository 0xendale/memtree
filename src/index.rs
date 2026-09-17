//! The index, `MEMORY.md`: one list line per note, and the append-only update that `index` makes.

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::path::Path;
use std::process;

use crate::finding::{self, Code, Finding};
use crate::store::{INDEX_FILE, IndexFile, Store, is_note_file_name};

/// An index line that points at a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 1-based line.
    pub line: usize,
    /// The store-relative path the line points at, without a leading `./`.
    pub target: String,
}

/// The entries in index text: lines that start with `- [` (indentation allowed) and continue
/// with `](target)`, where the target is a relative path with the extension `md`, matched
/// exactly as note files are. Other lines are free text and are ignored.
#[must_use]
pub fn entries(text: &str) -> Vec<Entry> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let (_, rest) = line.trim_start().strip_prefix("- [")?.split_once("](")?;
            let (target, _) = rest.split_once(')')?;
            let target = target.strip_prefix("./").unwrap_or(target);
            (is_note_file_name(target) && !target.contains("://")).then(|| Entry {
                line: index + 1,
                target: target.to_owned(),
            })
        })
        .collect()
}

/// The entry `index` appends for a note: `- [name](path) — description`.
#[must_use]
pub fn entry_line(name: &str, path: &str, description: &str) -> String {
    format!("- [{name}]({path}) — {}", description.trim())
}

/// What `index` does to a store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// Every note is listed; nothing is written.
    UpToDate,
    /// Write `text` as the index.
    Write {
        /// The whole new index: the existing bytes followed by the appended entries.
        text: String,
        /// The appended entries, in path order, without line endings.
        appended: Vec<String>,
        /// Whether the index is new.
        created: bool,
    },
    /// Nothing is written because of these errors, sorted.
    Blocked(Vec<Finding>),
}

/// Plans the append-only update of `store`'s index.
///
/// Every unindexed note gets an entry, appended in path order after the existing text, which is
/// kept byte for byte. When the text does not end in a line break, one is added first. Appended
/// lines use the line ending of the text's first line break, or `\n` when it has none. A missing
/// index is created, even for a store with no notes.
///
/// The update is all or nothing: it is [`Update::Blocked`] when the index is not UTF-8, when an
/// unindexed note has errors, or when an unindexed note's path cannot round-trip through an entry.
/// Notes that are already listed never block it.
#[must_use]
pub fn update(store: &Store) -> Update {
    let existing = match &store.index {
        IndexFile::Missing => None,
        IndexFile::NotUtf8 => {
            return Update::Blocked(vec![Finding::new(
                INDEX_FILE,
                None,
                Code::Encoding,
                "index is not valid UTF-8",
            )]);
        }
        IndexFile::Text(text) => Some(text.as_str()),
    };
    let listed: BTreeSet<String> = existing
        .map(entries)
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.target)
        .collect();

    let mut findings = Vec::new();
    let mut appended = Vec::new();
    for note in store
        .notes
        .iter()
        .filter(|note| !listed.contains(&note.path))
    {
        match (&note.name, &note.description) {
            (Some((name, _)), Some(description)) if note.is_valid() => {
                let line = entry_line(name, &note.path, description);
                let target = Entry {
                    line: 1,
                    target: note.path.clone(),
                };
                if entries(&line) == [target] {
                    appended.push(line);
                } else {
                    findings.push(Finding::new(
                        note.path.as_str(),
                        None,
                        Code::PathUnindexable,
                        "an index entry cannot point at a path containing `)` or a line break",
                    ));
                }
            }
            _ => findings.extend(note.findings.iter().cloned()),
        }
    }

    if !findings.is_empty() {
        finding::sort(&mut findings);
        return Update::Blocked(findings);
    }
    if existing.is_some() && appended.is_empty() {
        return Update::UpToDate;
    }

    let mut text = existing.unwrap_or_default().to_owned();
    let newline = match text.split_once('\n') {
        Some((first, _)) if first.ends_with('\r') => "\r\n",
        _ => "\n",
    };
    if !text.is_empty() && !text.ends_with('\n') {
        text.push_str(newline);
    }
    for line in &appended {
        text.push_str(line);
        text.push_str(newline);
    }
    Update::Write {
        text,
        appended,
        created: existing.is_none(),
    }
}

/// Why the index could not be written. It displays as the complete message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteError(String);

impl fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for WriteError {}

/// Replaces the index at `root` with `text` atomically.
///
/// The text goes to a hidden temporary file in the root, created exclusively, which takes the
/// existing index's permissions, is synced, and is renamed over the index. If a step after its
/// creation fails, the temporary file is removed.
///
/// # Errors
///
/// Returns [`WriteError`] when any step fails; the index is then unchanged.
pub fn write(root: &Path, text: &str) -> Result<(), WriteError> {
    let index = root.join(INDEX_FILE);
    let permissions = match fs::symlink_metadata(&index) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(cannot_write(INDEX_FILE, &error)),
    };
    let temporary_name = format!(".memtree-index-{}.tmp", process::id());
    let temporary = root.join(&temporary_name);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| cannot_write(&temporary_name, &error))?;
    let written = fill(file, text, permissions)
        .map_err(|error| cannot_write(&temporary_name, &error))
        .and_then(|()| {
            fs::rename(&temporary, &index).map_err(|error| cannot_write(INDEX_FILE, &error))
        });
    if written.is_err() {
        // The write already failed; a temporary file that cannot be removed changes nothing more.
        let _ = fs::remove_file(&temporary);
    }
    written
}

fn fill(mut file: File, text: &str, permissions: Option<Permissions>) -> io::Result<()> {
    file.write_all(text.as_bytes())?;
    if let Some(permissions) = permissions {
        file.set_permissions(permissions)?;
    }
    file.sync_all()
}

fn cannot_write(shown: &str, error: &io::Error) -> WriteError {
    WriteError(format!("cannot write `{shown}`: {error}"))
}
