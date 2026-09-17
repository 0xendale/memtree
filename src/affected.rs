//! `affected`: the notes a change touches, and the notes that link to them.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use crate::git;
use crate::note::Note;
use crate::store::{Store, is_note_path};

/// Why a note is reported.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    /// The note is new.
    Added,
    /// The note existed and still does, with different contents.
    Changed,
    /// The note is gone.
    Deleted,
    /// The note links to this name, which a touched note had or has.
    LinksTo(String),
}

/// One reported note. The derived order is by path, then touched before linking, then name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Affected {
    /// Store-relative path, `/`-separated.
    pub path: String,
    /// Why the note is reported.
    pub reason: Reason,
}

impl fmt::Display for Affected {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: ", self.path)?;
        match &self.reason {
            Reason::Added => formatter.write_str("added"),
            Reason::Changed => formatter.write_str("changed"),
            Reason::Deleted => formatter.write_str("deleted"),
            Reason::LinksTo(name) => write!(formatter, "links to [[{name}]]"),
        }
    }
}

/// Why `affected` could not be worked out. It displays as the complete message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffectedError(String);

impl fmt::Display for AffectedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for AffectedError {}

/// The notes of `store`, loaded from `root`, that the change from the merge base of `base` and
/// `HEAD` to the work tree touches, and the notes that link to them, sorted.
///
/// The change includes staged and untracked files and leaves out ignored ones; only paths under
/// `root` count. A path was a note when it was a regular file at a note path (see
/// [`is_note_path`]) at the merge base, and is one when `store` has it. Was and is makes it
/// [`Reason::Changed`] when its contents differ, with the path's Git filters applied; a new
/// modification time or file mode alone touches nothing. Was only makes it [`Reason::Deleted`],
/// and is only [`Reason::Added`]. The touched names are the names the touched notes had at the
/// merge base and have now. Every note in `store` that links to a touched name other than its own
/// is reported with [`Reason::LinksTo`], once per name.
///
/// Git only reads: no command used here refreshes or rewrites the index, and Git never prompts.
///
/// # Errors
///
/// Returns [`AffectedError`] when `base` starts with `-`, when Git cannot run, when `root` is not
/// in a Git work tree, when `base` does not name a commit, when that commit has no merge base with
/// `HEAD`, or when a Git command fails or prints something unexpected.
pub fn affected(root: &Path, store: &Store, base: &str) -> Result<Vec<Affected>, AffectedError> {
    if base.starts_with('-') {
        return Err(AffectedError(format!(
            "a ref cannot start with `-`: `{base}`"
        )));
    }

    let merge_base = merge_base(root, base)?;
    let old_blobs = changed_paths(root, &merge_base)?;

    let notes: BTreeMap<&str, &Note> = store
        .notes
        .iter()
        .map(|note| (note.path.as_str(), note))
        .collect();
    let candidates: Vec<(&str, &str)> = old_blobs
        .iter()
        .filter_map(|(path, old_blob)| {
            let (path, _) = notes.get_key_value(path.as_str())?;
            Some((*path, old_blob.as_deref()?))
        })
        .collect();
    let unchanged = same_contents(root, &candidates)?;

    let mut affected = Vec::new();
    let mut touched_names = BTreeSet::new();
    for (path, old_blob) in old_blobs {
        let note = notes.get(path.as_str());
        let reason = match (&old_blob, note) {
            (Some(_), Some(_)) if unchanged.contains(path.as_str()) => continue,
            (Some(_), Some(_)) => Reason::Changed,
            (Some(_), None) => Reason::Deleted,
            (None, Some(_)) => Reason::Added,
            (None, None) => continue,
        };
        if let Some(blob) = &old_blob {
            touched_names.extend(old_name(root, &path, blob)?);
        }
        if let Some((name, _)) = note.and_then(|note| note.name.as_ref()) {
            touched_names.insert(name.clone());
        }
        affected.push(Affected { path, reason });
    }

    for note in &store.notes {
        let own = note.name.as_ref().map(|(name, _)| name.as_str());
        let targets: BTreeSet<&str> = note
            .links
            .iter()
            .map(|link| link.target.as_str())
            .filter(|target| touched_names.contains(*target) && Some(*target) != own)
            .collect();
        affected.extend(targets.into_iter().map(|target| Affected {
            path: note.path.clone(),
            reason: Reason::LinksTo(target.to_owned()),
        }));
    }
    affected.sort();
    Ok(affected)
}

/// The merge base of `base` and `HEAD`, once `root` is known to be in a Git work tree.
fn merge_base(root: &Path, base: &str) -> Result<String, AffectedError> {
    let inside = run_git(root, &["rev-parse", "--is-inside-work-tree"])?;
    if !inside.success() {
        return Err(AffectedError(format!(
            "`{}` is not in a Git work tree: {}",
            root.display(),
            inside.detail()
        )));
    }
    if inside.stdout.trim_ascii() != b"true" {
        return Err(AffectedError(format!(
            "`{}` is not in a Git work tree",
            root.display()
        )));
    }

    let revision = format!("{base}^{{commit}}");
    let verified = run_git(root, &["rev-parse", "--verify", "--quiet", &revision])?;
    if !verified.success() {
        return Err(AffectedError(format!("`{base}` does not name a commit")));
    }
    let commit = object_id("rev-parse", &verified.stdout)?;

    let merge_base = run_git(root, &["merge-base", &commit, "HEAD"])?;
    if merge_base.code == Some(1) {
        return Err(AffectedError(format!(
            "`{base}` has no merge base with HEAD"
        )));
    }
    object_id("merge-base", &stdout("merge-base", merge_base)?)
}

/// Each path under `root` that differs from `merge_base` in the work tree or is untracked, with
/// its blob at the merge base when it was a note then.
fn changed_paths(
    root: &Path,
    merge_base: &str,
) -> Result<BTreeMap<String, Option<String>>, AffectedError> {
    // Plumbing, not `git diff`: the porcelain rewrites the index to refresh stale stat data, even
    // without optional locks. `diff-index` reports such a file as modified instead, and its
    // contents are compared below.
    let diff = run_git(
        root,
        &[
            "diff-index",
            "--raw",
            "-z",
            "--no-renames",
            "--no-abbrev",
            "--relative",
            "--ignore-submodules=all",
            merge_base,
            "--",
        ],
    )?;
    let changes =
        raw_changes(&stdout("diff-index", diff)?).ok_or_else(|| unexpected("diff-index"))?;
    let others = run_git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    let untracked =
        nul_terminated(&stdout("ls-files", others)?).ok_or_else(|| unexpected("ls-files"))?;

    let mut old_blobs: BTreeMap<String, Option<String>> = BTreeMap::new();
    for change in changes {
        let old_blob = change.old_blob.filter(|_| is_note_path(&change.path));
        let slot = old_blobs.entry(change.path).or_default();
        *slot = slot.take().or(old_blob);
    }
    for path in untracked {
        old_blobs.entry(path).or_default();
    }
    Ok(old_blobs)
}

fn run_git(root: &Path, args: &[&str]) -> Result<git::Output, AffectedError> {
    git::run(root, args).map_err(|error| AffectedError(format!("cannot run git: {error}")))
}

/// The stdout of a Git command that succeeded.
fn stdout(command: &str, output: git::Output) -> Result<Vec<u8>, AffectedError> {
    if output.success() {
        Ok(output.stdout)
    } else {
        Err(AffectedError(format!(
            "git {command} failed: {}",
            output.detail()
        )))
    }
}

fn unexpected(command: &str) -> AffectedError {
    AffectedError(format!("unexpected output from git {command}"))
}

/// The object id that a Git command printed on a line of its own.
fn object_id(command: &str, stdout: &[u8]) -> Result<String, AffectedError> {
    std::str::from_utf8(stdout)
        .ok()
        .and_then(|text| text.strip_suffix('\n'))
        .filter(|id| is_object_id(id))
        .map(str::to_owned)
        .ok_or_else(|| unexpected(command))
}

/// A full SHA-1 or SHA-256 object id.
fn is_object_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Paths per `hash-object` run, which keeps each command line far below the system limit.
const PATHS_PER_HASH: usize = 1000;

/// The paths among `candidates`, each a file now paired with the blob it was, whose contents are
/// still that blob. `hash-object` applies each path's filters, as Git does when it compares a
/// file with stale stat data, and never writes.
fn same_contents<'a>(
    root: &Path,
    candidates: &[(&'a str, &str)],
) -> Result<BTreeSet<&'a str>, AffectedError> {
    let mut same = BTreeSet::new();
    for chunk in candidates.chunks(PATHS_PER_HASH) {
        let mut args = vec!["hash-object", "--"];
        args.extend(chunk.iter().map(|(path, _)| *path));
        let output = stdout("hash-object", run_git(root, &args)?)?;
        let ids: Vec<&str> = std::str::from_utf8(&output)
            .map_err(|_| unexpected("hash-object"))?
            .lines()
            .collect();
        if ids.len() != chunk.len() || !ids.iter().all(|id| is_object_id(id)) {
            return Err(unexpected("hash-object"));
        }
        same.extend(
            chunk
                .iter()
                .zip(ids)
                .filter(|((_, old_blob), id)| old_blob == id)
                .map(|((path, _), _)| *path),
        );
    }
    Ok(same)
}

/// The name in the note that was the blob `blob` at `path`, if it had one.
fn old_name(root: &Path, path: &str, blob: &str) -> Result<Option<String>, AffectedError> {
    let bytes = stdout("cat-file", run_git(root, &["cat-file", "blob", blob])?)?;
    Ok(String::from_utf8(bytes)
        .ok()
        .and_then(|text| Note::parse(path, &text).name)
        .map(|(name, _)| name))
}

/// One changed path from `git diff-index --raw -z --no-renames`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Change {
    /// Root-relative path.
    path: String,
    /// The old blob, when the old side was a regular file.
    old_blob: Option<String>,
}

/// Parses raw diff output, or returns `None` for anything but unpaired records.
///
/// A record is `:old-mode new-mode old-id new-id status`, then the path, each field ending in NUL.
/// Modes are six octal digits, ids are full, and the status is one of `A`, `D`, `M`, `T`, and `U`;
/// a rename or copy, which names two paths, is refused rather than guessed at.
fn raw_changes(output: &[u8]) -> Option<Vec<Change>> {
    let mut fields = nul_terminated(output)?.into_iter();
    let mut changes = Vec::new();
    while let Some(meta) = fields.next() {
        let meta: Vec<&str> = meta.strip_prefix(':')?.split(' ').collect();
        let &[old_mode, new_mode, old_id, new_id, status] = meta.as_slice() else {
            return None;
        };
        let is_mode =
            |mode: &str| mode.len() == 6 && mode.bytes().all(|byte| matches!(byte, b'0'..=b'7'));
        if !(is_mode(old_mode)
            && is_mode(new_mode)
            && is_object_id(old_id)
            && is_object_id(new_id)
            && matches!(status, "A" | "D" | "M" | "T" | "U"))
        {
            return None;
        }
        let path = fields.next().filter(|path| !path.is_empty())?;
        let old_blob = matches!(old_mode, "100644" | "100755").then(|| old_id.to_owned());
        changes.push(Change { path, old_blob });
    }
    Some(changes)
}

/// The NUL-terminated fields of `output`, decoded lossily as store paths are, or `None` when the
/// last field is not terminated.
fn nul_terminated(output: &[u8]) -> Option<Vec<String>> {
    if output.is_empty() {
        return Some(Vec::new());
    }
    Some(
        output
            .strip_suffix(b"\0")?
            .split(|byte| *byte == 0)
            .map(|field| String::from_utf8_lossy(field).into_owned())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::IndexFile;

    const OLD: &str = "1111111111111111111111111111111111111111";
    const NEW: &str = "2222222222222222222222222222222222222222";
    const ZERO: &str = "0000000000000000000000000000000000000000";

    fn change(path: &str, old_blob: Option<&str>) -> Change {
        Change {
            path: path.to_owned(),
            old_blob: old_blob.map(str::to_owned),
        }
    }

    #[test]
    fn raw_records_give_paths_and_the_blobs_of_old_regular_files() {
        let raw = format!(
            ":100644 100644 {OLD} {ZERO} M\0a.md\0\
             :000000 100644 {ZERO} {NEW} A\0sub/b c.md\0\
             :100755 000000 {OLD} {ZERO} D\0x.md\0\
             :120000 100644 {OLD} {ZERO} T\0link.md\0\
             :100644 000000 {OLD} {ZERO} U\0conflict.md\0"
        );
        assert_eq!(
            raw_changes(raw.as_bytes()),
            Some(vec![
                change("a.md", Some(OLD)),
                change("sub/b c.md", None),
                change("x.md", Some(OLD)),
                change("link.md", None),
                change("conflict.md", Some(OLD)),
            ])
        );
        assert_eq!(raw_changes(b""), Some(Vec::new()));
    }

    #[test]
    fn paired_or_malformed_raw_output_is_refused() {
        for raw in [
            format!(":100644 100644 {OLD} {NEW} R100\0old.md\0new.md\0"),
            format!(":100644 100644 {OLD} {NEW} C075\0old.md\0new.md\0"),
            format!(":100644 100644 {OLD} {NEW} X\0a.md\0"),
            format!(":100644 100644 {OLD} {NEW} M\0"),
            format!("100644 100644 {OLD} {NEW} M\0a.md\0"),
            format!(":100644 {OLD} {NEW} M\0a.md\0"),
            format!(":100644 100644 {OLD} {NEW} M\0a.md"),
        ] {
            assert_eq!(raw_changes(raw.as_bytes()), None, "{raw:?}");
        }
    }

    #[test]
    fn a_base_that_looks_like_an_option_is_refused_before_git_runs() {
        let store = Store {
            notes: Vec::new(),
            index: IndexFile::Missing,
            findings: Vec::new(),
        };
        // No Git work tree exists here, so an answer that mentions Git came from running it.
        let root = Path::new("/nonexistent/memtree-root");
        for base in ["-p", "--output=x"] {
            assert_eq!(
                affected(root, &store, base).map_err(|error| error.to_string()),
                Err(format!("a ref cannot start with `-`: `{base}`"))
            );
        }
    }
}
