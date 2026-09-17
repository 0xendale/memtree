//! Loading a store: walking its root for notes and reading its index.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use crate::finding::{Code, Finding};
use crate::note::Note;

/// The index file, which lives at the store root.
pub const INDEX_FILE: &str = "MEMORY.md";

/// Whether a file name, or a path ending in one, has the note extension `md`.
///
/// The match is exact, as the match on [`INDEX_FILE`] is: `notes.MD` is not a note, so a store
/// reads the same on case-sensitive and case-insensitive file systems.
#[must_use]
pub fn is_note_file_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension == "md")
}

/// Whether a store-relative, `/`-separated path names a note: no component starts with `.`, and
/// the file name has the note extension and is not `MEMORY.md`. These are the files [`load`] reads
/// as notes.
#[must_use]
pub fn is_note_path(path: &str) -> bool {
    !path.split('/').any(|component| component.starts_with('.'))
        && path
            .rsplit('/')
            .next()
            .is_some_and(|name| is_note_file_name(name) && name != INDEX_FILE)
}

/// The state of the root index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexFile {
    /// The root has no index.
    Missing,
    /// The index exists but is not UTF-8.
    NotUtf8,
    /// The index's text.
    Text(String),
}

/// A loaded store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    /// Every note, sorted by path.
    pub notes: Vec<Note>,
    /// The root index.
    pub index: IndexFile,
    /// Problems found while walking: symbolic links, which are never followed.
    pub findings: Vec<Finding>,
}

/// Why a store could not be loaded. It displays as the complete message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError(String);

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for LoadError {}

/// Loads the store rooted at `root`.
///
/// Notes are the files under the root with the extension `md` (see [`is_note_file_name`]) other
/// than files named `MEMORY.md`; the root `MEMORY.md` is the index. Entries whose names start with
/// `.` are skipped, and symbolic links are reported and not followed. `root` itself may be a
/// symbolic link to a directory. A file name that is not UTF-8 is shown with replacement
/// characters but still read.
///
/// # Errors
///
/// Returns [`LoadError`] when the root is not a readable directory, when a directory or note in
/// it cannot be read, or when the root `MEMORY.md` is not a regular file.
pub fn load(root: &Path) -> Result<Store, LoadError> {
    let metadata = fs::metadata(root).map_err(|error| cannot_read(root.display(), &error))?;
    if !metadata.is_dir() {
        return Err(LoadError(format!(
            "`{}` is not a directory",
            root.display()
        )));
    }
    let mut store = Store {
        notes: Vec::new(),
        index: IndexFile::Missing,
        findings: Vec::new(),
    };
    walk(root, Path::new(""), "", &mut store)?;
    store.notes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(store)
}

/// Walks `dir`, which is relative to `root` and shown as the store-relative `shown` (empty for
/// the root itself).
fn walk(root: &Path, dir: &Path, shown: &str, store: &mut Store) -> Result<(), LoadError> {
    let unreadable = |error: io::Error| {
        if shown.is_empty() {
            cannot_read(root.display(), &error)
        } else {
            cannot_read(shown, &error)
        }
    };
    let mut entries = Vec::new();
    for entry in fs::read_dir(root.join(dir)).map_err(unreadable)? {
        let entry = entry.map_err(unreadable)?;
        let file_type = entry.file_type().map_err(unreadable)?;
        entries.push((entry.file_name(), file_type));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    for (file_name, file_type) in entries {
        let name = file_name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        let relative = dir.join(&file_name);
        let path = if shown.is_empty() {
            name.to_string()
        } else {
            format!("{shown}/{name}")
        };
        let is_index = path == INDEX_FILE;
        if is_index && !file_type.is_file() {
            return Err(LoadError(format!("`{INDEX_FILE}` is not a regular file")));
        }
        if file_type.is_symlink() {
            store.findings.push(Finding::new(
                path,
                None,
                Code::Symlink,
                "symbolic link not followed",
            ));
        } else if file_type.is_dir() {
            walk(root, &relative, &path, store)?;
        } else if is_index {
            store.index = String::from_utf8(read(root, &relative, &path)?)
                .map_or(IndexFile::NotUtf8, IndexFile::Text);
        } else if file_type.is_file() && is_note_file_name(&name) && name != INDEX_FILE {
            let note = match String::from_utf8(read(root, &relative, &path)?) {
                Ok(text) => Note::parse(&path, &text),
                Err(_) => Note::not_utf8(&path),
            };
            store.notes.push(note);
        }
    }
    Ok(())
}

fn read(root: &Path, relative: &Path, shown: &str) -> Result<Vec<u8>, LoadError> {
    fs::read(root.join(relative)).map_err(|error| cannot_read(shown, &error))
}

fn cannot_read(shown: impl fmt::Display, error: &io::Error) -> LoadError {
    LoadError(format!("cannot read `{shown}`: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_paths_are_visible_md_files_not_named_like_the_index() {
        for path in ["a.md", "sub/b c.md", "x.y.md", "sub/deeper/MEMORY.md.md"] {
            assert!(is_note_path(path), "{path}");
        }
        for path in [
            "",
            "a",
            "md",
            "a.MD",
            "a.txt",
            ".a.md",
            "sub/.a.md",
            ".hidden/a.md",
            "sub/.hidden/a.md",
            "MEMORY.md",
            "sub/MEMORY.md",
            "sub/",
        ] {
            assert!(!is_note_path(path), "{path}");
        }
    }
}
