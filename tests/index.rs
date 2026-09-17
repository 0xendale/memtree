#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! `memtree index`: append-only, all-or-nothing, atomic updates to `MEMORY.md`.

mod support;

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use support::{Output, TempDir, memtree, note};

fn index(store: &TempDir) -> Output {
    memtree(store.path(), &["index"])
}

fn assert_output(output: &Output, code: i32, stdout: &str, stderr: &str) {
    assert_eq!(output.stdout, stdout, "stdout");
    assert_eq!(output.stderr, stderr, "stderr");
    assert_eq!(output.code, code, "exit code");
}

/// The names in the store root, hidden ones included, sorted.
fn root_names(store: &TempDir) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(store.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn unindexed_notes_are_appended_in_path_order_after_every_existing_line() {
    let store = TempDir::create();
    let existing = "# Memory\n\nNotes worth keeping.\n- [b](./b.md) — kept as written\n- [gone](gone.md) — stale, left for a person to remove\n";
    store.write("MEMORY.md", existing);
    store.write("b.md", note("b", "second", ""));
    store.write("z.md", note("z", "'  last  '", ""));
    store.write("a.md", note("a", "first", ""));
    store.write("sub/c.md", note("c", "third: with a colon", ""));

    let appended =
        "- [a](a.md) — first\n- [c](sub/c.md) — third: with a colon\n- [z](z.md) — last\n";
    assert_output(
        &index(&store),
        0,
        appended,
        "memtree: indexed 3 notes in MEMORY.md\n",
    );
    let updated = format!("{existing}{appended}");
    assert_eq!(store.read("MEMORY.md"), updated.as_bytes());

    assert_output(&index(&store), 0, "", "memtree: MEMORY.md is up to date\n");
    assert_eq!(store.read("MEMORY.md"), updated.as_bytes());

    assert_output(
        &memtree(store.path(), &["check"]),
        1,
        "MEMORY.md:5: error: stale-index-entry: `gone.md` is not a note in the store\n",
        "memtree: checked 4 notes: 1 error, 0 warnings\n",
    );
}

#[test]
fn a_missing_index_is_created_even_for_an_empty_store() {
    let store = TempDir::create();
    store.write("b.md", note("b", "second", ""));
    store.write("a.md", note("a", "first", "See [[b]].\n"));
    let elsewhere = TempDir::create();

    let lines = "- [a](a.md) — first\n- [b](b.md) — second\n";
    assert_output(
        &memtree(
            elsewhere.path(),
            &["index", "--root", store.path().to_str().unwrap()],
        ),
        0,
        lines,
        "memtree: created MEMORY.md with 2 notes\n",
    );
    assert_eq!(store.read("MEMORY.md"), lines.as_bytes());
    assert_output(
        &memtree(store.path(), &["check"]),
        0,
        "",
        "memtree: checked 2 notes: 0 errors, 0 warnings\n",
    );

    let empty = TempDir::create();
    assert_output(
        &memtree(
            elsewhere.path(),
            &["index", &format!("--root={}", empty.path().display())],
        ),
        0,
        "",
        "memtree: created MEMORY.md with 0 notes\n",
    );
    assert_eq!(empty.read("MEMORY.md"), b"");
    assert_output(
        &memtree(empty.path(), &["check"]),
        0,
        "",
        "memtree: checked 0 notes: 0 errors, 0 warnings\n",
    );
}

#[test]
fn an_up_to_date_index_is_left_untouched() {
    let store = TempDir::create();
    store.write("a.md", note("a", "first", ""));
    store.write("MEMORY.md", "- [a](a.md) — first");
    let inode = fs::metadata(store.path().join("MEMORY.md")).unwrap().ino();

    fs::set_permissions(store.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let output = index(&store);
    fs::set_permissions(store.path(), fs::Permissions::from_mode(0o755)).unwrap();

    assert_output(&output, 0, "", "memtree: MEMORY.md is up to date\n");
    assert_eq!(store.read("MEMORY.md"), "- [a](a.md) — first".as_bytes());
    assert_eq!(
        fs::metadata(store.path().join("MEMORY.md")).unwrap().ino(),
        inode
    );

    let empty = TempDir::create();
    empty.write("MEMORY.md", "");
    assert_output(&index(&empty), 0, "", "memtree: MEMORY.md is up to date\n");
    assert_eq!(root_names(&empty), ["MEMORY.md"]);
}

#[test]
fn errors_in_unindexed_notes_block_every_write() {
    let store = TempDir::create();
    let existing = "- [broken](broken.md) — indexed notes with errors do not block\n";
    store.write("MEMORY.md", existing);
    store.write("broken.md", "no frontmatter\n");
    store.write("a.md", note("a", "valid, but held back with the rest", ""));
    store.write("b.md", "---\nname: Bad_Name\ndescription: d\n---\n");
    store.write("c.md", "---\nname: c\n---\n");

    assert_output(
        &index(&store),
        1,
        "b.md:2: error: name-invalid: `Bad_Name` is not a kebab-case name (lowercase letters and digits joined by single hyphens)\n\
         c.md: error: description-missing: frontmatter has no `description`\n",
        "memtree: MEMORY.md not changed: 2 errors\n",
    );
    assert_eq!(store.read("MEMORY.md"), existing.as_bytes());

    let unindexed = TempDir::create();
    unindexed.write("a.md", note("a", "first", ""));
    unindexed.write("b.md", "no frontmatter\n");
    assert_output(
        &index(&unindexed),
        1,
        "b.md: error: frontmatter-missing: note does not start with a `---` frontmatter block\n",
        "memtree: MEMORY.md not changed: 1 error\n",
    );
    assert_eq!(root_names(&unindexed), ["a.md", "b.md"]);
}

#[test]
fn appended_lines_follow_the_index_line_endings() {
    let cases = [
        (
            "- [a](a.md) — first",
            "- [a](a.md) — first\n- [b](b.md) — second\n",
        ),
        (
            "# Memory\r\n- [a](a.md) — first\r\n",
            "# Memory\r\n- [a](a.md) — first\r\n- [b](b.md) — second\r\n",
        ),
        (
            "# Memory\r\n- [a](a.md) — first",
            "# Memory\r\n- [a](a.md) — first\r\n- [b](b.md) — second\r\n",
        ),
        (
            "\u{feff}- [a](a.md) — first\n",
            "\u{feff}- [a](a.md) — first\n- [b](b.md) — second\n",
        ),
    ];
    for (before, after) in cases {
        let store = TempDir::create();
        store.write("MEMORY.md", before);
        store.write("a.md", note("a", "first", ""));
        store.write("b.md", note("b", "second", ""));

        assert_output(
            &index(&store),
            0,
            "- [b](b.md) — second\n",
            "memtree: indexed 1 note in MEMORY.md\n",
        );
        assert_eq!(
            String::from_utf8(store.read("MEMORY.md")).unwrap(),
            after,
            "index before: {before:?}"
        );
    }
}

#[test]
fn an_index_that_is_not_utf8_blocks_the_write() {
    let store = TempDir::create();
    store.write("MEMORY.md", b"- [a](a.md) \xff\n");
    store.write("a.md", note("a", "first", ""));
    store.write("b.md", note("b", "second", ""));

    assert_output(
        &index(&store),
        1,
        "MEMORY.md: error: encoding: index is not valid UTF-8\n",
        "memtree: MEMORY.md not changed: 1 error\n",
    );
    assert_eq!(store.read("MEMORY.md"), b"- [a](a.md) \xff\n");
}

#[test]
fn a_path_no_entry_can_point_at_blocks_the_write() {
    let store = TempDir::create();
    store.write("a.md", note("a", "first", ""));
    store.write("draft (old).md", note("draft-old", "superseded", ""));

    assert_output(
        &index(&store),
        1,
        "draft (old).md: error: path-unindexable: an index entry cannot point at a path containing `)` or a line break\n",
        "memtree: MEMORY.md not changed: 1 error\n",
    );
    assert_eq!(root_names(&store), ["a.md", "draft (old).md"]);
}

#[test]
fn the_rewritten_index_keeps_its_permissions() {
    let store = TempDir::create();
    store.write("MEMORY.md", "- [a](a.md) — first\n");
    store.write("a.md", note("a", "first", ""));
    store.write("b.md", note("b", "second", ""));
    let index_path = store.path().join("MEMORY.md");
    // No common umask produces this mode, so only a copied mode can match it.
    fs::set_permissions(&index_path, fs::Permissions::from_mode(0o604)).unwrap();

    assert_output(
        &index(&store),
        0,
        "- [b](b.md) — second\n",
        "memtree: indexed 1 note in MEMORY.md\n",
    );
    assert_eq!(
        fs::metadata(&index_path).unwrap().permissions().mode() & 0o7777,
        0o604
    );
    assert_eq!(root_names(&store), ["MEMORY.md", "a.md", "b.md"]);
}

#[test]
fn a_failed_write_changes_nothing_and_is_an_operational_failure() {
    let base = TempDir::create();
    let missing = base.path().join("missing");
    let output = memtree(base.path(), &["index", "--root", missing.to_str().unwrap()]);
    assert_eq!((output.code, output.stdout.as_str()), (3, ""));
    let prefix = format!("memtree: cannot read `{}`: ", missing.display());
    assert!(output.stderr.starts_with(&prefix), "{}", output.stderr);

    let store = TempDir::create();
    store.write("MEMORY.md", "- [a](a.md) — first\n");
    store.write("a.md", note("a", "first", ""));
    store.write("b.md", note("b", "second", ""));

    fs::set_permissions(store.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let writable = fs::write(store.path().join(".probe"), "").is_ok();
    let output = index(&store);
    fs::set_permissions(store.path(), fs::Permissions::from_mode(0o755)).unwrap();
    if writable {
        // Permission bits do not bind this user (for example root), so the case cannot be staged.
        return;
    }

    assert_eq!((output.code, output.stdout.as_str()), (3, ""));
    assert!(
        output
            .stderr
            .starts_with("memtree: cannot write `.memtree-index-"),
        "{}",
        output.stderr
    );
    assert_eq!(store.read("MEMORY.md"), "- [a](a.md) — first\n".as_bytes());
    assert_eq!(root_names(&store), ["MEMORY.md", "a.md", "b.md"]);
}
