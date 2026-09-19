# memtree

memtree checks, indexes, and diffs a markdown memory store: a directory tree of notes with
frontmatter, `[[name]]` links between them, and a `MEMORY.md` index with one line per note.

- `memtree check` reports errors and warnings in the store. It writes nothing.
- `memtree index` appends an entry to `MEMORY.md` for each note the index does not list, and keeps
  every existing line.
- `memtree affected --base REF` lists the notes added, changed, or deleted since the merge base of
  `REF` and `HEAD`, and the notes that link to them. It writes nothing.
- `memtree links [NAME...]` names every note, or lists the note lines that link to a given name.
  It writes nothing.

## Example

A store with two notes and an index that lists one of them:

```text
MEMORY.md
ci-gates.md
release-checklist.md
```

`release-checklist.md`:

```markdown
---
name: release-checklist
description: Steps to cut a release, in order
metadata:
  type: project
---
Run the checks in [[ci-gates]] first, then follow [[deploy-notes]].
```

`MEMORY.md`:

```markdown
# Memory

- [release-checklist](release-checklist.md) — Steps to cut a release, in order
```

Results go to stdout. The summary line, which starts with `memtree:`, goes to stderr.

```console
$ memtree check
ci-gates.md: error: orphan: note is not listed in MEMORY.md
release-checklist.md:7: warning: dangling-link: no note is named `deploy-notes`
memtree: checked 2 notes: 1 error, 1 warning
$ memtree index
- [ci-gates](ci-gates.md) — The gates every change must pass
memtree: indexed 1 note in MEMORY.md
$ memtree check
release-checklist.md:7: warning: dangling-link: no note is named `deploy-notes`
memtree: checked 2 notes: 0 errors, 1 warning
```

After the store is committed on `main` and the description in `ci-gates.md` is edited:

```console
$ memtree affected --base main
ci-gates.md: changed
release-checklist.md: links to [[ci-gates]]
memtree: since the merge base with `main`: 1 touched note, 1 linking note
```

## Store format

**Notes.** Every file under the root whose extension is exactly `md` is a note, except files named
`MEMORY.md`. Files and directories whose names start with `.` are skipped. Symbolic links are
reported and not followed; the root itself may be a symbolic link to a directory.

**Frontmatter.** A note starts with a `---` line, and its frontmatter ends at the next `---` line.
The syntax is a small, line-based relative of YAML, not YAML itself:

- Each line is `key: value`. A key is ASCII letters, digits, `_`, and `-`, and a space follows the
  colon.
- A key with no value can hold one level of nested `key: value` lines, all indented by the same
  number of spaces. Tabs in indentation are an error.
- A value in double quotes allows only the escapes `\"` and `\\`. In single quotes, `''` stands for
  `'`. Text after the closing quote is an error.
- Any other value is taken literally to the end of the line, so `#` and `: ` need no quoting.
  Values starting with `|`, `>`, `[`, or `{` are errors: block scalars and flow collections are not
  supported.
- Blank lines and comment lines, whose first character after any indentation is `#`, are ignored.
  A key given twice is an error.
- A UTF-8 byte order mark and CRLF line endings are accepted.

The fields:

| Field | Rule |
|---|---|
| `name` | Required. Lowercase ASCII letters and digits in groups joined by single hyphens. Unique in the store. |
| `description` | Required. Not blank. |
| `metadata` | Optional. Nested keys, not a plain value. |
| `metadata.type` | Optional. One of `user`, `feedback`, `project`, or `reference`. |

Other keys are allowed and ignored.

**Links.** A link is `[[`, then text containing no `[` or `]`, then `]]`, anywhere in the body. A
note without frontmatter is all body; a note whose frontmatter is unterminated has no links. Links
are matched left to right without overlapping, and their names are trimmed. Links inside
fenced code blocks and inline code spans do not count. A link to a name that no note has is a
warning, not an error: it can mark a note worth writing later.

**Index.** `MEMORY.md` at the root. An entry is a line that starts with `- [`, optionally indented,
and continues with `](path.md)`, where the path is relative to the root and a leading `./` is
ignored. A target containing `://` is a URL, not an entry. Every other line is free text, which
memtree keeps and does not read.

## Findings

A finding prints as `path:line: severity: code: message`, without `:line` when it concerns the
whole file. Findings are sorted by path, then line (whole-file findings first), code, and message.

| Code | Severity | Meaning |
|---|---|---|
| `encoding` | error | A note or the index is not valid UTF-8. |
| `frontmatter-missing` | error | A note does not start with a `---` line. |
| `frontmatter-unterminated` | error | The frontmatter has no closing `---` line. |
| `frontmatter-syntax` | error | A frontmatter line is outside the syntax above. Only the first is reported. |
| `name-missing` | error | There is no `name`, or it is empty. |
| `name-invalid` | error | The `name` is not lowercase letters and digits joined by single hyphens. |
| `description-missing` | error | There is no `description`, or it is blank. |
| `metadata-invalid` | error | `metadata` is a plain value instead of nested keys. |
| `type-invalid` | error | `metadata.type` is not one of the four types. |
| `duplicate-name` | error | Two or more notes share a name. |
| `index-missing` | error | The root has no `MEMORY.md`. |
| `orphan` | error | A note is not listed in the index. |
| `stale-index-entry` | error | An index entry points at no note. |
| `duplicate-index-entry` | error | A note is listed in the index more than once. |
| `path-unindexable` | error | Reported by `index` only: a note's path contains `)` or a line break, so no entry can point at it. |
| `dangling-link` | warning | A link names no note. |
| `symlink` | warning | A symbolic link was not followed. |

## Commands

`check`, `index`, `affected`, and `links` take `--root DIR`, the store root, which defaults to the
current directory. An option can be written `--name VALUE` or `--name=VALUE`, at most once. `links`
also takes NAME arguments, which never start with `-`. `memtree help` prints the usage and these
options; `memtree --version` prints the version.

### `memtree check [--root DIR]`

Prints every finding to stdout, then `memtree: checked N notes: E errors, W warnings` to stderr.
It writes nothing. The exit status is 1 when there is at least one error; warnings alone exit 0.

### `memtree index [--root DIR]`

Appends `- [name](path) — description` for each note the index does not list, in path order, and
prints the appended lines to stdout. The description is trimmed.

- Existing text is kept byte for byte, stale and duplicate entries included. `check` reports them;
  removing them is left to a person.
- If the index has text that does not end in a line break, one is added first. Appended lines use
  the line ending of the index's first line break, or `\n` when it has none.
- A missing `MEMORY.md` is created, even for a store with no notes.
- When every note is already listed, nothing is written and stderr says
  `memtree: MEMORY.md is up to date`.
- The update is all or nothing. When the index is not UTF-8, when an unlisted note has errors, or
  when an unlisted note's path cannot be written as an entry, nothing is written: the blocking
  findings go to stdout, stderr says `memtree: MEMORY.md not changed: N errors`, and the exit status
  is 1. Notes that are already listed never block the update.
- The write is atomic: a hidden temporary file in the root takes the existing index's permissions
  and is renamed over `MEMORY.md`.

### `memtree affected --base REF [--root DIR]`

Compares the merge base of `REF` and `HEAD` with the work tree. Committed, staged, and unstaged
changes count, and so do untracked files that are not ignored. Only paths under the root count,
and paths print relative to it.

- A touched note prints as `path: added`, `path: changed`, or `path: deleted`. A note is `changed`
  only when its contents differ once its Git filters are applied; a new modification time or file
  mode alone touches nothing. Renames are not detected, so a renamed note is deleted and added.
- The touched names are the names the touched notes had at the merge base and have now. Every note
  that links to a touched name other than its own prints as `path: links to [[name]]`, once per
  name.
- Lines are sorted by path, touched before linking, then by name. stderr says
  ``memtree: since the merge base with `REF`: N touched notes, M linking notes``.
- It only reads. The Git commands it runs never refresh or rewrite the Git index, and Git never
  prompts.
- A `REF` starting with `-` is a usage error. The exit status is 3 when the root is not in a Git
  work tree, when `REF` does not name a commit, or when it has no merge base with `HEAD`.

### `memtree links [--root DIR] [NAME...]`

Without NAME arguments, prints `name path` for every note that has a name, sorted by name. With
one or more NAME arguments, prints for each given name every note path and 1-based line that
holds a `[[name]]` link to it, as `path:line: links to [[name]]`, sorted by path then line, once
per path, line, and name. A name nothing links to produces no lines and is not an error. Links
inside fenced code blocks and inline code spans do not count, exactly as `check` counts them.

It writes nothing. stderr ends with `memtree: links over N notes: M links`, where N is the number
of notes and M is the number of links found in the store when no NAME is given, and the number of
printed lines when NAMEs are given. The exit status is 0 on success, 2 on a usage error, and 3 on
failure.

## Exit status

| Status | Meaning |
|---|---|
| 0 | Success. |
| 1 | The store has errors (`check`), or they blocked the update (`index`). |
| 2 | Usage error. The reason and the usage go to stderr. |
| 3 | Failure: the store cannot be read, a write fails, Git fails, or output cannot be written. |

## Build and test

memtree has no dependencies. It needs the Rust toolchain pinned in `rust-toolchain.toml`, and Git
for `affected` and the tests.

```sh
cargo build --release
cargo test
```

`.github/workflows/ci.yml` lists every check that CI runs.
