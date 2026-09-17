//! Running Git read-only in a store's root.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

/// What a finished Git command produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// The exit status, or `None` when a signal ended Git.
    pub code: Option<i32>,
    /// Everything Git wrote to stdout.
    pub stdout: Vec<u8>,
    /// Everything Git wrote to stderr, decoded lossily.
    pub stderr: String,
}

impl Output {
    /// Whether Git exited with status 0.
    #[must_use]
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// The first non-blank line of stderr, or the exit status when stderr is blank.
    #[must_use]
    pub fn detail(&self) -> String {
        self.stderr
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map_or_else(
                || {
                    self.code.map_or_else(
                        || "terminated by a signal".to_owned(),
                        |code| format!("exit status {code}"),
                    )
                },
                str::to_owned,
            )
    }
}

/// Runs `git --no-optional-locks -C <root> <args>` with stdin closed and terminal prompts off, so
/// Git never waits for input.
///
/// Optional locks cover only some writes: porcelain `git diff` still rewrites the index to refresh
/// stale stat data. Callers that must not write choose commands that never do.
///
/// # Errors
///
/// Returns the error from starting Git, for example when it is not installed.
pub fn run(root: &Path, args: &[&str]) -> io::Result<Output> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()?;
    Ok(Output {
        code: output.status.code(),
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}
