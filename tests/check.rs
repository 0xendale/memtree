#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! `memtree check`: frontmatter, schema, link, and index findings.

mod support;

use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};

use support::{Output, TempDir, memtree, note};

fn check(store: &TempDir) -> Output {
    memtree(store.path(), &["check"])
}

fn assert_output(output: &Output, code: i32, stdout: &str, stderr: &str) {
    assert_eq!(output.stdout, stdout, "stdout");
    assert_eq!(output.stderr, stderr, "stderr");
    assert_eq!(output.code, code, "exit code");
}

#[test]
fn a_consistent_store_passes() {
    let store = TempDir::create();
    store.write("a.md", note("a", "first", "See [[b]].\n"));
    store.write("nested/b.md", note("b", "second", "Back to [[a]].\n"));
    store.write(
        "MEMORY.md",
        "# Memory\n\n- [a](a.md) — first\n- [b](nested/b.md) — second\n",
    );

    assert_output(
        &check(&store),
        0,
        "",
        "memtree: checked 2 notes: 0 errors, 0 warnings\n",
    );
}

#[test]
fn frontmatter_problems_are_errors_at_their_lines() {
    let store = TempDir::create();
    store.write("missing.md", "# No frontmatter\n");
    store.write("open.md", "---\nname: open\n");
    store.write(
        "syntax.md",
        "---\nname: syntax\ntags: [a]\ndescription: d\n---\n",
    );
    store.write(
        "MEMORY.md",
        "- [missing](missing.md)\n- [open](open.md)\n- [syntax](syntax.md)\n",
    );

    assert_output(
        &check(&store),
        1,
        "missing.md: error: frontmatter-missing: note does not start with a `---` frontmatter block\n\
         open.md:1: error: frontmatter-unterminated: frontmatter block has no closing `---`\n\
         syntax.md:3: error: frontmatter-syntax: flow collections are not supported; quote the value to keep it as text\n",
        "memtree: checked 3 notes: 3 errors, 0 warnings\n",
    );
}

#[test]
fn schema_problems_are_errors() {
    let store = TempDir::create();
    let notes = [
        ("no-name.md", "---\ndescription: d\n---\n"),
        ("empty-name.md", "---\nname:\ndescription: d\n---\n"),
        ("bad-name.md", "---\nname: Bad_Name\ndescription: d\n---\n"),
        ("no-description.md", "---\nname: no-description\n---\n"),
        (
            "empty-description.md",
            "---\nname: empty-description\ndescription: '  '\n---\n",
        ),
        (
            "bad-type.md",
            "---\nname: bad-type\ndescription: d\nmetadata:\n  type: opinion\n---\n",
        ),
        (
            "bad-metadata.md",
            "---\nname: bad-metadata\ndescription: d\nmetadata: project\n---\n",
        ),
        ("two-problems.md", "---\nname: Two\n---\n"),
        (
            "fine.md",
            "---\nname: fine\ndescription: d\nsource: chat\nmetadata:\n  origin: review\n---\n",
        ),
    ];
    let mut index = String::new();
    for (path, text) in notes {
        store.write(path, text);
        writeln!(index, "- [{path}]({path})").unwrap();
    }
    store.write("MEMORY.md", index);

    assert_output(
        &check(&store),
        1,
        "bad-metadata.md:4: error: metadata-invalid: `metadata` must hold indented keys such as `type`\n\
         bad-name.md:2: error: name-invalid: `Bad_Name` is not a kebab-case name (lowercase letters and digits joined by single hyphens)\n\
         bad-type.md:5: error: type-invalid: `opinion` is not a memory type; expected one of user, feedback, project, reference\n\
         empty-description.md:3: error: description-missing: `description` is empty\n\
         empty-name.md:2: error: name-missing: `name` is empty\n\
         no-description.md: error: description-missing: frontmatter has no `description`\n\
         no-name.md: error: name-missing: frontmatter has no `name`\n\
         two-problems.md: error: description-missing: frontmatter has no `description`\n\
         two-problems.md:2: error: name-invalid: `Two` is not a kebab-case name (lowercase letters and digits joined by single hyphens)\n",
        "memtree: checked 9 notes: 9 errors, 0 warnings\n",
    );
}

#[test]
fn every_note_sharing_a_name_is_an_error() {
    let store = TempDir::create();
    store.write("a.md", note("same", "one", ""));
    store.write("sub/b.md", note("same", "two", ""));
    store.write("sub/c.md", note("same", "three", ""));
    store.write("x.md", "---\nname: Not_Kebab\ndescription: d\n---\n");
    store.write("y.md", "---\nname: Not_Kebab\ndescription: d\n---\n");
    store.write(
        "MEMORY.md",
        "- [a](a.md)\n- [b](sub/b.md)\n- [c](sub/c.md)\n- [x](x.md)\n- [y](y.md)\n",
    );

    assert_output(
        &check(&store),
        1,
        "a.md:2: error: duplicate-name: `same` is also the name of sub/b.md, sub/c.md\n\
         sub/b.md:2: error: duplicate-name: `same` is also the name of a.md, sub/c.md\n\
         sub/c.md:2: error: duplicate-name: `same` is also the name of a.md, sub/b.md\n\
         x.md:2: error: duplicate-name: `Not_Kebab` is also the name of y.md\n\
         x.md:2: error: name-invalid: `Not_Kebab` is not a kebab-case name (lowercase letters and digits joined by single hyphens)\n\
         y.md:2: error: duplicate-name: `Not_Kebab` is also the name of x.md\n\
         y.md:2: error: name-invalid: `Not_Kebab` is not a kebab-case name (lowercase letters and digits joined by single hyphens)\n",
        "memtree: checked 5 notes: 7 errors, 0 warnings\n",
    );
}

#[test]
fn dangling_links_are_warnings() {
    let store = TempDir::create();
    store.write(
        "a.md",
        note("a", "first", "See [[b]] and [[ghost]].\n\n`[[in-code]]`\n"),
    );
    store.write("b.md", note("b", "second", ""));
    store.write("MEMORY.md", "- [a](a.md)\n- [b](b.md)\n");

    assert_output(
        &check(&store),
        0,
        "a.md:7: warning: dangling-link: no note is named `ghost`\n",
        "memtree: checked 2 notes: 0 errors, 1 warning\n",
    );
}

#[test]
fn links_resolve_to_names_that_are_present_even_if_invalid() {
    let store = TempDir::create();
    store.write("a.md", note("a", "first", "[[Odd_Name]]\n"));
    store.write("odd.md", "---\nname: Odd_Name\ndescription: d\n---\n");
    store.write("MEMORY.md", "- [a](a.md)\n- [odd](odd.md)\n");

    assert_output(
        &check(&store),
        1,
        "odd.md:2: error: name-invalid: `Odd_Name` is not a kebab-case name (lowercase letters and digits joined by single hyphens)\n",
        "memtree: checked 2 notes: 1 error, 0 warnings\n",
    );
}

#[test]
fn index_entries_must_match_the_notes_exactly_once() {
    let store = TempDir::create();
    store.write("a.md", note("a", "first", ""));
    store.write("b.md", note("b", "second", ""));
    store.write(
        "MEMORY.md",
        "# Memory\n\n- [First](a.md) — first\n- [Gone](gone.md) — deleted\n  - [First again](./a.md) — repeated\n- [Site](https://example.com/page.md) — external\nSee [the first note](a.md) inline.\n",
    );

    assert_output(
        &check(&store),
        1,
        "MEMORY.md:4: error: stale-index-entry: `gone.md` is not a note in the store\n\
         MEMORY.md:5: error: duplicate-index-entry: `a.md` is already listed on line 3\n\
         b.md: error: orphan: note is not listed in MEMORY.md\n",
        "memtree: checked 2 notes: 3 errors, 0 warnings\n",
    );
}

#[test]
fn an_index_with_a_byte_order_mark_still_lists_its_first_note() {
    let store = TempDir::create();
    store.write("a.md", note("a", "first", ""));
    store.write("MEMORY.md", "\u{feff}- [a](a.md) — first\n");

    assert_output(
        &check(&store),
        0,
        "",
        "memtree: checked 1 note: 0 errors, 0 warnings\n",
    );
}

#[test]
fn a_missing_index_is_one_error_and_check_does_not_create_it() {
    let store = TempDir::create();
    store.write("a.md", note("a", "first", ""));
    store.write("b.md", note("b", "second", ""));

    assert_output(
        &check(&store),
        1,
        "MEMORY.md: error: index-missing: the store root has no MEMORY.md\n",
        "memtree: checked 2 notes: 1 error, 0 warnings\n",
    );
    assert!(!store.path().join("MEMORY.md").exists());
}

#[test]
fn hidden_entries_nested_index_files_and_other_files_are_not_notes() {
    let store = TempDir::create();
    store.write("a.md", note("a", "first", ""));
    store.write(".hidden/x.md", "broken");
    store.write(".draft.md", "broken");
    store.write("sub/MEMORY.md", "broken");
    store.write("sub/notes.txt", "broken");
    store.write("MEMORY.md", "- [a](a.md) — first\n");

    assert_output(
        &check(&store),
        0,
        "",
        "memtree: checked 1 note: 0 errors, 0 warnings\n",
    );
}

#[test]
fn symbolic_links_are_warnings_and_are_not_followed() {
    let store = TempDir::create();
    let outside = TempDir::create();
    outside.write("elsewhere.md", "broken");
    store.write("a.md", note("a", "first", ""));
    symlink(store.path().join("a.md"), store.path().join("alias.md")).unwrap();
    symlink(outside.path(), store.path().join("linked")).unwrap();
    store.write("MEMORY.md", "- [a](a.md) — first\n");

    assert_output(
        &check(&store),
        0,
        "alias.md: warning: symlink: symbolic link not followed\n\
         linked: warning: symlink: symbolic link not followed\n",
        "memtree: checked 1 note: 0 errors, 2 warnings\n",
    );
}

#[test]
fn a_note_that_is_not_utf8_is_an_encoding_error() {
    let store = TempDir::create();
    store.write("a.md", note("a", "first", ""));
    store.write("bad.md", [0xff_u8, 0xfe, b'\n']);
    store.write("MEMORY.md", "- [a](a.md)\n- [bad](bad.md)\n");

    assert_output(
        &check(&store),
        1,
        "bad.md: error: encoding: note is not valid UTF-8\n",
        "memtree: checked 2 notes: 1 error, 0 warnings\n",
    );
}

#[test]
fn an_index_that_is_not_utf8_is_an_encoding_error_without_index_checks() {
    let store = TempDir::create();
    store.write("a.md", note("a", "first", ""));
    store.write("MEMORY.md", [0xff_u8, b'\n']);

    assert_output(
        &check(&store),
        1,
        "MEMORY.md: error: encoding: index is not valid UTF-8\n",
        "memtree: checked 1 note: 1 error, 0 warnings\n",
    );
}

#[test]
fn root_selects_the_store_from_any_directory() {
    let store = TempDir::create();
    store.write("a.md", note("a", "first", ""));
    store.write("MEMORY.md", "- [a](a.md) — first\n");
    let elsewhere = TempDir::create();
    let root = store.path().to_str().unwrap();
    let clean = "memtree: checked 1 note: 0 errors, 0 warnings\n";

    assert_output(
        &memtree(elsewhere.path(), &["check", "--root", root]),
        0,
        "",
        clean,
    );
    assert_output(
        &memtree(elsewhere.path(), &["check", &format!("--root={root}")]),
        0,
        "",
        clean,
    );
}

#[test]
fn a_store_that_cannot_be_read_is_an_operational_failure() {
    let base = TempDir::create();
    base.write("file.md", note("a", "first", ""));

    let missing = base.path().join("missing");
    let output = memtree(base.path(), &["check", "--root", missing.to_str().unwrap()]);
    assert_eq!((output.code, output.stdout.as_str()), (3, ""));
    let prefix = format!("memtree: cannot read `{}`: ", missing.display());
    assert!(output.stderr.starts_with(&prefix), "{}", output.stderr);

    let file = base.path().join("file.md");
    assert_output(
        &memtree(base.path(), &["check", "--root", file.to_str().unwrap()]),
        3,
        "",
        &format!("memtree: `{}` is not a directory\n", file.display()),
    );

    let linked_index = TempDir::create();
    linked_index.write("a.md", note("a", "first", ""));
    symlink(
        linked_index.path().join("a.md"),
        linked_index.path().join("MEMORY.md"),
    )
    .unwrap();
    assert_output(
        &check(&linked_index),
        3,
        "",
        "memtree: `MEMORY.md` is not a regular file\n",
    );

    let sealed = TempDir::create();
    sealed.write("MEMORY.md", "- [secret](secret.md)\n");
    sealed.write("secret.md", note("secret", "hidden", ""));
    let secret = sealed.path().join("secret.md");
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&secret).is_ok() {
        // Permission bits do not bind this user (for example root), so the case cannot be staged.
        return;
    }
    let output = check(&sealed);
    assert_eq!((output.code, output.stdout.as_str()), (3, ""));
    assert!(
        output
            .stderr
            .starts_with("memtree: cannot read `secret.md`: "),
        "{}",
        output.stderr
    );
}
