#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! `memtree links [--root DIR] [NAME...]`: list every named note, or the links that target one
//! of the requested names.

mod support;

use support::{Output, TempDir, memtree, note};

fn assert_output(output: &Output, code: i32, stdout: &str, stderr: &str) {
    assert_eq!(output.stdout, stdout, "stdout");
    assert_eq!(output.stderr, stderr, "stderr");
    assert_eq!(output.code, code, "exit code");
}

#[test]
fn links_lists_notes_or_filters_links_per_name() {
    let dir = TempDir::create();
    dir.write("store/a.md", note("a", "first", "[[b]] [[b]]\n[[c]]\n"));
    dir.write("store/b.md", note("b", "second", ""));
    dir.write(
        "store/c.md",
        note(
            "c",
            "third",
            "[[b]]\n`[[code-only]]`\n```\n[[fenced]]\n```\n[[d]]\n",
        ),
    );

    // No NAME: every named note, sorted by name, one `<name> <path>` line each. The summary
    // counts every link the parser found across the store.
    assert_output(
        &memtree(dir.path(), &["links", "--root", "store"]),
        0,
        "a a.md\nb b.md\nc c.md\n",
        "memtree: links over 3 notes: 5 links\n",
    );

    // Multiple NAMEs: every link whose target is one of them, deduped per (path, line, name) and
    // sorted by path then line. The duplicate `[[b]]` on line 7 of `a.md` collapses to one line;
    // the `[[c]]` on line 8 stays; the `[[d]]` in `c.md` is filtered out. Fenced and inline code
    // in `c.md` contribute nothing.
    assert_output(
        &memtree(dir.path(), &["links", "--root", "store", "b", "c"]),
        0,
        "a.md:7: links to [[b]]\na.md:8: links to [[c]]\nc.md:7: links to [[b]]\n",
        "memtree: links over 3 notes: 3 links\n",
    );

    // Single NAME: only that target, still sorted by path then line. `a.md:7` and `c.md:7` are
    // both a hit for `b` on different lines.
    assert_output(
        &memtree(dir.path(), &["links", "--root", "store", "b"]),
        0,
        "a.md:7: links to [[b]]\nc.md:7: links to [[b]]\n",
        "memtree: links over 3 notes: 2 links\n",
    );

    // Unknown NAME: silent empty output, summary still reports the store size, exit 0.
    assert_output(
        &memtree(dir.path(), &["links", "--root", "store", "ghost"]),
        0,
        "",
        "memtree: links over 3 notes: 0 links\n",
    );
}
