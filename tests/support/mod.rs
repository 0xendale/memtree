#![allow(dead_code)]

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::sync::atomic::{AtomicUsize, Ordering};

const GIT_DATE: &str = "2026-01-01T00:00:00Z";

/// A fresh directory under the system temp dir, removed on drop.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn create() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("memtree-test-{}-{unique}", process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self {
            path: path.canonicalize().unwrap(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&self, relative: &str, contents: impl AsRef<[u8]>) {
        let path = self.path.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    pub fn read(&self, relative: &str) -> Vec<u8> {
        fs::read(self.path.join(relative)).unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// What one memtree run produced.
#[derive(Debug)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Runs the memtree binary in `cwd`.
pub fn memtree<A: AsRef<OsStr>>(cwd: &Path, args: &[A]) -> Output {
    let output = pinned("memtree", env!("CARGO_BIN_EXE_memtree"), cwd)
        .args(args)
        .output()
        .unwrap();
    Output {
        code: output.status.code().unwrap(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

/// Runs the memtree binary in `cwd` with a stdout pipe whose read end is closed before memtree
/// starts, so every write to stdout fails.
pub fn memtree_closed_stdout<A: AsRef<OsStr>>(cwd: &Path, args: &[A]) -> Output {
    let (reader, writer) = io::pipe().unwrap();
    drop(reader);
    let output = pinned("memtree", env!("CARGO_BIN_EXE_memtree"), cwd)
        .args(args)
        .stdout(writer)
        .output()
        .unwrap();
    Output {
        code: output.status.code().unwrap(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

/// Runs `git` in `cwd`, failing the test if it fails, and returns its stdout.
pub fn git(cwd: &Path, args: &[&str]) -> String {
    let output = pinned("git", "git", cwd).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// A Git repository in `dir` on branch `main`.
pub fn git_init(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
}

/// Stages everything in `dir` and commits it.
pub fn git_commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "--no-gpg-sign", "-m", message]);
}

/// A command whose locale, time zone, and Git configuration, identity, dates, and repository
/// discovery do not depend on the machine running the tests.
fn pinned(label: &str, program: &str, cwd: &Path) -> Command {
    let mut command = Command::new(program);
    for (key, _) in env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .current_dir(cwd)
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CEILING_DIRECTORIES",
            env::temp_dir().canonicalize().unwrap(),
        )
        .env("GIT_AUTHOR_NAME", label)
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_AUTHOR_DATE", GIT_DATE)
        .env("GIT_COMMITTER_NAME", label)
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_DATE", GIT_DATE);
    command
}

/// A valid note. Its body starts on line 7.
pub fn note(name: &str, description: &str, body: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: {description}\nmetadata:\n  type: project\n---\n{body}"
    )
}
