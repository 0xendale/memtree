#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The command line itself: help, version, usage errors, and output that cannot be written.

mod support;

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use support::{Output, TempDir, memtree, memtree_closed_stdout};

const USAGE: &str = "\
usage: memtree check [--root DIR]
       memtree index [--root DIR]
       memtree affected --base REF [--root DIR]
       memtree help | --version
";

const HELP: &str = "\
memtree checks, indexes, and diffs a markdown memory store: notes with
frontmatter, [[name]] links, and a MEMORY.md index.

usage: memtree check [--root DIR]
       memtree index [--root DIR]
       memtree affected --base REF [--root DIR]
       memtree help | --version

commands:
  check     report errors and warnings in the store; writes nothing
  index     append an entry to MEMORY.md for each note it does not list,
            keeping every existing line
  affected  list the notes added, changed, or deleted since the merge base,
            and the notes that link to them; writes nothing

options:
  --root DIR     the store root (default: the current directory)
  --base REF     changes are measured from the merge base of REF and HEAD
  -h, --help     print this help
  -V, --version  print the version

exit status: 0 success, 1 the store has errors, 2 usage error, 3 failure
";

fn assert_output(output: &Output, code: i32, stdout: &str, stderr: &str, context: &str) {
    assert_eq!(output.stdout, stdout, "stdout: {context}");
    assert_eq!(output.stderr, stderr, "stderr: {context}");
    assert_eq!(output.code, code, "exit code: {context}");
}

#[test]
fn help_goes_to_stdout_for_help_and_its_flags() {
    let dir = TempDir::create();
    for flag in ["help", "-h", "--help"] {
        assert_output(&memtree(dir.path(), &[flag]), 0, HELP, "", flag);
    }
}

#[test]
fn the_version_goes_to_stdout() {
    let dir = TempDir::create();
    for flag in ["-V", "--version"] {
        assert_output(
            &memtree(dir.path(), &[flag]),
            0,
            "memtree 0.1.0\n",
            "",
            flag,
        );
    }
}

#[test]
fn a_usage_error_names_the_problem_then_prints_the_usage_and_exits_2() {
    let dir = TempDir::create();
    let cases: &[(&[&str], &str)] = &[
        (&[], "missing command"),
        (&["frobnicate"], "unknown command `frobnicate`"),
        (&["--frobnicate"], "unknown option `--frobnicate`"),
        (&["--frobnicate=1"], "unknown option `--frobnicate`"),
        (&["-x"], "unknown option `-x`"),
        (&["help", "check"], "unexpected argument `check`"),
        (&["--help", "--version"], "unexpected argument `--version`"),
        (&["-V", "x"], "unexpected argument `x`"),
        (&["--version", "x", "y"], "unexpected argument `x`"),
        (&["check", "--help"], "unknown option `--help`"),
        (&["index", "-h"], "unknown option `-h`"),
        (&["index", "-r=."], "unknown option `-r=.`"),
        (&["check", "extra"], "unexpected argument `extra`"),
        (&["check", "--base", "main"], "unknown option `--base`"),
        (&["index", "--root"], "`--root` needs a value"),
        (&["index", "--root="], "`--root` needs a value"),
        (
            &["index", "--root", "a", "--root=b"],
            "`--root` is given more than once",
        ),
        (
            &["affected", "--root", "."],
            "`affected` needs `--base REF`",
        ),
        (
            &["affected", "--base", "-p"],
            "a ref cannot start with `-`: `-p`",
        ),
    ];
    for &(args, reason) in cases {
        assert_output(
            &memtree(dir.path(), args),
            2,
            "",
            &format!("memtree: {reason}\n{USAGE}"),
            &format!("{args:?}"),
        );
    }
}

#[test]
fn an_argument_that_is_not_utf8_is_a_usage_error() {
    let dir = TempDir::create();
    let args = [
        OsStr::new("check"),
        OsStr::new("--root"),
        OsStr::from_bytes(b"caf\xe9"),
    ];
    assert_output(
        &memtree(dir.path(), &args),
        2,
        "",
        &format!("memtree: argument `caf\u{FFFD}` is not valid UTF-8\n{USAGE}"),
        "non-UTF-8 root",
    );
}

#[test]
fn output_that_cannot_be_written_is_an_operational_failure() {
    let dir = TempDir::create();
    // A store without an index has a finding to print.
    let cases: [&[&str]; 2] = [&["help"], &["check"]];
    for args in cases {
        assert_output(
            &memtree_closed_stdout(dir.path(), args),
            3,
            "",
            "memtree: cannot write output: Broken pipe (os error 32)\n",
            &format!("{args:?}"),
        );
    }
}
