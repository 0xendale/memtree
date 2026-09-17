#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! `memtree affected`: the notes a change touches, and the notes that link to them.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, SystemTime};

use support::{Output, TempDir, git, git_commit_all, git_init, memtree, note};

fn affected(dir: &Path, base: &str) -> Output {
    memtree(dir, &["affected", "--base", base])
}

fn assert_output(output: &Output, code: i32, stdout: &str, stderr: &str) {
    assert_eq!(output.stdout, stdout, "stdout");
    assert_eq!(output.stderr, stderr, "stderr");
    assert_eq!(output.code, code, "exit code");
}

/// Gives a file a new modification time and leaves its bytes alone.
fn restamp(path: &Path) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000))
        .unwrap();
}

#[test]
fn touched_notes_and_the_notes_linking_to_their_names_are_reported() {
    let repo = TempDir::create();
    git_init(repo.path());
    repo.write("MEMORY.md", "- [a](a.md) — first\n");
    repo.write(
        "a.md",
        note("a", "first", "See [[b]], [[c]], and [[b]] again.\n"),
    );
    repo.write("b.md", note("b", "second", ""));
    repo.write("c.md", note("c", "third", "Back to [[a]].\n"));
    repo.write("d.md", note("d", "fourth", "Unrelated.\n"));
    repo.write("old.md", note("renamed-old", "moved", ""));
    repo.write(
        "sub/e.md",
        note(
            "e",
            "fifth",
            "See [[renamed-old]], [[renamed-new]], and [[gone]].\n",
        ),
    );
    repo.write(
        "x.md",
        note(
            "x",
            "sixth",
            "Links [[b]] and itself [[x]]; `[[c]]` is code.\n",
        ),
    );
    repo.write(".gitignore", "ignored.md\n");
    git_commit_all(repo.path(), "base");
    git(repo.path(), &["checkout", "-q", "-b", "work"]);

    repo.write("b.md", note("b", "second, revised", "Self [[b]].\n"));
    fs::remove_file(repo.path().join("c.md")).unwrap();
    fs::remove_file(repo.path().join("old.md")).unwrap();
    repo.write("new.md", note("renamed-new", "moved", ""));
    git_commit_all(repo.path(), "work");

    repo.write("f.md", note("f", "staged", ""));
    git(repo.path(), &["add", "f.md"]);
    repo.write("g.md", note("g", "untracked", ""));
    repo.write("ignored.md", note("ignored", "ignored by Git", ""));
    repo.write("notes.txt", "not a note");
    // The index is tracked and has the note extension, but it is not a note.
    repo.write("MEMORY.md", "- [a](a.md) — first, revised\n");
    // Same bytes, new modification time: Git has to look, and must find nothing and record nothing.
    restamp(&repo.path().join("d.md"));
    let git_index = repo.read(".git/index");

    assert_output(
        &affected(repo.path(), "main"),
        0,
        "a.md: links to [[b]]\n\
         a.md: links to [[c]]\n\
         b.md: changed\n\
         c.md: deleted\n\
         f.md: added\n\
         g.md: added\n\
         new.md: added\n\
         old.md: deleted\n\
         sub/e.md: links to [[renamed-new]]\n\
         sub/e.md: links to [[renamed-old]]\n\
         x.md: links to [[b]]\n",
        "memtree: since the merge base with `main`: 6 touched notes, 3 linking notes\n",
    );
    assert_eq!(
        repo.read(".git/index"),
        git_index,
        "the Git index must not be rewritten"
    );
}

#[test]
fn a_note_is_touched_only_when_its_contents_change() {
    let repo = TempDir::create();
    git_init(repo.path());
    repo.write(".gitattributes", "crlf.md text eol=crlf\n");
    repo.write("stat.md", note("stat", "new modification time", ""));
    repo.write(
        "crlf.md",
        note("crlf", "line endings the filter undoes", ""),
    );
    repo.write("mode.md", note("mode", "made executable", ""));
    repo.write(
        "staged.md",
        note("staged", "made executable and staged", ""),
    );
    repo.write("edited.md", note("edited", "first", ""));
    repo.write(
        "forgotten.md",
        note("forgotten", "removed from the index, same bytes", ""),
    );
    repo.write(
        "links.md",
        note(
            "links",
            "links to all of them",
            "[[stat]] [[crlf]] [[mode]] [[staged]] [[edited]] [[forgotten]]\n",
        ),
    );
    git_commit_all(repo.path(), "base");

    restamp(&repo.path().join("stat.md"));
    repo.write(
        "crlf.md",
        note("crlf", "line endings the filter undoes", "").replace('\n', "\r\n"),
    );
    for executable in ["mode.md", "staged.md"] {
        fs::set_permissions(
            repo.path().join(executable),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    git(repo.path(), &["add", "staged.md"]);
    // Deleted from the index and listed as untracked, yet the same file as at the base.
    git(repo.path(), &["rm", "-q", "--cached", "forgotten.md"]);
    repo.write("edited.md", note("edited", "first, revised", ""));

    assert_output(
        &affected(repo.path(), "HEAD"),
        0,
        "edited.md: changed\nlinks.md: links to [[edited]]\n",
        "memtree: since the merge base with `HEAD`: 1 touched note, 1 linking note\n",
    );
}

#[test]
fn changes_are_measured_from_the_merge_base_not_the_tip_of_the_base() {
    let repo = TempDir::create();
    git_init(repo.path());
    repo.write("a.md", note("a", "first", ""));
    repo.write("b.md", note("b", "second", "See [[a]].\n"));
    git_commit_all(repo.path(), "base");
    git(repo.path(), &["checkout", "-q", "-b", "work"]);
    repo.write("a.md", note("a", "first, revised on work", ""));
    git_commit_all(repo.path(), "work");
    git(repo.path(), &["checkout", "-q", "main"]);
    repo.write("b.md", note("b", "second, revised on main", "See [[a]].\n"));
    git_commit_all(repo.path(), "main moves on");
    git(repo.path(), &["checkout", "-q", "work"]);

    assert_output(
        &affected(repo.path(), "main"),
        0,
        "a.md: changed\nb.md: links to [[a]]\n",
        "memtree: since the merge base with `main`: 1 touched note, 1 linking note\n",
    );
}

#[test]
fn only_notes_under_the_root_count_and_paths_are_relative_to_it() {
    let repo = TempDir::create();
    git_init(repo.path());
    repo.write("README.md", "# Project\n");
    repo.write("other/x.md", note("x", "outside", ""));
    repo.write("memory/a.md", note("a", "first", ""));
    repo.write("memory/b.md", note("b", "second", "See [[a]] and [[x]].\n"));
    git_commit_all(repo.path(), "base");

    assert_output(
        &memtree(
            repo.path(),
            &["affected", "--base", "HEAD", "--root", "memory"],
        ),
        0,
        "",
        "memtree: since the merge base with `HEAD`: 0 touched notes, 0 linking notes\n",
    );

    repo.write("README.md", "# Project, revised\n");
    repo.write("other/x.md", note("x", "outside, revised", ""));
    repo.write("memory/a.md", note("a", "first, revised", ""));
    assert_output(
        &memtree(repo.path(), &["affected", "--base=HEAD", "--root=memory"]),
        0,
        "a.md: changed\nb.md: links to [[a]]\n",
        "memtree: since the merge base with `HEAD`: 1 touched note, 1 linking note\n",
    );
}

#[test]
fn a_store_outside_git_or_a_base_git_cannot_use_is_an_operational_failure() {
    let plain = TempDir::create();
    plain.write("a.md", note("a", "first", ""));
    let output = affected(plain.path(), "main");
    assert_eq!((output.code, output.stdout.as_str()), (3, ""));
    assert!(
        output
            .stderr
            .starts_with("memtree: `.` is not in a Git work tree"),
        "{}",
        output.stderr
    );

    let repo = TempDir::create();
    git_init(repo.path());
    repo.write("a.md", note("a", "first", ""));
    git_commit_all(repo.path(), "base");
    assert_output(
        &affected(repo.path(), "nope"),
        3,
        "",
        "memtree: `nope` does not name a commit\n",
    );
    // Inside the Git directory, Git runs and answers that this is not a work tree.
    assert_output(
        &memtree(
            repo.path(),
            &["affected", "--base", "main", "--root", ".git"],
        ),
        3,
        "",
        "memtree: `.git` is not in a Git work tree\n",
    );

    git(repo.path(), &["checkout", "-q", "--orphan", "unrelated"]);
    git_commit_all(repo.path(), "unrelated root");
    assert_output(
        &affected(repo.path(), "main"),
        3,
        "",
        "memtree: `main` has no merge base with HEAD\n",
    );
}

#[test]
fn a_base_is_required_and_cannot_look_like_an_option() {
    let repo = TempDir::create();
    let cases: [(&[&str], &str); 3] = [
        (&["affected"], "memtree: `affected` needs `--base REF`\n"),
        (
            &["affected", "--base", "-p"],
            "memtree: a ref cannot start with `-`: `-p`\n",
        ),
        (
            &["affected", "--base=--output=x"],
            "memtree: a ref cannot start with `-`: `--output=x`\n",
        ),
    ];
    for (args, first_line) in cases {
        let output = memtree(repo.path(), args);
        assert_eq!((output.code, output.stdout.as_str()), (2, ""), "{args:?}");
        assert!(
            output.stderr.starts_with(first_line),
            "{args:?}: {}",
            output.stderr
        );
    }
}
